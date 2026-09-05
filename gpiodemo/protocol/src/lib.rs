#![no_std]

use core::{
    fmt,
    ops::{Index, IndexMut},
};

pub const WIRE_PIN_COUNT: u8 = 117;
pub const MAX_PACKET_LEN: usize = 64;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(u16);

impl RequestId {
    pub const MIN: u16 = 1;
    pub const MAX: u16 = 999;
    pub const COUNT: usize = (Self::MAX - Self::MIN + 1) as usize;
    pub const FIRST: Self = Self(Self::MIN);

    pub const fn new(raw: u16) -> Option<Self> {
        if raw >= Self::MIN && raw <= Self::MAX {
            Some(Self(raw))
        } else {
            None
        }
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub const fn slot(self) -> usize {
        (self.0 - Self::MIN) as usize
    }

    pub const fn next(self) -> Self {
        if self.0 == Self::MAX {
            Self(Self::MIN)
        } else {
            Self(self.0 + 1)
        }
    }
}

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
    pub id: RequestId,
    pub body: T,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message<'a> {
    pub id: RequestId,
    pub route: &'a [u8],
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
pub struct PinCapabilities(u8);

impl PinCapabilities {
    const INPUT_BIT: u8 = 1 << 0;
    const OUTPUT_BIT: u8 = 1 << 1;
    const PULL_UP_BIT: u8 = 1 << 2;

    pub const NONE: Self = Self(0);
    pub const INPUT: Self = Self(Self::INPUT_BIT);
    pub const INPUT_PULLUP: Self = Self(Self::INPUT_BIT | Self::PULL_UP_BIT);
    pub const GPIO: Self = Self(Self::INPUT_BIT | Self::OUTPUT_BIT | Self::PULL_UP_BIT);

    pub const fn new(input: bool, output: bool, pull_up: bool) -> Self {
        Self(
            (if input { Self::INPUT_BIT } else { 0 })
                | (if output { Self::OUTPUT_BIT } else { 0 })
                | (if pull_up { Self::PULL_UP_BIT } else { 0 }),
        )
    }

    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits <= Self::GPIO.0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn available(self) -> bool {
        self.0 != 0
    }

    pub const fn input(self) -> bool {
        self.0 & Self::INPUT_BIT != 0
    }

    pub const fn output(self) -> bool {
        self.0 & Self::OUTPUT_BIT != 0
    }

    pub const fn pull_up(self) -> bool {
        self.0 & Self::PULL_UP_BIT != 0
    }

    pub const fn supports_direction(self, direction: Direction) -> bool {
        match direction {
            Direction::Input => self.input(),
            Direction::Output => self.output(),
        }
    }
}

impl TryFrom<&[u8]> for PinCapabilities {
    type Error = ParseTokenError;

