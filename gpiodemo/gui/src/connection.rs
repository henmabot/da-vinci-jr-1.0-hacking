use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    sync::mpsc::{self, Receiver, Sender, SyncSender},
    thread,
    time::Duration,
};

use da_vinci_protocol::{
    Level, LineBuffer, LineError, MAX_PACKET_ID, MAX_PACKET_LEN, Packet, Pin, PinTarget, Query,
    QueryValue, Request, Response, ResponseError, WIRE_PIN_COUNT, decode_response, encode_request,
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
    IoError(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        error: ResponseError,
    },
    Unknown {
        request: Request,
    },
    Bye,
    Untracked,
}

pub(super) struct Connection {
    next_id: u16,
    pending: [Option<Request>; MAX_PACKET_ID as usize + 1],
    inputs: [bool; WIRE_PIN_COUNT as usize],
    listeners: [Option<u16>; WIRE_PIN_COUNT as usize],
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
            inputs: [false; WIRE_PIN_COUNT as usize],
            listeners: [None; WIRE_PIN_COUNT as usize],
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

    pub(super) fn next_event(&mut self) -> Option<Event> {
        match self.events.try_recv() {
            Ok(IoEvent::Connected(port)) => {
                self.clear();
                Some(Event::Connected(port))
            }
            Ok(IoEvent::Disconnected(reason)) => {
                self.clear();
                Some(Event::Disconnected(reason))
            }
            Ok(IoEvent::Line(line)) => {
                let event = decode_response(line.as_bytes())
                    .map(|packet| self.received(packet))
                    .map_err(|error| format!("Malformed response: {error:?}"));
                Some(Event::Received { line, event })
            }
            Ok(IoEvent::Error(error)) => Some(Event::IoError(error)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.clear();
                Some(Event::Disconnected(Some("Serial worker stopped".into())))
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
        let len = encode_request(Packet { id, body: request }, &mut buffer)
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

    fn received(&mut self, packet: Packet<Response>) -> DeviceEvent {
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
                if !self.is_listener_response(packet.id, pin) && !is_grouped_get(request) {
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
    }

    fn ack(&mut self, id: u16, request: Request) -> DeviceEvent {
        if let Request::Direction { target, direction } = request {
            for (index, pin) in Pin::all().enumerate() {
                if target.contains(pin) && pin.is_available() {
                    self.inputs[index] = direction == da_vinci_protocol::Direction::Input;
                }
            }
        }

        if let Request::Listen { target, enabled } = request {
            if enabled {
                let mut listening = false;
                let mut replaced = [None; WIRE_PIN_COUNT as usize];
                for (index, pin) in Pin::all().enumerate() {
                    if target.contains(pin) && self.inputs[index] {
                        listening = true;
                        replaced[index] = self.listeners[index].replace(id);
                    }
                }
                for previous in replaced.into_iter().flatten() {
                    if previous != id {
                        self.release_listener(previous);
                    }
                }
                if listening {
                    return DeviceEvent::Ack(request);
                }
            }

            let mut removed = [None; WIRE_PIN_COUNT as usize];
            for (index, pin) in Pin::all().enumerate() {
                if target.contains(pin) {
                    removed[index] = self.listeners[index].take();
                }
            }
            for previous in removed.into_iter().flatten() {
                self.release_listener(previous);
            }
        }

        self.pending[id as usize] = None;
        DeviceEvent::Ack(request)
    }

    fn is_listener_response(&self, id: u16, pin: Pin) -> bool {
        self.listeners[pin.index() as usize] == Some(id)
    }

    fn release_listener(&mut self, id: u16) {
        if !self.listeners.contains(&Some(id)) {
            self.pending[id as usize] = None;
        }
    }

    fn retire(&mut self, id: u16) {
        for listener in &mut self.listeners {
            if *listener == Some(id) {
                *listener = None;
            }
        }
        self.pending[id as usize] = None;
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
}

enum IoEvent {
    Connected(String),
    Disconnected(Option<String>),
    Line(String),
    Error(String),
}

fn io_worker(commands: Receiver<IoCommand>, events: SyncSender<IoEvent>) {
    let mut port: Option<Box<dyn serialport::SerialPort>> = None;
    let mut reader = LineBuffer::new();
    let mut writes = VecDeque::new();
    let mut write_offset = 0;
    let mut buffer = [0u8; 64];

    loop {
        if port.is_none() {
            let Ok(command) = commands.recv() else {
                return;
            };
            handle_io_command(
                command,
                &mut port,
                &mut reader,
                &mut writes,
                &mut write_offset,
                &events,
            );
        } else {
            loop {
                match commands.try_recv() {
                    Ok(command) => handle_io_command(
                        command,
                        &mut port,
                        &mut reader,
                        &mut writes,
                        &mut write_offset,
                        &events,
                    ),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }
        }

        if port.is_none() {
            continue;
        }

        if let Some(bytes) = writes.front() {
            let result = port
                .as_mut()
                .expect("connected serial port")
                .write(&bytes[write_offset..]);
            match result {
                Ok(written) => {
                    write_offset += written;
                    if write_offset == bytes.len() {
                        writes.pop_front();
                        write_offset = 0;
                    }
                }
                Err(error) if transient_io_error(&error) => {}
                Err(error) => {
                    port = None;
                    writes.clear();
                    write_offset = 0;
                    reader.clear();
                    let _ = events.send(IoEvent::Disconnected(Some(format!(
                        "Serial write failed: {error}"
                    ))));
                    continue;
                }
            }
        }

        match port
            .as_mut()
            .expect("connected serial port")
            .read(&mut buffer)
        {
            Ok(count) => {
                for &byte in &buffer[..count] {
                    match reader.push(byte) {
                        Ok(Some(line)) => {
                            let packet = String::from_utf8_lossy(line).into_owned();
                            let _ = events.send(IoEvent::Line(packet));
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
                port = None;
                writes.clear();
                write_offset = 0;
                reader.clear();
                let _ = events.send(IoEvent::Disconnected(Some(format!(
                    "Serial read failed: {error}"
                ))));
            }
        }
    }
}

fn handle_io_command(
    command: IoCommand,
    port: &mut Option<Box<dyn serialport::SerialPort>>,
    reader: &mut LineBuffer,
    writes: &mut VecDeque<Vec<u8>>,
    write_offset: &mut usize,
    events: &SyncSender<IoEvent>,
) {
    match command {
        IoCommand::Connect(name) => {
            writes.clear();
            *write_offset = 0;
            reader.clear();
            match serialport::new(&name, 115_200)
                .timeout(Duration::from_millis(20))
                .open()
            {
                Ok(opened) => {
                    *port = Some(opened);
                    let _ = events.send(IoEvent::Connected(name));
                }
                Err(error) => {
                    *port = None;
                    let _ = events.send(IoEvent::Error(format!("Could not open {name}: {error}")));
                }
            }
        }
        IoCommand::Disconnect => {
            *port = None;
            writes.clear();
            *write_offset = 0;
            reader.clear();
            let _ = events.send(IoEvent::Disconnected(None));
        }
        IoCommand::Write(bytes) => {
            if port.is_some() {
                writes.push_back(bytes);
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

    fn response(id: u16, body: Response) -> Packet<Response> {
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
        let mut port = None;
        let mut reader = LineBuffer::new();
        let mut writes = VecDeque::new();
        let mut write_offset = 0;

        handle_io_command(
            IoCommand::Write(b"001 HAI\n".to_vec()),
            &mut port,
            &mut reader,
            &mut writes,
            &mut write_offset,
            &events,
        );

        assert!(writes.is_empty());
        assert!(received.try_recv().is_err());
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
