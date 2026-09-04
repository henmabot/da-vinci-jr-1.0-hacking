mod connection;
mod serial_log;

use std::{collections::VecDeque, fmt, path::Path, time::Duration};

use connection::{Connection, DeviceEvent, Event as ConnectionEvent};
use da_vinci_protocol::{
    Direction, Level, Pin, PinTarget, Port, Request, ResponseError, WIRE_PIN_COUNT,
};
use iced::{
    Background, Border, Color, Element, Length, Size, Subscription, Task, Theme,
    alignment::{Horizontal, Vertical},
    keyboard::{Event as KeyboardEvent, Key, key::Named},
    widget::{
        button, checkbox, column, container, pane_grid, pick_list, responsive, row, scrollable,
        text, text_editor, text_input,
    },
};
use serial_log::SerialLog;

const MAX_IO_EVENTS_PER_TICK: usize = 256;
const MAX_COMMAND_HISTORY: usize = 200;
const MODES: [Mode; 3] = [Mode::Input, Mode::InputPullup, Mode::Output];
const BULK_SCOPES: [PinTarget; 6] = [
    PinTarget::All,
    PinTarget::Bank(Port::A),
    PinTarget::Bank(Port::B),
    PinTarget::Bank(Port::C),
    PinTarget::Bank(Port::D),
    PinTarget::Bank(Port::E),
];
const BANK_TABS: [BankTab; 4] = [BankTab::A, BankTab::BAndE, BankTab::C, BankTab::D];
const ROW_HEIGHT: f32 = 34.0;
const CONTROL_TEXT_SIZE: f32 = 13.0;
const PIN_CONTROL_TEXT_SIZE: f32 = 12.0;
const PIN_NAME_SHARE: u16 = 5;
const PIN_MODE_SHARE: u16 = 7;
const PIN_STATUS_SHARE: u16 = 3;
const PIN_RW_SHARE: u16 = 7;
const PIN_LISTEN_SHARE: u16 = 5;
const CONNECTION_ACTIONS_INLINE_MIN: f32 = 1_050.0;
const PIN_TABLE_TWO_COLUMN_MIN: f32 = 800.0;
const CELL_GAP: f32 = 4.0;

const WINDOW_BG: Color = Color::from_rgb8(0x24, 0x24, 0x24);
const PANEL_BG: Color = Color::from_rgb8(0x2B, 0x2B, 0x2B);
const RAISED_BG: Color = Color::from_rgb8(0x3A, 0x3A, 0x3A);
const RAISED_HOVER: Color = Color::from_rgb8(0x46, 0x46, 0x46);
const INPUT_BG: Color = Color::from_rgb8(0x34, 0x36, 0x38);
const UI_BORDER: Color = Color::from_rgb8(0x56, 0x5B, 0x5E);
const UI_TEXT: Color = Color::from_rgb8(0xDC, 0xE4, 0xEE);
const MUTED: Color = Color::from_rgb8(0xB0, 0xB0, 0xB0);
const HIGH_BG: Color = Color::from_rgb8(0x3D, 0xDC, 0x97);
const LOW_BG: Color = Color::from_rgb8(0x4A, 0x4A, 0x4A);
const UNSET_BG: Color = Color::from_rgb8(0x38, 0x38, 0x38);
const DANGER: Color = Color::from_rgb8(0xE0, 0x6C, 0x75);

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("GPIO Controller")
        .subscription(App::subscription)
        .theme(app_theme())
        .window(iced::window::Settings {
            size: Size::new(1280.0, 820.0),
            min_size: Some(Size::new(900.0, 640.0)),
            ..Default::default()
        })
        .run()
}

