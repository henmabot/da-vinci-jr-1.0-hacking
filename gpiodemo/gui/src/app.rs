use std::{collections::VecDeque, fmt, path::Path, time::Duration};

use da_vinci_protocol::Level;
use iced::{
    Element, Subscription, Task,
    keyboard::{Event as KeyboardEvent, Key, key::Named},
    widget::{pane_grid, text_editor},
};

use crate::{
    serial_log::SerialLog,
    session::{
        DeviceEvent, DeviceSession, Event as ConnectionEvent, Mode, PinKey,
        Request as RoutedRequest, ResponseError, RouteKey, Target as RoutedTarget,
    },
    view::{self, pin_display},
};

const MAX_IO_EVENTS_PER_TICK: usize = 256;
const MAX_COMMAND_HISTORY: usize = 200;
const ROUTES: [&str; 2] = ["SAM", "LPC"];
pub(super) const BANK_TABS: [BankTab; 4] = [BankTab::A, BankTab::BAndE, BankTab::C, BankTab::D];

type Request = RoutedRequest;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ScopeChoice {
    target: RoutedTarget,
    label: String,
}

impl ScopeChoice {
    fn all() -> Self {
        Self {
            target: RoutedTarget::All,
            label: "ALL".into(),
        }
    }
}

impl fmt::Display for ScopeChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PortChoice {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BankTab {
    A,
    BAndE,
    C,
    D,
}

impl BankTab {
    pub(super) fn index(self) -> usize {
        BANK_TABS.iter().position(|tab| *tab == self).unwrap()
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::A => "PIOA",
            Self::BAndE => "PIOB + PIOE",
            Self::C => "PIOC",
            Self::D => "PIOD",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum PaneKind {
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
pub(super) enum HistoryDirection {
    Previous,
    Next,
}

#[derive(Clone, Debug)]
pub(super) enum Message {
    Tick,
    PortsLoaded(Result<Vec<String>, String>),
    RefreshPorts,
    PortSelected(PortChoice),
    Connect,
    Disconnect,
    PreviousTab,
    NextTab,
    TabSelected(BankTab),
    ModeSelected(PinKey, Mode),
    Read(PinKey),
    Write(PinKey),
    Listen(PinKey),
    BulkScopeSelected(ScopeChoice),
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

pub(super) struct App {
    pub(super) bank_tab: BankTab,
    pub(super) bulk_scope: ScopeChoice,
    pub(super) bulk_scopes: Vec<ScopeChoice>,
    pub(super) bulk_mode: Mode,
    pub(super) overwrite: bool,
    pub(super) confirm_set: Option<(ScopeChoice, Level)>,
    pub(super) panes: pane_grid::State<PaneKind>,
    pub(super) ports: Vec<PortChoice>,
    pub(super) selected_port: Option<PortChoice>,
    pub(super) connected_port: Option<String>,
    pub(super) session: DeviceSession,
    pub(super) sam_route: RouteKey,
    lpc_route: RouteKey,
    pub(super) log: SerialLog,
    pub(super) autoscroll: bool,
    pub(super) log_scroll: iced::widget::Id,
    pub(super) raw_input_id: iced::widget::Id,
    pub(super) raw_input: String,
    command_history: VecDeque<String>,
    history_index: Option<usize>,
    pub(super) device_status: String,
    pub(super) error: Option<String>,
    pub(super) confirm_reboot: bool,
}

impl App {
    pub(super) fn new() -> (Self, Task<Message>) {
        let (mut panes, pins_pane) = pane_grid::State::new(PaneKind::Pins);
        let (_, split) = panes
            .split(pane_grid::Axis::Vertical, pins_pane, PaneKind::Log)
            .expect("initial GPIO/log split must succeed");
        panes.resize(split, 0.76);
        let session = DeviceSession::spawn(&ROUTES);
        let sam_route = session.route_key("SAM").expect("SAM route is configured");
        let lpc_route = session.route_key("LPC").expect("LPC route is configured");

        (
            Self {
                bank_tab: BankTab::A,
                bulk_scope: ScopeChoice::all(),
                bulk_scopes: vec![ScopeChoice::all()],
                bulk_mode: Mode::Input,
                overwrite: false,
                confirm_set: None,
                panes,
                ports: Vec::new(),
                selected_port: None,
                connected_port: None,
                session,
                sam_route,
                lpc_route,
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

    pub(super) fn view(&self) -> Element<'_, Message> {
        view::view(self)
    }

    pub(super) fn subscription(&self) -> Subscription<Message> {
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

    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
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
            Message::PortsLoaded(result) => match result {
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
            },
            Message::RefreshPorts => return load_ports(),
            Message::PortSelected(port) => self.selected_port = Some(port),
            Message::Connect => {
                if let Some(port) = &self.selected_port {
                    self.error = self.session.connect(port.path.clone()).err();
                }
            }
            Message::Disconnect => self.error = self.session.disconnect().err(),
            Message::PreviousTab => {
                let index = self.bank_tab.index();
                if index > 0 {
                    self.bank_tab = BANK_TABS[index - 1];
                }
            }
            Message::NextTab => {
                let index = self.bank_tab.index();
                if index + 1 < BANK_TABS.len() {
                    self.bank_tab = BANK_TABS[index + 1];
                }
            }
            Message::TabSelected(tab) => self.bank_tab = tab,
            Message::ModeSelected(pin, mode) => self.change_mode(pin, mode),
            Message::Read(pin) => self.read_pin(pin),
            Message::Write(pin) => self.write_pin(pin),
            Message::Listen(pin) => self.toggle_listener(pin),
            Message::BulkScopeSelected(scope) => {
                self.bulk_scope = scope;
                self.confirm_set = None;
            }
            Message::BulkModeSelected(mode) => self.bulk_mode = mode,
            Message::OverwriteChanged(overwrite) => self.overwrite = overwrite,
            Message::ApplyBulkMode => self.apply_bulk_mode(),
            Message::BulkRead => self.read_scope(self.bulk_scope.target),
            Message::BulkListen(enabled) => {
                self.set_listener_scope(self.bulk_scope.target, enabled)
            }
            Message::BulkSet(level) => self.confirm_set = Some((self.bulk_scope.clone(), level)),
            Message::BulkSetConfirm => {
                if let Some((target, level)) = self.confirm_set.take() {
                    self.set_scope_level(target.target, level);
                }
            }
            Message::BulkSetCancel => self.confirm_set = None,
            Message::Handshake => self.send_request(Request::Hello),
            Message::Status => self.send_request(Request::Status),
            Message::Reboot => self.confirm_reboot = true,
            Message::RebootConfirm => {
                self.confirm_reboot = false;
                self.send_request(Request::Bye);
            }
            Message::RebootCancel => self.confirm_reboot = false,
            Message::PaneResized(event) => self.panes.resize(event.split, event.ratio),
            Message::ClearLog => self.log.clear(),
            Message::ShowTimestamps(enabled) => self.log.set_show_timestamps(enabled),
            Message::Autoscroll(enabled) => {
                self.autoscroll = enabled;
                if enabled {
                    return self.snap_log();
                }
            }
            Message::LogAction(action) => {
                if !action.is_edit() {
                    self.log.perform(action);
                }
            }
            Message::RawChanged(value) => {
                self.raw_input = value;
                self.history_index = None;
            }
            Message::RawSend => self.send_raw(),
            Message::HistoryKey(direction) => {
                return iced::widget::operation::is_focused(self.raw_input_id.clone())
                    .map(move |focused| Message::HistoryKeyFocus { direction, focused });
            }
            Message::HistoryKeyFocus { direction, focused } => {
                if focused {
                    match direction {
                        HistoryDirection::Previous => self.history_previous(),
                        HistoryDirection::Next => self.history_next(),
                    }
                }
            }
        }
        Task::none()
    }

    fn change_mode(&mut self, pin: PinKey, mode: Mode) {
        if !self.require_connection() {
            return;
        }
        let result = self.session.change_mode(pin, mode);
        self.record_session_action(result);
    }

    fn read_pin(&mut self, pin: PinKey) {
        if !self.require_connection() {
            return;
        }
        let result = self.session.read_pin(pin);
        self.record_session_action(result);
    }

    fn write_pin(&mut self, pin: PinKey) {
        if !self.require_connection() {
            return;
        }
        let result = self.session.write_pin(pin);
        self.record_session_action(result);
    }

    fn toggle_listener(&mut self, pin: PinKey) {
        if !self.require_connection() {
            return;
        }
        let result = self.session.toggle_listener(pin);
        self.record_session_action(result);
    }

    fn apply_bulk_mode(&mut self) {
        if !self.require_connection() {
            return;
        }
        let result = self.session.apply_mode(
            self.sam_route,
            self.bulk_scope.target,
            self.bulk_mode,
            self.overwrite,
        );
        if self.record_session_action(result) == 0 {
            self.device_status = "No eligible pins in selected scope".into();
        }
    }

    fn read_scope(&mut self, target: RoutedTarget) {
        if !self.require_connection() {
            return;
        }
        let result = self.session.read_scope(self.sam_route, target);
        self.record_session_action(result);
    }

    fn set_listener_scope(&mut self, target: RoutedTarget, enabled: bool) {
        if !self.require_connection() {
            return;
        }
        let result = self
            .session
            .set_listener_scope(self.sam_route, target, enabled);
        self.record_session_action(result);
    }

    fn set_scope_level(&mut self, target: RoutedTarget, level: Level) {
        if !self.require_connection() {
            return;
        }
        let result = self.session.set_scope_level(self.sam_route, target, level);
        self.record_session_action(result);
    }

    fn sync_sam_state(&mut self) {
        self.bulk_scopes.clear();
        self.bulk_scopes.push(ScopeChoice::all());
        self.bulk_scopes
            .extend(
                self.session
                    .banks(self.sam_route)
                    .map(|(bank, info)| ScopeChoice {
                        target: RoutedTarget::Bank(bank),
                        label: info.token.clone(),
                    }),
            );
        self.bulk_scope = ScopeChoice::all();
        self.confirm_set = None;
    }

    fn record_session_action(&mut self, result: Result<Vec<String>, String>) -> usize {
        match result {
            Ok(lines) => {
                let count = lines.len();
                for line in lines {
                    self.push_log(format!("TX {line}"));
                }
                count
            }
            Err(error) => {
                self.error = Some(error);
                0
            }
        }
    }

    fn send_routed_request(&mut self, route: RouteKey, request: RoutedRequest) {
        match self.session.send(route, request) {
            Ok(line) => self.push_log(format!("TX {line}")),
            Err(error) => self.error = Some(error),
        }
    }

    fn send_request(&mut self, request: Request) {
        if !self.require_connection() {
            return;
        }
        match self.session.send(self.sam_route, request) {
            Ok(line) => self.push_log(format!("TX {line}")),
            Err(error) => self.error = Some(error),
        }
    }

    fn send_raw(&mut self) {
        if self.raw_input.is_empty() {
            return;
        }
        if !self.require_connection() {
            return;
        }

        let line = std::mem::take(&mut self.raw_input);
        self.command_history.push_back(line.clone());
        if self.command_history.len() > MAX_COMMAND_HISTORY {
            self.command_history.pop_front();
        }
        self.history_index = None;
        match self.session.send_raw(&line) {
            Ok(()) => self.push_log(format!("TX {line}")),
            Err(error) => self.error = Some(error),
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

    fn drain_io(&mut self) {
        self.session.poll_listener_updates();
        for _ in 0..MAX_IO_EVENTS_PER_TICK {
            let Some(event) = self.session.next_event() else {
                break;
            };
            match event {
                ConnectionEvent::Connected(port) => {
                    self.connected_port = Some(port.clone());
                    self.device_status = "Connected; discovering pin maps".into();
                    self.error = None;
                    self.bulk_scopes = vec![ScopeChoice::all()];
                    self.bulk_scope = ScopeChoice::all();
                    self.send_routed_request(self.sam_route, RoutedRequest::Map);
                    self.send_routed_request(self.lpc_route, RoutedRequest::Map);
                }
                ConnectionEvent::Disconnected(reason) => {
                    self.connected_port = None;
                    self.device_status = "Disconnected".into();
                    self.bulk_scopes = vec![ScopeChoice::all()];
                    self.bulk_scope = ScopeChoice::all();
                    self.confirm_set = None;
                    self.error = reason;
                }
                ConnectionEvent::Received { line, event } => {
                    self.push_log(format!("RX {line}"));
                    match event {
                        Ok(event) => self.handle_device_event(event),
                        Err(error) => self.error = Some(error),
                    }
                }
                ConnectionEvent::ListenerValues(values) => {
                    for value in values {
                        self.push_log(format!("RX {}", value.line()));
                        if value.coalesced != 0 {
                            self.push_log(format!(
                                "RX ({} intermediate listener updates coalesced)",
                                value.coalesced
                            ));
                        }
                        self.handle_device_event(DeviceEvent::PinValue {
                            pin: value.pin,
                            level: value.level,
                        });
                    }
                }
                ConnectionEvent::IoError(error) => self.error = Some(error),
            }
        }
    }

    fn handle_device_event(&mut self, event: DeviceEvent) {
        match event {
            DeviceEvent::Hello { route } => {
                self.device_status = format!("{} replied HII", self.session.route_name(route));
            }
            DeviceEvent::Status { route, identity } => {
                self.device_status = format!("{}: {identity}", self.session.route_name(route));
            }
            DeviceEvent::MapReady { route } => {
                if route == self.sam_route {
                    self.sync_sam_state();
                }
                self.device_status = format!(
                    "{} map: {} banks, {} pins",
                    self.session.route_name(route),
                    self.session.banks(route).count(),
                    self.session.pins(route).count()
                );
            }
            DeviceEvent::Ack { route: _, sent } => {
                if let Some(line) = sent {
                    self.push_log(format!("TX {line}"));
                }
            }
            DeviceEvent::PinValue { .. } => {}
            DeviceEvent::PinState { pin, what, value } => {
                self.device_status =
                    format!("{} {what:?}: {value:?}", self.routed_pin_display(pin));
            }
            DeviceEvent::DeviceError {
                route: _,
                source,
                error,
            } => {
                self.error = Some(match error {
                    ResponseError::BadPacket => {
                        format!("{source} rejected a malformed packet")
                    }
                    ResponseError::Target {
                        target: pin,
                        reason,
                    } => format!("{}: {reason:?}", self.routed_pin_display(pin)),
                    ResponseError::NoRoute { destination } => {
                        format!("{source}: no route to {destination}")
                    }
                    ResponseError::RouteBusy { next_hop } => {
                        format!("{source}: route {next_hop} is busy")
                    }
                    ResponseError::RouteDown { next_hop } => {
                        format!("{source}: route {next_hop} is down")
                    }
                });
            }
            DeviceEvent::Unknown { route } => {
                self.error = Some(format!("{} returned IDK", self.session.route_name(route)));
            }
            DeviceEvent::Bye { route } => {
                self.device_status =
                    format!("{} reset acknowledged", self.session.route_name(route));
            }
            DeviceEvent::Untracked => {}
        }
    }

    fn routed_pin_display(&self, pin: PinKey) -> String {
        self.session
            .pin_info(pin)
            .map_or_else(|| "unknown pin".into(), pin_display)
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
fn load_ports() -> Task<Message> {
    Task::perform(
        async { DeviceSession::available_ports() },
        Message::PortsLoaded,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use da_vinci_protocol::PinCapabilities;

    fn connected_app() -> App {
        let (mut app, _) = App::new();
        app.connected_port = Some("test".into());
        let mut pins = Vec::new();
        pins.extend((0..32).map(|bit| (format!("PA{bit:02}"), 0, bit, PinCapabilities::GPIO)));
        pins.extend((0..32).map(|bit| (format!("PC{bit:02}"), 1, bit, PinCapabilities::GPIO)));
        app.session
            .install_map_for_test(app.sam_route, vec!["PIOA".into(), "PIOC".into()], pins);
        app.sync_sam_state();
        app
    }

    fn bank(app: &App, token: &str) -> RoutedTarget {
        RoutedTarget::Bank(app.session.bank_key(app.sam_route, token).unwrap())
    }

    fn scope(app: &App, token: &str) -> ScopeChoice {
        app.bulk_scopes
            .iter()
            .find(|scope| scope.label == token)
            .unwrap()
            .clone()
    }

    fn last_log(app: &App) -> &str {
        app.log.last_text().unwrap()
    }

    #[test]
    fn bulk_controls_send_selected_symbolic_scope() {
        let mut app = connected_app();
        let port_c = bank(&app, "PIOC");
        app.bulk_scope = scope(&app, "PIOC");
        app.bulk_mode = Mode::InputPullup;
        app.overwrite = true;

        app.apply_bulk_mode();
        assert_eq!(last_log(&app), "TX 001 SAM DIR PIOC IN OK?");

        app.set_listener_scope(port_c, true);
        assert_eq!(last_log(&app), "TX 002 SAM LSN PIOC ON OK?");

        app.read_scope(port_c);
        assert_eq!(last_log(&app), "TX 003 SAM GET PIOC OK?");
    }

    #[test]
    fn bulk_set_waits_for_confirmation() {
        let mut app = connected_app();

        let _ = app.update(Message::BulkSet(Level::High));
        assert!(app.log.is_empty());
        assert_eq!(
            app.confirm_set
                .as_ref()
                .map(|(scope, level)| (&scope.target, *level)),
            Some((&RoutedTarget::All, Level::High))
        );

        let _ = app.update(Message::BulkSetConfirm);
        assert_eq!(last_log(&app), "TX 001 SAM SET ALL HIGH OK?");
    }

    #[test]
    fn tab_selection_does_not_change_bulk_scope() {
        let mut app = connected_app();
        let selected = scope(&app, "PIOC");
        app.bulk_scope = selected.clone();

        let _ = app.update(Message::TabSelected(BankTab::D));

        assert_eq!(app.bank_tab, BankTab::D);
        assert_eq!(app.bulk_scope, selected);
    }
}
