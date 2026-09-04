#![no_std]

use core::fmt;

pub const WIRE_PIN_COUNT: u8 = 117;
pub const MAX_PACKET_LEN: usize = 64;
pub const MAX_PACKET_ID: u16 = 999;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineError {
    TooLong,
}

pub struct LineBuffer {
    bytes: [u8; MAX_PACKET_LEN],
    len: usize,
    discarding: bool,
}

impl Default for LineBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl LineBuffer {
    pub const fn new() -> Self {
        Self {
            bytes: [0; MAX_PACKET_LEN],
            len: 0,
            discarding: false,
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.discarding = false;
    }

    pub fn push(&mut self, byte: u8) -> Result<Option<&[u8]>, LineError> {
        if byte == b'\r' {
            return Ok(None);
        }
        if byte == b'\n' {
            let line = (!self.discarding && self.len != 0).then_some(&self.bytes[..self.len]);
            self.len = 0;
            self.discarding = false;
            return Ok(line);
        }
        if self.discarding {
            return Ok(None);
        }
        if self.len + 1 >= self.bytes.len() {
            self.len = 0;
            self.discarding = true;
            return Err(LineError::TooLong);
        }
        self.bytes[self.len] = byte;
        self.len += 1;
        Ok(None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Packet<T> {
    pub id: u16,
    pub body: T,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Input,
    Output,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    A,
    B,
    C,
    D,
    E,
}

impl Port {
    const fn first_index(self) -> u8 {
        match self {
            Self::A => 0,
            Self::B => 32,
            Self::C => 47,
            Self::D => 79,
            Self::E => 111,
        }
    }

    pub const fn pin_count(self) -> u8 {
        match self {
            Self::A | Self::C | Self::D => 32,
            Self::B => 15,
            Self::E => 6,
        }
    }

    pub const fn letter(self) -> char {
        match self {
            Self::A => 'A',
            Self::B => 'B',
            Self::C => 'C',
            Self::D => 'D',
            Self::E => 'E',
        }
    }
}

impl fmt::Display for Port {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PIO{}", self.letter())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pin(u8);

const PACKAGE_PINS: [u8; WIRE_PIN_COUNT as usize] = [
    102, 99, 93, 91, 77, 73, 114, 35, 36, 75, 66, 64, 68, 42, 51, 49, 45, 25, 24, 23, 22, 32, 37,
    46, 56, 59, 62, 70, 112, 129, 116, 118, 21, 20, 26, 31, 105, 109, 79, 89, 141, 142, 136, 137,
    87, 144, 140, 11, 38, 39, 40, 41, 58, 54, 48, 82, 86, 90, 94, 17, 19, 97, 18, 100, 103, 111,
    117, 120, 122, 124, 127, 130, 133, 13, 12, 76, 16, 15, 14, 1, 132, 131, 128, 126, 125, 121,
    119, 113, 110, 101, 98, 92, 88, 84, 106, 78, 74, 69, 67, 65, 63, 60, 57, 55, 52, 53, 47, 71,
    108, 34, 2, 4, 6, 7, 10, 27, 28,
];

impl Pin {
    pub const fn from_index(index: u8) -> Option<Self> {
        if index < WIRE_PIN_COUNT {
            Some(Self(index))
        } else {
            None
        }
    }

    pub const fn new(port: Port, bit: u8) -> Option<Self> {
        if bit < port.pin_count() {
            Some(Self(port.first_index() + bit))
        } else {
            None
        }
    }

    pub const fn index(self) -> u8 {
        self.0
    }

    pub const fn port(self) -> Port {
        match self.0 {
            0..=31 => Port::A,
            32..=46 => Port::B,
            47..=78 => Port::C,
            79..=110 => Port::D,
            _ => Port::E,
        }
    }

    pub const fn bit(self) -> u8 {
        self.0 - self.port().first_index()
    }

    pub const fn package_pin(self) -> u8 {
        PACKAGE_PINS[self.0 as usize]
    }

    pub const fn is_available(self) -> bool {
        !matches!(self.0, 40..=43)
    }
}

impl fmt::Display for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{}{:02}", self.port().letter(), self.bit())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinTarget {
    Pin(Pin),
    Bank(Port),
    All,
}

impl PinTarget {
    pub fn contains(self, pin: Pin) -> bool {
        match self {
            Self::Pin(target) => target.index() == pin.index(),
            Self::Bank(port) => pin.port() == port,
            Self::All => true,
        }
    }
}

impl fmt::Display for PinTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pin(pin) => pin.fmt(f),
            Self::Bank(port) => port.fmt(f),
            Self::All => f.write_str("ALL"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Query {
    Direction,
    Pullup,
    Listen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Low,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryValue {
    Unset,
    Direction(Direction),
    Enabled(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Hello,
    Status,
    Direction {
        target: PinTarget,
        direction: Direction,
    },
    Get {
        target: PinTarget,
    },
    Set {
        target: PinTarget,
        level: Level,
    },
    Pullup {
        target: PinTarget,
        enabled: bool,
    },
    Listen {
        target: PinTarget,
        enabled: bool,
    },
    Query {
        target: PinTarget,
        what: Query,
    },
    Bye,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinError {
    Unset,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseError {
    BadPacket,
    Pin { pin: Pin, reason: PinError },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    Hello,
    Status,
    Ack,
    Value {
        pin: Pin,
        level: Level,
    },
    State {
        pin: Pin,
        what: Query,
        value: QueryValue,
    },
    Error(ResponseError),
    Unknown,
    Bye,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeErrorKind {
    Malformed,
    UnknownCommand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeError {
    pub id: Option<u16>,
    pub kind: DecodeErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    OutputTooSmall,
    InvalidPacketId,
}

pub fn decode_request(line: &[u8]) -> Result<Packet<Request>, DecodeError> {
    let mut tokens = split_tokens(line);
    let id = tokens.next().and_then(parse_decimal3).ok_or(DecodeError {
        id: None,
        kind: DecodeErrorKind::Malformed,
    })?;
    let command = tokens.next().ok_or(DecodeError {
        id: None,
        kind: DecodeErrorKind::Malformed,
    })?;

    let malformed = || DecodeError {
        id: Some(id),
        kind: DecodeErrorKind::Malformed,
    };

    let body = match command {
        b"HAI" if tokens.next().is_none() => Request::Hello,
        b"HRU" if tokens.next().is_none() => Request::Status,
        b"BYE" if tokens.next().is_none() => Request::Bye,
        b"DIR" => {
            let target = tokens.next().and_then(parse_target).ok_or_else(malformed)?;
            let direction = match tokens.next() {
                Some(b"IN") => Direction::Input,
                Some(b"OUT") => Direction::Output,
                _ => return Err(malformed()),
            };
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Direction { target, direction }
        }
        b"GET" => {
            let target = tokens.next().and_then(parse_target).ok_or_else(malformed)?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Get { target }
        }
        b"SET" => {
            let target = tokens.next().and_then(parse_target).ok_or_else(malformed)?;
            let level = tokens.next().and_then(parse_level).ok_or_else(malformed)?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Set { target, level }
        }
        b"PLL" => {
            let target = tokens.next().and_then(parse_target).ok_or_else(malformed)?;
            let enabled = tokens
                .next()
                .and_then(parse_enabled)
                .ok_or_else(malformed)?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Pullup { target, enabled }
        }
        b"LSN" => {
            let target = tokens.next().and_then(parse_target).ok_or_else(malformed)?;
            let enabled = tokens
                .next()
                .and_then(parse_enabled)
                .ok_or_else(malformed)?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Listen { target, enabled }
        }
        b"WYD" => {
            let target = tokens.next().and_then(parse_target).ok_or_else(malformed)?;
            let what = tokens.next().and_then(parse_query).ok_or_else(malformed)?;
            if tokens.next().is_some() {
                return Err(malformed());
            }
            Request::Query { target, what }
        }
        b"HAI" | b"HRU" | b"BYE" => return Err(malformed()),
        _ => {
            return Err(DecodeError {
                id: Some(id),
                kind: DecodeErrorKind::UnknownCommand,
            });
        }
    };

    Ok(Packet { id, body })
}

pub fn decode_response(line: &[u8]) -> Result<Packet<Response>, DecodeError> {
    let mut tokens = split_tokens(line);
    let id = tokens.next().and_then(parse_decimal3).ok_or(DecodeError {
        id: None,
        kind: DecodeErrorKind::Malformed,
    })?;
    let command = tokens.next().ok_or(DecodeError {
        id: None,
        kind: DecodeErrorKind::Malformed,
    })?;
    let malformed = || DecodeError {
        id: Some(id),
        kind: DecodeErrorKind::Malformed,
    };

    let body = match command {
        b"HII" => {
            expect_suffix(&mut tokens, b"<3", malformed())?;
            Response::Hello
        }
        b"IAM" => {
            if tokens.next() != Some(b"SAM4E8E") || tokens.next() != Some(b"GPIO") {
                return Err(malformed());
            }
            expect_suffix(&mut tokens, b"<3", malformed())?;
            Response::Status
        }
        b"OKA" => {
            expect_suffix(&mut tokens, b"<3", malformed())?;
            Response::Ack
        }
        b"CYA" => {
            expect_suffix(&mut tokens, b"<3", malformed())?;
            Response::Bye
        }
        b"IDK" => {
            expect_suffix(&mut tokens, b"<3", malformed())?;
            Response::Unknown
        }
        b"UMM" => {
            let error = match tokens.next() {
                Some(b"BAD_PACKET") => ResponseError::BadPacket,
                Some(pin) => {
                    let pin = parse_pin(pin).ok_or_else(malformed)?;
                    let reason = match tokens.next() {
                        Some(b"UNSET") => PinError::Unset,
                        Some(b"UNAVAILABLE") => PinError::Unavailable,
                        _ => return Err(malformed()),
                    };
                    ResponseError::Pin { pin, reason }
                }
                None => return Err(malformed()),
            };
            expect_suffix(&mut tokens, b"<3", malformed())?;
            Response::Error(error)
        }
        b"HYG" => {
            let pin = tokens.next().and_then(parse_pin).ok_or_else(malformed)?;
            let next = tokens.next().ok_or_else(malformed)?;
            if let Some(level) = parse_level(next) {
                expect_suffix(&mut tokens, b"<3", malformed())?;
                Response::Value { pin, level }
            } else {
                let what = parse_query(next).ok_or_else(malformed)?;
                let value_token = tokens.next().ok_or_else(malformed)?;
                let value = match what {
                    Query::Direction => match value_token {
                        b"UNSET" => QueryValue::Unset,
                        b"IN" => QueryValue::Direction(Direction::Input),
                        b"OUT" => QueryValue::Direction(Direction::Output),
                        _ => return Err(malformed()),
                    },
                    Query::Pullup | Query::Listen => match value_token {
                        b"UNSET" => QueryValue::Unset,
                        b"ON" => QueryValue::Enabled(true),
                        b"OFF" => QueryValue::Enabled(false),
                        _ => return Err(malformed()),
                    },
                };
                expect_suffix(&mut tokens, b"<3", malformed())?;
                Response::State { pin, what, value }
            }
        }
        _ => {
            return Err(DecodeError {
                id: Some(id),
                kind: DecodeErrorKind::UnknownCommand,
            });
        }
    };

    Ok(Packet { id, body })
}

pub fn encode_request(packet: Packet<Request>, out: &mut [u8]) -> Result<usize, EncodeError> {
    let mut writer = Writer::new(out);
    writer.id(packet.id)?;
    match packet.body {
        Request::Hello => writer.bytes(b" HAI\n")?,
        Request::Status => writer.bytes(b" HRU\n")?,
        Request::Direction { target, direction } => {
            writer.bytes(b" DIR ")?;
            writer.target(target)?;
            writer.bytes(match direction {
                Direction::Input => b" IN OK?\n",
                Direction::Output => b" OUT OK?\n",
            })?;
        }
        Request::Get { target } => {
            writer.bytes(b" GET ")?;
            writer.target(target)?;
            writer.bytes(b" OK?\n")?;
        }
        Request::Set { target, level } => {
            writer.bytes(b" SET ")?;
            writer.target(target)?;
            writer.bytes(match level {
                Level::Low => b" LOW OK?\n",
                Level::High => b" HIGH OK?\n",
            })?;
        }
        Request::Pullup { target, enabled } => {
            writer.bytes(b" PLL ")?;
            writer.target(target)?;
            writer.bytes(if enabled { b" ON OK?\n" } else { b" OFF OK?\n" })?;
        }
        Request::Listen { target, enabled } => {
            writer.bytes(b" LSN ")?;
            writer.target(target)?;
            writer.bytes(if enabled { b" ON OK?\n" } else { b" OFF OK?\n" })?;
        }
        Request::Query { target, what } => {
            writer.bytes(b" WYD ")?;
            writer.target(target)?;
            writer.bytes(match what {
                Query::Direction => b" DIR\n",
                Query::Pullup => b" PLL\n",
                Query::Listen => b" LSN\n",
            })?;
        }
        Request::Bye => writer.bytes(b" BYE\n")?,
    }
    Ok(writer.len())
}

pub fn encode_response(packet: Packet<Response>, out: &mut [u8]) -> Result<usize, EncodeError> {
    let mut writer = Writer::new(out);
    writer.id(packet.id)?;
    match packet.body {
        Response::Hello => writer.bytes(b" HII <3\n")?,
        Response::Status => writer.bytes(b" IAM SAM4E8E GPIO <3\n")?,
        Response::Ack => writer.bytes(b" OKA <3\n")?,
        Response::Value { pin, level } => {
            writer.bytes(b" HYG ")?;
            writer.pin(pin)?;
            writer.bytes(match level {
                Level::Low => b" LOW <3\n",
                Level::High => b" HIGH <3\n",
            })?;
        }
        Response::State { pin, what, value } => {
            writer.bytes(b" HYG ")?;
            writer.pin(pin)?;
            writer.bytes(match what {
                Query::Direction => b" DIR ",
                Query::Pullup => b" PLL ",
                Query::Listen => b" LSN ",
            })?;
            writer.bytes(match value {
                QueryValue::Unset => b"UNSET",
                QueryValue::Direction(Direction::Input) => b"IN",
                QueryValue::Direction(Direction::Output) => b"OUT",
                QueryValue::Enabled(true) => b"ON",
                QueryValue::Enabled(false) => b"OFF",
            })?;
            writer.bytes(b" <3\n")?;
        }
        Response::Error(ResponseError::BadPacket) => writer.bytes(b" UMM BAD_PACKET <3\n")?,
        Response::Error(ResponseError::Pin { pin, reason }) => {
            writer.bytes(b" UMM ")?;
            writer.pin(pin)?;
            writer.bytes(match reason {
                PinError::Unset => b" UNSET <3\n",
                PinError::Unavailable => b" UNAVAILABLE <3\n",
            })?;
        }
        Response::Unknown => writer.bytes(b" IDK <3\n")?,
        Response::Bye => writer.bytes(b" CYA <3\n")?,
    }
    Ok(writer.len())
}

fn split_tokens(line: &[u8]) -> Tokens<'_> {
    Tokens(line)
}

struct Tokens<'a>(&'a [u8]);

impl<'a> Iterator for Tokens<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        self.0 = self.0.trim_ascii_start();
        let end = self
            .0
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(self.0.len());
        let (token, rest) = self.0.split_at(end);
        self.0 = rest;
        (!token.is_empty()).then_some(token)
    }
}

fn expect_suffix(
    tokens: &mut Tokens<'_>,
    suffix: &[u8],
    error: DecodeError,
) -> Result<(), DecodeError> {
    (tokens.next() == Some(suffix) && tokens.next().is_none())
        .then_some(())
        .ok_or(error)
}

fn parse_target(token: &[u8]) -> Option<PinTarget> {
    if token == b"ALL" {
        Some(PinTarget::All)
    } else if let Some(port) = parse_bank(token) {
        Some(PinTarget::Bank(port))
    } else {
        parse_pin(token).map(PinTarget::Pin)
    }
}

fn parse_bank(token: &[u8]) -> Option<Port> {
    match token {
        b"PIOA" => Some(Port::A),
        b"PIOB" => Some(Port::B),
        b"PIOC" => Some(Port::C),
        b"PIOD" => Some(Port::D),
        b"PIOE" => Some(Port::E),
        _ => None,
    }
}

fn parse_pin(token: &[u8]) -> Option<Pin> {
    let [b'P', port, tens, ones] = *token else {
        return None;
    };
    let port = match port {
        b'A' => Port::A,
        b'B' => Port::B,
        b'C' => Port::C,
        b'D' => Port::D,
        b'E' => Port::E,
        _ => return None,
    };
    if !tens.is_ascii_digit() || !ones.is_ascii_digit() {
        return None;
    }
    Pin::new(port, (tens - b'0') * 10 + (ones - b'0'))
}

fn parse_level(token: &[u8]) -> Option<Level> {
    match token {
        b"LOW" => Some(Level::Low),
        b"HIGH" => Some(Level::High),
        _ => None,
    }
}

fn parse_enabled(token: &[u8]) -> Option<bool> {
    match token {
        b"ON" => Some(true),
        b"OFF" => Some(false),
        _ => None,
    }
}

fn parse_query(token: &[u8]) -> Option<Query> {
    match token {
        b"DIR" => Some(Query::Direction),
        b"PLL" => Some(Query::Pullup),
        b"LSN" => Some(Query::Listen),
        _ => None,
    }
}

fn parse_decimal3(token: &[u8]) -> Option<u16> {
    if token.is_empty() || token.len() > 3 {
        return None;
    }
    let mut value = 0u16;
    for &byte in token {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u16::from(byte - b'0');
    }
    Some(value)
}

struct Writer<'a> {
    out: &'a mut [u8],
    len: usize,
}

impl<'a> Writer<'a> {
    fn new(out: &'a mut [u8]) -> Self {
        Self { out, len: 0 }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn bytes(&mut self, bytes: &[u8]) -> Result<(), EncodeError> {
        let end = self
            .len
            .checked_add(bytes.len())
            .filter(|end| *end <= self.out.len())
            .ok_or(EncodeError::OutputTooSmall)?;
        self.out[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }

    fn id(&mut self, value: u16) -> Result<(), EncodeError> {
        if value > MAX_PACKET_ID {
            return Err(EncodeError::InvalidPacketId);
        }
        self.decimal3(value)
    }

    fn target(&mut self, target: PinTarget) -> Result<(), EncodeError> {
        match target {
            PinTarget::Pin(pin) => self.pin(pin),
            PinTarget::Bank(port) => {
                self.bytes(b"PIO")?;
                self.bytes(&[port.letter() as u8])
            }
            PinTarget::All => self.bytes(b"ALL"),
        }
    }

    fn pin(&mut self, pin: Pin) -> Result<(), EncodeError> {
        self.bytes(&[b'P', pin.port().letter() as u8])?;
        self.decimal2(pin.bit())
    }

    fn decimal2(&mut self, value: u8) -> Result<(), EncodeError> {
        self.bytes(&[b'0' + value / 10, b'0' + value % 10])
    }

    fn decimal3(&mut self, value: u16) -> Result<(), EncodeError> {
        self.bytes(&[
            b'0' + ((value / 100) % 10) as u8,
            b'0' + ((value / 10) % 10) as u8,
            b'0' + (value % 10) as u8,
        ])
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::ToString;

    fn encoded_request(id: u16, body: Request) -> [u8; MAX_PACKET_LEN] {
        let mut out = [0u8; MAX_PACKET_LEN];
        let len = encode_request(Packet { id, body }, &mut out).unwrap();
        assert_eq!(decode_request(&out[..len]), Ok(Packet { id, body }));
        out
    }
    fn pin(index: u8) -> Pin {
        Pin::from_index(index).unwrap()
    }

    #[test]
    fn line_buffer_frames_and_recovers_after_overflow() {
        let mut buffer = LineBuffer::new();
        let mut seen = false;
        for &byte in b"\r001 HAI\r\n" {
            if let Some(line) = buffer.push(byte).unwrap() {
                assert_eq!(line, b"001 HAI");
                seen = true;
            }
        }
        assert!(seen);

        for _ in 0..MAX_PACKET_LEN - 1 {
            assert_eq!(buffer.push(b'x'), Ok(None));
        }
        assert_eq!(buffer.push(b'x'), Err(LineError::TooLong));
        assert_eq!(buffer.push(b'x'), Ok(None));
        assert_eq!(buffer.push(b'\n'), Ok(None));

        for &byte in b"008 HII <3\n" {
            if let Some(line) = buffer.push(byte).unwrap() {
                assert_eq!(line, b"008 HII <3");
            }
        }
    }

    #[test]
    fn request_wire_examples_use_symbolic_targets() {
        let cases = [
            (Request::Hello, "001 HAI\n"),
            (Request::Status, "001 HRU\n"),
            (
                Request::Direction {
                    target: PinTarget::Pin(pin(0)),
                    direction: Direction::Input,
                },
                "001 DIR PA00 IN OK?\n",
            ),
            (
                Request::Direction {
                    target: PinTarget::Pin(pin(116)),
                    direction: Direction::Output,
                },
                "001 DIR PE05 OUT OK?\n",
            ),
            (
                Request::Get {
                    target: PinTarget::Pin(pin(5)),
                },
                "001 GET PA05 OK?\n",
            ),
            (
                Request::Set {
                    target: PinTarget::Bank(Port::C),
                    level: Level::High,
                },
                "001 SET PIOC HIGH OK?\n",
            ),
            (
                Request::Pullup {
                    target: PinTarget::Bank(Port::B),
                    enabled: false,
                },
                "001 PLL PIOB OFF OK?\n",
            ),
            (
                Request::Listen {
                    target: PinTarget::Bank(Port::E),
                    enabled: true,
                },
                "001 LSN PIOE ON OK?\n",
            ),
            (
                Request::Query {
                    target: PinTarget::Pin(pin(72)),
                    what: Query::Direction,
                },
                "001 WYD PC25 DIR\n",
            ),
            (
                Request::Direction {
                    target: PinTarget::All,
                    direction: Direction::Input,
                },
                "001 DIR ALL IN OK?\n",
            ),
            (
                Request::Get {
                    target: PinTarget::All,
                },
                "001 GET ALL OK?\n",
            ),
            (
                Request::Set {
                    target: PinTarget::All,
                    level: Level::High,
                },
                "001 SET ALL HIGH OK?\n",
            ),
            (
                Request::Pullup {
                    target: PinTarget::All,
                    enabled: true,
                },
                "001 PLL ALL ON OK?\n",
            ),
            (
                Request::Listen {
                    target: PinTarget::All,
                    enabled: true,
                },
                "001 LSN ALL ON OK?\n",
            ),
            (
                Request::Query {
                    target: PinTarget::All,
                    what: Query::Listen,
                },
                "001 WYD ALL LSN\n",
            ),
            (Request::Bye, "001 BYE\n"),
        ];

        for (body, expected) in cases {
            let out = encoded_request(1, body);
            assert_eq!(&out[..expected.len()], expected.as_bytes());
        }
    }

    #[test]
    fn response_wire_examples_use_symbolic_pins() {
        let cases = [
            (Response::Hello, "008 HII <3\n"),
            (Response::Status, "008 IAM SAM4E8E GPIO <3\n"),
            (Response::Ack, "008 OKA <3\n"),
            (
                Response::Value {
                    pin: pin(0),
                    level: Level::High,
                },
                "008 HYG PA00 HIGH <3\n",
            ),
            (
                Response::State {
                    pin: pin(0),
                    what: Query::Direction,
                    value: QueryValue::Direction(Direction::Input),
                },
                "008 HYG PA00 DIR IN <3\n",
            ),
            (
                Response::State {
                    pin: pin(0),
                    what: Query::Pullup,
                    value: QueryValue::Enabled(true),
                },
                "008 HYG PA00 PLL ON <3\n",
            ),
            (
                Response::State {
                    pin: pin(0),
                    what: Query::Listen,
                    value: QueryValue::Unset,
                },
                "008 HYG PA00 LSN UNSET <3\n",
            ),
            (
                Response::Error(ResponseError::BadPacket),
                "008 UMM BAD_PACKET <3\n",
            ),
            (
                Response::Error(ResponseError::Pin {
                    pin: pin(40),
                    reason: PinError::Unavailable,
                }),
                "008 UMM PB08 UNAVAILABLE <3\n",
            ),
            (
                Response::Error(ResponseError::Pin {
                    pin: pin(3),
                    reason: PinError::Unset,
                }),
                "008 UMM PA03 UNSET <3\n",
            ),
            (Response::Unknown, "008 IDK <3\n"),
            (Response::Bye, "008 CYA <3\n"),
        ];

        for (body, expected) in cases {
            let packet = Packet { id: 8, body };
            let mut out = [0u8; MAX_PACKET_LEN];
            let len = encode_response(packet, &mut out).unwrap();
            assert_eq!(&out[..len], expected.as_bytes());
            assert_eq!(decode_response(&out[..len]), Ok(packet));
        }
    }

    #[test]
    fn malformed_known_and_unknown_requests_are_distinct() {
        assert_eq!(
            decode_request(b"007 DIR PA00 SIDEWAYS OK?\n"),
            Err(DecodeError {
                id: Some(7),
                kind: DecodeErrorKind::Malformed,
            })
        );
        assert_eq!(
            decode_request(b"007 WAT PA00\n"),
            Err(DecodeError {
                id: Some(7),
                kind: DecodeErrorKind::UnknownCommand,
            })
        );
        assert_eq!(
            decode_request(b"nope HAI\n"),
            Err(DecodeError {
                id: None,
                kind: DecodeErrorKind::Malformed,
            })
        );
    }

    #[test]
    fn packet_ids_remain_decimal_but_numeric_gpio_targets_are_rejected() {
        assert_eq!(
            decode_request(b"9 GET PE05 OK?"),
            Ok(Packet {
                id: 9,
                body: Request::Get {
                    target: PinTarget::Pin(pin(116)),
                },
            })
        );
        assert!(decode_request(b"1000 HAI").is_err());
        assert!(decode_request(b"001 GET 116 OK?").is_err());
        assert!(decode_request(b"001 GET PE06 OK?").is_err());
    }

    #[test]
    fn pin_mapping_has_physical_package_numbers() {
        assert_eq!(pin(0).to_string(), "PA00");
        assert_eq!(pin(44).to_string(), "PB12");
        assert_eq!(pin(44).package_pin(), 87);
        assert_eq!(pin(72).to_string(), "PC25");
        assert_eq!(Pin::new(Port::E, 5), Some(pin(116)));
    }
}