fn app_theme() -> Theme {
    Theme::custom(
        "GPIO Controller",
        iced::theme::Palette {
            background: WINDOW_BG,
            text: UI_TEXT,
            primary: Color::from_rgb8(0x3B, 0x8E, 0xD0),
            success: HIGH_BG,
            warning: Color::from_rgb8(0xD6, 0xA8, 0x4B),
            danger: DANGER,
        },
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Input,
    InputPullup,
    Output,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PortChoice {
    path: String,
    label: String,
}

impl PortChoice {
    fn new(path: String) -> Self {
        let label = Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&path)
            .to_owned();
        Self { path, label }
    }
}

impl fmt::Display for PortChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

impl Mode {
    fn is_input(self) -> bool {
        matches!(self, Self::Input | Self::InputPullup)
    }

    fn direction(self) -> Direction {
        if self == Self::Output {
            Direction::Output
        } else {
            Direction::Input
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BankTab {
    A,
    BAndE,
    C,
    D,
}

impl BankTab {
    fn index(self) -> usize {
        BANK_TABS.iter().position(|tab| *tab == self).unwrap()
    }
    fn label(self) -> &'static str {
        match self {
            Self::A => "PIOA",
            Self::BAndE => "PIOB + PIOE",
            Self::C => "PIOC",
            Self::D => "PIOD",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum PaneKind {
    Pins,
    Log,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Input => "INPUT",
            Self::InputPullup => "IN_PULLUP",
            Self::Output => "OUTPUT",
        })
    }
}

#[derive(Clone, Copy, Debug)]
enum HistoryDirection {
    Previous,
    Next,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListenerState {
    Off,
    Enabling,
    On,
    Disabling,
}

impl ListenerState {
    fn is_pending(self) -> bool {
        matches!(self, Self::Enabling | Self::Disabling)
    }
}

#[derive(Clone, Copy)]
struct PinState {
    mode: Option<Mode>,
    target_mode: Option<Mode>,
    level: Option<Level>,
    listener: ListenerState,
    value_pending: bool,
}

impl PinState {
    const UNSET: Self = Self {
        mode: None,
        target_mode: None,
        level: None,
        listener: ListenerState::Off,
        value_pending: false,
    };
}

#[derive(Clone, Debug)]
enum Message {
    Tick,
    PortsLoaded(Result<Vec<String>, String>),
    RefreshPorts,
    PortSelected(PortChoice),
    Connect,
    Disconnect,
    PreviousTab,
    NextTab,
    TabSelected(BankTab),
    ModeSelected(Pin, Mode),
    Read(Pin),
    Write(Pin),
    Listen(Pin),
    BulkScopeSelected(PinTarget),
    BulkModeSelected(Mode),
    OverwriteChanged(bool),
    ApplyBulkMode,
    BulkRead,
    BulkListen(bool),
    BulkSet(Level),
    BulkSetConfirm,
    BulkSetCancel,
    Handshake,
    Status,
    Reboot,
    RebootConfirm,
    RebootCancel,
    PaneResized(pane_grid::ResizeEvent),
    ClearLog,
    ShowTimestamps(bool),
    Autoscroll(bool),
    LogAction(text_editor::Action),
    RawChanged(String),
    RawSend,
    HistoryKey(HistoryDirection),
    HistoryKeyFocus {
        direction: HistoryDirection,
        focused: bool,
    },
}

struct App {
    pins: [PinState; WIRE_PIN_COUNT as usize],
    bank_tab: BankTab,
    bulk_scope: PinTarget,
    bulk_mode: Mode,
    overwrite: bool,
    confirm_set: Option<(PinTarget, Level)>,
    panes: pane_grid::State<PaneKind>,
    ports: Vec<PortChoice>,
    selected_port: Option<PortChoice>,
    connected_port: Option<String>,
    connection: Connection,
    log: SerialLog,
    autoscroll: bool,
    log_scroll: iced::widget::Id,
    raw_input_id: iced::widget::Id,
    raw_input: String,
    command_history: VecDeque<String>,
    history_index: Option<usize>,
    device_status: String,
    error: Option<String>,
    confirm_reboot: bool,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let (mut panes, pins_pane) = pane_grid::State::new(PaneKind::Pins);
        let (_, split) = panes
            .split(pane_grid::Axis::Vertical, pins_pane, PaneKind::Log)
            .expect("initial GPIO/log split must succeed");
        panes.resize(split, 0.76);
        (
            Self {
                pins: [PinState::UNSET; WIRE_PIN_COUNT as usize],
                bank_tab: BankTab::A,
                bulk_scope: PinTarget::All,
                bulk_mode: Mode::Input,
                overwrite: false,
                confirm_set: None,
                panes,
                ports: Vec::new(),
                selected_port: None,
                connected_port: None,
                connection: Connection::spawn(),
                log: SerialLog::new(),
                autoscroll: true,
                log_scroll: iced::widget::Id::unique(),
                raw_input_id: iced::widget::Id::unique(),
                raw_input: String::new(),
                command_history: VecDeque::new(),
                history_index: None,
                device_status: "Disconnected".into(),
                error: None,
                confirm_reboot: false,
            },
            load_ports(),
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(Duration::from_millis(40)).map(|_| Message::Tick),
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Keyboard(KeyboardEvent::KeyPressed {
                    key: Key::Named(Named::ArrowUp),
                    ..
                }) => Some(Message::HistoryKey(HistoryDirection::Previous)),
                iced::Event::Keyboard(KeyboardEvent::KeyPressed {
                    key: Key::Named(Named::ArrowDown),
                    ..
                }) => Some(Message::HistoryKey(HistoryDirection::Next)),
                _ => None,
            }),
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle_message(message);
        if self.log.flush() {
            Task::batch([task, self.snap_log()])
        } else {
            task
        }
    }

    fn handle_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => self.drain_io(),
            Message::PortsLoaded(result) => {
                match result {
                    Ok(ports) => {
                        self.ports = ports.into_iter().map(PortChoice::new).collect();
                        if self
                            .selected_port
                            .as_ref()
                            .is_none_or(|selected| !self.ports.contains(selected))
                        {
                            self.selected_port = self.ports.first().cloned();
                        }
                    }
                    Err(error) => self.error = Some(error),
                }
                Task::none()
            }
            Message::RefreshPorts => load_ports(),
            Message::PortSelected(port) => {
                self.selected_port = Some(port);
                Task::none()
            }
            Message::Connect => {
                if let Some(port) = &self.selected_port {
                    self.error = self.connection.connect(port.path.clone()).err();
                }
                Task::none()
            }
            Message::Disconnect => {
                self.error = self.connection.disconnect().err();
                Task::none()
            }
            Message::PreviousTab => {
                let index = self.bank_tab.index();
                if index > 0 {
                    self.bank_tab = BANK_TABS[index - 1];
                }
                Task::none()
            }
            Message::NextTab => {
                let index = self.bank_tab.index();
                if index + 1 < BANK_TABS.len() {
                    self.bank_tab = BANK_TABS[index + 1];
                }
                Task::none()
            }
            Message::TabSelected(tab) => {
                self.bank_tab = tab;
                Task::none()
            }
            Message::ModeSelected(pin, mode) => self.change_mode(pin, mode),
            Message::Read(pin) => self.read_pin(pin),
            Message::Write(pin) => self.write_pin(pin),
            Message::Listen(pin) => self.toggle_listener(pin),
            Message::BulkScopeSelected(scope) => {
                self.bulk_scope = scope;
                self.confirm_set = None;
                Task::none()
            }
            Message::BulkModeSelected(mode) => {
                self.bulk_mode = mode;
                Task::none()
            }
            Message::OverwriteChanged(overwrite) => {
                self.overwrite = overwrite;
                Task::none()
            }
            Message::ApplyBulkMode => self.apply_bulk_mode(),
            Message::BulkRead => self.read_scope(self.bulk_scope),
            Message::BulkListen(enabled) => self.set_listener_scope(self.bulk_scope, enabled),
            Message::BulkSet(level) => {
                self.confirm_set = Some((self.bulk_scope, level));
                Task::none()
            }
            Message::BulkSetConfirm => {
                let Some((target, level)) = self.confirm_set.take() else {
                    return Task::none();
                };
                self.set_scope_level(target, level)
            }
            Message::BulkSetCancel => {
                self.confirm_set = None;
                Task::none()
            }
            Message::Handshake => self.send_request(Request::Hello),
            Message::Status => self.send_request(Request::Status),
            Message::Reboot => {
                self.confirm_reboot = true;
                Task::none()
            }
            Message::RebootConfirm => {
                self.confirm_reboot = false;
                self.send_request(Request::Bye)
            }
            Message::RebootCancel => {
                self.confirm_reboot = false;
                Task::none()
            }
            Message::PaneResized(event) => {
                self.panes.resize(event.split, event.ratio);
                Task::none()
            }
            Message::ClearLog => {
                self.log.clear();
                Task::none()
            }
            Message::ShowTimestamps(enabled) => {
                self.log.set_show_timestamps(enabled);
                Task::none()
            }
            Message::Autoscroll(enabled) => {
                self.autoscroll = enabled;
                if enabled {
                    self.snap_log()
                } else {
                    Task::none()
                }
            }
            Message::LogAction(action) => {
                if !action.is_edit() {
                    self.log.perform(action);
                }
                Task::none()
            }
            Message::RawChanged(value) => {
                self.raw_input = value;
                self.history_index = None;
                Task::none()
            }
            Message::RawSend => self.send_raw(),
            Message::HistoryKey(direction) => {
                iced::widget::operation::is_focused(self.raw_input_id.clone())
                    .map(move |focused| Message::HistoryKeyFocus { direction, focused })
            }
            Message::HistoryKeyFocus { direction, focused } => {
                if focused {
                    match direction {
                        HistoryDirection::Previous => self.history_previous(),
                        HistoryDirection::Next => self.history_next(),
                    }
                }
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let top = self.connection_controls();
        let body = pane_grid(&self.panes, |_, pane, _| {
            pane_grid::Content::new(match pane {
                PaneKind::Pins => self.pin_panel(),
                PaneKind::Log => self.log_panel(),
            })
        })
        .spacing(8)
        .min_size(400)
        .on_resize(8, Message::PaneResized)
        .height(Length::Fill);

        let mut content = column![top, body].spacing(8).padding(8);
        if let Some(error) = &self.error {
            content = content.push(
                container(text(format!("Error: {error}")).size(13))
                    .padding([6, 10])
                    .style(container::danger),
            );
        }
        container(content)
            .height(Length::Fill)
            .width(Length::Fill)
            .style(|_| container::Style {
                background: Some(Background::Color(WINDOW_BG)),
                ..Default::default()
            })
            .into()
    }

    fn connection_controls(&self) -> Element<'_, Message> {
        let content = responsive(|size| self.connection_content(size.width))
            .height(Length::Shrink)
            .width(Length::Fill);

        container(content)
            .padding(8)
            .width(Length::Fill)
            .style(panel_style)
            .into()
    }

    fn connection_content(&self, available_width: f32) -> Element<'_, Message> {
        let ports = pick_list(
            self.ports.as_slice(),
            self.selected_port.as_ref(),
            Message::PortSelected,
        )
        .placeholder("Serial port")
        .text_size(CONTROL_TEXT_SIZE)
        .padding([5, 8])
        .width(Length::Fill);

        let connection_button = if self.connected_port.is_some() {
            native_button("Disconnect").on_press(Message::Disconnect)
        } else {
            native_button("Connect")
                .on_press_maybe(self.selected_port.as_ref().map(|_| Message::Connect))
        };

        let connection = row![
            text("Connection").size(18),
            ports,
            native_button("Refresh").on_press(Message::RefreshPorts),
            connection_button,
            text(&self.device_status).size(13),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill);

        let actions = row![
            native_button("Handshake").on_press(Message::Handshake),
            native_button("Status").on_press(Message::Status),
            danger_native_button("Reset device").on_press(Message::Reboot),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        let connection: Element<'_, Message> = if available_width >= CONNECTION_ACTIONS_INLINE_MIN {
            row![connection, actions]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .width(Length::Fill)
                .into()
        } else {
            column![
                connection,
                container(actions)
                    .width(Length::Fill)
                    .align_x(Horizontal::Right)
            ]
            .spacing(6)
            .into()
        };

        let content: Element<'_, Message> = if self.confirm_reboot {
            column![
                connection,
                row![
                    text("Reset device and drop the connection?").size(12),
                    danger_native_button("Reset device").on_press(Message::RebootConfirm),
                    native_button("Cancel").on_press(Message::RebootCancel),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(6)
            .into()
        } else {
            connection
        };

        content
    }

    fn pin_panel(&self) -> Element<'_, Message> {
        let index = self.bank_tab.index();
        let mut tabs =
            row![native_button("‹").on_press_maybe((index > 0).then_some(Message::PreviousTab))]
                .spacing(4)
                .align_y(iced::Alignment::Center);
        for tab in BANK_TABS {
            let tab_button = if tab == self.bank_tab {
                native_button(tab.label()).style(selected_tab_button)
            } else {
                native_button(tab.label())
            };
            tabs = tabs.push(tab_button.on_press(Message::TabSelected(tab)));
        }
        tabs = tabs.push(
            native_button("›")
                .on_press_maybe((index + 1 < BANK_TABS.len()).then_some(Message::NextTab)),
        );

        let bulk_scope = row![
            text("Scope").size(12),
            pick_list(
                BULK_SCOPES,
                Some(self.bulk_scope),
                Message::BulkScopeSelected
            )
            .text_size(PIN_CONTROL_TEXT_SIZE)
            .padding([5, 8]),
            text("Mode").size(12),
            pick_list(MODES, Some(self.bulk_mode), Message::BulkModeSelected)
                .text_size(PIN_CONTROL_TEXT_SIZE)
                .padding([5, 8]),
            checkbox(self.overwrite)
                .label("Overwrite")
                .size(16)
                .spacing(5)
                .text_size(CONTROL_TEXT_SIZE)
                .on_toggle(Message::OverwriteChanged),
            native_button("Apply mode").on_press(Message::ApplyBulkMode),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .wrap()
        .vertical_spacing(4);

        let bulk_actions: Element<'_, Message> = if let Some((target, level)) = self.confirm_set {
            let level_name = match level {
                Level::High => "HIGH",
                Level::Low => "LOW",
            };
            row![
                text(format!("Set every output in {target} {level_name}?")),
                danger_native_button(format!("Set {level_name}")).on_press(Message::BulkSetConfirm),
                native_button("Cancel").on_press(Message::BulkSetCancel),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .wrap()
            .vertical_spacing(4)
            .into()
        } else {
            row![
                native_button("Read").on_press(Message::BulkRead),
                native_button("Listen").on_press(Message::BulkListen(true)),
                native_button("Stop listening").on_press(Message::BulkListen(false)),
                native_button("Set HIGH").on_press(Message::BulkSet(Level::High)),
                native_button("Set LOW").on_press(Message::BulkSet(Level::Low)),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .wrap()
            .vertical_spacing(4)
            .into()
        };
        let bulk = column![bulk_scope, bulk_actions].spacing(4);

        let table: Element<'_, Message> = match self.bank_tab {
            BankTab::A => self.full_bank_table(Port::A),
            BankTab::C => self.full_bank_table(Port::C),
            BankTab::D => self.full_bank_table(Port::D),
            BankTab::BAndE => responsive(|size| {
                responsive_pin_columns(
                    size.width,
                    self.port_column(Port::B, 0, Port::B.pin_count(), true),
                    self.port_column(Port::E, 0, Port::E.pin_count(), true),
                )
            })
            .height(Length::Fill)
            .into(),
        };

        let content = column![tabs, bulk, table].spacing(6);

        container(content)
            .padding(8)
            .style(panel_style)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn full_bank_table(&self, port: Port) -> Element<'_, Message> {
        responsive(move |size| {
            responsive_pin_columns(
                size.width,
                self.port_column(port, 0, 16, false),
                self.port_column(port, 16, 16, false),
            )
        })
        .height(Length::Fill)
        .into()
    }

    fn port_column(
        &self,
        port: Port,
        start_bit: u8,
        count: u8,
        show_bank_name: bool,
    ) -> iced::widget::Column<'_, Message> {
        let mut column = column![].spacing(2);
        if show_bank_name {
            column = column.push(text(port.to_string()).size(14));
        }
        column = column.push(pin_header());
        for pin in port
            .pins()
            .skip(usize::from(start_bit))
            .take(usize::from(count))
        {
            column = column.push(self.pin_row(pin));
        }
        column
    }

    fn pin_row(&self, pin: Pin) -> Element<'_, Message> {
        let name = pin_cell(text(pin_display(pin)).size(12), PIN_NAME_SHARE);
        if !pin.is_available() {
            return row![
                name,
                pin_cell(text("RESERVED").size(11), PIN_MODE_SHARE),
                level_box(None, false),
                pin_cell(text("System").size(11), PIN_RW_SHARE),
                pin_cell(text(""), PIN_LISTEN_SHARE),
            ]
            .spacing(CELL_GAP)
            .width(Length::Fill)
            .height(Length::Fixed(ROW_HEIGHT))
            .align_y(iced::Alignment::Center)
            .into();
        }

        let state = self.pins[pin.index() as usize];
        let mode: Element<'_, Message> = if state.target_mode.is_some() {
            container(
                text(
                    state
                        .mode
                        .map(|mode| mode.to_string())
                        .unwrap_or_else(|| "UNSET".into()),
                )
                .size(PIN_CONTROL_TEXT_SIZE)
                .wrapping(text::Wrapping::None),
            )
            .padding([5, 10])
            .width(Length::Fill)
            .style(input_style)
            .into()
        } else {
            pick_list(MODES, state.mode, move |mode| {
                Message::ModeSelected(pin, mode)
            })
            .placeholder("UNSET")
            .text_size(PIN_CONTROL_TEXT_SIZE)
            .padding([5, 8])
            .width(Length::Fill)
            .into()
        };

        let rw: Element<'_, Message> = if state.mode.is_some_and(Mode::is_input) {
            native_button("Read")
                .on_press_maybe((!state.value_pending).then_some(Message::Read(pin)))
                .into()
        } else if state.mode == Some(Mode::Output) {
            let label = if state.level == Some(Level::High) {
                "Write LOW"
            } else {
                "Write HIGH"
            };
            native_button(label)
                .on_press_maybe((!state.value_pending).then_some(Message::Write(pin)))
                .into()
        } else {
            text("").into()
        };

        let listen: Element<'_, Message> = if state.mode.is_some_and(Mode::is_input) {
            let label = if matches!(state.listener, ListenerState::On | ListenerState::Disabling) {
                "Stop"
            } else {
                "Listen"
            };
            native_button(label)
                .on_press_maybe((!state.listener.is_pending()).then_some(Message::Listen(pin)))
                .into()
        } else {
            text("").into()
        };

        row![
            name,
            pin_cell(mode, PIN_MODE_SHARE),
            level_box(state.level, state.value_pending),
            pin_cell(rw, PIN_RW_SHARE),
            pin_cell(listen, PIN_LISTEN_SHARE),
        ]
        .spacing(CELL_GAP)
        .width(Length::Fill)
        .height(Length::Fixed(ROW_HEIGHT))
        .align_y(iced::Alignment::Center)
        .into()
    }

    fn log_panel(&self) -> Element<'_, Message> {
        let log = scrollable(
            text_editor(self.log.content())
                .font(iced::Font::MONOSPACE)
                .size(12)
                .padding(8)
                .on_action(Message::LogAction),
        )
        .id(self.log_scroll.clone())
        .height(Length::Fill)
        .width(Length::Fill);
        let options = column![
            text("Serial Log").size(18),
            row![
                native_button("Clear log").on_press(Message::ClearLog),
                checkbox(self.log.show_timestamps())
                    .label("Timestamps")
                    .size(16)
                    .spacing(5)
                    .text_size(CONTROL_TEXT_SIZE)
                    .on_toggle(Message::ShowTimestamps),
                checkbox(self.autoscroll)
                    .label("Auto-scroll")
                    .size(16)
                    .spacing(5)
                    .text_size(CONTROL_TEXT_SIZE)
                    .on_toggle(Message::Autoscroll),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .wrap()
            .vertical_spacing(4),
        ]
        .spacing(6);
        let command = row![
            text_input("Enter a command…", &self.raw_input)
                .id(self.raw_input_id.clone())
                .font(iced::Font::MONOSPACE)
                .on_input(Message::RawChanged)
                .on_submit(Message::RawSend),
            native_button("Send command").on_press(Message::RawSend),
        ]
        .spacing(5);

        container(column![options, log, command].spacing(8))
            .padding(12)
            .style(container::bordered_box)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn change_mode(&mut self, pin: Pin, mode: Mode) -> Task<Message> {
        if !self.require_connection() || !pin.is_available() {
            return Task::none();
        }
        let state = &mut self.pins[pin.index() as usize];
        if state.target_mode.is_some() || state.listener.is_pending() {
            return Task::none();
        }

        let mut tasks = Vec::new();
        if mode == Mode::Output && state.listener == ListenerState::On {
            state.listener = ListenerState::Disabling;
            tasks.push(self.send_request(Request::Listen {
                target: PinTarget::Pin(pin),
                enabled: false,
            }));
        }

        let state = &mut self.pins[pin.index() as usize];
        state.target_mode = Some(mode);
        state.level = None;
        tasks.push(self.send_request(Request::Direction {
            target: PinTarget::Pin(pin),
            direction: mode.direction(),
        }));
        Task::batch(tasks)
    }

    fn read_pin(&mut self, pin: Pin) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        let state = &mut self.pins[pin.index() as usize];
        if state.mode.is_none() || state.value_pending {
            return Task::none();
        }
        state.value_pending = true;
        self.send_request(Request::Get {
            target: PinTarget::Pin(pin),
        })
    }

    fn write_pin(&mut self, pin: Pin) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        let state = &mut self.pins[pin.index() as usize];
        if state.mode != Some(Mode::Output) || state.value_pending {
            return Task::none();
        }
        state.value_pending = true;
        let level = if state.level == Some(Level::High) {
            Level::Low
        } else {
            Level::High
        };
        self.send_request(Request::Set {
            target: PinTarget::Pin(pin),
            level,
        })
    }

    fn toggle_listener(&mut self, pin: Pin) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        let state = &mut self.pins[pin.index() as usize];
        if !state.mode.is_some_and(Mode::is_input) {
            return Task::none();
        }
        let (enabled, pending) = match state.listener {
            ListenerState::Off => (true, ListenerState::Enabling),
            ListenerState::On => (false, ListenerState::Disabling),
            ListenerState::Enabling | ListenerState::Disabling => return Task::none(),
        };
        state.listener = pending;
        self.send_request(Request::Listen {
            target: PinTarget::Pin(pin),
            enabled,
        })
    }

    fn apply_bulk_mode(&mut self) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        let target = self.bulk_scope;
        let mode = self.bulk_mode;

        if self.overwrite {
            if self.target_has_pending(target) {
                return Task::none();
            }
            let mut tasks = Vec::new();
            if mode == Mode::Output && self.target_has_listener(target) {
                self.mark_listener_pending(target, false);
                tasks.push(self.send_request(Request::Listen {
                    target,
                    enabled: false,
                }));
            }
            self.mark_mode_pending(target, mode);
            tasks.push(self.send_request(Request::Direction {
                target,
                direction: mode.direction(),
            }));
            return Task::batch(tasks);
        }

        let mut tasks = Vec::new();
        for (index, pin) in Pin::all().enumerate() {
            let state = self.pins[index];
            if target.contains(pin)
                && pin.is_available()
                && state.mode.is_none()
                && state.target_mode.is_none()
            {
                self.pins[index].target_mode = Some(mode);
                self.pins[index].level = None;
                tasks.push(self.send_request(Request::Direction {
                    target: PinTarget::Pin(pin),
                    direction: mode.direction(),
                }));
            }
        }
        if tasks.is_empty() {
            self.device_status = "No UNSET pins in selected scope".into();
        }
        Task::batch(tasks)
    }

    fn read_scope(&mut self, target: PinTarget) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        for (index, pin) in Pin::all().enumerate() {
            let state = &mut self.pins[index];
            if target.contains(pin) && pin.is_available() && state.mode.is_some() {
                state.value_pending = true;
            }
        }
        self.send_request(Request::Get { target })
    }

    fn set_listener_scope(&mut self, target: PinTarget, enabled: bool) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        self.mark_listener_pending(target, enabled);
        self.send_request(Request::Listen { target, enabled })
    }

    fn set_scope_level(&mut self, target: PinTarget, level: Level) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        for (index, pin) in Pin::all().enumerate() {
            let state = &mut self.pins[index];
            if target.contains(pin) && state.mode == Some(Mode::Output) {
                state.value_pending = true;
            }
        }
        self.send_request(Request::Set { target, level })
    }

    fn target_has_pending(&self, target: PinTarget) -> bool {
        Pin::all().enumerate().any(|(index, pin)| {
            target.contains(pin)
                && (self.pins[index].target_mode.is_some()
                    || self.pins[index].listener.is_pending())
        })
    }

    fn target_has_listener(&self, target: PinTarget) -> bool {
        Pin::all().enumerate().any(|(index, pin)| {
            target.contains(pin) && self.pins[index].listener == ListenerState::On
        })
    }

    fn mark_mode_pending(&mut self, target: PinTarget, mode: Mode) {
        for (index, pin) in Pin::all().enumerate() {
            if target.contains(pin) && pin.is_available() {
                let state = &mut self.pins[index];
                state.target_mode = Some(mode);
                state.level = None;
            }
        }
    }

    fn mark_listener_pending(&mut self, target: PinTarget, enabled: bool) {
        for (index, pin) in Pin::all().enumerate() {
            let state = &mut self.pins[index];
            if target.contains(pin) && state.mode.is_some_and(Mode::is_input) {
                state.listener = if enabled {
                    ListenerState::Enabling
                } else {
                    ListenerState::Disabling
                };
            }
        }
    }

    fn send_request(&mut self, request: Request) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        match self.connection.send(request) {
            Ok(line) => {
                self.push_log(format!("TX {line}"));
                Task::none()
            }
            Err(error) => {
                self.fail_request(request);
                self.error = Some(error);
                Task::none()
            }
        }
    }

    fn send_raw(&mut self) -> Task<Message> {
        if self.raw_input.is_empty() {
            return Task::none();
        }
        if !self.require_connection() {
            return Task::none();
        }

        let line = std::mem::take(&mut self.raw_input);
        self.command_history.push_back(line.clone());
        if self.command_history.len() > MAX_COMMAND_HISTORY {
            self.command_history.pop_front();
        }
        self.history_index = None;
        match self.connection.send_raw(&line) {
            Ok(()) => {
                self.push_log(format!("TX {line}"));
                Task::none()
            }
            Err(error) => {
                self.error = Some(error);
                Task::none()
            }
        }
    }

    fn require_connection(&mut self) -> bool {
        if self.connected_port.is_some() {
            true
        } else {
            self.error = Some("No serial device connected".into());
            false
        }
    }

    fn drain_io(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        for _ in 0..MAX_IO_EVENTS_PER_TICK {
            let Some(event) = self.connection.next_event() else {
                break;
            };
            match event {
                ConnectionEvent::Connected(port) => {
                    self.connected_port = Some(port.clone());
                    self.device_status = "Connected".into();
                    self.error = None;
                    self.reset_pins();
                }
                ConnectionEvent::Disconnected(reason) => {
                    self.connected_port = None;
                    self.device_status = "Disconnected".into();
                    self.reset_pending();
                    self.error = reason;
                }
                ConnectionEvent::Received { line, event } => {
                    self.push_log(format!("RX {line}"));
                    match event {
                        Ok(event) => self.handle_device_event(event, &mut tasks),
                        Err(error) => self.error = Some(error),
                    }
                }
                ConnectionEvent::IoError(error) => self.error = Some(error),
            }
        }
        Task::batch(tasks)
    }

    fn handle_device_event(&mut self, event: DeviceEvent, tasks: &mut Vec<Task<Message>>) {
        match event {
            DeviceEvent::Hello => self.device_status = "SAM4E8E replied HII".into(),
            DeviceEvent::Status => self.device_status = "SAM4E8E GPIO".into(),
            DeviceEvent::Ack(request) => match request {
                Request::Direction { target, .. } => {
                    let Some(mode) = self.pending_mode(target) else {
                        return;
                    };
                    tasks.push(self.send_request(Request::Pullup {
                        target,
                        enabled: mode == Mode::InputPullup,
                    }));
                }
                Request::Pullup { target, .. } => {
                    let mut read = false;
                    for (index, pin) in Pin::all().enumerate() {
                        if !target.contains(pin) {
                            continue;
                        }
                        let state = &mut self.pins[index];
                        if let Some(mode) = state.target_mode.take() {
                            state.mode = Some(mode);
                            if mode.is_input() {
                                read = true;
                            } else {
                                state.level = Some(Level::Low);
                            }
                        }
                    }
                    if read {
                        tasks.push(match target {
                            PinTarget::Pin(pin) => self.read_pin(pin),
                            PinTarget::Bank(_) | PinTarget::All => self.read_scope(target),
                        });
                    }
                }
                Request::Set { target, level } => {
                    for (index, pin) in Pin::all().enumerate() {
                        if target.contains(pin) && self.pins[index].mode == Some(Mode::Output) {
                            let state = &mut self.pins[index];
                            state.level = Some(level);
                            state.value_pending = false;
                        }
                    }
                }
                Request::Listen { target, enabled } => {
                    for (index, pin) in Pin::all().enumerate() {
                        if target.contains(pin) && self.pins[index].mode.is_some_and(Mode::is_input)
                        {
                            self.pins[index].listener = if enabled {
                                ListenerState::On
                            } else {
                                ListenerState::Off
                            };
                        }
                    }
                }
                _ => {}
            },
            DeviceEvent::PinValue { pin, level } => {
                let state = &mut self.pins[pin.index() as usize];
                state.level = Some(level);
                state.value_pending = false;
            }
            DeviceEvent::PinState { pin, what, value } => {
                self.device_status = format!("{} {what:?}: {value:?}", pin_display(pin));
            }
            DeviceEvent::DeviceError { request, error } => {
                self.fail_request(request);
                self.error = Some(match error {
                    ResponseError::BadPacket => "Device rejected a malformed packet".into(),
                    ResponseError::Pin { pin, reason } => {
                        format!("{}: {reason:?}", pin_display(pin))
                    }
                });
            }
            DeviceEvent::Unknown { request } => {
                self.fail_request(request);
                self.error = Some("Device returned IDK".into());
            }
            DeviceEvent::Bye => {
                self.reset_pins();
                self.device_status = "Device reset acknowledged".into();
            }
            DeviceEvent::Untracked => {}
        }
    }

    fn pending_mode(&self, target: PinTarget) -> Option<Mode> {
        Pin::all().enumerate().find_map(|(index, pin)| {
            target
                .contains(pin)
                .then_some(self.pins[index].target_mode)
                .flatten()
        })
    }

    fn fail_request(&mut self, request: Request) {
        match request {
            Request::Direction { target, .. } | Request::Pullup { target, .. } => {
                for (index, pin) in Pin::all().enumerate() {
                    if target.contains(pin) {
                        self.pins[index].target_mode = None;
                    }
                }
            }
            Request::Get { target } | Request::Set { target, .. } => {
                for (index, pin) in Pin::all().enumerate() {
                    if target.contains(pin) {
                        self.pins[index].value_pending = false;
                    }
                }
            }
            Request::Listen { target, enabled } => {
                for (index, pin) in Pin::all().enumerate() {
                    if target.contains(pin) && self.pins[index].mode.is_some() {
                        self.pins[index].listener = if enabled {
                            ListenerState::Off
                        } else {
                            ListenerState::On
                        };
                    }
                }
            }
            _ => {}
        }
    }

    fn reset_pins(&mut self) {
        self.pins.fill(PinState::UNSET);
    }

    fn reset_pending(&mut self) {
        for pin in &mut self.pins {
            pin.value_pending = false;
            pin.target_mode = None;
            pin.listener = ListenerState::Off;
        }
    }

    fn push_log(&mut self, text: String) {
        self.log.push(text);
    }

    fn snap_log(&self) -> Task<Message> {
        if self.autoscroll {
            iced::widget::operation::snap_to_end(self.log_scroll.clone())
        } else {
            Task::none()
        }
    }

    fn history_previous(&mut self) {
        if self.command_history.is_empty() {
            return;
        }
        let index = self
            .history_index
            .map_or(self.command_history.len() - 1, |index| {
                index.saturating_sub(1)
            });
        self.history_index = Some(index);
        self.raw_input.clone_from(&self.command_history[index]);
    }

    fn history_next(&mut self) {
        let Some(index) = self.history_index else {
            return;
        };
        if index + 1 < self.command_history.len() {
            self.history_index = Some(index + 1);
            self.raw_input.clone_from(&self.command_history[index + 1]);
        } else {
            self.history_index = None;
            self.raw_input.clear();
        }
    }
}

