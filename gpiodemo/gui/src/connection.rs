use std::{
    array,
    collections::VecDeque,
    io::{self, Read, Write},
    sync::mpsc::{self, Receiver, Sender, SyncSender},
    thread,
    time::Duration,
};

use da_vinci_protocol::{
    DecodeError, DecodeErrorKind, Direction, Level, LineBuffer, LineError, MAX_PACKET_LEN, Packet,
    PinCapabilities, Query, QueryValue, Request as ProtocolRequest, RequestId,
    Response as ProtocolResponse, ResponseError as ProtocolResponseError, decode_message,
    decode_response, encode_request,
};

const EVENT_QUEUE_CAPACITY: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RouteKey(usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BankKey {
    route: RouteKey,
    index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PinKey {
    route: RouteKey,
    index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Pin(PinKey),
    Bank(BankKey),
    All,
}

pub(super) type Request = ProtocolRequest<Target>;
pub(super) type ResponseError = ProtocolResponseError<PinKey, String>;
type WireResponse = ProtocolResponse<String, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BankInfo {
    pub(super) token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PinInfo {
    pub(super) token: String,
    pub(super) package_pin: Option<u16>,
    pub(super) bank: BankKey,
    pub(super) bit: u8,
    pub(super) capabilities: PinCapabilities,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Event {
    Connected(String),
    Disconnected(Option<String>),
    Received {
        line: String,
        event: Result<DeviceEvent, String>,
    },
    ListenerValues(Vec<ListenerValue>),
    IoError(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ListenerValue {
    line: WireLine,
    id: RequestId,
    pub(super) pin: PinKey,
    pub(super) level: Level,
    pub(super) coalesced: u32,
}

impl ListenerValue {
    pub(super) fn line(self) -> String {
        self.line.text()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DeviceEvent {
    Hello {
        route: RouteKey,
    },
    Status {
        route: RouteKey,
        identity: String,
    },
    MapReady {
        route: RouteKey,
    },
    Ack {
        route: RouteKey,
        sent: Option<String>,
    },
    PinValue {
        pin: PinKey,
        level: Level,
    },
    PinState {
        pin: PinKey,
        what: Query,
        value: QueryValue,
    },
    DeviceError {
        route: RouteKey,
        source: String,
        error: ResponseError,
    },
    Unknown {
        route: RouteKey,
    },
    Bye {
        route: RouteKey,
    },
    Untracked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WireLine {
    bytes: [u8; MAX_PACKET_LEN],
    len: usize,
}

impl WireLine {
    fn new(bytes: &[u8]) -> Self {
        let mut line = Self {
            bytes: [0; MAX_PACKET_LEN],
            len: bytes.len(),
        };
        line.bytes[..bytes.len()].copy_from_slice(bytes);
        line
    }

    fn text(self) -> String {
        String::from_utf8_lossy(&self.bytes[..self.len]).into_owned()
    }
    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequestLifetime {
    OneShot,
    StreamUntilAck,
    PersistentListener,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Input,
    InputPullup,
    Output,
}

impl Mode {
    pub(super) const fn is_input(self) -> bool {
        matches!(self, Self::Input | Self::InputPullup)
    }

    const fn direction(self) -> Direction {
        if matches!(self, Self::Output) {
            Direction::Output
        } else {
            Direction::Input
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ListenerState {
    Off,
    Enabling,
    On,
    Disabling,
}

impl ListenerState {
    pub(super) const fn is_pending(self) -> bool {
        matches!(self, Self::Enabling | Self::Disabling)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PinState {
    pub(super) mode: Option<Mode>,
    pub(super) target_mode: Option<Mode>,
    pub(super) level: Option<Level>,
    pub(super) listener: ListenerState,
    pub(super) value_pending: bool,
}

impl PinState {
    pub(super) const UNSET: Self = Self {
        mode: None,
        target_mode: None,
        level: None,
        listener: ListenerState::Off,
        value_pending: false,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pending {
    route: RouteKey,
    request: Request,
    lifetime: RequestLifetime,
}

#[derive(Clone, Debug)]
struct RoutePin {
    info: PinInfo,
    state: PinState,
    listener_id: Option<RequestId>,
}

#[derive(Clone, Debug, Default)]
struct RouteMap {
    banks: Vec<BankInfo>,
    pins: Vec<RoutePin>,
}

#[derive(Clone, Debug)]
struct MapBuilder {
    route: RouteKey,
    banks: Vec<BankInfo>,
    pins: Vec<PinInfo>,
}

impl MapBuilder {
    fn new(route: RouteKey) -> Self {
        Self {
            route,
            banks: Vec::new(),
            pins: Vec::new(),
        }
    }

    fn bank(&mut self, token: String) -> Result<(), String> {
        if self.banks.iter().any(|bank| bank.token == token) {
            return Err(format!("Duplicate MAP bank {token}"));
        }
        self.banks.push(BankInfo { token });
        Ok(())
    }

    fn pin(
        &mut self,
        token: String,
        package_pin: Option<u16>,
        bank_token: String,
        bit: u8,
        capabilities: PinCapabilities,
    ) -> Result<(), String> {
        if self.pins.iter().any(|pin| pin.token == token) {
            return Err(format!("Duplicate MAP pin {token}"));
        }
        let Some(bank_index) = self.banks.iter().position(|bank| bank.token == bank_token) else {
            return Err(format!(
                "MAP pin {token} references unknown bank {bank_token}"
            ));
        };
        if self
            .pins
            .iter()
            .any(|pin| pin.bank.index == bank_index && pin.bit == bit)
        {
            return Err(format!("Duplicate MAP bank bit {bank_token}:{bit}"));
        }
        self.pins.push(PinInfo {
            token,
            package_pin,
            bank: BankKey {
                route: self.route,
                index: bank_index,
            },
            bit,
            capabilities,
        });
        Ok(())
    }

    fn finish(self) -> RouteMap {
        RouteMap {
            banks: self.banks,
            pins: self
                .pins
                .into_iter()
                .map(|info| RoutePin {
                    info,
                    state: PinState::UNSET,
                    listener_id: None,
                })
                .collect(),
        }
    }
}

struct RouteState {
    name: String,
    map: Option<RouteMap>,
    discovery: Option<MapBuilder>,
}

impl RouteState {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            map: None,
            discovery: None,
        }
    }
}

pub(super) struct DeviceSession {
    next_id: RequestId,
    pending: [Option<Pending>; RequestId::COUNT],
    routes: Vec<RouteState>,
    commands: Sender<IoCommand>,
    events: Receiver<IoEvent>,
}

impl DeviceSession {
    pub(super) fn spawn(route_names: &[&str]) -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::sync_channel(EVENT_QUEUE_CAPACITY);
        thread::spawn(move || io_worker(command_rx, event_tx));
        Self {
            next_id: RequestId::FIRST,
            pending: array::from_fn(|_| None),
            routes: route_names
                .iter()
                .map(|name| RouteState::new(name))
                .collect(),
            commands: command_tx,
            events: event_rx,
        }
    }

    pub(super) fn route_key(&self, name: &str) -> Option<RouteKey> {
        self.routes
            .iter()
            .position(|route| route.name == name)
            .map(RouteKey)
    }

    pub(super) fn route_name(&self, route: RouteKey) -> &str {
        &self.routes[route.0].name
    }

    pub(super) fn pin_key(&self, route: RouteKey, token: &str) -> Option<PinKey> {
        self.routes
            .get(route.0)?
            .map
            .as_ref()?
            .pins
            .iter()
            .position(|pin| pin.info.token == token)
            .map(|index| PinKey { route, index })
    }

    pub(super) fn bank_key(&self, route: RouteKey, token: &str) -> Option<BankKey> {
        self.routes
            .get(route.0)?
            .map
            .as_ref()?
            .banks
            .iter()
            .position(|bank| bank.token == token)
            .map(|index| BankKey { route, index })
    }

    pub(super) fn pin_info(&self, pin: PinKey) -> Option<&PinInfo> {
        self.routes
            .get(pin.route.0)?
            .map
            .as_ref()?
            .pins
            .get(pin.index)
            .map(|pin| &pin.info)
    }

    pub(super) fn bank_info(&self, bank: BankKey) -> Option<&BankInfo> {
        self.routes
            .get(bank.route.0)?
            .map
            .as_ref()?
            .banks
            .get(bank.index)
    }

    pub(super) fn pins(&self, route: RouteKey) -> impl Iterator<Item = (PinKey, &PinInfo)> {
        self.routes[route.0]
            .map
            .iter()
            .flat_map(|map| map.pins.iter().enumerate())
            .map(move |(index, pin)| (PinKey { route, index }, &pin.info))
    }

    pub(super) fn banks(&self, route: RouteKey) -> impl Iterator<Item = (BankKey, &BankInfo)> {
        self.routes[route.0]
            .map
            .iter()
            .flat_map(|map| map.banks.iter().enumerate())
            .map(move |(index, bank)| (BankKey { route, index }, bank))
    }

    pub(super) fn target_pins(&self, route: RouteKey, target: Target) -> Vec<PinKey> {
        let Some(map) = self
            .routes
            .get(route.0)
            .and_then(|route| route.map.as_ref())
        else {
            return Vec::new();
        };
        map.pins
            .iter()
            .enumerate()
            .filter(|(index, pin)| target_contains(route, target, *index, pin.info.bank))
            .map(|(index, _)| PinKey { route, index })
            .collect()
    }

    pub(super) fn pin_state(&self, pin: PinKey) -> Option<PinState> {
        self.routes
            .get(pin.route.0)?
            .map
            .as_ref()?
            .pins
            .get(pin.index)
            .map(|pin| pin.state)
    }

    pub(super) fn change_mode(&mut self, pin: PinKey, mode: Mode) -> Result<Vec<String>, String> {
        let route = pin.route;
        let Some(route_pin) = self.route_pin(pin) else {
            return Err("Unknown pin key".into());
        };
        if !route_pin.info.capabilities.available() {
            return Ok(Vec::new());
        }
        let state = route_pin.state;
        if state.target_mode.is_some() || state.listener.is_pending() {
            return Ok(Vec::new());
        }

        let mut sent = Vec::with_capacity(2);
        if mode == Mode::Output && state.listener == ListenerState::On {
            self.route_pin_mut(pin).unwrap().state.listener = ListenerState::Disabling;
            let request = Request::Listen {
                target: Target::Pin(pin),
                enabled: false,
            };
            sent.push(self.send(route, request)?);
        }

        let state = &mut self.route_pin_mut(pin).unwrap().state;
        state.target_mode = Some(mode);
        state.level = None;
        let request = Request::Direction {
            target: Target::Pin(pin),
            direction: mode.direction(),
        };
        sent.push(self.send(route, request)?);
        Ok(sent)
    }

    pub(super) fn read_pin(&mut self, pin: PinKey) -> Result<Vec<String>, String> {
        let route = pin.route;
        let Some(state) = self.pin_state(pin) else {
            return Err("Unknown pin key".into());
        };
        if state.mode.is_none() || state.value_pending {
            return Ok(Vec::new());
        }
        self.route_pin_mut(pin).unwrap().state.value_pending = true;
        let request = Request::Get {
            target: Target::Pin(pin),
        };
        self.send(route, request).map(|line| vec![line])
    }

    pub(super) fn write_pin(&mut self, pin: PinKey) -> Result<Vec<String>, String> {
        let route = pin.route;
        let Some(state) = self.pin_state(pin) else {
            return Err("Unknown pin key".into());
        };
        if state.mode != Some(Mode::Output) || state.value_pending {
            return Ok(Vec::new());
        }
        let level = if state.level == Some(Level::High) {
            Level::Low
        } else {
            Level::High
        };
        self.route_pin_mut(pin).unwrap().state.value_pending = true;
        let request = Request::Set {
            target: Target::Pin(pin),
            level,
        };
        self.send(route, request).map(|line| vec![line])
    }

    pub(super) fn toggle_listener(&mut self, pin: PinKey) -> Result<Vec<String>, String> {
        let route = pin.route;
        let Some(state) = self.pin_state(pin) else {
            return Err("Unknown pin key".into());
        };
        if !state.mode.is_some_and(Mode::is_input) {
            return Ok(Vec::new());
        }
        let (enabled, pending) = match state.listener {
            ListenerState::Off => (true, ListenerState::Enabling),
            ListenerState::On => (false, ListenerState::Disabling),
            ListenerState::Enabling | ListenerState::Disabling => return Ok(Vec::new()),
        };
        self.route_pin_mut(pin).unwrap().state.listener = pending;
        let request = Request::Listen {
            target: Target::Pin(pin),
            enabled,
        };
        self.send(route, request).map(|line| vec![line])
    }

    pub(super) fn apply_mode(
        &mut self,
        route: RouteKey,
        target: Target,
        mode: Mode,
        overwrite: bool,
    ) -> Result<Vec<String>, String> {
        if overwrite {
            if self.target_has_pending(route, target) {
                return Ok(Vec::new());
            }
            let mut sent = Vec::with_capacity(2);
            if mode == Mode::Output && self.target_has_listener(route, target) {
                self.mark_listener_pending(route, target, false);
                let request = Request::Listen {
                    target,
                    enabled: false,
                };
                sent.push(self.send(route, request)?);
            }
            self.mark_mode_pending(route, target, mode);
            let request = Request::Direction {
                target,
                direction: mode.direction(),
            };
            sent.push(self.send(route, request)?);
            return Ok(sent);
        }

        let mut sent = Vec::new();
        for pin in self.target_pins(route, target) {
            let Some(route_pin) = self.route_pin(pin) else {
                continue;
            };
            if !route_pin.info.capabilities.available()
                || route_pin.state.mode.is_some()
                || route_pin.state.target_mode.is_some()
            {
                continue;
            }
            let state = &mut self.route_pin_mut(pin).unwrap().state;
            state.target_mode = Some(mode);
            state.level = None;
            let request = Request::Direction {
                target: Target::Pin(pin),
                direction: mode.direction(),
            };
            sent.push(self.send(route, request)?);
        }
        Ok(sent)
    }

    pub(super) fn read_scope(
        &mut self,
        route: RouteKey,
        target: Target,
    ) -> Result<Vec<String>, String> {
        for pin in self.target_pins(route, target) {
            if let Some(route_pin) = self.route_pin_mut(pin)
                && route_pin.state.mode.is_some()
            {
                route_pin.state.value_pending = true;
            }
        }
        let request = Request::Get { target };
        self.send(route, request).map(|line| vec![line])
    }

    pub(super) fn set_listener_scope(
        &mut self,
        route: RouteKey,
        target: Target,
        enabled: bool,
    ) -> Result<Vec<String>, String> {
        self.mark_listener_pending(route, target, enabled);
        let request = Request::Listen { target, enabled };
        self.send(route, request).map(|line| vec![line])
    }

    pub(super) fn set_scope_level(
        &mut self,
        route: RouteKey,
        target: Target,
        level: Level,
    ) -> Result<Vec<String>, String> {
        for pin in self.target_pins(route, target) {
            if let Some(route_pin) = self.route_pin_mut(pin)
                && route_pin.state.mode == Some(Mode::Output)
            {
                route_pin.state.value_pending = true;
            }
        }
        let request = Request::Set { target, level };
        self.send(route, request).map(|line| vec![line])
    }

    fn route_pin(&self, pin: PinKey) -> Option<&RoutePin> {
        self.routes
            .get(pin.route.0)?
            .map
            .as_ref()?
            .pins
            .get(pin.index)
    }

    fn route_pin_mut(&mut self, pin: PinKey) -> Option<&mut RoutePin> {
        self.routes
            .get_mut(pin.route.0)?
            .map
            .as_mut()?
            .pins
            .get_mut(pin.index)
    }

    fn target_has_pending(&self, route: RouteKey, target: Target) -> bool {
        self.target_pins(route, target).into_iter().any(|pin| {
            self.pin_state(pin)
                .is_some_and(|state| state.target_mode.is_some() || state.listener.is_pending())
        })
    }

    fn target_has_listener(&self, route: RouteKey, target: Target) -> bool {
        self.target_pins(route, target).into_iter().any(|pin| {
            self.pin_state(pin)
                .is_some_and(|state| state.listener == ListenerState::On)
        })
    }

    fn mark_mode_pending(&mut self, route: RouteKey, target: Target, mode: Mode) {
        for pin in self.target_pins(route, target) {
            let available = self
                .route_pin(pin)
                .is_some_and(|pin| pin.info.capabilities.available());
            if available && let Some(pin) = self.route_pin_mut(pin) {
                pin.state.target_mode = Some(mode);
                pin.state.level = None;
            }
        }
    }

    fn mark_listener_pending(&mut self, route: RouteKey, target: Target, enabled: bool) {
        for pin in self.target_pins(route, target) {
            if let Some(pin) = self.route_pin_mut(pin)
                && pin.state.mode.is_some_and(Mode::is_input)
            {
                pin.state.listener = if enabled {
                    ListenerState::Enabling
                } else {
                    ListenerState::Disabling
                };
            }
        }
    }

    fn fail_request_state(&mut self, route: RouteKey, request: Request) {
        match request {
            Request::Direction { target, .. } | Request::Pullup { target, .. } => {
                for pin in self.target_pins(route, target) {
                    if let Some(pin) = self.route_pin_mut(pin) {
                        pin.state.target_mode = None;
                    }
                }
            }
            Request::Get { target } | Request::Set { target, .. } => {
                for pin in self.target_pins(route, target) {
                    if let Some(pin) = self.route_pin_mut(pin) {
                        pin.state.value_pending = false;
                    }
                }
            }
            Request::Listen { target, enabled } => {
                for pin in self.target_pins(route, target) {
                    if let Some(pin) = self.route_pin_mut(pin)
                        && pin.state.mode.is_some()
                    {
                        pin.state.listener = if enabled {
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

    #[cfg(test)]
    pub(super) fn install_map_for_test(
        &mut self,
        route: RouteKey,
        banks: Vec<String>,
        pins: Vec<(String, usize, u8, PinCapabilities)>,
    ) {
        let banks: Vec<_> = banks.into_iter().map(|token| BankInfo { token }).collect();
        let pins = pins
            .into_iter()
            .map(|(token, bank, bit, capabilities)| RoutePin {
                info: PinInfo {
                    token,
                    package_pin: None,
                    bank: BankKey { route, index: bank },
                    bit,
                    capabilities,
                },
                state: PinState::UNSET,
                listener_id: None,
            })
            .collect();
        self.routes[route.0].map = Some(RouteMap { banks, pins });
    }

    pub(super) fn available_ports() -> Result<Vec<String>, String> {
        serialport::available_ports()
            .map(|ports| ports.into_iter().map(|port| port.port_name).collect())
            .map_err(|error| error.to_string())
    }

    pub(super) fn connect(&self, name: String) -> Result<(), String> {
        self.send_command(IoCommand::Connect(name))
    }

    pub(super) fn disconnect(&self) -> Result<(), String> {
        self.send_command(IoCommand::Disconnect)
    }

    pub(super) fn send(&mut self, route: RouteKey, request: Request) -> Result<String, String> {
        let (id, bytes) = self.prepare(route, request)?;
        let line = String::from_utf8_lossy(&bytes)
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        if let Err(error) = self.send_command(IoCommand::Write(bytes)) {
            self.cancel(id);
            return Err(error);
        }
        Ok(line)
    }

    pub(super) fn send_raw(&self, line: &str) -> Result<(), String> {
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(b'\n');
        self.send_command(IoCommand::Write(bytes))
    }

    pub(super) fn poll_listener_updates(&self) {
        let _ = self.commands.send(IoCommand::DrainListeners);
    }

    pub(super) fn next_event(&mut self) -> Option<Event> {
        loop {
            match self.events.try_recv() {
                Ok(IoEvent::Connected(port)) => {
                    self.clear();
                    return Some(Event::Connected(port));
                }
                Ok(IoEvent::Disconnected(reason)) => {
                    self.clear();
                    return Some(Event::Disconnected(reason));
                }
                Ok(IoEvent::Line { line, packet }) => {
                    let event = match packet {
                        Ok(packet) => self.received(packet),
                        Err(error) => {
                            if let Some(id) = error.id {
                                self.retire(id);
                            }
                            Err(format!("Malformed response: {error:?}"))
                        }
                    };
                    return Some(Event::Received {
                        line: line.text(),
                        event,
                    });
                }
                Ok(IoEvent::ListenerValues(mut values)) => {
                    values.retain(|value| self.listener_is_active(value.pin, value.id));
                    if !values.is_empty() {
                        return Some(Event::ListenerValues(values));
                    }
                }
                Ok(IoEvent::Error(error)) => return Some(Event::IoError(error)),
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.clear();
                    return Some(Event::Disconnected(Some("Serial worker stopped".into())));
                }
            }
        }
    }

    fn prepare(
        &mut self,
        route: RouteKey,
        request: Request,
    ) -> Result<(RequestId, Vec<u8>), String> {
        if route.0 >= self.routes.len() {
            return Err("Unknown route key".into());
        }
        if self.routes[route.0].discovery.is_some() {
            return Err(if matches!(request, Request::Map) {
                format!("MAP for {} is already in progress", self.route_name(route))
            } else {
                format!(
                    "{} pin-map discovery is still in progress",
                    self.route_name(route)
                )
            });
        }

        let id = self.allocate_request_id()?;
        let destination = self.routes[route.0].name.clone();
        let wire_request = request.try_map_target(|target| self.target_token(route, target))?;
        let mut buffer = [0u8; MAX_PACKET_LEN];
        let len = encode_request(
            Packet {
                id,
                body: wire_request,
            },
            destination.as_bytes(),
            &mut buffer,
        )
        .map_err(|error| format!("Could not encode request: {error:?}"))?;

        if matches!(request, Request::Map) {
            self.routes[route.0].discovery = Some(MapBuilder::new(route));
        }
        self.pending[id.slot()] = Some(Pending {
            route,
            request,
            lifetime: request_lifetime(request),
        });
        Ok((id, buffer[..len].to_vec()))
    }

    fn target_token(&self, route: RouteKey, target: Target) -> Result<String, String> {
        match target {
            Target::All => Ok("ALL".into()),
            Target::Pin(pin) if pin.route == route => self
                .pin_info(pin)
                .map(|info| info.token.clone())
                .ok_or_else(|| "Unknown pin key".into()),
            Target::Bank(bank) if bank.route == route => self
                .bank_info(bank)
                .map(|info| info.token.clone())
                .ok_or_else(|| "Unknown bank key".into()),
            Target::Pin(_) | Target::Bank(_) => Err("Target belongs to another route".into()),
        }
    }

    fn allocate_request_id(&mut self) -> Result<RequestId, String> {
        for _ in RequestId::MIN..=RequestId::MAX {
            let id = self.next_id;
            self.next_id = id.next();
            if self.pending[id.slot()].is_none() {
                return Ok(id);
            }
        }
        Err("All 999 request IDs are still in use".into())
    }

    fn received(&mut self, incoming: OwnedResponse) -> Result<DeviceEvent, String> {
        let id = incoming.packet.id;
        let Some(pending) = self.pending[id.slot()] else {
            return Ok(DeviceEvent::Untracked);
        };
        let routing_error = matches!(
            incoming.packet.body,
            WireResponse::Error(
                ProtocolResponseError::NoRoute { .. }
                    | ProtocolResponseError::RouteBusy { .. }
                    | ProtocolResponseError::RouteDown { .. }
            )
        );
        let expected = self.route_name(pending.route);
        if !routing_error && incoming.source != expected {
            return Err(format!(
                "Response {:03} came from {}, expected {expected}",
                id.get(),
                incoming.source
            ));
        }

        match incoming.packet.body {
            WireResponse::Hello => {
                self.complete(id, false, false);
                Ok(DeviceEvent::Hello {
                    route: pending.route,
                })
            }
            WireResponse::Status { identity } => {
                self.complete(id, false, false);
                Ok(DeviceEvent::Status {
                    route: pending.route,
                    identity,
                })
            }
            WireResponse::MapBank { bank } => {
                if let Err(error) = self.require_map(pending, id).and_then(|map| map.bank(bank)) {
                    self.retire(id);
                    return Err(error);
                }
                Ok(DeviceEvent::Untracked)
            }
            WireResponse::MapPin {
                target,
                package_pin,
                bank,
                bit,
                capabilities,
            } => {
                if let Err(error) = self
                    .require_map(pending, id)
                    .and_then(|map| map.pin(target, package_pin, bank, bit, capabilities))
                {
                    self.retire(id);
                    return Err(error);
                }
                Ok(DeviceEvent::Untracked)
            }
            WireResponse::Ack => self.ack(id, pending),
            WireResponse::Value { target, level } => {
                let pin = match self.resolve_pin(pending.route, &target) {
                    Ok(pin) => pin,
                    Err(error) => {
                        self.retire(id);
                        return Err(error);
                    }
                };
                if pending.lifetime == RequestLifetime::PersistentListener
                    && !self.listener_is_active(pin, id)
                {
                    return Ok(DeviceEvent::Untracked);
                }
                if let Some(route_pin) = self.route_pin_mut(pin) {
                    route_pin.state.level = Some(level);
                    route_pin.state.value_pending = false;
                }
                self.complete(id, false, false);
                Ok(DeviceEvent::PinValue { pin, level })
            }
            WireResponse::State {
                target,
                what,
                value,
            } => {
                let pin = match self.resolve_pin(pending.route, &target) {
                    Ok(pin) => pin,
                    Err(error) => {
                        self.retire(id);
                        return Err(error);
                    }
                };
                self.apply_query_state(pin, what, value);
                self.complete(id, false, false);
                Ok(DeviceEvent::PinState { pin, what, value })
            }
            WireResponse::Error(error) => {
                let error = match self.resolve_error(pending.route, error) {
                    Ok(error) => error,
                    Err(error) => {
                        self.retire(id);
                        return Err(error);
                    }
                };
                self.retire(id);
                Ok(DeviceEvent::DeviceError {
                    route: pending.route,
                    source: incoming.source,
                    error,
                })
            }
            WireResponse::Unknown => {
                self.retire(id);
                Ok(DeviceEvent::Unknown {
                    route: pending.route,
                })
            }
            WireResponse::Bye => {
                self.reset_route(pending.route);
                Ok(DeviceEvent::Bye {
                    route: pending.route,
                })
            }
        }
    }

    fn require_map(&mut self, pending: Pending, id: RequestId) -> Result<&mut MapBuilder, String> {
        if pending.request != Request::Map || pending.lifetime != RequestLifetime::StreamUntilAck {
            self.retire(id);
            return Err(format!(
                "Unexpected MAP response for request {:03}",
                id.get()
            ));
        }
        self.routes[pending.route.0]
            .discovery
            .as_mut()
            .ok_or_else(|| format!("MAP response for {:03} has no active discovery", id.get()))
    }

    fn resolve_pin(&self, route: RouteKey, token: &str) -> Result<PinKey, String> {
        self.pin_key(route, token).ok_or_else(|| {
            format!(
                "{} response referenced undiscovered pin {token}",
                self.route_name(route)
            )
        })
    }

    fn resolve_error(
        &self,
        route: RouteKey,
        error: ProtocolResponseError<String, String>,
    ) -> Result<ResponseError, String> {
        Ok(match error {
            ProtocolResponseError::BadPacket => ProtocolResponseError::BadPacket,
            ProtocolResponseError::Target { target, reason } => ProtocolResponseError::Target {
                target: self.resolve_pin(route, &target)?,
                reason,
            },
            ProtocolResponseError::NoRoute { destination } => {
                ProtocolResponseError::NoRoute { destination }
            }
            ProtocolResponseError::RouteBusy { next_hop } => {
                ProtocolResponseError::RouteBusy { next_hop }
            }
            ProtocolResponseError::RouteDown { next_hop } => {
                ProtocolResponseError::RouteDown { next_hop }
            }
        })
    }

    fn ack(&mut self, id: RequestId, pending: Pending) -> Result<DeviceEvent, String> {
        let mut follow_up = None;
        match pending.request {
            Request::Map => {
                let Some(builder) = self.routes[pending.route.0].discovery.take() else {
                    self.retire(id);
                    return Err(format!(
                        "MAP {:03} completed without discovery state",
                        id.get()
                    ));
                };
                self.routes[pending.route.0].map = Some(builder.finish());
                self.complete(id, true, false);
                self.sync_listeners();
                return Ok(DeviceEvent::MapReady {
                    route: pending.route,
                });
            }
            Request::Direction { target, direction } => {
                if let Some(mode) = self.pending_mode(pending.route, target) {
                    let request = Request::Pullup {
                        target,
                        enabled: mode == Mode::InputPullup,
                    };
                    follow_up = Some(request);
                } else {
                    self.for_target_pins_mut(pending.route, target, |pin| {
                        if pin.info.capabilities.supports_direction(direction) {
                            pin.state.mode = Some(if direction == Direction::Input {
                                Mode::Input
                            } else {
                                Mode::Output
                            });
                            pin.state.level =
                                (direction == Direction::Output).then_some(Level::Low);
                        }
                    });
                }
            }
            Request::Pullup { target, .. } => {
                let mut read = false;
                self.for_target_pins_mut(pending.route, target, |pin| {
                    if let Some(mode) = pin.state.target_mode.take() {
                        pin.state.mode = Some(mode);
                        if mode.is_input() {
                            pin.state.value_pending = true;
                            read = true;
                        } else {
                            pin.state.level = Some(Level::Low);
                            pin.state.value_pending = false;
                        }
                    }
                });
                if read {
                    follow_up = Some(Request::Get { target });
                }
            }
            Request::Set { target, level } => {
                self.for_target_pins_mut(pending.route, target, |pin| {
                    if pin.state.mode == Some(Mode::Output) {
                        pin.state.level = Some(level);
                        pin.state.value_pending = false;
                    }
                });
            }
            Request::Listen { target, enabled } => {
                let previous = self.listener_ids(pending.route, target);
                if enabled {
                    self.for_target_pins_mut(pending.route, target, |pin| {
                        if pin.state.mode.is_some_and(Mode::is_input)
                            && pin.info.capabilities.input()
                        {
                            pin.listener_id = Some(id);
                            pin.state.listener = ListenerState::On;
                        }
                    });
                } else {
                    self.for_target_pins_mut(pending.route, target, |pin| {
                        pin.listener_id = None;
                        pin.state.listener = ListenerState::Off;
                    });
                }
                for previous in previous {
                    self.release_listener(previous);
                }
                self.sync_listeners();
            }
            _ => {}
        }

        let listener_active =
            pending.lifetime == RequestLifetime::PersistentListener && self.listener_id_active(id);
        self.complete(id, true, listener_active);
        let sent = follow_up
            .map(|request| self.send(pending.route, request))
            .transpose()?;
        Ok(DeviceEvent::Ack {
            route: pending.route,
            sent,
        })
    }

    fn pending_mode(&self, route: RouteKey, target: Target) -> Option<Mode> {
        self.target_pins(route, target)
            .into_iter()
            .find_map(|pin| self.pin_state(pin).and_then(|state| state.target_mode))
    }

    fn apply_query_state(&mut self, pin: PinKey, what: Query, value: QueryValue) {
        let Some(route_pin) = self.route_pin_mut(pin) else {
            return;
        };
        match (what, value) {
            (Query::Direction, QueryValue::Direction(Direction::Input)) => {
                if route_pin.state.mode != Some(Mode::InputPullup) {
                    route_pin.state.mode = Some(Mode::Input);
                }
            }
            (Query::Direction, QueryValue::Direction(Direction::Output)) => {
                route_pin.state.mode = Some(Mode::Output);
            }
            (Query::Pullup, QueryValue::Enabled(true))
                if route_pin.state.mode.is_some_and(Mode::is_input) =>
            {
                route_pin.state.mode = Some(Mode::InputPullup);
            }
            (Query::Pullup, QueryValue::Enabled(false))
                if route_pin.state.mode.is_some_and(Mode::is_input) =>
            {
                route_pin.state.mode = Some(Mode::Input);
            }
            _ => {}
        }
    }

    fn complete(&mut self, id: RequestId, terminal_ack: bool, listener_active: bool) {
        let Some(pending) = self.pending[id.slot()] else {
            return;
        };
        let done = match pending.lifetime {
            RequestLifetime::OneShot => true,
            RequestLifetime::StreamUntilAck => terminal_ack,
            RequestLifetime::PersistentListener => terminal_ack && !listener_active,
        };
        if done {
            self.pending[id.slot()] = None;
        }
    }

    fn for_target_pins_mut(
        &mut self,
        route: RouteKey,
        target: Target,
        mut apply: impl FnMut(&mut RoutePin),
    ) {
        let Some(map) = self.routes[route.0].map.as_mut() else {
            return;
        };
        for (index, pin) in map.pins.iter_mut().enumerate() {
            if target_contains(route, target, index, pin.info.bank) {
                apply(pin);
            }
        }
    }

    fn listener_ids(&self, route: RouteKey, target: Target) -> Vec<RequestId> {
        let Some(map) = self.routes[route.0].map.as_ref() else {
            return Vec::new();
        };
        map.pins
            .iter()
            .enumerate()
            .filter(|(index, pin)| target_contains(route, target, *index, pin.info.bank))
            .filter_map(|(_, pin)| pin.listener_id)
            .collect()
    }

    fn listener_is_active(&self, pin: PinKey, id: RequestId) -> bool {
        self.routes
            .get(pin.route.0)
            .and_then(|route| route.map.as_ref())
            .and_then(|map| map.pins.get(pin.index))
            .is_some_and(|pin| pin.listener_id == Some(id))
    }

    fn listener_id_active(&self, id: RequestId) -> bool {
        self.routes.iter().any(|route| {
            route
                .map
                .as_ref()
                .is_some_and(|map| map.pins.iter().any(|pin| pin.listener_id == Some(id)))
        })
    }

    fn release_listener(&mut self, id: RequestId) {
        if !self.listener_id_active(id) {
            self.pending[id.slot()] = None;
        }
    }

    fn retire(&mut self, id: RequestId) {
        let pending = self.pending[id.slot()];
        if let Some(pending) = pending {
            self.fail_request_state(pending.route, pending.request);
        }
        for route in &mut self.routes {
            if let Some(map) = &mut route.map {
                for pin in &mut map.pins {
                    if pin.listener_id == Some(id) {
                        pin.listener_id = None;
                        pin.state.listener = ListenerState::Off;
                    }
                }
            }
        }
        if let Some(pending) = pending
            && pending.request == Request::Map
        {
            self.routes[pending.route.0].discovery = None;
        }
        self.pending[id.slot()] = None;
        self.sync_listeners();
    }

    fn cancel(&mut self, id: RequestId) {
        if let Some(pending) = self.pending[id.slot()] {
            self.fail_request_state(pending.route, pending.request);
            if pending.request == Request::Map {
                self.routes[pending.route.0].discovery = None;
            }
        }
        self.pending[id.slot()] = None;
    }

    fn reset_route(&mut self, route: RouteKey) {
        if let Some(map) = self.routes[route.0].map.as_mut() {
            for pin in &mut map.pins {
                pin.state = PinState::UNSET;
                pin.listener_id = None;
            }
        }
        self.routes[route.0].discovery = None;
        for pending in &mut self.pending {
            if pending.is_some_and(|pending| pending.route == route) {
                *pending = None;
            }
        }
        self.sync_listeners();
    }

    fn clear(&mut self) {
        self.pending.fill(None);
        for route in &mut self.routes {
            route.map = None;
            route.discovery = None;
        }
        self.sync_listeners();
    }

    fn sync_listeners(&self) {
        let routes = self
            .routes
            .iter()
            .enumerate()
            .map(|(route_index, route)| {
                let route_key = RouteKey(route_index);
                let pin_count = route.map.as_ref().map_or(0, |map| map.pins.len());
                let pins = route.map.as_ref().map_or_else(Vec::new, |map| {
                    map.pins
                        .iter()
                        .enumerate()
                        .filter_map(|(index, pin)| {
                            pin.listener_id.map(|id| ListenerPin {
                                key: PinKey {
                                    route: route_key,
                                    index,
                                },
                                token: pin.info.token.as_bytes().into(),
                                id,
                            })
                        })
                        .collect()
                });
                ListenerRoute {
                    name: route.name.as_bytes().into(),
                    pin_count,
                    pins,
                }
            })
            .collect();
        let _ = self.commands.send(IoCommand::Listeners(routes));
    }

    fn send_command(&self, command: IoCommand) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|_| "Serial worker stopped".into())
    }
}

fn target_contains(route: RouteKey, target: Target, pin_index: usize, bank: BankKey) -> bool {
    match target {
        Target::Pin(pin) => pin.route == route && pin.index == pin_index,
        Target::Bank(target) => target == bank,
        Target::All => true,
    }
}

fn request_lifetime(request: Request) -> RequestLifetime {
    match request {
        Request::Map
        | Request::Get {
            target: Target::Bank(_) | Target::All,
        }
        | Request::Query {
            target: Target::Bank(_) | Target::All,
            ..
        } => RequestLifetime::StreamUntilAck,
        Request::Listen { enabled: true, .. } => RequestLifetime::PersistentListener,
        _ => RequestLifetime::OneShot,
    }
}

#[derive(Clone)]
struct ListenerPin {
    key: PinKey,
    token: Box<[u8]>,
    id: RequestId,
}

#[derive(Clone)]
struct ListenerRoute {
    name: Box<[u8]>,
    pin_count: usize,
    pins: Vec<ListenerPin>,
}

enum IoCommand {
    Connect(String),
    Disconnect,
    Write(Vec<u8>),
    Listeners(Vec<ListenerRoute>),
    DrainListeners,
}

struct OwnedResponse {
    source: String,
    packet: Packet<WireResponse>,
}

enum IoEvent {
    Connected(String),
    Disconnected(Option<String>),
    Line {
        line: WireLine,
        packet: Result<OwnedResponse, DecodeError>,
    },
    ListenerValues(Vec<ListenerValue>),
    Error(String),
}

struct IoState {
    port: Option<Box<dyn serialport::SerialPort>>,
    reader: LineBuffer,
    writes: VecDeque<Vec<u8>>,
    write_offset: usize,
    listeners: Vec<ListenerRoute>,
    listener_updates: Vec<Vec<Option<ListenerValue>>>,
}

impl IoState {
    fn new() -> Self {
        Self {
            port: None,
            reader: LineBuffer::new(),
            writes: VecDeque::new(),
            write_offset: 0,
            listeners: Vec::new(),
            listener_updates: Vec::new(),
        }
    }

    fn clear_listeners(&mut self) {
        self.listeners.clear();
        self.listener_updates.clear();
    }
}

fn io_worker(commands: Receiver<IoCommand>, events: SyncSender<IoEvent>) {
    let mut state = IoState::new();
    let mut buffer = [0u8; 64];

    loop {
        if state.port.is_none() {
            let Ok(command) = commands.recv() else {
                return;
            };
            handle_io_command(command, &mut state, &events);
        } else {
            loop {
                match commands.try_recv() {
                    Ok(command) => handle_io_command(command, &mut state, &events),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }
        }

        if state.port.is_none() {
            continue;
        }

        if let Some(bytes) = state.writes.front() {
            let result = state
                .port
                .as_mut()
                .expect("connected serial port")
                .write(&bytes[state.write_offset..]);
            match result {
                Ok(written) => {
                    state.write_offset += written;
                    if state.write_offset == bytes.len() {
                        state.writes.pop_front();
                        state.write_offset = 0;
                    }
                }
                Err(error) if transient_io_error(&error) => {}
                Err(error) => {
                    state.port = None;
                    state.writes.clear();
                    state.write_offset = 0;
                    state.reader.clear();
                    state.clear_listeners();
                    let _ = events.send(IoEvent::Disconnected(Some(format!(
                        "Serial write failed: {error}"
                    ))));
                    continue;
                }
            }
        }

        match state
            .port
            .as_mut()
            .expect("connected serial port")
            .read(&mut buffer)
        {
            Ok(count) => {
                for &byte in &buffer[..count] {
                    match state.reader.push(byte) {
                        Ok(Some(line)) => {
                            let line = WireLine::new(line);
                            route_line(line, &events, &mut state);
                        }
                        Ok(None) => {}
                        Err(LineError::TooLong) => {
                            let _ = events.send(IoEvent::Error(format!(
                                "Incoming serial line exceeded {} bytes; discarded",
                                MAX_PACKET_LEN - 1
                            )));
                        }
                    }
                }
            }
            Err(error) if transient_io_error(&error) => {}
            Err(error) => {
                state.port = None;
                state.writes.clear();
                state.write_offset = 0;
                state.reader.clear();
                state.clear_listeners();
                let _ = events.send(IoEvent::Disconnected(Some(format!(
                    "Serial read failed: {error}"
                ))));
            }
        }
    }
}

fn route_line(wire_line: WireLine, events: &SyncSender<IoEvent>, state: &mut IoState) {
    let decoded = decode_message(wire_line.as_bytes()).and_then(|envelope| {
        decode_response(Packet {
            id: envelope.id,
            body: envelope.body,
        })
        .map(|packet| (envelope.route, packet))
    });
    match decoded {
        Ok((
            source,
            Packet {
                id,
                body: ProtocolResponse::Value { target, level },
            },
        )) => {
            if let Some(pin) = active_listener(&state.listeners, source, id, target) {
                coalesce_listener_update(&mut state.listener_updates, pin, wire_line, id, level);
            } else {
                let _ = events.send(IoEvent::Line {
                    line: wire_line,
                    packet: own_response(
                        source,
                        Packet {
                            id,
                            body: ProtocolResponse::Value { target, level },
                        },
                    ),
                });
            }
        }
        Ok((source, packet)) => {
            let _ = events.send(IoEvent::Line {
                line: wire_line,
                packet: own_response(source, packet),
            });
        }
        Err(error) => {
            let _ = events.send(IoEvent::Line {
                line: wire_line,
                packet: Err(error),
            });
        }
    }
}

fn own_response(
    source: &[u8],
    packet: Packet<ProtocolResponse<&[u8], &[u8]>>,
) -> Result<OwnedResponse, DecodeError> {
    let malformed = || DecodeError {
        id: Some(packet.id),
        kind: DecodeErrorKind::Malformed,
    };
    let body = packet.body.try_map(
        |target| {
            core::str::from_utf8(target)
                .map(str::to_owned)
                .map_err(|_| malformed())
        },
        |data| {
            core::str::from_utf8(data)
                .map(str::to_owned)
                .map_err(|_| malformed())
        },
    )?;
    Ok(OwnedResponse {
        source: String::from_utf8_lossy(source).into_owned(),
        packet: Packet {
            id: packet.id,
            body,
        },
    })
}

fn active_listener(
    routes: &[ListenerRoute],
    source: &[u8],
    id: RequestId,
    target: &[u8],
) -> Option<PinKey> {
    routes
        .iter()
        .find(|route| route.name.as_ref() == source)?
        .pins
        .iter()
        .find(|pin| pin.id == id && pin.token.as_ref() == target)
        .map(|pin| pin.key)
}

fn listener_is_configured(routes: &[ListenerRoute], key: PinKey, id: RequestId) -> bool {
    routes
        .iter()
        .any(|route| route.pins.iter().any(|pin| pin.key == key && pin.id == id))
}

fn coalesce_listener_update(
    updates: &mut [Vec<Option<ListenerValue>>],
    pin: PinKey,
    line: WireLine,
    id: RequestId,
    level: Level,
) {
    let slot = &mut updates[pin.route.0][pin.index];
    let coalesced = slot.map_or(0, |previous| previous.coalesced.saturating_add(1));
    *slot = Some(ListenerValue {
        line,
        id,
        pin,
        level,
        coalesced,
    });
}

fn handle_io_command(command: IoCommand, state: &mut IoState, events: &SyncSender<IoEvent>) {
    match command {
        IoCommand::Connect(name) => {
            state.writes.clear();
            state.write_offset = 0;
            state.reader.clear();
            state.clear_listeners();
            match serialport::new(&name, 115_200)
                .timeout(Duration::from_millis(20))
                .open()
            {
                Ok(opened) => {
                    state.port = Some(opened);
                    let _ = events.send(IoEvent::Connected(name));
                }
                Err(error) => {
                    state.port = None;
                    let _ = events.send(IoEvent::Error(format!("Could not open {name}: {error}")));
                }
            }
        }
        IoCommand::Disconnect => {
            state.port = None;
            state.writes.clear();
            state.write_offset = 0;
            state.reader.clear();
            state.clear_listeners();
            let _ = events.send(IoEvent::Disconnected(None));
        }
        IoCommand::Write(bytes) => {
            if state.port.is_some() {
                state.writes.push_back(bytes);
            }
        }
        IoCommand::Listeners(routes) => {
            let mut updates: Vec<Vec<Option<ListenerValue>>> = routes
                .iter()
                .map(|route| vec![None; route.pin_count])
                .collect();
            for update in state
                .listener_updates
                .iter_mut()
                .flat_map(|route| route.iter_mut())
                .filter_map(Option::take)
            {
                if listener_is_configured(&routes, update.pin, update.id) {
                    updates[update.pin.route.0][update.pin.index] = Some(update);
                }
            }
            state.listener_updates = updates;
            state.listeners = routes;
        }
        IoCommand::DrainListeners => {
            let updates: Vec<ListenerValue> = state
                .listener_updates
                .iter_mut()
                .flat_map(|route| route.iter_mut())
                .filter_map(Option::take)
                .collect();
            if !updates.is_empty() {
                let _ = events.send(IoEvent::ListenerValues(updates));
            }
        }
    }
}

fn transient_io_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (DeviceSession, RouteKey, RouteKey, PinKey, PinKey, BankKey) {
        let mut connection = DeviceSession::spawn(&["SAM", "LPC"]);
        let sam = connection.route_key("SAM").unwrap();
        let lpc = connection.route_key("LPC").unwrap();
        connection.routes[sam.0].map = Some(RouteMap {
            banks: vec![BankInfo {
                token: "PIOA".into(),
            }],
            pins: vec![
                RoutePin {
                    info: PinInfo {
                        token: "PA00".into(),
                        package_pin: Some(102),
                        bank: BankKey {
                            route: sam,
                            index: 0,
                        },
                        bit: 0,
                        capabilities: PinCapabilities::GPIO,
                    },
                    state: PinState::UNSET,
                    listener_id: None,
                },
                RoutePin {
                    info: PinInfo {
                        token: "PA01".into(),
                        package_pin: Some(99),
                        bank: BankKey {
                            route: sam,
                            index: 0,
                        },
                        bit: 1,
                        capabilities: PinCapabilities::GPIO,
                    },
                    state: PinState::UNSET,
                    listener_id: None,
                },
            ],
        });
        connection.routes[lpc.0].map = Some(RouteMap {
            banks: vec![BankInfo {
                token: "PIO2".into(),
            }],
            pins: vec![RoutePin {
                info: PinInfo {
                    token: "PIO2_3".into(),
                    package_pin: Some(38),
                    bank: BankKey {
                        route: lpc,
                        index: 0,
                    },
                    bit: 3,
                    capabilities: PinCapabilities::INPUT,
                },
                state: PinState::UNSET,
                listener_id: None,
            }],
        });
        let pa00 = connection.pin_key(sam, "PA00").unwrap();
        let lpc23 = connection.pin_key(lpc, "PIO2_3").unwrap();
        let pioa = connection.bank_key(sam, "PIOA").unwrap();
        (connection, sam, lpc, pa00, lpc23, pioa)
    }

    fn request_id(raw: u16) -> RequestId {
        RequestId::new(raw).unwrap()
    }
    fn line_id(line: &str) -> RequestId {
        request_id(line[..3].parse().unwrap())
    }

    fn incoming(source: &str, id: RequestId, body: WireResponse) -> OwnedResponse {
        OwnedResponse {
            source: source.into(),
            packet: Packet { id, body },
        }
    }

    #[test]
    fn route_request_encoding_uses_discovered_targets_and_global_ids() {
        let (mut connection, sam, lpc, pa00, lpc23, _) = setup();
        let (sam_id, sam_wire) = connection
            .prepare(
                sam,
                Request::Get {
                    target: Target::Pin(pa00),
                },
            )
            .unwrap();
        let (lpc_id, lpc_wire) = connection
            .prepare(
                lpc,
                Request::Get {
                    target: Target::Pin(lpc23),
                },
            )
            .unwrap();
        assert_eq!(sam_id, request_id(1));
        assert_eq!(lpc_id, request_id(2));
        assert_eq!(sam_wire, b"001 SAM GET PA00 OK?\n");
        assert_eq!(lpc_wire, b"002 LPC GET PIO2_3 OK?\n");
    }

    #[test]
    fn route_normal_response_source_is_checked_without_retiring_request() {
        let (mut connection, sam, _, _, _, _) = setup();
        let (id, _) = connection.prepare(sam, Request::Hello).unwrap();
        assert!(
            connection
                .received(incoming("LPC", id, WireResponse::Hello))
                .unwrap_err()
                .contains("expected SAM")
        );
        assert!(connection.pending[id.slot()].is_some());
        assert_eq!(
            connection
                .received(incoming("SAM", id, WireResponse::Hello))
                .unwrap(),
            DeviceEvent::Hello { route: sam }
        );
        assert!(connection.pending[id.slot()].is_none());
    }

    #[test]
    fn route_intermediate_error_can_retire_downstream_request() {
        let (mut connection, _, lpc, _, _, _) = setup();
        let (id, _) = connection.prepare(lpc, Request::Hello).unwrap();
        assert_eq!(
            connection
                .received(incoming(
                    "SAM",
                    id,
                    WireResponse::Error(ProtocolResponseError::RouteDown {
                        next_hop: "LPC".into(),
                    }),
                ))
                .unwrap(),
            DeviceEvent::DeviceError {
                route: lpc,
                source: "SAM".into(),
                error: ProtocolResponseError::RouteDown {
                    next_hop: "LPC".into(),
                },
            }
        );
        assert!(connection.pending[id.slot()].is_none());
    }

    #[test]
    fn map_stream_builds_dynamic_route_state_only_on_ack() {
        let (mut connection, sam, _, _, _, _) = setup();
        connection.routes[sam.0].map = None;
        let (id, wire) = connection.prepare(sam, Request::Map).unwrap();
        assert_eq!(wire, b"001 SAM MAP\n");
        assert_eq!(
            request_lifetime(Request::Map),
            RequestLifetime::StreamUntilAck
        );

        for body in [
            WireResponse::MapBank {
                bank: "GPIO0".into(),
            },
            WireResponse::MapBank {
                bank: "GPIO1".into(),
            },
            WireResponse::MapPin {
                target: "P0_7".into(),
                package_pin: None,
                bank: "GPIO0".into(),
                bit: 7,
                capabilities: PinCapabilities::INPUT_PULLUP,
            },
            WireResponse::MapPin {
                target: "LED_A".into(),
                package_pin: Some(48),
                bank: "GPIO1".into(),
                bit: 3,
                capabilities: PinCapabilities::GPIO,
            },
        ] {
            assert_eq!(
                connection.received(incoming("SAM", id, body)).unwrap(),
                DeviceEvent::Untracked
            );
            assert!(connection.routes[sam.0].map.is_none());
            assert!(connection.pending[id.slot()].is_some());
        }

        assert_eq!(
            connection
                .received(incoming("SAM", id, WireResponse::Ack))
                .unwrap(),
            DeviceEvent::MapReady { route: sam }
        );
        assert!(connection.pending[id.slot()].is_none());
        assert_eq!(connection.banks(sam).count(), 2);
        assert_eq!(connection.pins(sam).count(), 2);
        let led = connection.pin_key(sam, "LED_A").unwrap();
        assert_eq!(connection.pin_info(led).unwrap().package_pin, Some(48));
        assert!(connection.pin_info(led).unwrap().capabilities.output());
    }

    #[test]
    fn malformed_correlated_response_retires_partial_map() {
        let (mut connection, sam, _, _, _, _) = setup();
        connection.routes[sam.0].map = None;
        let (id, _) = connection.prepare(sam, Request::Map).unwrap();
        connection.routes[sam.0]
            .discovery
            .as_mut()
            .unwrap()
            .bank("PIOA".into())
            .unwrap();

        let (events, received) = mpsc::sync_channel(1);
        connection.events = received;
        events
            .send(IoEvent::Line {
                line: WireLine::new(b"001 SAM MAP PIN broken"),
                packet: Err(DecodeError {
                    id: Some(id),
                    kind: DecodeErrorKind::Malformed,
                }),
            })
            .unwrap();

        let Some(Event::Received { event, .. }) = connection.next_event() else {
            panic!("expected malformed response event");
        };
        assert!(event.unwrap_err().contains("Malformed response"));
        assert!(connection.pending[id.slot()].is_none());
        assert!(connection.routes[sam.0].discovery.is_none());
    }

    #[test]
    fn map_validation_failure_retires_request_and_discards_partial_map() {
        let (mut connection, sam, _, _, _, _) = setup();
        connection.routes[sam.0].map = None;
        let (id, _) = connection.prepare(sam, Request::Map).unwrap();
        connection
            .received(incoming(
                "SAM",
                id,
                WireResponse::MapBank {
                    bank: "PIOA".into(),
                },
            ))
            .unwrap();

        let error = connection
            .received(incoming(
                "SAM",
                id,
                WireResponse::MapPin {
                    target: "PA00".into(),
                    package_pin: Some(102),
                    bank: "MISSING".into(),
                    bit: 0,
                    capabilities: PinCapabilities::GPIO,
                },
            ))
            .unwrap_err();

        assert!(error.contains("unknown bank MISSING"));
        assert!(connection.pending[id.slot()].is_none());
        assert!(connection.routes[sam.0].discovery.is_none());
        assert!(connection.routes[sam.0].map.is_none());
    }

    #[test]
    fn route_target_keys_cannot_cross_routes() {
        let (mut connection, sam, lpc, _, lpc23, _) = setup();
        let error = connection
            .prepare(
                sam,
                Request::Get {
                    target: Target::Pin(lpc23),
                },
            )
            .unwrap_err();
        assert_eq!(error, "Target belongs to another route");
        assert!(connection.prepare(lpc, Request::Map).is_ok());
        assert!(connection.prepare(lpc, Request::Map).is_err());
    }

    #[test]
    fn route_typed_requests_do_not_interrupt_map_discovery() {
        let (mut connection, sam, _, _, _, _) = setup();
        connection.routes[sam.0].map = None;
        let (map_id, _) = connection.prepare(sam, Request::Map).unwrap();

        assert_eq!(
            connection.prepare(sam, Request::Hello).unwrap_err(),
            "SAM pin-map discovery is still in progress"
        );
        assert!(connection.pending[map_id.slot()].is_some());
        assert!(connection.routes[sam.0].discovery.is_some());
    }

    #[test]
    fn mode_change_followups_update_session_state_once() {
        let (mut connection, _, _, pa00, _, _) = setup();

        let sent = connection.change_mode(pa00, Mode::InputPullup).unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            connection.pin_state(pa00).unwrap().target_mode,
            Some(Mode::InputPullup)
        );

        let direction_id = line_id(&sent[0]);
        let DeviceEvent::Ack {
            sent: Some(pullup), ..
        } = connection
            .received(incoming("SAM", direction_id, WireResponse::Ack))
            .unwrap()
        else {
            panic!("direction ACK should schedule pull-up configuration");
        };
        assert!(connection.pending[direction_id.slot()].is_none());

        let pullup_id = line_id(&pullup);
        let DeviceEvent::Ack {
            sent: Some(read), ..
        } = connection
            .received(incoming("SAM", pullup_id, WireResponse::Ack))
            .unwrap()
        else {
            panic!("pull-up ACK should schedule an input read");
        };
        assert!(connection.pending[pullup_id.slot()].is_none());
        assert_eq!(
            connection.pin_state(pa00).unwrap(),
            PinState {
                mode: Some(Mode::InputPullup),
                target_mode: None,
                level: None,
                listener: ListenerState::Off,
                value_pending: true,
            }
        );

        let read_id = line_id(&read);
        connection
            .received(incoming(
                "SAM",
                read_id,
                WireResponse::Value {
                    target: "PA00".into(),
                    level: Level::High,
                },
            ))
            .unwrap();
        assert!(connection.pending[read_id.slot()].is_none());
        assert_eq!(connection.pin_state(pa00).unwrap().level, Some(Level::High));
        assert!(!connection.pin_state(pa00).unwrap().value_pending);
    }

    #[test]
    fn output_mode_stops_listener_and_initializes_low() {
        let (mut connection, _, _, pa00, _, _) = setup();
        connection.route_pin_mut(pa00).unwrap().state = PinState {
            mode: Some(Mode::Input),
            target_mode: None,
            level: Some(Level::High),
            listener: ListenerState::On,
            value_pending: false,
        };

        let sent = connection.change_mode(pa00, Mode::Output).unwrap();
        assert_eq!(sent.len(), 2);
        assert!(sent[0].contains("LSN PA00 OFF"));
        assert!(sent[1].contains("DIR PA00 OUT"));
        assert_eq!(
            connection.pin_state(pa00).unwrap().listener,
            ListenerState::Disabling
        );
        assert_eq!(
            connection.pin_state(pa00).unwrap().target_mode,
            Some(Mode::Output)
        );

        connection
            .received(incoming("SAM", line_id(&sent[0]), WireResponse::Ack))
            .unwrap();
        let DeviceEvent::Ack {
            sent: Some(pullup), ..
        } = connection
            .received(incoming("SAM", line_id(&sent[1]), WireResponse::Ack))
            .unwrap()
        else {
            panic!("direction ACK should clear pull-up state");
        };
        connection
            .received(incoming("SAM", line_id(&pullup), WireResponse::Ack))
            .unwrap();

        assert_eq!(
            connection.pin_state(pa00).unwrap(),
            PinState {
                mode: Some(Mode::Output),
                target_mode: None,
                level: Some(Level::Low),
                listener: ListenerState::Off,
                value_pending: false,
            }
        );
    }

    #[test]
    fn failed_followup_send_does_not_leak_completed_request() {
        let (mut connection, sam, _, pa00, _, _) = setup();
        connection.route_pin_mut(pa00).unwrap().state.target_mode = Some(Mode::InputPullup);
        let (direction_id, _) = connection
            .prepare(
                sam,
                Request::Direction {
                    target: Target::Pin(pa00),
                    direction: Direction::Input,
                },
            )
            .unwrap();
        let (commands, receiver) = mpsc::channel();
        drop(receiver);
        connection.commands = commands;

        let error = connection
            .received(incoming("SAM", direction_id, WireResponse::Ack))
            .unwrap_err();

        assert_eq!(error, "Serial worker stopped");
        assert!(connection.pending[direction_id.slot()].is_none());
        assert!(connection.pending.iter().all(Option::is_none));
        assert_eq!(connection.pin_state(pa00).unwrap().target_mode, None);
    }

    #[test]
    fn routing_error_retires_staged_state_for_destination_route() {
        let (mut connection, _, _, _, lpc23, _) = setup();
        let sent = connection.change_mode(lpc23, Mode::Input).unwrap();
        let id = line_id(&sent[0]);

        connection
            .received(incoming(
                "SAM",
                id,
                WireResponse::Error(ProtocolResponseError::RouteDown {
                    next_hop: "LPC".into(),
                }),
            ))
            .unwrap();

        assert!(connection.pending[id.slot()].is_none());
        assert_eq!(connection.pin_state(lpc23).unwrap().target_mode, None);
        assert_eq!(connection.pin_state(lpc23).unwrap().mode, None);
    }

    #[test]
    fn resetting_one_route_preserves_unrelated_route_state_and_requests() {
        let (mut connection, sam, lpc, _, lpc23, _) = setup();
        connection.route_pin_mut(lpc23).unwrap().state.mode = Some(Mode::Input);
        let (lpc_request, _) = connection.prepare(lpc, Request::Hello).unwrap();
        let (sam_reset, _) = connection.prepare(sam, Request::Bye).unwrap();

        assert_eq!(
            connection
                .received(incoming("SAM", sam_reset, WireResponse::Bye))
                .unwrap(),
            DeviceEvent::Bye { route: sam }
        );

        assert!(connection.pending[sam_reset.slot()].is_none());
        assert!(connection.pending[lpc_request.slot()].is_some());
        assert_eq!(connection.pin_state(lpc23).unwrap().mode, Some(Mode::Input));
    }

    #[test]
    fn listener_lifetime_persists_and_grouped_streams_end_at_ack() {
        let (mut connection, sam, _, pa00, _, pioa) = setup();
        let direction = Request::Direction {
            target: Target::Pin(pa00),
            direction: Direction::Input,
        };
        let (direction_id, _) = connection.prepare(sam, direction).unwrap();
        connection
            .received(incoming("SAM", direction_id, WireResponse::Ack))
            .unwrap();

        let listen = Request::Listen {
            target: Target::Pin(pa00),
            enabled: true,
        };
        let (listen_id, _) = connection.prepare(sam, listen).unwrap();
        connection
            .received(incoming("SAM", listen_id, WireResponse::Ack))
            .unwrap();
        assert!(connection.pending[listen_id.slot()].is_some());
        assert_eq!(
            connection
                .received(incoming(
                    "SAM",
                    listen_id,
                    WireResponse::Value {
                        target: "PA00".into(),
                        level: Level::High,
                    },
                ))
                .unwrap(),
            DeviceEvent::PinValue {
                pin: pa00,
                level: Level::High,
            }
        );
        assert!(connection.pending[listen_id.slot()].is_some());

        let (get_id, _) = connection
            .prepare(
                sam,
                Request::Get {
                    target: Target::Bank(pioa),
                },
            )
            .unwrap();
        connection
            .received(incoming(
                "SAM",
                get_id,
                WireResponse::Value {
                    target: "PA00".into(),
                    level: Level::Low,
                },
            ))
            .unwrap();
        assert!(connection.pending[get_id.slot()].is_some());
        connection
            .received(incoming("SAM", get_id, WireResponse::Ack))
            .unwrap();
        assert!(connection.pending[get_id.slot()].is_none());
    }

    #[test]
    fn listener_updates_coalesce_by_route_and_pin_key() {
        let (_connection, sam, lpc, pa00, lpc23, _) = setup();
        let routes = vec![
            ListenerRoute {
                name: b"SAM".as_slice().into(),
                pin_count: 2,
                pins: vec![ListenerPin {
                    key: pa00,
                    token: b"PA00".as_slice().into(),
                    id: request_id(8),
                }],
            },
            ListenerRoute {
                name: b"LPC".as_slice().into(),
                pin_count: 1,
                pins: vec![ListenerPin {
                    key: lpc23,
                    token: b"PIO2_3".as_slice().into(),
                    id: request_id(9),
                }],
            },
        ];
        let mut updates = vec![vec![None; 2], vec![None; 1]];
        assert_eq!(
            active_listener(&routes, b"SAM", request_id(8), b"PA00"),
            Some(pa00)
        );
        assert_eq!(
            active_listener(&routes, b"LPC", request_id(9), b"PIO2_3"),
            Some(lpc23)
        );
        assert_eq!(
            active_listener(&routes, b"SAM", request_id(9), b"PIO2_3"),
            None
        );

        coalesce_listener_update(
            &mut updates,
            pa00,
            WireLine::new(b"008 SAM HYG PA00 LOW <3"),
            request_id(8),
            Level::Low,
        );
        coalesce_listener_update(
            &mut updates,
            pa00,
            WireLine::new(b"008 SAM HYG PA00 HIGH <3"),
            request_id(8),
            Level::High,
        );
        coalesce_listener_update(
            &mut updates,
            lpc23,
            WireLine::new(b"009 LPC HYG PIO2_3 HIGH <3"),
            request_id(9),
            Level::High,
        );
        assert_eq!(updates[sam.0][pa00.index].unwrap().coalesced, 1);
        assert_eq!(updates[sam.0][pa00.index].unwrap().level, Level::High);
        assert_eq!(updates[lpc.0][lpc23.index].unwrap().coalesced, 0);
        assert_eq!(updates[lpc.0][lpc23.index].unwrap().pin, lpc23);
    }

    #[test]
    fn listener_map_discards_stale_updates_when_snapshot_changes() {
        let (events, received) = mpsc::sync_channel(2);
        let mut state = IoState::new();
        let route = RouteKey(0);
        let pin = PinKey { route, index: 0 };
        handle_io_command(
            IoCommand::Listeners(vec![ListenerRoute {
                name: b"SAM".as_slice().into(),
                pin_count: 1,
                pins: vec![ListenerPin {
                    key: pin,
                    token: b"PA00".as_slice().into(),
                    id: request_id(8),
                }],
            }]),
            &mut state,
            &events,
        );
        coalesce_listener_update(
            &mut state.listener_updates,
            pin,
            WireLine::new(b"008 SAM HYG PA00 HIGH <3"),
            request_id(8),
            Level::High,
        );
        handle_io_command(
            IoCommand::Listeners(vec![ListenerRoute {
                name: b"SAM".as_slice().into(),
                pin_count: 1,
                pins: vec![ListenerPin {
                    key: pin,
                    token: b"PA00".as_slice().into(),
                    id: request_id(9),
                }],
            }]),
            &mut state,
            &events,
        );
        handle_io_command(IoCommand::DrainListeners, &mut state, &events);
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn listener_snapshot_preserves_updates_for_unchanged_listener() {
        let (events, received) = mpsc::sync_channel(2);
        let mut state = IoState::new();
        let route = RouteKey(0);
        let first = PinKey { route, index: 0 };
        let second = PinKey { route, index: 1 };
        handle_io_command(
            IoCommand::Listeners(vec![ListenerRoute {
                name: b"SAM".as_slice().into(),
                pin_count: 2,
                pins: vec![ListenerPin {
                    key: first,
                    token: b"PA00".as_slice().into(),
                    id: request_id(8),
                }],
            }]),
            &mut state,
            &events,
        );
        coalesce_listener_update(
            &mut state.listener_updates,
            first,
            WireLine::new(b"008 SAM HYG PA00 HIGH <3"),
            request_id(8),
            Level::High,
        );

        handle_io_command(
            IoCommand::Listeners(vec![ListenerRoute {
                name: b"SAM".as_slice().into(),
                pin_count: 2,
                pins: vec![
                    ListenerPin {
                        key: first,
                        token: b"PA00".as_slice().into(),
                        id: request_id(8),
                    },
                    ListenerPin {
                        key: second,
                        token: b"PA01".as_slice().into(),
                        id: request_id(9),
                    },
                ],
            }]),
            &mut state,
            &events,
        );
        handle_io_command(IoCommand::DrainListeners, &mut state, &events);

        let Ok(IoEvent::ListenerValues(values)) = received.try_recv() else {
            panic!("expected preserved listener update");
        };
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].pin, first);
        assert_eq!(values[0].level, Level::High);
    }

    #[test]
    fn request_ids_wrap_and_skip_persistent_listener() {
        let (mut connection, sam, _, pa00, _, _) = setup();
        connection.routes[sam.0].map.as_mut().unwrap().pins[pa00.index]
            .state
            .mode = Some(Mode::Input);
        let (listener, _) = connection
            .prepare(
                sam,
                Request::Listen {
                    target: Target::Pin(pa00),
                    enabled: true,
                },
            )
            .unwrap();
        connection
            .received(incoming("SAM", listener, WireResponse::Ack))
            .unwrap();
        for _ in 2..=RequestId::MAX {
            let (id, _) = connection.prepare(sam, Request::Hello).unwrap();
            connection
                .received(incoming("SAM", id, WireResponse::Hello))
                .unwrap();
        }
        let (id, _) = connection.prepare(sam, Request::Hello).unwrap();
        assert_eq!(id, request_id(2));
        assert!(connection.pending[listener.slot()].is_some());
    }

    #[test]
    fn transient_serial_errors_are_retryable() {
        for kind in [
            io::ErrorKind::TimedOut,
            io::ErrorKind::WouldBlock,
            io::ErrorKind::Interrupted,
        ] {
            assert!(transient_io_error(&io::Error::from(kind)));
        }
        assert!(!transient_io_error(&io::Error::from(
            io::ErrorKind::BrokenPipe
        )));
    }

    #[test]
    fn stale_write_after_disconnect_is_dropped_without_error() {
        let (events, received) = mpsc::sync_channel(1);
        let mut state = IoState::new();
        handle_io_command(
            IoCommand::Write(b"001 SAM HAI\n".to_vec()),
            &mut state,
            &events,
        );
        assert!(state.writes.is_empty());
        assert!(received.try_recv().is_err());
    }
}
