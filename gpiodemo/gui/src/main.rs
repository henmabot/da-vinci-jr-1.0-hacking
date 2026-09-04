mod connection;

use std::{collections::VecDeque, fmt, time::Duration};

use chrono::Local;
use connection::{Connection, DeviceEvent, Event as ConnectionEvent};
use da_vinci_protocol::{Direction, Level, Request, ResponseError, WIRE_PIN_COUNT};
use iced::{
    Element, Length, Subscription, Task,
    keyboard::{Event as KeyboardEvent, Key, key::Named},
    widget::{button, checkbox, column, container, pick_list, row, scrollable, text, text_input},
};

const PINS_PER_PAGE: usize = 36;
const PINS_PER_COLUMN: usize = 18;
const MAX_LOG_LINES: usize = 2_000;
const MAX_COMMAND_HISTORY: usize = 200;
const MODES: [Mode; 3] = [Mode::Input, Mode::InputPullup, Mode::Output];

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("GPIO Controller")
        .subscription(App::subscription)
        .window_size((1250.0, 800.0))
        .run()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Input,
    InputPullup,
    Output,
}

impl Mode {
    fn is_input(self) -> bool {
        matches!(self, Self::Input | Self::InputPullup)
    }
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

#[derive(Clone)]
struct LogEntry {
    timestamp: String,
    text: String,
}

#[derive(Clone, Debug)]
enum Message {
    Tick,
    PortsLoaded(Result<Vec<String>, String>),
    RefreshPorts,
    PortSelected(String),
    Connect,
    Disconnect,
    PreviousPage,
    NextPage,
    ModeSelected(u8, Mode),
    Read(u8),
    Toggle(u8),
    Listen(u8),
    ReadAll,
    ListenAll,
    Handshake,
    Status,
    Reboot,
    RebootConfirm,
    RebootCancel,
    ClearLog,
    ShowTimestamps(bool),
    Autoscroll(bool),
    LogScrolled(f32),
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
    page: usize,
    ports: Vec<String>,
    selected_port: Option<String>,
    connected_port: Option<String>,
    connection: Connection,
    logs: VecDeque<LogEntry>,
    show_timestamps: bool,
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
        (
            Self {
                pins: [PinState::UNSET; WIRE_PIN_COUNT as usize],
                page: 0,
                ports: Vec::new(),
                selected_port: None,
                connected_port: None,
                connection: Connection::spawn(),
                logs: VecDeque::new(),
                show_timestamps: true,
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
        match message {
            Message::Tick => self.drain_io(),
            Message::PortsLoaded(result) => {
                match result {
                    Ok(ports) => {
                        self.ports = ports;
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
                if let Some(port) = self.selected_port.clone() {
                    self.error = self.connection.connect(port).err();
                }
                Task::none()
            }
            Message::Disconnect => {
                self.error = self.connection.disconnect().err();
                Task::none()
            }
            Message::PreviousPage => {
                self.page = self.page.saturating_sub(1);
                Task::none()
            }
            Message::NextPage => {
                if self.page + 1 < page_count() {
                    self.page += 1;
                }
                Task::none()
            }
            Message::ModeSelected(pin, mode) => self.change_mode(pin, mode),
            Message::Read(pin) => self.read_pin(pin),
            Message::Toggle(pin) => self.toggle_pin(pin),
            Message::Listen(pin) => self.toggle_listener(pin),
            Message::ReadAll => self.read_all(),
            Message::ListenAll => self.listen_all(),
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
            Message::ClearLog => {
                self.logs.clear();
                Task::none()
            }
            Message::ShowTimestamps(enabled) => {
                self.show_timestamps = enabled;
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
            Message::LogScrolled(offset) => {
                if self.autoscroll && offset < 0.999 {
                    self.autoscroll = false;
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
        let pins = self.pin_panel();
        let logs = self.log_panel();
        let body = row![pins, logs].spacing(12).height(Length::Fill);

        let mut content = column![top, body].spacing(10).padding(10);
        if let Some(error) = &self.error {
            content = content.push(text(format!("Error: {error}")));
        }
        container(content)
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    }

    fn connection_controls(&self) -> Element<'_, Message> {
        let ports = pick_list(
            self.ports.as_slice(),
            self.selected_port.as_ref(),
            Message::PortSelected,
        )
        .placeholder("Serial port");

        let connection_button = if self.connected_port.is_some() {
            button("Disconnect").on_press(Message::Disconnect)
        } else {
            button("Connect").on_press_maybe(self.selected_port.as_ref().map(|_| Message::Connect))
        };

        let controls = row![
            ports,
            button("Refresh").on_press(Message::RefreshPorts),
            connection_button,
            text(&self.device_status),
            button("Read All").on_press(Message::ReadAll),
            button("Listen All").on_press(Message::ListenAll),
            button("Handshake").on_press(Message::Handshake),
            button("Status").on_press(Message::Status),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        if self.confirm_reboot {
            column![
                controls,
                row![
                    text("Send BYE and reset the device? This will drop the connection."),
                    button("Confirm reboot").on_press(Message::RebootConfirm),
                    button("Cancel").on_press(Message::RebootCancel),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(6)
            .into()
        } else {
            row![controls, button("Reboot").on_press(Message::Reboot)]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into()
        }
    }

    fn pin_panel(&self) -> Element<'_, Message> {
        let start = self.page * PINS_PER_PAGE;
        let end = (start + PINS_PER_PAGE).min(WIRE_PIN_COUNT as usize);
        let middle = (start + PINS_PER_COLUMN).min(end);

        let mut left = column![pin_header()].spacing(3);
        for pin in start..middle {
            left = left.push(self.pin_row(pin as u8));
        }

        let mut right = column![pin_header()].spacing(3);
        for pin in middle..end {
            right = right.push(self.pin_row(pin as u8));
        }

        let pager = row![
            button("Previous").on_press_maybe((self.page > 0).then_some(Message::PreviousPage)),
            text(format!("Page {}/{}", self.page + 1, page_count())),
            button("Next")
                .on_press_maybe((self.page + 1 < page_count()).then_some(Message::NextPage)),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);

        container(
            column![
                row![
                    left.width(Length::FillPortion(1)),
                    right.width(Length::FillPortion(1)),
                ]
                .spacing(12),
                pager,
            ]
            .spacing(8),
        )
        .width(Length::FillPortion(3))
        .height(Length::Fill)
        .into()
    }

    fn pin_row(&self, pin: u8) -> Element<'_, Message> {
        if is_reserved(pin) {
            return row![
                text(format!("{} ({pin:03})", pin_name(pin))).width(Length::Fixed(90.0)),
                text("RESERVED").width(Length::Fixed(100.0)),
                text("--").width(Length::Fixed(55.0)),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into();
        }

        let state = self.pins[pin as usize];
        let mode = pick_list(MODES, state.mode, move |mode| {
            Message::ModeSelected(pin, mode)
        })
        .placeholder("UNSET")
        .width(Length::Fixed(105.0));

        let status = match state.level {
            Some(Level::High) => "HIGH",
            Some(Level::Low) => "LOW",
            None => "--",
        };

        let actions: Element<'_, Message> = if state.mode.is_some_and(Mode::is_input) {
            let read = button(if state.value_pending {
                "Reading..."
            } else {
                "Read"
            })
            .on_press_maybe((!state.value_pending).then_some(Message::Read(pin)));
            let listen_label = match state.listener {
                ListenerState::Enabling | ListenerState::Disabling => "Sending...",
                ListenerState::On => "Listening",
                ListenerState::Off => "Listen",
            };
            let listen = button(listen_label)
                .on_press_maybe((!state.listener.is_pending()).then_some(Message::Listen(pin)));
            row![read, listen].spacing(4).into()
        } else if state.mode == Some(Mode::Output) {
            button(if state.value_pending {
                "Sending..."
            } else {
                "Toggle"
            })
            .on_press_maybe((!state.value_pending).then_some(Message::Toggle(pin)))
            .into()
        } else {
            text(if state.target_mode.is_some() {
                "Setting..."
            } else {
                ""
            })
            .into()
        };

        row![
            text(format!("{} ({pin:03})", pin_name(pin))).width(Length::Fixed(90.0)),
            mode,
            text(status).width(Length::Fixed(55.0)),
            actions,
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
    }

    fn log_panel(&self) -> Element<'_, Message> {
        let mut lines = String::new();
        for entry in &self.logs {
            if self.show_timestamps {
                lines.push('[');
                lines.push_str(&entry.timestamp);
                lines.push_str("] ");
            }
            lines.push_str(&entry.text);
            lines.push('\n');
        }

        let log = scrollable(text(lines).size(13))
            .id(self.log_scroll.clone())
            .on_scroll(|viewport| Message::LogScrolled(viewport.relative_offset().y))
            .height(Length::Fill)
            .width(Length::Fill);
        let options = row![
            button("Clear").on_press(Message::ClearLog),
            checkbox(self.show_timestamps)
                .label("Timestamps")
                .on_toggle(Message::ShowTimestamps),
            checkbox(self.autoscroll)
                .label("Auto-scroll")
                .on_toggle(Message::Autoscroll),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let command = row![
            text_input("Enter a command...", &self.raw_input)
                .id(self.raw_input_id.clone())
                .on_input(Message::RawChanged)
                .on_submit(Message::RawSend),
            button("Send").on_press(Message::RawSend),
        ]
        .spacing(5);

        container(column![options, log, command].spacing(6))
            .width(Length::FillPortion(2))
            .height(Length::Fill)
            .into()
    }

    fn change_mode(&mut self, pin: u8, mode: Mode) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        let state = &mut self.pins[pin as usize];
        if state.target_mode.is_some() || state.listener.is_pending() || is_reserved(pin) {
            return Task::none();
        }

        if !mode.is_input() && state.listener == ListenerState::On {
            state.listener = ListenerState::Disabling;
            let _ = self.send_request(Request::Listen {
                pin,
                enabled: false,
            });
        }

        let state = &mut self.pins[pin as usize];
        state.target_mode = Some(mode);
        state.level = None;
        self.send_request(Request::Direction {
            pin,
            direction: if mode == Mode::Output {
                Direction::Output
            } else {
                Direction::Input
            },
        })
    }

    fn read_pin(&mut self, pin: u8) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        let state = &mut self.pins[pin as usize];
        if !state.mode.is_some_and(Mode::is_input) || state.value_pending {
            return Task::none();
        }
        state.value_pending = true;
        self.send_request(Request::Get { pin })
    }

    fn toggle_pin(&mut self, pin: u8) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        let state = &mut self.pins[pin as usize];
        if state.mode != Some(Mode::Output) || state.value_pending {
            return Task::none();
        }
        state.value_pending = true;
        let level = if state.level == Some(Level::High) {
            Level::Low
        } else {
            Level::High
        };
        self.send_request(Request::Set { pin, level })
    }

    fn toggle_listener(&mut self, pin: u8) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        let state = &mut self.pins[pin as usize];
        if !state.mode.is_some_and(Mode::is_input) {
            return Task::none();
        }
        let (enabled, pending) = match state.listener {
            ListenerState::Off => (true, ListenerState::Enabling),
            ListenerState::On => (false, ListenerState::Disabling),
            ListenerState::Enabling | ListenerState::Disabling => return Task::none(),
        };
        state.listener = pending;
        self.send_request(Request::Listen { pin, enabled })
    }

    fn read_all(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        for pin in 0..WIRE_PIN_COUNT {
            if self.pins[pin as usize].mode.is_some_and(Mode::is_input)
                && !self.pins[pin as usize].value_pending
            {
                tasks.push(self.read_pin(pin));
            }
        }
        Task::batch(tasks)
    }

    fn listen_all(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        for pin in 0..WIRE_PIN_COUNT {
            let state = self.pins[pin as usize];
            if state.mode.is_some_and(Mode::is_input) && state.listener == ListenerState::Off {
                tasks.push(self.toggle_listener(pin));
            }
        }
        Task::batch(tasks)
    }

    fn send_request(&mut self, request: Request) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        match self.connection.send(request) {
            Ok(line) => self.push_log(format!("TX {line}")),
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
            Ok(()) => self.push_log(format!("TX {line}")),
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
        while let Some(event) = self.connection.next_event() {
            match event {
                ConnectionEvent::Connected(port) => {
                    self.connected_port = Some(port.clone());
                    self.device_status = format!("Connected: {port}");
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
                    tasks.push(self.push_log(format!("RX {line}")));
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
                Request::Direction { pin, .. } => {
                    let target = self.pins[pin as usize]
                        .target_mode
                        .expect("direction ACK requires a pending mode change");
                    tasks.push(self.send_request(Request::Pullup {
                        pin,
                        enabled: target == Mode::InputPullup,
                    }));
                }
                Request::Pullup { pin, .. } => {
                    let state = &mut self.pins[pin as usize];
                    state.mode = Some(
                        state
                            .target_mode
                            .take()
                            .expect("pull-up ACK requires a pending mode change"),
                    );
                    if state.mode.is_some_and(Mode::is_input) {
                        tasks.push(self.read_pin(pin));
                    }
                }
                Request::Set { pin, level } => {
                    let state = &mut self.pins[pin as usize];
                    state.level = Some(level);
                    state.value_pending = false;
                }
                Request::Listen { pin, enabled } => {
                    self.pins[pin as usize].listener = if enabled {
                        ListenerState::On
                    } else {
                        ListenerState::Off
                    };
                }
                _ => {}
            },
            DeviceEvent::PinValue { pin, level } => {
                let state = &mut self.pins[pin as usize];
                state.level = Some(level);
                state.value_pending = false;
            }
            DeviceEvent::PinState { pin, what, value } => {
                self.device_status = format!("{} {what:?}: {value:?}", pin_name(pin));
            }
            DeviceEvent::DeviceError { request, error } => {
                self.fail_request(request);
                self.error = Some(match error {
                    ResponseError::BadPacket => "Device rejected a malformed packet".into(),
                    ResponseError::Pin { pin, reason } => {
                        format!("{}: {reason:?}", pin_name(pin))
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

    fn fail_request(&mut self, request: Request) {
        match request {
            Request::Direction { pin, .. } | Request::Pullup { pin, .. } => {
                let state = &mut self.pins[pin as usize];
                state.target_mode = None;
            }
            Request::Get { pin } | Request::Set { pin, .. } => {
                self.pins[pin as usize].value_pending = false;
            }
            Request::Listen { pin, enabled } => {
                self.pins[pin as usize].listener = if enabled {
                    ListenerState::Off
                } else {
                    ListenerState::On
                };
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

    fn push_log(&mut self, text: String) -> Task<Message> {
        self.logs.push_back(LogEntry {
            timestamp: Local::now().format("%H:%M:%S%.3f").to_string(),
            text,
        });
        if self.logs.len() > MAX_LOG_LINES {
            self.logs.pop_front();
        }
        self.snap_log()
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
        text("Pin").width(Length::Fixed(90.0)),
        text("Mode").width(Length::Fixed(105.0)),
        text("Status").width(Length::Fixed(55.0)),
        text("Action"),
    ]
    .spacing(6)
    .into()
}

fn page_count() -> usize {
    (WIRE_PIN_COUNT as usize).div_ceil(PINS_PER_PAGE)
}

fn is_reserved(pin: u8) -> bool {
    matches!(pin, 40..=43)
}

fn pin_name(pin: u8) -> String {
    match pin {
        0..=31 => format!("PA{pin}"),
        32..=46 => format!("PB{}", pin - 32),
        47..=78 => format!("PC{}", pin - 47),
        79..=110 => format!("PD{}", pin - 79),
        111..=116 => format!("PE{}", pin - 111),
        _ => format!("?{pin}"),
    }
}

fn load_ports() -> Task<Message> {
    Task::perform(
        async { Connection::available_ports() },
        Message::PortsLoaded,
    )
}
