#![no_std]

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
    Direction { pin: u8, direction: Direction },
    Get { pin: u8 },
    Set { pin: u8, level: Level },
    Pullup { pin: u8, enabled: bool },
    Listen { pin: u8, enabled: bool },
    Query { pin: u8, what: Query },
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
    Pin { pin: u8, reason: PinError },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    Hello,
    Status,
    Ack,
    Value {
        pin: u8,
        level: Level,
    },
    State {
        pin: u8,
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
            let pin = tokens.next().and_then(parse_pin).ok_or_else(malformed)?;
            let direction = match tokens.next() {
                Some(b"IN") => Direction::Input,
                Some(b"OUT") => Direction::Output,
                _ => return Err(malformed()),
            };
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Direction { pin, direction }
        }
        b"GET" => {
            let pin = tokens.next().and_then(parse_pin).ok_or_else(malformed)?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Get { pin }
        }
        b"SET" => {
            let pin = tokens.next().and_then(parse_pin).ok_or_else(malformed)?;
            let level = tokens.next().and_then(parse_level).ok_or_else(malformed)?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Set { pin, level }
        }
        b"PLL" => {
            let pin = tokens.next().and_then(parse_pin).ok_or_else(malformed)?;
            let enabled = tokens
                .next()
                .and_then(parse_enabled)
                .ok_or_else(malformed)?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Pullup { pin, enabled }
        }
        b"LSN" => {
            let pin = tokens.next().and_then(parse_pin).ok_or_else(malformed)?;
            let enabled = tokens
                .next()
                .and_then(parse_enabled)
                .ok_or_else(malformed)?;
            expect_suffix(&mut tokens, b"OK?", malformed())?;
            Request::Listen { pin, enabled }
        }
        b"WYD" => {
            let pin = tokens.next().and_then(parse_pin).ok_or_else(malformed)?;
            let what = tokens.next().and_then(parse_query).ok_or_else(malformed)?;
            if tokens.next().is_some() {
                return Err(malformed());
            }
            Request::Query { pin, what }
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
        Request::Direction { pin, direction } => {
            writer.bytes(b" DIR ")?;
            writer.pin(pin)?;
            writer.bytes(match direction {
                Direction::Input => b" IN OK?\n",
                Direction::Output => b" OUT OK?\n",
            })?;
        }
        Request::Get { pin } => {
            writer.bytes(b" GET ")?;
            writer.pin(pin)?;
            writer.bytes(b" OK?\n")?;
        }
        Request::Set { pin, level } => {
            writer.bytes(b" SET ")?;
            writer.pin(pin)?;
            writer.bytes(match level {
                Level::Low => b" LOW OK?\n",
                Level::High => b" HIGH OK?\n",
            })?;
        }
        Request::Pullup { pin, enabled } => {
            writer.bytes(b" PLL ")?;
            writer.pin(pin)?;
            writer.bytes(if enabled { b" ON OK?\n" } else { b" OFF OK?\n" })?;
        }
        Request::Listen { pin, enabled } => {
            writer.bytes(b" LSN ")?;
            writer.pin(pin)?;
            writer.bytes(if enabled { b" ON OK?\n" } else { b" OFF OK?\n" })?;
        }
        Request::Query { pin, what } => {
            writer.bytes(b" WYD ")?;
            writer.pin(pin)?;
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

fn parse_pin(token: &[u8]) -> Option<u8> {
    u8::try_from(parse_decimal3(token)?)
        .ok()
        .filter(|pin| *pin < WIRE_PIN_COUNT)
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

    fn pin(&mut self, value: u8) -> Result<(), EncodeError> {
        self.decimal3(u16::from(value))
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
    use super::*;

    fn encoded_request(id: u16, body: Request) -> [u8; MAX_PACKET_LEN] {
        let mut out = [0u8; MAX_PACKET_LEN];
        let len = encode_request(Packet { id, body }, &mut out).unwrap();
        assert_eq!(decode_request(&out[..len]), Ok(Packet { id, body }));
        out
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
    fn request_wire_examples_match_current_controller() {
        let cases = [
            (Request::Hello, "001 HAI\n"),
            (Request::Status, "001 HRU\n"),
            (
                Request::Direction {
                    pin: 0,
                    direction: Direction::Input,
                },
                "001 DIR 000 IN OK?\n",
            ),
            (
                Request::Direction {
                    pin: 116,
                    direction: Direction::Output,
                },
                "001 DIR 116 OUT OK?\n",
            ),
            (Request::Get { pin: 5 }, "001 GET 005 OK?\n"),
            (
                Request::Set {
                    pin: 5,
                    level: Level::High,
                },
                "001 SET 005 HIGH OK?\n",
            ),
            (
                Request::Pullup {
                    pin: 5,
                    enabled: false,
                },
                "001 PLL 005 OFF OK?\n",
            ),
            (
                Request::Listen {
                    pin: 5,
                    enabled: true,
                },
                "001 LSN 005 ON OK?\n",
            ),
            (
                Request::Query {
                    pin: 5,
                    what: Query::Direction,
                },
                "001 WYD 005 DIR\n",
            ),
            (
                Request::Query {
                    pin: 5,
                    what: Query::Pullup,
                },
                "001 WYD 005 PLL\n",
            ),
            (
                Request::Query {
                    pin: 5,
                    what: Query::Listen,
                },
                "001 WYD 005 LSN\n",
            ),
            (Request::Bye, "001 BYE\n"),
        ];

        for (body, expected) in cases {
            let out = encoded_request(1, body);
            assert_eq!(&out[..expected.len()], expected.as_bytes());
        }
    }

    #[test]
    fn response_wire_examples_match_current_firmware() {
        let cases = [
            (Response::Hello, "008 HII <3\n"),
            (Response::Status, "008 IAM SAM4E8E GPIO <3\n"),
            (Response::Ack, "008 OKA <3\n"),
            (
                Response::Value {
                    pin: 0,
                    level: Level::High,
                },
                "008 HYG 000 HIGH <3\n",
            ),
            (
                Response::State {
                    pin: 0,
                    what: Query::Direction,
                    value: QueryValue::Direction(Direction::Input),
                },
                "008 HYG 000 DIR IN <3\n",
            ),
            (
                Response::State {
                    pin: 0,
                    what: Query::Pullup,
                    value: QueryValue::Enabled(true),
                },
                "008 HYG 000 PLL ON <3\n",
            ),
            (
                Response::State {
                    pin: 0,
                    what: Query::Listen,
                    value: QueryValue::Unset,
                },
                "008 HYG 000 LSN UNSET <3\n",
            ),
            (
                Response::Error(ResponseError::BadPacket),
                "008 UMM BAD_PACKET <3\n",
            ),
            (
                Response::Error(ResponseError::Pin {
                    pin: 40,
                    reason: PinError::Unavailable,
                }),
                "008 UMM 040 UNAVAILABLE <3\n",
            ),
            (
                Response::Error(ResponseError::Pin {
                    pin: 3,
                    reason: PinError::Unset,
                }),
                "008 UMM 003 UNSET <3\n",
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
            decode_request(b"007 DIR 000 SIDEWAYS OK?\n"),
            Err(DecodeError {
                id: Some(7),
                kind: DecodeErrorKind::Malformed,
            })
        );
        assert_eq!(
            decode_request(b"007 WAT 000\n"),
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
    fn packet_ids_and_pins_accept_the_same_decimal_range_as_current_firmware() {
        assert_eq!(
            decode_request(b"9 GET 116 OK?"),
            Ok(Packet {
                id: 9,
                body: Request::Get { pin: 116 },
            })
        );
        assert!(decode_request(b"1000 HAI").is_err());
        assert!(decode_request(b"001 GET 117 OK?").is_err());
    }
}
