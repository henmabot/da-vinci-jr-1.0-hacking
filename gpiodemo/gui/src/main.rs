mod connection;

use std::{
    fmt,
    io::{self, Read, Write},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use connection::{Connection, Event as ConnectionEvent};
use da_vinci_protocol::{
    Direction, Level, Request, ResponseError, WIRE_PIN_COUNT, decode_response,
};
use iced::{
    Element, Length, Subscription, Task,
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
    Unset,
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
            Self::Unset => "UNSET",
            Self::Input => "INPUT",
            Self::InputPullup => "IN_PULLUP",
            Self::Output => "OUTPUT",
        })
    }
}

#[derive(Clone, Copy)]
struct PinState {
    mode: Mode,
    target_mode: Option<Mode>,
    level: Option<Level>,
    listening: bool,
    mode_pending: bool,
    read_pending: bool,
    listen_pending: bool,
    toggle_pending: bool,
}

impl PinState {
    const UNSET: Self = Self {
        mode: Mode::Unset,
        target_mode: None,
        level: None,
        listening: false,
        mode_pending: false,
        read_pending: false,
        listen_pending: false,
        toggle_pending: false,
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
    HistoryPrevious,
    HistoryNext,
}

struct App {
    pins: [PinState; WIRE_PIN_COUNT as usize],
    page: usize,
    ports: Vec<String>,
    selected_port: Option<String>,
    connected_port: Option<String>,
    connection: Connection,
    io: IoHandle,
    logs: Vec<LogEntry>,
    show_timestamps: bool,
    autoscroll: bool,
    log_scroll: iced::widget::Id,
    raw_input: String,
    command_history: Vec<String>,
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
                connection: Connection::new(),
                io: IoHandle::spawn(),
                logs: Vec::new(),
                show_timestamps: true,
                autoscroll: true,
                log_scroll: iced::widget::Id::unique(),
                raw_input: String::new(),
                command_history: Vec::new(),
                history_index: None,
                device_status: "Disconnected".into(),
                error: None,
                confirm_reboot: false,
            },
            load_ports(),
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_millis(40)).map(|_| Message::Tick)
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
                    self.error = None;
                    self.io.send(IoCommand::Connect(port));
                }
                Task::none()
            }
            Message::Disconnect => {
                self.io.send(IoCommand::Disconnect);
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
            Message::HistoryPrevious => {
                self.history_previous();
                Task::none()
            }
            Message::HistoryNext => {
                self.history_next();
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
        let mode = pick_list(
            MODES,
            (state.mode != Mode::Unset).then_some(state.mode),
            move |mode| Message::ModeSelected(pin, mode),
        )
        .placeholder("UNSET")
        .width(Length::Fixed(105.0));

        let status = match state.level {
            Some(Level::High) => "HIGH",
            Some(Level::Low) => "LOW",
            None => "--",
        };

        let actions: Element<'_, Message> = if state.mode.is_input() {
            let read = button(if state.read_pending {
                "Reading..."
            } else {
                "Read"
            })
            .on_press_maybe((!state.read_pending).then_some(Message::Read(pin)));
            let listen_label = if state.listen_pending {
                "Sending..."
            } else if state.listening {
                "Listening"
            } else {
                "Listen"
            };
            let listen = button(listen_label)
                .on_press_maybe((!state.listen_pending).then_some(Message::Listen(pin)));
            row![read, listen].spacing(4).into()
        } else if state.mode == Mode::Output {
            button(if state.toggle_pending {
                "Sending..."
            } else {
                "Toggle"
            })
            .on_press_maybe((!state.toggle_pending).then_some(Message::Toggle(pin)))
            .into()
        } else {
            text(if state.mode_pending { "Setting..." } else { "" }).into()
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
        let mut lines = column![];
        for entry in &self.logs {
            let line = if self.show_timestamps {
                format!("[{}] {}", entry.timestamp, entry.text)
            } else {
                entry.text.clone()
            };
            lines = lines.push(text(line).size(13));
        }

        let log = scrollable(lines.spacing(2))
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
            button("↑").on_press(Message::HistoryPrevious),
            button("↓").on_press(Message::HistoryNext),
            text_input("Enter a command...", &self.raw_input)
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
        if state.mode_pending || is_reserved(pin) {
            return Task::none();
        }

        if !mode.is_input() && state.listening && !state.listen_pending {
            state.listen_pending = true;
            let _ = self.send_request(Request::Listen {
                pin,
                enabled: false,
            });
        }

        let state = &mut self.pins[pin as usize];
        state.mode_pending = true;
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
        if !state.mode.is_input() || state.read_pending {
            return Task::none();
        }
        state.read_pending = true;
        self.send_request(Request::Get { pin })
    }

    fn toggle_pin(&mut self, pin: u8) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        let state = &mut self.pins[pin as usize];
        if state.mode != Mode::Output || state.toggle_pending {
            return Task::none();
        }
        state.toggle_pending = true;
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
        if !state.mode.is_input() || state.listen_pending {
            return Task::none();
        }
        let enabled = !state.listening;
        state.listen_pending = true;
        self.send_request(Request::Listen { pin, enabled })
    }

    fn read_all(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        for pin in 0..WIRE_PIN_COUNT {
            if self.pins[pin as usize].mode.is_input() && !self.pins[pin as usize].read_pending {
                tasks.push(self.read_pin(pin));
            }
        }
        Task::batch(tasks)
    }

    fn listen_all(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        for pin in 0..WIRE_PIN_COUNT {
            let state = self.pins[pin as usize];
            if state.mode.is_input() && !state.listening && !state.listen_pending {
                tasks.push(self.toggle_listener(pin));
            }
        }
        Task::batch(tasks)
    }

    fn send_request(&mut self, request: Request) -> Task<Message> {
        if !self.require_connection() {
            return Task::none();
        }
        let bytes = self.connection.send(request);
        let line = wire_text(&bytes);
        self.io.send(IoCommand::Write(bytes));
        self.push_log(format!("TX {line}"))
    }

    fn send_raw(&mut self) -> Task<Message> {
        if self.raw_input.is_empty() {
            return Task::none();
        }
        if !self.require_connection() {
            return Task::none();
        }

        let line = std::mem::take(&mut self.raw_input);
        self.command_history.push(line.clone());
        if self.command_history.len() > MAX_COMMAND_HISTORY {
            self.command_history.remove(0);
        }
        self.history_index = None;
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(b'\n');
        self.io.send(IoCommand::Write(bytes));
        self.push_log(format!("TX {line}"))
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
        while let Ok(event) = self.io.events.try_recv() {
            match event {
                IoEvent::Connected(port) => {
                    self.connected_port = Some(port.clone());
                    self.device_status = format!("Connected: {port}");
                    self.error = None;
                    self.connection.clear();
                    self.reset_pins();
                }
                IoEvent::Disconnected(reason) => {
                    self.connected_port = None;
                    self.device_status = "Disconnected".into();
                    self.connection.clear();
                    self.reset_pending();
                    self.error = reason;
                }
                IoEvent::Line(line) => {
                    tasks.push(self.push_log(format!("RX {line}")));
                    match decode_response(line.as_bytes()) {
                        Ok(packet) => {
                            let event = self.connection.received(packet);
                            self.handle_connection_event(event, &mut tasks);
                        }
                        Err(error) => {
                            self.error = Some(format!("Malformed response: {error:?}"));
                        }
                    }
                }
                IoEvent::Error(error) => self.error = Some(error),
            }
        }
        Task::batch(tasks)
    }

    fn handle_connection_event(&mut self, event: ConnectionEvent, tasks: &mut Vec<Task<Message>>) {
        match event {
            ConnectionEvent::Hello => self.device_status = "SAM4E8E replied HII".into(),
            ConnectionEvent::Status => self.device_status = "SAM4E8E GPIO".into(),
            ConnectionEvent::Ack(request) => match request {
                Request::Direction { pin, .. } => {
                    let target = self.pins[pin as usize].target_mode;
                    if let Some(target) = target {
                        tasks.push(self.send_request(Request::Pullup {
                            pin,
                            enabled: target == Mode::InputPullup,
                        }));
                    }
                }
                Request::Pullup { pin, .. } => {
                    let state = &mut self.pins[pin as usize];
                    if let Some(mode) = state.target_mode.take() {
                        state.mode = mode;
                    }
                    state.mode_pending = false;
                    if state.mode.is_input() {
                        tasks.push(self.read_pin(pin));
                    }
                }
                Request::Set { pin, level } => {
                    let state = &mut self.pins[pin as usize];
                    state.level = Some(level);
                    state.toggle_pending = false;
                }
                Request::Listen { pin, enabled } => {
                    let state = &mut self.pins[pin as usize];
                    state.listening = enabled;
                    state.listen_pending = false;
                }
                _ => {}
            },
            ConnectionEvent::PinValue { pin, level } => {
                let state = &mut self.pins[pin as usize];
                state.level = Some(level);
                state.read_pending = false;
                state.toggle_pending = false;
            }
            ConnectionEvent::PinState { pin, what, value } => {
                self.device_status = format!("{} {what:?}: {value:?}", pin_name(pin));
            }
            ConnectionEvent::DeviceError { request, error } => {
                if let Some(request) = request {
                    self.fail_request(request);
                }
                self.error = Some(format_device_error(error));
            }
            ConnectionEvent::Unknown { request } => {
                if let Some(request) = request {
                    self.fail_request(request);
                }
                self.error = Some("Device returned IDK".into());
            }
            ConnectionEvent::Bye => {
                self.reset_pins();
                self.device_status = "Device reset acknowledged".into();
            }
            ConnectionEvent::Untracked(_) => {}
        }
    }

    fn fail_request(&mut self, request: Request) {
        match request {
            Request::Direction { pin, .. } | Request::Pullup { pin, .. } => {
                let state = &mut self.pins[pin as usize];
                state.mode_pending = false;
                state.target_mode = None;
            }
            Request::Get { pin } => self.pins[pin as usize].read_pending = false,
            Request::Set { pin, .. } => self.pins[pin as usize].toggle_pending = false,
            Request::Listen { pin, .. } => self.pins[pin as usize].listen_pending = false,
            _ => {}
        }
    }

    fn reset_pins(&mut self) {
        self.pins.fill(PinState::UNSET);
    }

    fn reset_pending(&mut self) {
        for pin in &mut self.pins {
            pin.mode_pending = false;
            pin.read_pending = false;
            pin.listen_pending = false;
            pin.toggle_pending = false;
            pin.target_mode = None;
            pin.listening = false;
        }
    }

    fn push_log(&mut self, text: String) -> Task<Message> {
        self.logs.push(LogEntry {
            timestamp: timestamp(),
            text,
        });
        if self.logs.len() > MAX_LOG_LINES {
            self.logs.remove(0);
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

fn format_device_error(error: ResponseError) -> String {
    match error {
        ResponseError::BadPacket => "Device rejected a malformed packet".into(),
        ResponseError::Pin { pin, reason } => format!("{}: {reason:?}", pin_name(pin)),
    }
}

fn timestamp() -> String {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let millis = elapsed.subsec_millis();
    let seconds = elapsed.as_secs() % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{millis:03}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60,
    )
}

fn wire_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches(['\r', '\n'])
        .to_owned()
}

