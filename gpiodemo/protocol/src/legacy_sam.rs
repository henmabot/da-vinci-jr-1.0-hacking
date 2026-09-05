use core::{
    fmt,
    ops::{Index, IndexMut},
};

use crate::command::ParseTokenError;

pub const WIRE_PIN_COUNT: u8 = 117;

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