fn pin_header<'a>() -> Element<'a, Message> {
    row![
        pin_cell(text("PIN").size(11), PIN_NAME_SHARE),
        pin_cell(text("MODE").size(11), PIN_MODE_SHARE),
        pin_cell(text("LEVEL").size(11), PIN_STATUS_SHARE),
        pin_cell(text("READ/WRITE").size(10), PIN_RW_SHARE),
        pin_cell(text("LISTEN/STOP").size(10), PIN_LISTEN_SHARE),
    ]
    .spacing(CELL_GAP)
    .width(Length::Fill)
    .into()
}

fn responsive_pin_columns<'a>(
    available_width: f32,
    left: iced::widget::Column<'a, Message>,
    right: iced::widget::Column<'a, Message>,
) -> Element<'a, Message> {
    let content: Element<'a, Message> = if available_width >= PIN_TABLE_TWO_COLUMN_MIN {
        row![
            left.width(Length::FillPortion(1)),
            right.width(Length::FillPortion(1)),
        ]
        .spacing(8)
        .width(Length::Fill)
        .into()
    } else {
        column![left.width(Length::Fill), right.width(Length::Fill)]
            .spacing(8)
            .width(Length::Fill)
            .into()
    };

    scrollable(content)
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

fn pin_cell<'a>(content: impl Into<Element<'a, Message>>, share: u16) -> Element<'a, Message> {
    container(content)
        .width(Length::FillPortion(share))
        .height(Length::Fixed(ROW_HEIGHT))
        .align_x(Horizontal::Left)
        .align_y(Vertical::Center)
        .into()
}