fn load_ports() -> Task<Message> {
    Task::perform(
        async {
            serialport::available_ports()
                .map(|ports| ports.into_iter().map(|port| port.port_name).collect())
                .map_err(|error| error.to_string())
        },
        Message::PortsLoaded,
    )
}

enum IoCommand {
    Connect(String),
    Disconnect,
    Write(Vec<u8>),
}

enum IoEvent {
    Connected(String),
    Disconnected(Option<String>),
    Line(String),
    Error(String),
}

struct IoHandle {
    commands: Sender<IoCommand>,
    events: Receiver<IoEvent>,
}

impl IoHandle {
    fn spawn() -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        thread::spawn(move || io_worker(command_rx, event_tx));
        Self {
            commands: command_tx,
            events: event_rx,
        }
    }

    fn send(&self, command: IoCommand) {
        let _ = self.commands.send(command);
    }
}

fn io_worker(commands: Receiver<IoCommand>, events: Sender<IoEvent>) {
    let mut port: Option<Box<dyn serialport::SerialPort>> = None;
    let mut line = Vec::with_capacity(128);
    let mut discarding = false;
    let mut buffer = [0u8; 64];

    loop {
        let command = if port.is_some() {
            match commands.try_recv() {
                Ok(command) => Some(command),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        } else {
            match commands.recv() {
                Ok(command) => Some(command),
                Err(_) => return,
            }
        };

        if let Some(command) = command {
            match command {
                IoCommand::Connect(name) => {
                    line.clear();
                    discarding = false;
                    match serialport::new(&name, 115_200)
                        .timeout(Duration::from_millis(20))
                        .open()
                    {
                        Ok(opened) => {
                            port = Some(opened);
                            let _ = events.send(IoEvent::Connected(name));
                        }
                        Err(error) => {
                            port = None;
                            let _ = events
                                .send(IoEvent::Error(format!("Could not open {name}: {error}")));
                        }
                    }
                }
                IoCommand::Disconnect => {
                    port = None;
                    line.clear();
                    discarding = false;
                    let _ = events.send(IoEvent::Disconnected(None));
                }
                IoCommand::Write(bytes) => {
                    let Some(opened) = port.as_mut() else {
                        let _ = events.send(IoEvent::Error("Serial device is disconnected".into()));
                        continue;
                    };
                    if let Err(error) = opened.write_all(&bytes) {
                        port = None;
                        let _ = events.send(IoEvent::Disconnected(Some(format!(
                            "Serial write failed: {error}"
                        ))));
                    }
                }
            }
            continue;
        }

        let Some(opened) = port.as_mut() else {
            continue;
        };

        match opened.read(&mut buffer) {
            Ok(count) => {
                for &byte in &buffer[..count] {
                    if byte == b'\r' {
                        continue;
                    }
                    if byte == b'\n' {
                        if !discarding && !line.is_empty() {
                            let packet = String::from_utf8_lossy(&line).into_owned();
                            let _ = events.send(IoEvent::Line(packet));
                        }
                        line.clear();
                        discarding = false;
                    } else if !discarding {
                        if line.len() < 1024 {
                            line.push(byte);
                        } else {
                            line.clear();
                            discarding = true;
                            let _ = events.send(IoEvent::Error(
                                "Incoming serial line exceeded 1024 bytes; discarded".into(),
                            ));
                        }
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => {
                port = None;
                let _ = events.send(IoEvent::Disconnected(Some(format!(
                    "Serial read failed: {error}"
                ))));
            }
        }
    }
}
