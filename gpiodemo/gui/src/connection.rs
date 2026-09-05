use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    sync::mpsc::{self, Receiver, Sender, SyncSender},
    thread,
    time::Duration,
};

use da_vinci_protocol::{
    DecodeError, Level, LineBuffer, LineError, MAX_PACKET_ID, MAX_PACKET_LEN, Packet, Pin,
    PinTable, PinTarget, Query, QueryValue, Request, Response, ResponseError, decode_response,
    decode_response_envelope, encode_request,
};

const EVENT_QUEUE_CAPACITY: usize = 1_024;

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
    id: u16,
    pub(super) pin: Pin,
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
    Hello,
    Status,
    Ack(Request),
    PinValue {
        pin: Pin,
        level: Level,
    },
    PinState {
        pin: Pin,
        what: Query,
        value: QueryValue,
    },
    DeviceError {
        request: Request,
        error: ResponseError<String>,
    },
    Unknown {
        request: Request,
    },
    Bye,
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
}

pub(super) struct Connection {
    next_id: u16,
    pending: [Option<Request>; MAX_PACKET_ID as usize + 1],
    inputs: PinTable<bool>,
    listeners: PinTable<Option<u16>>,
    commands: Sender<IoCommand>,
    events: Receiver<IoEvent>,
}

impl Connection {
    pub(super) fn spawn() -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::sync_channel(EVENT_QUEUE_CAPACITY);
        thread::spawn(move || io_worker(command_rx, event_tx));
        Self {
            next_id: 1,
            pending: [None; MAX_PACKET_ID as usize + 1],
            inputs: PinTable::filled(false),
            listeners: PinTable::filled(None),
            commands: command_tx,
            events: event_rx,
        }
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

    pub(super) fn send(&mut self, request: Request) -> Result<String, String> {
        let (id, bytes) = self.prepare(request)?;
        let line = String::from_utf8_lossy(&bytes)
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        if let Err(error) = self.send_command(IoCommand::Write(bytes)) {
            self.pending[id as usize] = None;
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
                    let event = packet
                        .map(|packet| self.received(packet))
                        .map_err(|error| format!("Malformed response: {error:?}"));
                    return Some(Event::Received {
                        line: line.text(),
                        event,
                    });
                }
                Ok(IoEvent::ListenerValues(mut values)) => {
                    values.retain(|value| self.listeners[value.pin] == Some(value.id));
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

    fn send_command(&self, command: IoCommand) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|_| "Serial worker stopped".into())
    }

    fn prepare(&mut self, request: Request) -> Result<(u16, Vec<u8>), String> {
        let id = self.allocate_id()?;
        let mut buffer = [0u8; MAX_PACKET_LEN];
        let len = encode_request(Packet { id, body: request }, b"SAM", &mut buffer)
            .expect("protocol request always fits fixed packet buffer");
        self.pending[id as usize] = Some(request);
        Ok((id, buffer[..len].to_vec()))
    }

    fn allocate_id(&mut self) -> Result<u16, String> {
        for _ in 0..MAX_PACKET_ID {
            let id = self.next_id;
            self.next_id = if id == MAX_PACKET_ID { 1 } else { id + 1 };
            if self.pending[id as usize].is_none() {
                return Ok(id);
            }
        }
        Err("All 999 request IDs are still in use".into())
    }

