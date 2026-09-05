#![no_std]

use core::{
    fmt,
    ops::{Index, IndexMut},
};

pub const WIRE_PIN_COUNT: u8 = 117;
pub const MAX_PACKET_LEN: usize = 64;
pub const MAX_PACKET_ID: u16 = 999;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinTable<T>([T; WIRE_PIN_COUNT as usize]);

impl<T: Copy> PinTable<T> {
    pub const fn filled(value: T) -> Self {
        Self([value; WIRE_PIN_COUNT as usize])
    }

    pub fn fill(&mut self, value: T) {
        self.0.fill(value);
    }
}

impl<T> PinTable<T> {
    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.0.iter()
    }

    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, T> {
        self.0.iter_mut()
    }
}

impl<T> Index<Pin> for PinTable<T> {
    type Output = T;

    fn index(&self, pin: Pin) -> &Self::Output {
        &self.0[pin.index() as usize]
    }
}

impl<T> IndexMut<Pin> for PinTable<T> {
    fn index_mut(&mut self, pin: Pin) -> &mut Self::Output {
        &mut self.0[pin.index() as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseTokenError;

impl fmt::Display for ParseTokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid protocol token")
    }
}

impl core::error::Error for ParseTokenError {}

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
pub struct RequestEnvelope<'a> {
    pub id: u16,
    pub destination: &'a [u8],
    pub body: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResponseEnvelope<'a> {
    pub id: u16,
    pub source: &'a [u8],
    pub body: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Input,
    Output,
}

impl TryFrom<&[u8]> for Direction {
    type Error = ParseTokenError;

    fn try_from(token: &[u8]) -> Result<Self, Self::Error> {
        match token {
            b"IN" => Ok(Self::Input),
            b"OUT" => Ok(Self::Output),
            _ => Err(ParseTokenError),
        }
    }
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
    fn from_letter(letter: u8) -> Result<Self, ParseTokenError> {
        match letter {
            b'A' => Ok(Self::A),
            b'B' => Ok(Self::B),
            b'C' => Ok(Self::C),
            b'D' => Ok(Self::D),
            b'E' => Ok(Self::E),
            _ => Err(ParseTokenError),
        }
    }

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

    pub fn pins(self) -> impl Iterator<Item = Pin> {
        (0..self.pin_count()).map(move |bit| Pin(self.first_index() + bit))
    }
}

impl TryFrom<&[u8]> for Port {
    type Error = ParseTokenError;