    fn try_from(token: &[u8]) -> Result<Self, Self::Error> {
        let [digit] = token else {
            return Err(ParseTokenError);
        };
        let bits = match digit {
            b'0'..=b'7' => *digit - b'0',
            _ => return Err(ParseTokenError),
        };
        Self::from_bits(bits).ok_or(ParseTokenError)
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
pub enum Request<T> {
    Hello,
    Status,
    Map,
    Direction { target: T, direction: Direction },
    Get { target: T },
    Set { target: T, level: Level },
    Pullup { target: T, enabled: bool },
    Listen { target: T, enabled: bool },
    Query { target: T, what: Query },
    Bye,
}

impl<T> Request<T> {
    pub fn map_target<U>(self, map: impl FnOnce(T) -> U) -> Request<U> {
        match self {
            Self::Hello => Request::Hello,
            Self::Status => Request::Status,
            Self::Map => Request::Map,
            Self::Direction { target, direction } => Request::Direction {
                target: map(target),
                direction,
            },
            Self::Get { target } => Request::Get {
                target: map(target),
            },
            Self::Set { target, level } => Request::Set {
                target: map(target),
                level,
            },
            Self::Pullup { target, enabled } => Request::Pullup {
                target: map(target),
                enabled,
            },
            Self::Listen { target, enabled } => Request::Listen {
                target: map(target),
                enabled,
            },
            Self::Query { target, what } => Request::Query {
                target: map(target),
                what,
            },
            Self::Bye => Request::Bye,
        }
    }

    pub fn try_map_target<U, E>(
        self,
        map: impl FnOnce(T) -> Result<U, E>,
    ) -> Result<Request<U>, E> {
        Ok(match self {
            Self::Hello => Request::Hello,
            Self::Status => Request::Status,
            Self::Map => Request::Map,
            Self::Direction { target, direction } => Request::Direction {
                target: map(target)?,
                direction,
            },
            Self::Get { target } => Request::Get {
                target: map(target)?,
            },
            Self::Set { target, level } => Request::Set {
                target: map(target)?,
                level,
            },
            Self::Pullup { target, enabled } => Request::Pullup {
                target: map(target)?,
                enabled,
            },
            Self::Listen { target, enabled } => Request::Listen {
                target: map(target)?,
                enabled,
            },
            Self::Query { target, what } => Request::Query {
                target: map(target)?,
                what,
            },
            Self::Bye => Request::Bye,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetError {
    Unset,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseError<T, D> {
    BadPacket,
    Target { target: T, reason: TargetError },
    NoRoute { destination: D },
    RouteBusy { next_hop: D },
    RouteDown { next_hop: D },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response<T, D> {
    Hello,
    Status {
        identity: D,
    },
    MapBank {
        bank: D,
    },
    MapPin {
        target: T,
        package_pin: Option<u16>,
        bank: D,
        bit: u8,
        capabilities: PinCapabilities,
    },
    Ack,
    Value {
        target: T,
        level: Level,
    },
    State {
        target: T,
        what: Query,
        value: QueryValue,
    },
    Error(ResponseError<T, D>),
    Unknown,
    Bye,
}

pub type DecodedRequest<'a> = Request<&'a [u8]>;
pub type DecodedResponse<'a> = Response<&'a [u8], &'a [u8]>;

impl<T, D> Response<T, D> {
    pub fn try_map<T2, D2, E>(
        self,
        map_target: impl FnOnce(T) -> Result<T2, E>,
        map_data: impl FnOnce(D) -> Result<D2, E>,
    ) -> Result<Response<T2, D2>, E> {
        Ok(match self {
            Self::Hello => Response::Hello,
            Self::Status { identity } => Response::Status {
                identity: map_data(identity)?,
            },
            Self::MapBank { bank } => Response::MapBank {
                bank: map_data(bank)?,
            },
            Self::MapPin {
                target,
                package_pin,
                bank,
                bit,
                capabilities,
            } => Response::MapPin {
                target: map_target(target)?,
                package_pin,
                bank: map_data(bank)?,
                bit,
                capabilities,
            },
            Self::Ack => Response::Ack,
            Self::Value { target, level } => Response::Value {
                target: map_target(target)?,
                level,
            },
            Self::State {
                target,
                what,
                value,
            } => Response::State {
                target: map_target(target)?,
                what,
                value,
            },
            Self::Error(ResponseError::BadPacket) => Response::Error(ResponseError::BadPacket),
            Self::Error(ResponseError::Target { target, reason }) => {
                Response::Error(ResponseError::Target {
                    target: map_target(target)?,
                    reason,
                })
            }
            Self::Error(ResponseError::NoRoute { destination }) => {
                Response::Error(ResponseError::NoRoute {
                    destination: map_data(destination)?,
                })
            }
            Self::Error(ResponseError::RouteBusy { next_hop }) => {
                Response::Error(ResponseError::RouteBusy {
                    next_hop: map_data(next_hop)?,
                })
            }
            Self::Error(ResponseError::RouteDown { next_hop }) => {
                Response::Error(ResponseError::RouteDown {
                    next_hop: map_data(next_hop)?,
                })
            }
            Self::Unknown => Response::Unknown,
            Self::Bye => Response::Bye,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeErrorKind {
    Malformed,
    UnknownCommand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeError {
    pub id: Option<RequestId>,
    pub kind: DecodeErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    OutputTooSmall,
    InvalidRouteToken,
    InvalidTargetToken,
    InvalidIdentity,
}

pub fn decode_message(line: &[u8]) -> Result<Message<'_>, DecodeError> {
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
    Ok(Message { id, route, body })
}

pub fn encode_message(message: Message<'_>, out: &mut [u8]) -> Result<usize, EncodeError> {
    encode_message_with(message.id, message.route, out, |writer| {
        writer.bytes(message.body)
    })
}

pub fn decode_request(packet: Packet<&[u8]>) -> Result<Packet<DecodedRequest<'_>>, DecodeError> {
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
        b"MAP" if tokens.next().is_none() => Request::Map,
        b"BYE" if tokens.next().is_none() => Request::Bye,
        b"DIR" => {
            let target = next_target(&mut tokens, malformed())?;
            let direction: Direction = next_as(&mut tokens, malformed())?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Direction { target, direction }
        }
        b"GET" => {
            let target = next_target(&mut tokens, malformed())?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Get { target }
        }
        b"SET" => {
            let target = next_target(&mut tokens, malformed())?;
            let level: Level = next_as(&mut tokens, malformed())?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Set { target, level }
        }
        b"PLL" => {
            let target = next_target(&mut tokens, malformed())?;
            let enabled =
                parse_enabled(tokens.next().ok_or_else(malformed)?).ok_or_else(malformed)?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Pullup { target, enabled }
        }
        b"LSN" => {
            let target = next_target(&mut tokens, malformed())?;
            let enabled =
                parse_enabled(tokens.next().ok_or_else(malformed)?).ok_or_else(malformed)?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Listen { target, enabled }
        }
        b"WYD" => {
            let target = next_target(&mut tokens, malformed())?;
            let what: Query = next_as(&mut tokens, malformed())?;
            if tokens.next().is_some() {
                return Err(malformed());
            }
            Request::Query { target, what }
        }
        b"HAI" | b"HRU" | b"MAP" | b"BYE" => return Err(malformed()),
        _ => {
            return Err(DecodeError {
                id: Some(id),
                kind: DecodeErrorKind::UnknownCommand,
            });
        }
    };

    Ok(Packet { id, body })
}

pub fn decode_response(packet: Packet<&[u8]>) -> Result<Packet<DecodedResponse<'_>>, DecodeError> {
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
            let identity = status_identity(packet.body).ok_or_else(malformed)?;
            Response::Status { identity }
        }
        b"MAP" => match tokens.next() {
            Some(b"BANK") => {
                let bank = next_target(&mut tokens, malformed())?;
                expect_suffix(&mut tokens, b"<3", malformed())?;
                Response::MapBank { bank }
            }
            Some(b"PIN") => {
                let target = next_target(&mut tokens, malformed())?;
                let package_pin = match tokens.next().ok_or_else(malformed)? {
                    b"-" => None,
                    token => Some(parse_u16(token).ok_or_else(malformed)?),
                };
                let bank = next_target(&mut tokens, malformed())?;
                let bit = parse_u8(tokens.next().ok_or_else(malformed)?).ok_or_else(malformed)?;
                let capabilities = next_as(&mut tokens, malformed())?;
                expect_suffix(&mut tokens, b"<3", malformed())?;
                Response::MapPin {
                    target,
                    package_pin,
                    bank,
                    bit,
                    capabilities,
                }
            }
            _ => return Err(malformed()),
        },
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
                Some(b"ROUTE_BUSY") => {
                    let next_hop = tokens.next().ok_or_else(malformed)?;
                    if !valid_route_token(next_hop) {
                        return Err(malformed());
                    }
                    ResponseError::RouteBusy { next_hop }
                }
                Some(b"ROUTE_DOWN") => {
                    let next_hop = tokens.next().ok_or_else(malformed)?;
                    if !valid_route_token(next_hop) {
                        return Err(malformed());
                    }
                    ResponseError::RouteDown { next_hop }
                }
                Some(target) if valid_target_token(target) => {
                    let reason = match tokens.next() {
                        Some(b"UNSET") => TargetError::Unset,
                        Some(b"UNAVAILABLE") => TargetError::Unavailable,
                        _ => return Err(malformed()),
                    };
                    ResponseError::Target { target, reason }
                }
                Some(_) => return Err(malformed()),
                None => return Err(malformed()),
            };
            expect_suffix(&mut tokens, b"<3", malformed())?;
            Response::Error(error)
        }
        b"HYG" => {
            let target = next_target(&mut tokens, malformed())?;
            let next = tokens.next().ok_or_else(malformed)?;
            if let Ok(level) = Level::try_from(next) {
                expect_suffix(&mut tokens, b"<3", malformed())?;
                Response::Value { target, level }
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
                Response::State {
                    target,
                    what,
                    value,
                }
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

pub fn encode_request<T: AsRef<[u8]>>(
    packet: Packet<Request<T>>,
    destination: &[u8],
    out: &mut [u8],
) -> Result<usize, EncodeError> {
    encode_message_with(packet.id, destination, out, |writer| {
        encode_request_body(writer, packet.body)
    })
}

fn encode_request_body<T: AsRef<[u8]>>(
    writer: &mut Writer<'_>,
    body: Request<T>,
) -> Result<(), EncodeError> {
    match body {
        Request::Hello => writer.bytes(b"HAI")?,
        Request::Status => writer.bytes(b"HRU")?,
        Request::Map => writer.bytes(b"MAP")?,
        Request::Direction { target, direction } => {
            writer.bytes(b"DIR ")?;
            writer.target(target.as_ref())?;
            writer.bytes(match direction {
                Direction::Input => b" IN OK?",
                Direction::Output => b" OUT OK?",
            })?;
        }
        Request::Get { target } => {
            writer.bytes(b"GET ")?;
            writer.target(target.as_ref())?;
            writer.bytes(b" OK?")?;
        }
        Request::Set { target, level } => {
            writer.bytes(b"SET ")?;
            writer.target(target.as_ref())?;
            writer.bytes(match level {
                Level::Low => b" LOW OK?",
                Level::High => b" HIGH OK?",
            })?;
        }
        Request::Pullup { target, enabled } => {
            writer.bytes(b"PLL ")?;
            writer.target(target.as_ref())?;
            writer.bytes(if enabled { b" ON OK?" } else { b" OFF OK?" })?;
        }
        Request::Listen { target, enabled } => {
            writer.bytes(b"LSN ")?;
            writer.target(target.as_ref())?;
            writer.bytes(if enabled { b" ON OK?" } else { b" OFF OK?" })?;
        }
        Request::Query { target, what } => {
            writer.bytes(b"WYD ")?;
            writer.target(target.as_ref())?;
            writer.bytes(match what {
                Query::Direction => b" DIR",
                Query::Pullup => b" PLL",
                Query::Listen => b" LSN",
            })?;
        }
        Request::Bye => writer.bytes(b"BYE")?,
    }
    Ok(())
}

pub fn encode_response<T: AsRef<[u8]>, D: AsRef<[u8]>>(
    packet: Packet<Response<T, D>>,
    source: &[u8],
    out: &mut [u8],
) -> Result<usize, EncodeError> {
    encode_message_with(packet.id, source, out, |writer| {
        encode_response_body(writer, packet.body)
    })
}

fn encode_response_body<T: AsRef<[u8]>, D: AsRef<[u8]>>(
    writer: &mut Writer<'_>,
    body: Response<T, D>,
) -> Result<(), EncodeError> {
    match body {
        Response::Hello => writer.bytes(b"HII <3")?,
        Response::Status { identity } => {
            if !valid_identity(identity.as_ref()) {
                return Err(EncodeError::InvalidIdentity);
            }
            writer.bytes(b"IAM ")?;
            writer.bytes(identity.as_ref())?;
            writer.bytes(b" <3")?;
        }
        Response::MapBank { bank } => {
            writer.bytes(b"MAP BANK ")?;
            writer.target(bank.as_ref())?;
            writer.bytes(b" <3")?;
        }
        Response::MapPin {
            target,
            package_pin,
            bank,
            bit,
            capabilities,
        } => {
            writer.bytes(b"MAP PIN ")?;
            writer.target(target.as_ref())?;
            writer.bytes(b" ")?;
            if let Some(package_pin) = package_pin {
                writer.decimal(package_pin)?;
            } else {
                writer.bytes(b"-")?;
            }
            writer.bytes(b" ")?;
            writer.target(bank.as_ref())?;
            writer.bytes(b" ")?;
            writer.decimal(u16::from(bit))?;
            writer.bytes(b" ")?;
            writer.bytes(&[b'0' + capabilities.bits()])?;
            writer.bytes(b" <3")?;
        }
        Response::Ack => writer.bytes(b"OKA <3")?,
        Response::Value { target, level } => {
            writer.bytes(b"HYG ")?;
            writer.target(target.as_ref())?;
            writer.bytes(match level {
                Level::Low => b" LOW <3",
                Level::High => b" HIGH <3",
            })?;
        }
        Response::State {
            target,
            what,
            value,
        } => {
            writer.bytes(b"HYG ")?;
            writer.target(target.as_ref())?;
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
            writer.bytes(b" <3")?;
        }
        Response::Error(ResponseError::BadPacket) => writer.bytes(b"UMM BAD_PACKET <3")?,
        Response::Error(ResponseError::Target { target, reason }) => {
            writer.bytes(b"UMM ")?;
            writer.target(target.as_ref())?;
            writer.bytes(match reason {
                TargetError::Unset => b" UNSET <3",
                TargetError::Unavailable => b" UNAVAILABLE <3",
            })?;
        }
        Response::Error(ResponseError::NoRoute { destination }) => {
            writer.bytes(b"UMM NO_ROUTE ")?;
            writer.route(destination.as_ref())?;
            writer.bytes(b" <3")?;
        }
        Response::Error(ResponseError::RouteBusy { next_hop }) => {
            writer.bytes(b"UMM ROUTE_BUSY ")?;
            writer.route(next_hop.as_ref())?;
            writer.bytes(b" <3")?;
        }
        Response::Error(ResponseError::RouteDown { next_hop }) => {
            writer.bytes(b"UMM ROUTE_DOWN ")?;
            writer.route(next_hop.as_ref())?;
            writer.bytes(b" <3")?;
        }
        Response::Unknown => writer.bytes(b"IDK <3")?,
        Response::Bye => writer.bytes(b"CYA <3")?,
    }
    Ok(())
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

fn next_target<'a>(
    tokens: &mut impl Iterator<Item = &'a [u8]>,
    error: DecodeError,
) -> Result<&'a [u8], DecodeError> {
    let target = tokens.next().ok_or(error)?;
    valid_target_token(target).then_some(target).ok_or(error)
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

fn parse_u8(token: &[u8]) -> Option<u8> {
    u8::try_from(parse_u16(token)?).ok()
}

fn parse_u16(token: &[u8]) -> Option<u16> {
    if token.is_empty() || !token.iter().all(u8::is_ascii_digit) {
        return None;
    }
    token.iter().try_fold(0u16, |value, digit| {
        value.checked_mul(10)?.checked_add(u16::from(*digit - b'0'))
    })
}

fn parse_enabled(token: &[u8]) -> Option<bool> {
    match token {
        b"ON" => Some(true),
        b"OFF" => Some(false),
        _ => None,
    }
}

fn encode_message_with(
    id: RequestId,
    route: &[u8],
    out: &mut [u8],
    write_body: impl FnOnce(&mut Writer<'_>) -> Result<(), EncodeError>,
) -> Result<usize, EncodeError> {
    let capacity = out.len().min(MAX_PACKET_LEN);
    let mut writer = Writer::new(&mut out[..capacity]);
    writer.id(id)?;
    writer.bytes(b" ")?;
    writer.route(route)?;
    writer.bytes(b" ")?;
    write_body(&mut writer)?;
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

fn valid_target_token(token: &[u8]) -> bool {
    token.first().is_some_and(u8::is_ascii_uppercase)
        && token
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn status_identity(body: &[u8]) -> Option<&[u8]> {
    let identity = body.strip_prefix(b"IAM ")?.strip_suffix(b" <3")?;
    valid_identity(identity).then_some(identity)
}

fn valid_identity(identity: &[u8]) -> bool {
    !identity.is_empty() && identity.split(|byte| *byte == b' ').all(valid_route_token)
}

fn parse_packet_id(token: &[u8]) -> Option<RequestId> {
    if token.is_empty() || token.len() > 3 || !token.iter().all(u8::is_ascii_digit) {
        return None;
    }
    RequestId::new(core::str::from_utf8(token).ok()?.parse().ok()?)
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

    fn id(&mut self, value: RequestId) -> Result<(), EncodeError> {
        self.decimal3(value.get())
    }

    fn route(&mut self, route: &[u8]) -> Result<(), EncodeError> {
        if !valid_route_token(route) {
            return Err(EncodeError::InvalidRouteToken);
        }
        self.bytes(route)
    }

    fn target(&mut self, target: &[u8]) -> Result<(), EncodeError> {
        if !valid_target_token(target) {
            return Err(EncodeError::InvalidTargetToken);
        }
        self.bytes(target)
    }

    fn decimal(&mut self, value: u16) -> Result<(), EncodeError> {
        let mut digits = [0u8; 5];
        let mut value = value;
        let mut start = digits.len();
        loop {
            start -= 1;
            digits[start] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                return self.bytes(&digits[start..]);
            }
        }
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

    fn id(raw: u16) -> RequestId {
        RequestId::new(raw).unwrap()
    }

    fn encoded_request(id: RequestId, body: Request<&'static [u8]>) -> [u8; MAX_PACKET_LEN] {
        let mut out = [0u8; MAX_PACKET_LEN];
        let len = encode_request(Packet { id, body }, b"SAM", &mut out).unwrap();
        assert_eq!(decoded_request(&out[..len]), Ok(Packet { id, body }));
        out
    }

    fn decoded_request(line: &[u8]) -> Result<Packet<DecodedRequest<'_>>, DecodeError> {
        let envelope = decode_message(line)?;
        decode_request(Packet {
            id: envelope.id,
            body: envelope.body,
        })
    }

    fn decoded_response(line: &[u8]) -> Result<Packet<DecodedResponse<'_>>, DecodeError> {
        let envelope = decode_message(line)?;
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
        let envelope = decode_message(request).unwrap();
        assert_eq!(
            envelope,
            Message {
                id: id(1),
                route: b"SAM",
                body: b"HAI",
            }
        );
        assert_eq!(envelope.route.as_ptr(), request[4..].as_ptr());
        assert_eq!(envelope.body.as_ptr(), request[8..].as_ptr());

        assert_eq!(
            decode_message(b"002 LPC GET PIO2_3 OK?"),
            Ok(Message {
                id: id(2),
                route: b"LPC",
                body: b"GET PIO2_3 OK?",
            })
        );
        assert_eq!(
            decode_message(b"003 ABC WAT opaque body"),
            Ok(Message {
                id: id(3),
                route: b"ABC",
                body: b"WAT opaque body",
            })
        );
        assert_eq!(
            decode_message(b"002 LPC HYG PIO2_3 HIGH <3"),
            Ok(Message {
                id: id(2),
                route: b"LPC",
                body: b"HYG PIO2_3 HIGH <3",
            })
        );
    }

    #[test]
    fn routed_envelope_encoding_validates_route_tokens_and_preserves_ids() {
        let mut out = [0; MAX_PACKET_LEN];
        let request = Message {
            id: id(999),
            route: b"ABC",
            body: b"HAI",
        };
        let len = encode_message(request, &mut out).unwrap();
        assert_eq!(&out[..len], b"999 ABC HAI\n");
        assert_eq!(decode_message(&out[..len]), Ok(request));

        let response = Message {
            id: id(7),
            route: b"SAM",
            body: b"HII <3",
        };
        let len = encode_message(response, &mut out).unwrap();
        assert_eq!(&out[..len], b"007 SAM HII <3\n");
        assert_eq!(decode_message(&out[..len]), Ok(response));

        for route in [b"".as_slice(), b"BAD ROUTE", b"BAD\nROUTE", b"\x01"] {
            assert_eq!(
                encode_message(
                    Message {
                        id: id(1),
                        route,
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
            (Request::Map, "001 SAM MAP\n"),
            (
                Request::Direction {
                    target: b"PA00".as_slice(),
                    direction: Direction::Input,
                },
                "001 SAM DIR PA00 IN OK?\n",
            ),
            (
                Request::Direction {
                    target: b"PE05".as_slice(),
                    direction: Direction::Output,
                },
                "001 SAM DIR PE05 OUT OK?\n",
            ),
            (
                Request::Get {
                    target: b"PA05".as_slice(),
                },
                "001 SAM GET PA05 OK?\n",
            ),
            (
                Request::Set {
                    target: b"PIOC".as_slice(),
                    level: Level::High,
                },
                "001 SAM SET PIOC HIGH OK?\n",
            ),
            (
                Request::Pullup {
                    target: b"PIOB".as_slice(),
                    enabled: false,
                },
                "001 SAM PLL PIOB OFF OK?\n",
            ),
            (
                Request::Listen {
                    target: b"PIOE".as_slice(),
                    enabled: true,
                },
                "001 SAM LSN PIOE ON OK?\n",
            ),
            (
                Request::Query {
                    target: b"PC25".as_slice(),
                    what: Query::Direction,
                },
                "001 SAM WYD PC25 DIR\n",
            ),
            (
                Request::Direction {
                    target: b"ALL".as_slice(),
                    direction: Direction::Input,
                },
                "001 SAM DIR ALL IN OK?\n",
            ),
            (
                Request::Get {
                    target: b"ALL".as_slice(),
                },
                "001 SAM GET ALL OK?\n",
            ),
            (
                Request::Set {
                    target: b"ALL".as_slice(),
                    level: Level::High,
                },
                "001 SAM SET ALL HIGH OK?\n",
            ),
            (
                Request::Pullup {
                    target: b"ALL".as_slice(),
                    enabled: true,
                },
                "001 SAM PLL ALL ON OK?\n",
            ),
            (
                Request::Listen {
                    target: b"ALL".as_slice(),
                    enabled: true,
                },
                "001 SAM LSN ALL ON OK?\n",
            ),
            (
                Request::Query {
                    target: b"ALL".as_slice(),
                    what: Query::Listen,
                },
                "001 SAM WYD ALL LSN\n",
            ),
            (Request::Bye, "001 SAM BYE\n"),
        ];

        for (body, expected) in cases {
            let out = encoded_request(id(1), body);
            assert_eq!(&out[..expected.len()], expected.as_bytes());
        }
    }

    #[test]
    fn response_wire_examples_use_symbolic_pins() {
        let cases = [
            (Response::Hello, "008 SAM HII <3\n"),
            (
                Response::Status {
                    identity: b"SAM4E8E GPIO".as_slice(),
                },
                "008 SAM IAM SAM4E8E GPIO <3\n",
            ),
            (
                Response::MapBank {
                    bank: b"PIOA".as_slice(),
                },
                "008 SAM MAP BANK PIOA <3\n",
            ),
            (
                Response::MapPin {
                    target: b"PA00".as_slice(),
                    package_pin: Some(102),
                    bank: b"PIOA".as_slice(),
                    bit: 0,
                    capabilities: PinCapabilities::GPIO,
                },
                "008 SAM MAP PIN PA00 102 PIOA 0 7 <3\n",
            ),
            (
                Response::MapPin {
                    target: b"PIO2_3".as_slice(),
                    package_pin: None,
                    bank: b"PIO2".as_slice(),
                    bit: 3,
                    capabilities: PinCapabilities::INPUT,
                },
                "008 SAM MAP PIN PIO2_3 - PIO2 3 1 <3\n",
            ),
            (Response::Ack, "008 SAM OKA <3\n"),
            (
                Response::Value {
                    target: b"PA00".as_slice(),
                    level: Level::High,
                },
                "008 SAM HYG PA00 HIGH <3\n",
            ),
            (
                Response::State {
                    target: b"PA00".as_slice(),
                    what: Query::Direction,
                    value: QueryValue::Direction(Direction::Input),
                },
                "008 SAM HYG PA00 DIR IN <3\n",
            ),
            (
                Response::State {
                    target: b"PA00".as_slice(),
                    what: Query::Pullup,
                    value: QueryValue::Enabled(true),
                },
                "008 SAM HYG PA00 PLL ON <3\n",
            ),
            (
                Response::State {
                    target: b"PA00".as_slice(),
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
                Response::Error(ResponseError::Target {
                    target: b"PB08".as_slice(),
                    reason: TargetError::Unavailable,
                }),
                "008 SAM UMM PB08 UNAVAILABLE <3\n",
            ),
            (
                Response::Error(ResponseError::Target {
                    target: b"PA03".as_slice(),
                    reason: TargetError::Unset,
                }),
                "008 SAM UMM PA03 UNSET <3\n",
            ),
            (
                Response::Error(ResponseError::NoRoute {
                    destination: b"LPC".as_slice(),
                }),
                "008 SAM UMM NO_ROUTE LPC <3\n",
            ),
            (
                Response::Error(ResponseError::RouteBusy {
                    next_hop: b"LPC".as_slice(),
                }),
                "008 SAM UMM ROUTE_BUSY LPC <3\n",
            ),
            (
                Response::Error(ResponseError::RouteDown {
                    next_hop: b"LPC".as_slice(),
                }),
                "008 SAM UMM ROUTE_DOWN LPC <3\n",
            ),
            (Response::Unknown, "008 SAM IDK <3\n"),
            (Response::Bye, "008 SAM CYA <3\n"),
        ];

        for (body, expected) in cases {
            let packet = Packet { id: id(8), body };
            let mut out = [0u8; MAX_PACKET_LEN];
            let len = encode_response(packet, b"SAM", &mut out).unwrap();
            assert_eq!(&out[..len], expected.as_bytes());
            assert_eq!(decoded_response(&out[..len]), Ok(packet));
        }
    }

    #[test]
    fn map_codec_rejects_bad_numeric_and_capability_fields() {
        for line in [
            b"008 SAM MAP PIN PA00 nope PIOA 0 7 <3".as_slice(),
            b"008 SAM MAP PIN PA00 102 PIOA 999 7 <3",
            b"008 SAM MAP PIN PA00 102 PIOA 0 8 <3",
        ] {
            assert_eq!(
                decoded_response(line),
                Err(DecodeError {
                    id: Some(id(8)),
                    kind: DecodeErrorKind::Malformed,
                })
            );
        }
    }

    #[test]
    fn malformed_known_and_unknown_requests_are_distinct() {
        assert_eq!(
            decoded_request(b"007 SAM DIR PA00 SIDEWAYS OK?\n"),
            Err(DecodeError {
                id: Some(id(7)),
                kind: DecodeErrorKind::Malformed,
            })
        );
        assert_eq!(
            decoded_request(b"007 SAM WAT PA00\n"),
            Err(DecodeError {
                id: Some(id(7)),
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
        assert_eq!(RequestId::new(0), None);
        assert_eq!(RequestId::new(1), Some(RequestId::FIRST));
        assert_eq!(RequestId::new(999).unwrap().next(), RequestId::FIRST);
        assert_eq!(RequestId::new(1000), None);
        assert_eq!(RequestId::new(1).unwrap().slot(), 0);
        assert_eq!(RequestId::new(999).unwrap().slot(), RequestId::COUNT - 1);
        assert!(decoded_request(b"000 SAM HAI").is_err());
        assert_eq!(
            decoded_request(b"9 SAM GET PE05 OK?"),
            Ok(Packet {
                id: id(9),
                body: Request::Get {
                    target: b"PE05".as_slice(),
                },
            })
        );
        assert!(decoded_request(b"1000 SAM HAI").is_err());
        assert!(decoded_request(b"001 SAM GET 116 OK?").is_err());
        assert_eq!(
            decoded_request(b"001 SAM GET PE06 OK?"),
            Ok(Packet {
                id: id(1),
                body: Request::Get {
                    target: b"PE06".as_slice(),
                },
            })
        );
        assert_eq!(
            decoded_request(b"002 LPC GET PIO2_3 OK?"),
            Ok(Packet {
                id: id(2),
                body: Request::Get {
                    target: b"PIO2_3".as_slice(),
                },
            })
        );
    }

    #[test]
    fn typed_codec_round_trips_non_sam_targets_and_identity() {
        let request = Packet {
            id: id(21),
            body: Request::Set {
                target: b"PIO2_3".as_slice(),
                level: Level::High,
            },
        };
        let mut out = [0; MAX_PACKET_LEN];
        let len = encode_request(request, b"LPC", &mut out).unwrap();
        assert_eq!(&out[..len], b"021 LPC SET PIO2_3 HIGH OK?\n");
        assert_eq!(decoded_request(&out[..len]), Ok(request));

        let response = Packet {
            id: id(22),
            body: Response::Value {
                target: b"PIO2_3".as_slice(),
                level: Level::Low,
            },
        };
        let len = encode_response(response, b"LPC", &mut out).unwrap();
        assert_eq!(&out[..len], b"022 LPC HYG PIO2_3 LOW <3\n");
        assert_eq!(decoded_response(&out[..len]), Ok(response));

        let status = Packet {
            id: id(23),
            body: Response::<&[u8], &[u8]>::Status {
                identity: b"LPC1115 GPIO",
            },
        };
        let len = encode_response(status, b"LPC", &mut out).unwrap();
        assert_eq!(&out[..len], b"023 LPC IAM LPC1115 GPIO <3\n");
        assert_eq!(decoded_response(&out[..len]), Ok(status));
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