    fn received(&mut self, packet: Packet<Response<String>>) -> DeviceEvent {
        if packet.body == Response::Bye {
            self.clear();
            return DeviceEvent::Bye;
        }

        let Some(request) = self.pending[packet.id as usize] else {
            return DeviceEvent::Untracked;
        };

        match packet.body {
            Response::Hello => {
                self.pending[packet.id as usize] = None;
                DeviceEvent::Hello
            }
            Response::Status => {
                self.pending[packet.id as usize] = None;
                DeviceEvent::Status
            }
            Response::Ack => self.ack(packet.id, request),
            Response::Value { pin, level } => {
                if self.listeners[pin] != Some(packet.id) && !is_grouped_get(request) {
                    self.pending[packet.id as usize] = None;
                }
                DeviceEvent::PinValue { pin, level }
            }
            Response::State { pin, what, value } => {
                if !is_grouped_query(request) {
                    self.pending[packet.id as usize] = None;
                }
                DeviceEvent::PinState { pin, what, value }
            }
            Response::Error(error) => {
                self.retire(packet.id);
                DeviceEvent::DeviceError { request, error }
            }
            Response::Unknown => {
                self.retire(packet.id);
                DeviceEvent::Unknown { request }
            }
            Response::Bye => unreachable!(),
        }
    }

    fn clear(&mut self) {
        self.pending.fill(None);
        self.inputs.fill(false);
        self.listeners.fill(None);
        self.sync_listeners();
    }

    fn ack(&mut self, id: u16, request: Request) -> DeviceEvent {
        if let Request::Direction { target, direction } = request {
            for pin in target.available_pins() {
                self.inputs[pin] = direction == da_vinci_protocol::Direction::Input;
            }
        }

        if let Request::Listen { target, enabled } = request {
            let previous = self.listeners;
            if enabled {
                let mut listening = false;
                for pin in target.pins() {
                    if self.inputs[pin] {
                        listening = true;
                        self.listeners[pin] = Some(id);
                    }
                }
                self.release_removed_listeners(previous);
                self.sync_listeners();
                if listening {
                    return DeviceEvent::Ack(request);
                }
            }

            for pin in target.pins() {
                self.listeners[pin] = None;
            }
            self.release_removed_listeners(previous);
            self.sync_listeners();
        }

        self.pending[id as usize] = None;
        DeviceEvent::Ack(request)
    }

    fn release_listener(&mut self, id: u16) {
        if !self.listeners.iter().any(|listener| *listener == Some(id)) {
            self.pending[id as usize] = None;
        }
    }

    fn release_removed_listeners(&mut self, previous: PinTable<Option<u16>>) {
        for id in previous.iter().copied().flatten() {
            self.release_listener(id);
        }
    }

    fn retire(&mut self, id: u16) {
        for listener in self.listeners.iter_mut() {
            if *listener == Some(id) {
                *listener = None;
            }
        }
        self.sync_listeners();
        self.pending[id as usize] = None;
    }

    fn sync_listeners(&self) {
        let _ = self
            .commands
            .send(IoCommand::Listeners(Box::new(self.listeners)));
    }
}

fn is_grouped_get(request: Request) -> bool {
    matches!(
        request,
        Request::Get {
            target: PinTarget::Bank(_) | PinTarget::All
        }
    )
}

fn is_grouped_query(request: Request) -> bool {
    matches!(
        request,
        Request::Query {
            target: PinTarget::Bank(_) | PinTarget::All,
            ..
        }
    )
}

enum IoCommand {
    Connect(String),
    Disconnect,
    Write(Vec<u8>),
    Listeners(Box<PinTable<Option<u16>>>),
    DrainListeners,
}

enum IoEvent {
    Connected(String),
    Disconnected(Option<String>),
    Line {
        line: WireLine,
        packet: Result<Packet<Response<String>>, DecodeError>,
    },
    ListenerValues(Vec<ListenerValue>),
    Error(String),
}

struct IoState {
    port: Option<Box<dyn serialport::SerialPort>>,
    reader: LineBuffer,
    writes: VecDeque<Vec<u8>>,
    write_offset: usize,
    listeners: PinTable<Option<u16>>,
    listener_updates: PinTable<Option<ListenerValue>>,
}

impl IoState {
    fn new() -> Self {
        Self {
            port: None,
            reader: LineBuffer::new(),
            writes: VecDeque::new(),
            write_offset: 0,
            listeners: PinTable::filled(None),
            listener_updates: PinTable::filled(None),
        }
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
                            route_line(line, &events, &state.listeners, &mut state.listener_updates)
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
                let _ = events.send(IoEvent::Disconnected(Some(format!(
                    "Serial read failed: {error}"
                ))));
            }
        }
    }
}