fn level_box(level: Option<Level>, pending: bool) -> Element<'static, Message> {
    let label = if pending {
        "…"
    } else {
        match level {
            Some(Level::High) => "HIGH",
            Some(Level::Low) => "LOW",
            None => "—",
        }
    };
    let background = if pending {
        UNSET_BG
    } else {
        match level {
            Some(Level::High) => HIGH_BG,
            Some(Level::Low) => LOW_BG,
            None => UNSET_BG,
        }
    };
    container(text(label).size(11))
        .width(Length::FillPortion(PIN_STATUS_SHARE))
        .height(Length::Fixed(28.0))
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(background)),
            text_color: Some(UI_TEXT),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn pin_display(pin: Pin) -> String {
    format!(
        "P{}{} ({})",
        pin.port().letter(),
        pin.bit(),
        pin.package_pin()
    )
}

fn panel_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL_BG)),
        border: Border {
            color: UI_BORDER,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn input_style(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(INPUT_BG)),
        text_color: Some(UI_TEXT),
        border: Border {
            color: UI_BORDER,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn native_button<'a>(label: impl text::IntoFragment<'a>) -> iced::widget::Button<'a, Message> {
    button(
        text(label)
            .size(CONTROL_TEXT_SIZE)
            .wrapping(text::Wrapping::None),
    )
    .padding([5, 10])
    .style(neutral_button)
}

