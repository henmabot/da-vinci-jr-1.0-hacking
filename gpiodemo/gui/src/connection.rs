use std::{
    io::{self, Read, Write},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

use da_vinci_protocol::{
    Level, MAX_PACKET_LEN, Packet, Query, QueryValue, Request, Response, ResponseError,
    WIRE_PIN_COUNT, decode_response, encode_request,
};

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

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DeviceEvent {
    Hello,
    Status,
    Ack(Request),
    PinValue {
        pin: u8,
        level: Level,
    },
    PinState {
        pin: u8,
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
    pending: [Option<Request>; 1000],
    listeners: [Option<u16>; WIRE_PIN_COUNT as usize],
    commands: Sender<IoCommand>,
    events: Receiver<IoEvent>,
}

impl Connection {
    pub(super) fn spawn() -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        thread::spawn(move || io_worker(command_rx, event_tx));
        Self {
            next_id: 1,
            pending: [None; 1000],
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
        for _ in 0..999 {
            let id = self.next_id;
            self.next_id = if id == 999 { 1 } else { id + 1 };
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
                if !self.is_listener_response(packet.id, request) {
                    self.pending[packet.id as usize] = None;
                }
                DeviceEvent::PinValue { pin, level }
            }
            Response::State { pin, what, value } => {
                self.pending[packet.id as usize] = None;
                DeviceEvent::PinState { pin, what, value }
            }
            Response::Error(error) => {
                self.retire(packet.id, request);
                DeviceEvent::DeviceError { request, error }
            }
            Response::Unknown => {
                self.retire(packet.id, request);
                DeviceEvent::Unknown { request }
            }
            Response::Bye => unreachable!(),
        }
    }

    fn clear(&mut self) {
        self.pending.fill(None);
        self.listeners.fill(None);
    }

    fn ack(&mut self, id: u16, request: Request) -> DeviceEvent {
        if let Request::Listen { pin, enabled } = request {
            let slot = &mut self.listeners[pin as usize];
            if enabled {
                if let Some(previous) = slot.replace(id)
                    && previous != id
                {
                    self.pending[previous as usize] = None;
                }
                return DeviceEvent::Ack(request);
            }
            if let Some(previous) = slot.take() {
                self.pending[previous as usize] = None;
            }
        }
        self.pending[id as usize] = None;
        DeviceEvent::Ack(request)
    }

    fn is_listener_response(&self, id: u16, request: Request) -> bool {
        matches!(request, Request::Listen { pin, enabled: true } if self.listeners[pin as usize] == Some(id))
    }

    fn retire(&mut self, id: u16, request: Request) {
        if let Request::Listen { pin, .. } = request
            && self.listeners[pin as usize] == Some(id)
        {
            self.listeners[pin as usize] = None;
        }
        self.pending[id as usize] = None;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn response(id: u16, body: Response) -> Packet<Response> {
        Packet { id, body }
    }

    fn prepared(connection: &mut Connection, request: Request) -> (u16, Vec<u8>) {
        connection.prepare(request).unwrap()
    }

    #[test]
    fn request_ids_wrap_from_999_to_001() {
        let mut connection = Connection::spawn();
        for expected in 1..=999 {
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
        let (listener, _) = prepared(
            &mut connection,
            Request::Listen {
                pin: 5,
                enabled: true,
            },
        );
        connection.received(response(listener, Response::Ack));

        for _ in 2..=999 {
            let (id, _) = prepared(&mut connection, Request::Hello);
            connection.received(response(id, Response::Hello));
        }

        let (id, _) = prepared(&mut connection, Request::Hello);
        assert_eq!(id, 2);
        assert_eq!(
            connection.received(response(
                listener,
                Response::Value {
                    pin: 5,
                    level: Level::High,
                },
            )),
            DeviceEvent::PinValue {
                pin: 5,
                level: Level::High,
            }
        );
    }

    #[test]
    fn ordinary_requests_are_retired_after_response() {
        let mut connection = Connection::spawn();
        let (id, _) = prepared(&mut connection, Request::Get { pin: 5 });
        assert_eq!(
            connection.received(response(
                id,
                Response::Value {
                    pin: 5,
                    level: Level::High,
                },
            )),
            DeviceEvent::PinValue {
                pin: 5,
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
        let (listener, _) = prepared(
            &mut connection,
            Request::Listen {
                pin: 5,
                enabled: true,
            },
        );
        assert_eq!(
            connection.received(response(listener, Response::Ack)),
            DeviceEvent::Ack(Request::Listen {
                pin: 5,
                enabled: true,
            })
        );
        for level in [Level::Low, Level::High] {
            assert_eq!(
                connection.received(response(listener, Response::Value { pin: 5, level },)),
                DeviceEvent::PinValue { pin: 5, level }
            );
        }
    }

    #[test]
    fn reenable_replaces_old_listener_only_after_ack() {
        let mut connection = Connection::spawn();
        let (first, _) = prepared(
            &mut connection,
            Request::Listen {
                pin: 5,
                enabled: true,
            },
        );
        connection.received(response(first, Response::Ack));

        let (second, _) = prepared(
            &mut connection,
            Request::Listen {
                pin: 5,
                enabled: true,
            },
        );
        assert!(matches!(
            connection.received(response(
                first,
                Response::Value {
                    pin: 5,
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
                    pin: 5,
                    level: Level::Low,
                }
            )),
            DeviceEvent::Untracked
        ));
    }

    #[test]
    fn listener_off_retires_persistent_id() {
        let mut connection = Connection::spawn();
        let (on, _) = prepared(
            &mut connection,
            Request::Listen {
                pin: 5,
                enabled: true,
            },
        );
        connection.received(response(on, Response::Ack));
        let (off, _) = prepared(
            &mut connection,
            Request::Listen {
                pin: 5,
                enabled: false,
            },
        );
        connection.received(response(off, Response::Ack));
        assert!(matches!(
            connection.received(response(
                on,
                Response::Value {
                    pin: 5,
                    level: Level::High,
                }
            )),
            DeviceEvent::Untracked
        ));
    }

    #[test]
    fn cya_clears_all_bookkeeping() {
        let mut connection = Connection::spawn();
        let (listener, _) = prepared(
            &mut connection,
            Request::Listen {
                pin: 5,
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
                    pin: 5,
                    level: Level::High,
                }
            )),
            DeviceEvent::Untracked
        ));
    }
}