fn route_line(
    line: &[u8],
    events: &SyncSender<IoEvent>,
    listeners: &PinTable<Option<u16>>,
    listener_updates: &mut PinTable<Option<ListenerValue>>,
) {
    let wire_line = WireLine::new(line);
    match decode_owned_response(line) {
        Ok(Packet {
            id,
            body: Response::Value { pin, level },
        }) if listeners[pin] == Some(id) => {
            coalesce_listener_update(listener_updates, pin, wire_line, id, level);
        }
        Ok(packet) => {
            let _ = events.send(IoEvent::Line {
                line: wire_line,
                packet: Ok(packet),
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

fn decode_owned_response(line: &[u8]) -> Result<Packet<Response<String>>, DecodeError> {
    let envelope = decode_response_envelope(line)?;
    let packet = decode_response(Packet {
        id: envelope.id,
        body: envelope.body,
    })?;
    let body = match packet.body {
        Response::Hello => Response::Hello,
        Response::Status => Response::Status,
        Response::Ack => Response::Ack,
        Response::Value { pin, level } => Response::Value { pin, level },
        Response::State { pin, what, value } => Response::State { pin, what, value },
        Response::Error(ResponseError::BadPacket) => Response::Error(ResponseError::BadPacket),
        Response::Error(ResponseError::Pin { pin, reason }) => {
            Response::Error(ResponseError::Pin { pin, reason })
        }
        Response::Error(ResponseError::NoRoute { destination }) => {
            Response::Error(ResponseError::NoRoute {
                destination: String::from_utf8_lossy(destination).into_owned(),
            })
        }
        Response::Unknown => Response::Unknown,
        Response::Bye => Response::Bye,
    };
    Ok(Packet {
        id: packet.id,
        body,
    })
}

fn coalesce_listener_update(
    updates: &mut PinTable<Option<ListenerValue>>,
    pin: Pin,
    line: WireLine,
    id: u16,
    level: Level,
) {
    let coalesced = updates[pin].map_or(0, |previous| previous.coalesced.saturating_add(1));
    updates[pin] = Some(ListenerValue {
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
            state.listeners.fill(None);
            state.listener_updates.fill(None);
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
            state.listeners.fill(None);
            state.listener_updates.fill(None);
            let _ = events.send(IoEvent::Disconnected(None));
        }
        IoCommand::Write(bytes) => {
            if state.port.is_some() {
                state.writes.push_back(bytes);
            }
        }
        IoCommand::Listeners(current) => {
            state.listeners = *current;
            for pin in Pin::all() {
                if state.listener_updates[pin]
                    .is_some_and(|update| state.listeners[pin] != Some(update.id))
                {
                    state.listener_updates[pin] = None;
                }
            }
        }
        IoCommand::DrainListeners => {
            let updates: Vec<_> = Pin::all()
                .filter_map(|pin| state.listener_updates[pin].take())
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
    use da_vinci_protocol::Direction;

    fn pin(index: u8) -> Pin {
        Pin::from_wire_index(index).unwrap()
    }

    fn response(id: u16, body: Response<String>) -> Packet<Response<String>> {
        Packet { id, body }
    }

    fn prepared(connection: &mut Connection, request: Request) -> (u16, Vec<u8>) {
        connection.prepare(request).unwrap()
    }

    fn initialize(connection: &mut Connection, index: u8) {
        let request = Request::Direction {
            target: PinTarget::Pin(pin(index)),
            direction: Direction::Input,
        };
        let (id, _) = prepared(connection, request);
        assert_eq!(
            connection.received(response(id, Response::Ack)),
            DeviceEvent::Ack(request)
        );
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

    #[test]
    fn listener_updates_coalesce_burst() {
        let target = pin(5);
        let mut listeners = PinTable::filled(None);
        listeners[target] = Some(8);
        let mut updates = PinTable::filled(None);
        let (events, received) = mpsc::sync_channel(1);

        for index in 0..EVENT_QUEUE_CAPACITY * 2 {
            let line = if index % 2 == 0 {
                b"008 SAM HYG PA05 LOW <3".as_slice()
            } else {
                b"008 SAM HYG PA05 HIGH <3".as_slice()
            };
            route_line(line, &events, &listeners, &mut updates);
        }

        assert!(received.try_recv().is_err());
        assert_eq!(updates.iter().filter(|update| update.is_some()).count(), 1);
        let update = updates[target].take().unwrap();
        assert_eq!(update.coalesced as usize, EVENT_QUEUE_CAPACITY * 2 - 1);
        assert_eq!(update.id, 8);
        assert_eq!(update.pin, target);
        assert_eq!(update.level, Level::High);
        assert_eq!(updates.iter().filter(|update| update.is_some()).count(), 0);

        for target in Pin::all() {
            coalesce_listener_update(
                &mut updates,
                target,
                WireLine::new(b"008 SAM HYG PA05 HIGH <3"),
                8,
                Level::High,
            );
        }
        assert_eq!(
            updates.iter().filter(|update| update.is_some()).count(),
            da_vinci_protocol::WIRE_PIN_COUNT as usize
        );
    }

    #[test]
    fn only_active_listener_values_are_coalescible() {
        let target = pin(5);
        let mut listeners = PinTable::filled(None);
        let mut updates = PinTable::filled(None);
        let (events, received) = mpsc::sync_channel(4);

        route_line(
            b"008 SAM HYG PA05 HIGH <3",
            &events,
            &listeners,
            &mut updates,
        );
        assert!(matches!(received.try_recv(), Ok(IoEvent::Line { .. })));
        assert!(updates[target].is_none());

        listeners[target] = Some(8);
        route_line(
            b"008 SAM HYG PA05 HIGH <3",
            &events,
            &listeners,
            &mut updates,
        );
        assert!(received.try_recv().is_err());
        assert!(updates[target].is_some());

        route_line(
            b"009 SAM HYG PA05 LOW <3",
            &events,
            &listeners,
            &mut updates,
        );
        assert!(matches!(received.try_recv(), Ok(IoEvent::Line { .. })));

        route_line(b"008 SAM KTHX <3", &events, &listeners, &mut updates);
        assert!(matches!(received.try_recv(), Ok(IoEvent::Line { .. })));
    }

    #[test]
    fn listener_map_discards_stale_updates_before_drain() {
        let target = pin(5);
        let (events, received) = mpsc::sync_channel(2);
        let mut state = IoState::new();

        state.listeners[target] = Some(8);
        coalesce_listener_update(
            &mut state.listener_updates,
            target,
            WireLine::new(b"008 SAM HYG PA05 HIGH <3"),
            8,
            Level::High,
        );

        let mut current = PinTable::filled(None);
        current[target] = Some(9);
        handle_io_command(IoCommand::Listeners(Box::new(current)), &mut state, &events);
        assert!(state.listener_updates[target].is_none());

        coalesce_listener_update(
            &mut state.listener_updates,
            target,
            WireLine::new(b"009 SAM HYG PA05 LOW <3"),
            9,
            Level::Low,
        );
        handle_io_command(IoCommand::DrainListeners, &mut state, &events);

        let Ok(IoEvent::ListenerValues(batch)) = received.try_recv() else {
            panic!("expected listener batch");
        };
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].id, 9);
        assert_eq!(batch[0].level, Level::Low);
        assert!(state.listener_updates[target].is_none());
    }

    #[test]
    fn request_ids_wrap_from_999_to_001() {
        let mut connection = Connection::spawn();
        for expected in 1..=MAX_PACKET_ID {
            let (id, _) = prepared(&mut connection, Request::Hello);
            assert_eq!(id, expected);
            connection.received(response(id, Response::Hello));
        }
        let (id, _) = prepared(&mut connection, Request::Hello);
        assert_eq!(id, 1);
    }

    #[test]
    fn wrap_skips_a_persistent_listener_id() {
        let mut connection = Connection::spawn();
        initialize(&mut connection, 5);
        let (listener, _) = prepared(
            &mut connection,
            Request::Listen {
                target: PinTarget::Pin(pin(5)),
                enabled: true,
            },
        );
        connection.received(response(listener, Response::Ack));

        for _ in 2..=MAX_PACKET_ID {
            let (id, _) = prepared(&mut connection, Request::Hello);
            connection.received(response(id, Response::Hello));
        }

        let (id, _) = prepared(&mut connection, Request::Hello);
        assert_eq!(id, 3);
        assert_eq!(
            connection.received(response(
                listener,
                Response::Value {
                    pin: pin(5),
                    level: Level::High,
                },
            )),
            DeviceEvent::PinValue {
                pin: pin(5),
                level: Level::High,
            }
        );
    }

    #[test]
    fn ordinary_requests_are_retired_after_response() {
        let mut connection = Connection::spawn();
        let (id, _) = prepared(
            &mut connection,
            Request::Get {
                target: PinTarget::Pin(pin(5)),
            },
        );
        assert_eq!(
            connection.received(response(
                id,
                Response::Value {
                    pin: pin(5),
                    level: Level::High,
                },
            )),
            DeviceEvent::PinValue {
                pin: pin(5),
                level: Level::High,
            }
        );
        assert!(matches!(
            connection.received(response(id, Response::Ack)),
            DeviceEvent::Untracked
        ));
    }

    #[test]
    fn successful_listener_id_persists_for_notifications() {
        let mut connection = Connection::spawn();
        initialize(&mut connection, 5);
        let (listener, _) = prepared(
            &mut connection,
            Request::Listen {
                target: PinTarget::Pin(pin(5)),
                enabled: true,
            },
        );
        assert_eq!(
            connection.received(response(listener, Response::Ack)),
            DeviceEvent::Ack(Request::Listen {
                target: PinTarget::Pin(pin(5)),
                enabled: true,
            })
        );
        for level in [Level::Low, Level::High] {
            assert_eq!(
                connection.received(response(listener, Response::Value { pin: pin(5), level },)),
                DeviceEvent::PinValue { pin: pin(5), level }
            );
        }
    }

    #[test]
    fn reenable_replaces_old_listener_only_after_ack() {
        let mut connection = Connection::spawn();
        initialize(&mut connection, 5);
        let (first, _) = prepared(
            &mut connection,
            Request::Listen {
                target: PinTarget::Pin(pin(5)),
                enabled: true,
            },
        );
        connection.received(response(first, Response::Ack));

        let (second, _) = prepared(
            &mut connection,
            Request::Listen {
                target: PinTarget::Pin(pin(5)),
                enabled: true,
            },
        );
        assert!(matches!(
            connection.received(response(
                first,
                Response::Value {
                    pin: pin(5),
                    level: Level::High,
                }
            )),
            DeviceEvent::PinValue { .. }
        ));
        connection.received(response(second, Response::Ack));
        assert!(matches!(
            connection.received(response(
                first,
                Response::Value {
                    pin: pin(5),
                    level: Level::Low,
                }
            )),
            DeviceEvent::Untracked
        ));
    }

    #[test]
    fn listener_off_retires_persistent_id() {
        let mut connection = Connection::spawn();
        initialize(&mut connection, 5);
        let (on, _) = prepared(
            &mut connection,
            Request::Listen {
                target: PinTarget::Pin(pin(5)),
                enabled: true,
            },
        );
        connection.received(response(on, Response::Ack));
        let (off, _) = prepared(
            &mut connection,
            Request::Listen {
                target: PinTarget::Pin(pin(5)),
                enabled: false,
            },
        );
        connection.received(response(off, Response::Ack));
        assert!(matches!(
            connection.received(response(
                on,
                Response::Value {
                    pin: pin(5),
                    level: Level::High,
                }
            )),
            DeviceEvent::Untracked
        ));
    }

    #[test]
    fn cya_clears_all_bookkeeping() {
        let mut connection = Connection::spawn();
        initialize(&mut connection, 5);
        let (listener, _) = prepared(
            &mut connection,
            Request::Listen {
                target: PinTarget::Pin(pin(5)),
                enabled: true,
            },
        );
        connection.received(response(listener, Response::Ack));
        let (bye, _) = prepared(&mut connection, Request::Bye);
        assert_eq!(
            connection.received(response(bye, Response::Bye)),
            DeviceEvent::Bye
        );
        assert!(matches!(
            connection.received(response(
                listener,
                Response::Value {
                    pin: pin(5),
                    level: Level::High,
                }
            )),
            DeviceEvent::Untracked
        ));
    }

    #[test]
    fn bulk_get_and_query_stay_pending_until_final_ack() {
        let mut connection = Connection::spawn();
        let (get, _) = prepared(
            &mut connection,
            Request::Get {
                target: PinTarget::All,
            },
        );
        assert!(matches!(
            connection.received(response(
                get,
                Response::Value {
                    pin: pin(0),
                    level: Level::High,
                }
            )),
            DeviceEvent::PinValue { .. }
        ));
        assert!(matches!(
            connection.received(response(
                get,
                Response::Value {
                    pin: pin(1),
                    level: Level::Low,
                }
            )),
            DeviceEvent::PinValue { .. }
        ));
        assert!(matches!(
            connection.received(response(get, Response::Ack)),
            DeviceEvent::Ack(Request::Get {
                target: PinTarget::All
            })
        ));
        assert!(matches!(
            connection.received(response(
                get,
                Response::Value {
                    pin: pin(2),
                    level: Level::Low,
                }
            )),
            DeviceEvent::Untracked
        ));

        let (query, _) = prepared(
            &mut connection,
            Request::Query {
                target: PinTarget::All,
                what: Query::Direction,
            },
        );
        assert!(matches!(
            connection.received(response(
                query,
                Response::State {
                    pin: pin(0),
                    what: Query::Direction,
                    value: QueryValue::Unset,
                }
            )),
            DeviceEvent::PinState { .. }
        ));
        assert!(matches!(
            connection.received(response(query, Response::Ack)),
            DeviceEvent::Ack(Request::Query {
                target: PinTarget::All,
                what: Query::Direction,
            })
        ));
    }

    #[test]
    fn empty_bulk_listener_does_not_leak_request_id() {
        let mut connection = Connection::spawn();
        let (id, _) = prepared(
            &mut connection,
            Request::Listen {
                target: PinTarget::All,
                enabled: true,
            },
        );

        assert_eq!(
            connection.received(response(id, Response::Ack)),
            DeviceEvent::Ack(Request::Listen {
                target: PinTarget::All,
                enabled: true,
            })
        );
        assert!(connection.pending[id as usize].is_none());
    }

    #[test]
    fn bulk_listener_id_persists_until_bulk_disable() {
        let mut connection = Connection::spawn();
        initialize(&mut connection, 7);
        let (on, _) = prepared(
            &mut connection,
            Request::Listen {
                target: PinTarget::All,
                enabled: true,
            },
        );
        connection.received(response(on, Response::Ack));
        assert!(matches!(
            connection.received(response(
                on,
                Response::Value {
                    pin: pin(7),
                    level: Level::High,
                }
            )),
            DeviceEvent::PinValue { .. }
        ));

        let (off, _) = prepared(
            &mut connection,
            Request::Listen {
                target: PinTarget::All,
                enabled: false,
            },
        );
        connection.received(response(off, Response::Ack));
        assert!(matches!(
            connection.received(response(
                on,
                Response::Value {
                    pin: pin(7),
                    level: Level::Low,
                }
            )),
            DeviceEvent::Untracked
        ));
    }
}