fn danger_native_button<'a>(
    label: impl text::IntoFragment<'a>,
) -> iced::widget::Button<'a, Message> {
    native_button(label).style(danger_button)
}

fn neutral_button(_: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Hovered => (RAISED_HOVER, UI_TEXT),
        button::Status::Disabled => (RAISED_BG, MUTED),
        button::Status::Active | button::Status::Pressed => (RAISED_BG, UI_TEXT),
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color,
        border: Border {
            color: UI_BORDER,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn selected_tab_button(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = neutral_button(theme, status);
    style.background = Some(Background::Color(if status == button::Status::Hovered {
        Color::from_rgb8(0x50, 0x50, 0x50)
    } else {
        Color::from_rgb8(0x46, 0x46, 0x46)
    }));
    style
}

fn danger_button(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = neutral_button(theme, status);
    style.text_color = if status == button::Status::Disabled {
        MUTED
    } else {
        DANGER
    };
    style
}

fn load_ports() -> Task<Message> {
    Task::perform(
        async { Connection::available_ports() },
        Message::PortsLoaded,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connected_app() -> App {
        let (mut app, _) = App::new();
        app.connected_port = Some("test".into());
        app
    }

    fn last_log(app: &App) -> &str {
        app.log.last_text().unwrap()
    }

    #[test]
    fn bulk_controls_send_selected_symbolic_scope() {
        let mut app = connected_app();
        let mut tasks = Vec::new();
        app.bulk_scope = PinTarget::Bank(Port::C);
        app.bulk_mode = Mode::InputPullup;
        app.overwrite = true;

        let _ = app.apply_bulk_mode();
        assert_eq!(last_log(&app), "TX 001 DIR PIOC IN OK?");

        app.handle_device_event(
            DeviceEvent::Ack(Request::Direction {
                target: PinTarget::Bank(Port::C),
                direction: Direction::Input,
            }),
            &mut tasks,
        );
        assert_eq!(last_log(&app), "TX 002 PLL PIOC ON OK?");

        app.handle_device_event(
            DeviceEvent::Ack(Request::Pullup {
                target: PinTarget::Bank(Port::C),
                enabled: true,
            }),
            &mut tasks,
        );
        assert_eq!(last_log(&app), "TX 003 GET PIOC OK?");

        let _ = app.set_listener_scope(PinTarget::Bank(Port::C), true);
        assert_eq!(last_log(&app), "TX 004 LSN PIOC ON OK?");

        let _ = app.read_scope(PinTarget::Bank(Port::C));
        assert_eq!(last_log(&app), "TX 005 GET PIOC OK?");
    }

    #[test]
    fn bulk_set_waits_for_confirmation() {
        let mut app = connected_app();

        let _ = app.update(Message::BulkSet(Level::High));
        assert!(app.log.is_empty());
        assert_eq!(app.confirm_set, Some((PinTarget::All, Level::High)));

        let _ = app.update(Message::BulkSetConfirm);
        assert_eq!(last_log(&app), "TX 001 SET ALL HIGH OK?");
    }

    #[test]
    fn tab_selection_does_not_change_bulk_scope() {
        let mut app = connected_app();
        app.bulk_scope = PinTarget::Bank(Port::C);

        let _ = app.update(Message::TabSelected(BankTab::D));

        assert_eq!(app.bank_tab, BankTab::D);
        assert_eq!(app.bulk_scope, PinTarget::Bank(Port::C));
    }

    #[test]
    fn overwrite_off_only_targets_unset_pins_individually() {
        let mut app = connected_app();
        let configured = Pin::try_from((Port::A, 0)).unwrap();
        app.pins[configured.index() as usize].mode = Some(Mode::Input);
        app.bulk_scope = PinTarget::Bank(Port::A);
        app.bulk_mode = Mode::Output;
        app.overwrite = false;

        let _ = app.apply_bulk_mode();

        assert!(!app.log.iter().any(|entry| entry.contains("DIR PA00")));
        assert!(app.log.iter().all(|entry| !entry.contains("DIR PIOA")));
        assert!(app.log.iter().any(|entry| entry.contains("DIR PA01 OUT")));
    }
}