    fn try_from(token: &[u8]) -> Result<Self, Self::Error> {
        let [b'P', b'I', b'O', letter] = *token else {
            return Err(ParseTokenError);
        };
        Self::from_letter(letter)
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
    pub const fn from_wire_index(index: u8) -> Option<Self> {
        if index < WIRE_PIN_COUNT {
            Some(Self(index))
        } else {
            None
        }
    }

    pub fn all() -> impl Iterator<Item = Self> {
        (0..WIRE_PIN_COUNT).map(Self)
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

impl TryFrom<(Port, u8)> for Pin {
    type Error = ParseTokenError;

    fn try_from((port, bit): (Port, u8)) -> Result<Self, Self::Error> {
        (bit < port.pin_count())
            .then_some(Self(port.first_index() + bit))
            .ok_or(ParseTokenError)
    }
}

impl TryFrom<&[u8]> for Pin {
    type Error = ParseTokenError;

    fn try_from(token: &[u8]) -> Result<Self, Self::Error> {
        let [b'P', port, tens, ones] = *token else {
            return Err(ParseTokenError);
        };
        if !tens.is_ascii_digit() || !ones.is_ascii_digit() {
            return Err(ParseTokenError);
        }
        Self::try_from((Port::from_letter(port)?, (tens - b'0') * 10 + (ones - b'0')))
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

impl TryFrom<&[u8]> for PinTarget {
    type Error = ParseTokenError;

    fn try_from(token: &[u8]) -> Result<Self, Self::Error> {
        if token == b"ALL" {
            Ok(Self::All)
        } else if let Ok(port) = Port::try_from(token) {
            Ok(Self::Bank(port))
        } else {
            Pin::try_from(token).map(Self::Pin)
        }
    }
}

impl PinTarget {
    pub fn pins(self) -> impl Iterator<Item = Pin> {
        let (start, end) = match self {
            Self::Pin(pin) => (pin.index(), pin.index() + 1),
            Self::Bank(port) => {
                let start = port.first_index();
                (start, start + port.pin_count())
            }
            Self::All => (0, WIRE_PIN_COUNT),
        };
        (start..end).map(Pin)
    }

    pub fn available_pins(self) -> impl Iterator<Item = Pin> {
        self.pins().filter(|pin| pin.is_available())
    }

    pub fn contains(self, pin: Pin) -> bool {
        match self {
            Self::Pin(target) => target == pin,
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

impl TryFrom<&[u8]> for Query {
    type Error = ParseTokenError;

    fn try_from(token: &[u8]) -> Result<Self, Self::Error> {
        match token {
            b"DIR" => Ok(Self::Direction),
            b"PLL" => Ok(Self::Pullup),
            b"LSN" => Ok(Self::Listen),
            _ => Err(ParseTokenError),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Low,
    High,
}

impl TryFrom<&[u8]> for Level {
    type Error = ParseTokenError;

    fn try_from(token: &[u8]) -> Result<Self, Self::Error> {
        match token {
            b"LOW" => Ok(Self::Low),
            b"HIGH" => Ok(Self::High),
            _ => Err(ParseTokenError),
        }
    }
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
pub enum ResponseError<R = &'static [u8]> {
    BadPacket,
    Pin { pin: Pin, reason: PinError },
    NoRoute { destination: R },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response<R = &'static [u8]> {
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
    Error(ResponseError<R>),
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
    InvalidRouteToken,
}

pub fn decode_request_envelope(line: &[u8]) -> Result<RequestEnvelope<'_>, DecodeError> {
    let (id, destination, body) = decode_envelope(line)?;
    Ok(RequestEnvelope {
        id,
        destination,
        body,
    })
}

pub fn decode_response_envelope(line: &[u8]) -> Result<ResponseEnvelope<'_>, DecodeError> {
    let (id, source, body) = decode_envelope(line)?;
    Ok(ResponseEnvelope { id, source, body })
}

pub fn encode_request_envelope(
    envelope: RequestEnvelope<'_>,
    out: &mut [u8],
) -> Result<usize, EncodeError> {
    encode_envelope(envelope.id, envelope.destination, envelope.body, out)
}

pub fn encode_response_envelope(
    envelope: ResponseEnvelope<'_>,
    out: &mut [u8],
) -> Result<usize, EncodeError> {
    encode_envelope(envelope.id, envelope.source, envelope.body, out)
}

pub fn decode_request(packet: Packet<&[u8]>) -> Result<Packet<Request>, DecodeError> {
    let id = packet.id;
    let mut tokens = packet
        .body
        .split(u8::is_ascii_whitespace)
        .filter(|token| !token.is_empty());
    let command = tokens.next().ok_or(DecodeError {
        id: Some(id),
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
            let target: PinTarget = next_as(&mut tokens, malformed())?;
            let direction: Direction = next_as(&mut tokens, malformed())?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Direction { target, direction }
        }
        b"GET" => {
            let target: PinTarget = next_as(&mut tokens, malformed())?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Get { target }
        }
        b"SET" => {
            let target: PinTarget = next_as(&mut tokens, malformed())?;
            let level: Level = next_as(&mut tokens, malformed())?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Set { target, level }
        }
        b"PLL" => {
            let target: PinTarget = next_as(&mut tokens, malformed())?;
            let enabled =
                parse_enabled(tokens.next().ok_or_else(malformed)?).ok_or_else(malformed)?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Pullup { target, enabled }
        }
        b"LSN" => {
            let target: PinTarget = next_as(&mut tokens, malformed())?;
            let enabled =
                parse_enabled(tokens.next().ok_or_else(malformed)?).ok_or_else(malformed)?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Listen { target, enabled }
        }
        b"WYD" => {
            let target: PinTarget = next_as(&mut tokens, malformed())?;
            let what: Query = next_as(&mut tokens, malformed())?;
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

pub fn decode_response(packet: Packet<&[u8]>) -> Result<Packet<Response<&[u8]>>, DecodeError> {
    let id = packet.id;
    let mut tokens = packet
        .body
        .split(u8::is_ascii_whitespace)
        .filter(|token| !token.is_empty());
    let command = tokens.next().ok_or(DecodeError {
        id: Some(id),
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
                Some(b"NO_ROUTE") => {
                    let destination = tokens.next().ok_or_else(malformed)?;
                    if !valid_route_token(destination) {
                        return Err(malformed());
                    }
                    ResponseError::NoRoute { destination }
                }
                Some(pin) => {
                    let pin = Pin::try_from(pin).map_err(|_| malformed())?;
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
            let pin: Pin = next_as(&mut tokens, malformed())?;
            let next = tokens.next().ok_or_else(malformed)?;
            if let Ok(level) = Level::try_from(next) {
                expect_suffix(&mut tokens, b"<3", malformed())?;
                Response::Value { pin, level }
            } else {
                let what = Query::try_from(next).map_err(|_| malformed())?;
                let value_token = tokens.next().ok_or_else(malformed)?;
                let value = match what {
                    Query::Direction if value_token == b"UNSET" => QueryValue::Unset,
                    Query::Direction => QueryValue::Direction(
                        Direction::try_from(value_token).map_err(|_| malformed())?,
                    ),
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

pub fn encode_request(
    packet: Packet<Request>,
    destination: &[u8],
    out: &mut [u8],
) -> Result<usize, EncodeError> {
    let capacity = out.len().min(MAX_PACKET_LEN);
    let mut writer = Writer::new(&mut out[..capacity]);
    writer.id(packet.id)?;
    writer.bytes(b" ")?;
    writer.route(destination)?;
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

pub fn encode_response<R: AsRef<[u8]>>(
    packet: Packet<Response<R>>,
    source: &[u8],
    out: &mut [u8],
) -> Result<usize, EncodeError> {
    let capacity = out.len().min(MAX_PACKET_LEN);
    let mut writer = Writer::new(&mut out[..capacity]);
    writer.id(packet.id)?;
    writer.bytes(b" ")?;
    writer.route(source)?;
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
        Response::Error(ResponseError::NoRoute { destination }) => {
            writer.bytes(b" UMM NO_ROUTE ")?;
            writer.route(destination.as_ref())?;
            writer.bytes(b" <3\n")?;
        }
        Response::Unknown => writer.bytes(b" IDK <3\n")?,
        Response::Bye => writer.bytes(b" CYA <3\n")?,
    }
    Ok(writer.len())
}

fn next_as<'a, T>(
    tokens: &mut impl Iterator<Item = &'a [u8]>,
    error: DecodeError,
) -> Result<T, DecodeError>
where
    T: TryFrom<&'a [u8]>,
{
    tokens.next().ok_or(error)?.try_into().map_err(|_| error)
}

fn expect_suffix<'a>(
    tokens: &mut impl Iterator<Item = &'a [u8]>,
    suffix: &[u8],
    error: DecodeError,
) -> Result<(), DecodeError> {
    (tokens.next() == Some(suffix) && tokens.next().is_none())
        .then_some(())
        .ok_or(error)
}

fn parse_enabled(token: &[u8]) -> Option<bool> {
    match token {
        b"ON" => Some(true),
        b"OFF" => Some(false),
        _ => None,
    }
}

fn decode_envelope(line: &[u8]) -> Result<(u16, &[u8], &[u8]), DecodeError> {
    let (id_token, rest) = next_token(line).ok_or(DecodeError {
        id: None,
        kind: DecodeErrorKind::Malformed,
    })?;
    let id = parse_packet_id(id_token).ok_or(DecodeError {
        id: None,
        kind: DecodeErrorKind::Malformed,
    })?;
    let malformed = || DecodeError {
        id: Some(id),
        kind: DecodeErrorKind::Malformed,
    };
    let (route, rest) = next_token(rest).ok_or_else(malformed)?;
    if !valid_route_token(route) {
        return Err(malformed());
    }
    let body = rest.trim_ascii();
    if body.is_empty() {
        return Err(malformed());
    }
    Ok((id, route, body))
}

fn encode_envelope(
    id: u16,
    route: &[u8],
    body: &[u8],
    out: &mut [u8],
) -> Result<usize, EncodeError> {
    let capacity = out.len().min(MAX_PACKET_LEN);
    let mut writer = Writer::new(&mut out[..capacity]);
    writer.id(id)?;
    writer.bytes(b" ")?;
    writer.route(route)?;
    writer.bytes(b" ")?;
    writer.bytes(body)?;
    writer.bytes(b"\n")?;
    Ok(writer.len())
}

fn next_token(input: &[u8]) -> Option<(&[u8], &[u8])> {
    let start = input.iter().position(|byte| !byte.is_ascii_whitespace())?;
    let input = &input[start..];
    let end = input
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(input.len());
    Some((&input[..end], &input[end..]))
}

fn valid_route_token(token: &[u8]) -> bool {
    !token.is_empty() && token.iter().all(u8::is_ascii_graphic)
}

fn parse_packet_id(token: &[u8]) -> Option<u16> {
    if token.is_empty() || token.len() > 3 || !token.iter().all(u8::is_ascii_digit) {
        return None;
    }
    core::str::from_utf8(token).ok()?.parse().ok()
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

    fn route(&mut self, route: &[u8]) -> Result<(), EncodeError> {
        if !valid_route_token(route) {
            return Err(EncodeError::InvalidRouteToken);
        }
        self.bytes(route)
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
        let len = encode_request(Packet { id, body }, b"SAM", &mut out).unwrap();
        assert_eq!(decoded_request(&out[..len]), Ok(Packet { id, body }));
        out
    }

    fn decoded_request(line: &[u8]) -> Result<Packet<Request>, DecodeError> {
        let envelope = decode_request_envelope(line)?;
        decode_request(Packet {
            id: envelope.id,
            body: envelope.body,
        })
    }

    fn decoded_response(line: &[u8]) -> Result<Packet<Response<&[u8]>>, DecodeError> {
        let envelope = decode_response_envelope(line)?;
        decode_response(Packet {
            id: envelope.id,
            body: envelope.body,
        })
    }
    fn pin(index: u8) -> Pin {
        Pin::from_wire_index(index).unwrap()
    }

    #[test]
    fn line_buffer_frames_and_recovers_after_overflow() {
        let mut buffer = LineBuffer::new();
        let mut seen = false;
        for &byte in b"\r001 SAM HAI\r\n" {
            if let Some(line) = buffer.push(byte).unwrap() {
                assert_eq!(line, b"001 SAM HAI");
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

        for &byte in b"008 SAM HII <3\n" {
            if let Some(line) = buffer.push(byte).unwrap() {
                assert_eq!(line, b"008 SAM HII <3");
            }
        }
    }

    #[test]
    fn routed_envelopes_borrow_route_and_opaque_body() {
        let request = b"001 SAM HAI";
        let envelope = decode_request_envelope(request).unwrap();
        assert_eq!(
            envelope,
            RequestEnvelope {
                id: 1,
                destination: b"SAM",
                body: b"HAI",
            }
        );
        assert_eq!(envelope.destination.as_ptr(), request[4..].as_ptr());
        assert_eq!(envelope.body.as_ptr(), request[8..].as_ptr());

        assert_eq!(
            decode_request_envelope(b"002 LPC GET PIO2_3 OK?"),
            Ok(RequestEnvelope {
                id: 2,
                destination: b"LPC",
                body: b"GET PIO2_3 OK?",
            })
        );
        assert_eq!(
            decode_request_envelope(b"003 ABC WAT opaque body"),
            Ok(RequestEnvelope {
                id: 3,
                destination: b"ABC",
                body: b"WAT opaque body",
            })
        );
        assert_eq!(
            decode_response_envelope(b"002 LPC HYG PIO2_3 HIGH <3"),
            Ok(ResponseEnvelope {
                id: 2,
                source: b"LPC",
                body: b"HYG PIO2_3 HIGH <3",
            })
        );
    }

    #[test]
    fn routed_envelope_encoding_validates_route_tokens_and_preserves_ids() {
        let mut out = [0; MAX_PACKET_LEN];
        let request = RequestEnvelope {
            id: 999,
            destination: b"ABC",
            body: b"HAI",
        };
        let len = encode_request_envelope(request, &mut out).unwrap();
        assert_eq!(&out[..len], b"999 ABC HAI\n");
        assert_eq!(decode_request_envelope(&out[..len]), Ok(request));

        let response = ResponseEnvelope {
            id: 7,
            source: b"SAM",
            body: b"HII <3",
        };
        let len = encode_response_envelope(response, &mut out).unwrap();
        assert_eq!(&out[..len], b"007 SAM HII <3\n");
        assert_eq!(decode_response_envelope(&out[..len]), Ok(response));

        for route in [b"".as_slice(), b"BAD ROUTE", b"BAD\nROUTE", b"\x01"] {
            assert_eq!(
                encode_request_envelope(
                    RequestEnvelope {
                        id: 1,
                        destination: route,
                        body: b"HAI",
                    },
                    &mut out,
                ),
                Err(EncodeError::InvalidRouteToken)
            );
        }
    }

    #[test]
    fn request_wire_examples_use_symbolic_targets() {
        let cases = [
            (Request::Hello, "001 SAM HAI\n"),
            (Request::Status, "001 SAM HRU\n"),
            (
                Request::Direction {
                    target: PinTarget::Pin(pin(0)),
                    direction: Direction::Input,
                },
                "001 SAM DIR PA00 IN OK?\n",
            ),
            (
                Request::Direction {
                    target: PinTarget::Pin(pin(116)),
                    direction: Direction::Output,
                },
                "001 SAM DIR PE05 OUT OK?\n",
            ),
            (
                Request::Get {
                    target: PinTarget::Pin(pin(5)),
                },
                "001 SAM GET PA05 OK?\n",
            ),
            (
                Request::Set {
                    target: PinTarget::Bank(Port::C),
                    level: Level::High,
                },
                "001 SAM SET PIOC HIGH OK?\n",
            ),
            (
                Request::Pullup {
                    target: PinTarget::Bank(Port::B),
                    enabled: false,
                },
                "001 SAM PLL PIOB OFF OK?\n",
            ),
            (
                Request::Listen {
                    target: PinTarget::Bank(Port::E),
                    enabled: true,
                },
                "001 SAM LSN PIOE ON OK?\n",
            ),
            (
                Request::Query {
                    target: PinTarget::Pin(pin(72)),
                    what: Query::Direction,
                },
                "001 SAM WYD PC25 DIR\n",
            ),
            (
                Request::Direction {
                    target: PinTarget::All,
                    direction: Direction::Input,
                },
                "001 SAM DIR ALL IN OK?\n",
            ),
            (
                Request::Get {
                    target: PinTarget::All,
                },
                "001 SAM GET ALL OK?\n",
            ),
            (
                Request::Set {
                    target: PinTarget::All,
                    level: Level::High,
                },
                "001 SAM SET ALL HIGH OK?\n",
            ),
            (
                Request::Pullup {
                    target: PinTarget::All,
                    enabled: true,
                },
                "001 SAM PLL ALL ON OK?\n",
            ),
            (
                Request::Listen {
                    target: PinTarget::All,
                    enabled: true,
                },
                "001 SAM LSN ALL ON OK?\n",
            ),
            (
                Request::Query {
                    target: PinTarget::All,
                    what: Query::Listen,
                },
                "001 SAM WYD ALL LSN\n",
            ),
            (Request::Bye, "001 SAM BYE\n"),
        ];

        for (body, expected) in cases {
            let out = encoded_request(1, body);
            assert_eq!(&out[..expected.len()], expected.as_bytes());
        }
    }

    #[test]
    fn response_wire_examples_use_symbolic_pins() {
        let cases = [
            (Response::Hello, "008 SAM HII <3\n"),
            (Response::Status, "008 SAM IAM SAM4E8E GPIO <3\n"),
            (Response::Ack, "008 SAM OKA <3\n"),
            (
                Response::Value {
                    pin: pin(0),
                    level: Level::High,
                },
                "008 SAM HYG PA00 HIGH <3\n",
            ),
            (
                Response::State {
                    pin: pin(0),
                    what: Query::Direction,
                    value: QueryValue::Direction(Direction::Input),
                },
                "008 SAM HYG PA00 DIR IN <3\n",
            ),
            (
                Response::State {
                    pin: pin(0),
                    what: Query::Pullup,
                    value: QueryValue::Enabled(true),
                },
                "008 SAM HYG PA00 PLL ON <3\n",
            ),
            (
                Response::State {
                    pin: pin(0),
                    what: Query::Listen,
                    value: QueryValue::Unset,
                },
                "008 SAM HYG PA00 LSN UNSET <3\n",
            ),
            (
                Response::Error(ResponseError::BadPacket),
                "008 SAM UMM BAD_PACKET <3\n",
            ),
            (
                Response::Error(ResponseError::Pin {
                    pin: pin(40),
                    reason: PinError::Unavailable,
                }),
                "008 SAM UMM PB08 UNAVAILABLE <3\n",
            ),
            (
                Response::Error(ResponseError::Pin {
                    pin: pin(3),
                    reason: PinError::Unset,
                }),
                "008 SAM UMM PA03 UNSET <3\n",
            ),
            (
                Response::Error(ResponseError::NoRoute {
                    destination: b"LPC".as_slice(),
                }),
                "008 SAM UMM NO_ROUTE LPC <3\n",
            ),
            (Response::Unknown, "008 SAM IDK <3\n"),
            (Response::Bye, "008 SAM CYA <3\n"),
        ];

        for (body, expected) in cases {
            let packet = Packet { id: 8, body };
            let mut out = [0u8; MAX_PACKET_LEN];
            let len = encode_response(packet, b"SAM", &mut out).unwrap();
            assert_eq!(&out[..len], expected.as_bytes());
            assert_eq!(decoded_response(&out[..len]), Ok(packet));
        }
    }

    #[test]
    fn malformed_known_and_unknown_requests_are_distinct() {
        assert_eq!(
            decoded_request(b"007 SAM DIR PA00 SIDEWAYS OK?\n"),
            Err(DecodeError {
                id: Some(7),
                kind: DecodeErrorKind::Malformed,
            })
        );
        assert_eq!(
            decoded_request(b"007 SAM WAT PA00\n"),
            Err(DecodeError {
                id: Some(7),
                kind: DecodeErrorKind::UnknownCommand,
            })
        );
        assert_eq!(
            decoded_request(b"nope SAM HAI\n"),
            Err(DecodeError {
                id: None,
                kind: DecodeErrorKind::Malformed,
            })
        );
    }

    #[test]
    fn packet_ids_remain_decimal_but_numeric_gpio_targets_are_rejected() {
        assert_eq!(
            decoded_request(b"9 SAM GET PE05 OK?"),
            Ok(Packet {
                id: 9,
                body: Request::Get {
                    target: PinTarget::Pin(pin(116)),
                },
            })
        );
        assert!(decoded_request(b"1000 SAM HAI").is_err());
        assert!(decoded_request(b"001 SAM GET 116 OK?").is_err());
        assert!(decoded_request(b"001 SAM GET PE06 OK?").is_err());
    }

    #[test]
    fn pin_mapping_has_physical_package_numbers() {
        assert_eq!(pin(0).to_string(), "PA00");
        assert_eq!(pin(44).to_string(), "PB12");
        assert_eq!(pin(44).package_pin(), 87);
        assert_eq!(pin(72).to_string(), "PC25");
        assert_eq!(Pin::try_from((Port::E, 5)), Ok(pin(116)));
    }

    #[test]
    fn pin_targets_iterate_contiguous_scope_and_filter_reserved_pins() {
        let single = PinTarget::Pin(pin(44));
        assert_eq!(single.pins().collect::<std::vec::Vec<_>>(), [pin(44)]);

        let bank = PinTarget::Bank(Port::B);
        let bank_pins = bank.pins().collect::<std::vec::Vec<_>>();
        assert_eq!(bank_pins.first(), Some(&pin(32)));
        assert_eq!(bank_pins.last(), Some(&pin(46)));
        assert_eq!(bank_pins.len(), 15);
        assert_eq!(bank.available_pins().count(), 11);

        assert_eq!(PinTarget::All.pins().count(), WIRE_PIN_COUNT as usize);
        assert_eq!(PinTarget::All.available_pins().count(), 113);
    }

    #[test]
    fn pin_table_indexes_with_pin_values() {
        let mut table = PinTable::filled(Level::Low);
        let target = pin(72);
        table[target] = Level::High;
        assert_eq!(table[target], Level::High);
        assert_eq!(table[pin(71)], Level::Low);
    }
}
