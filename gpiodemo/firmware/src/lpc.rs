use crate::gpio::{BankId, BankInfo, Capabilities, PinId, PinInfo, PinMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LpcBank {
    Pio0,
    Pio1,
    Pio2,
    Pio3,
}

impl LpcBank {
    pub const fn id(self) -> BankId {
        BankId::new(match self {
            Self::Pio0 => 0,
            Self::Pio1 => 1,
            Self::Pio2 => 2,
            Self::Pio3 => 3,
        })
    }

    pub fn from_id(id: BankId) -> Option<Self> {
        match id.index() {
            0 => Some(Self::Pio0),
            1 => Some(Self::Pio1),
            2 => Some(Self::Pio2),
            3 => Some(Self::Pio3),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LpcPadKind {
    Standard,
    Analog,
    I2cOpenDrain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LpcPinHw {
    bank: LpcBank,
    bit: u8,
    iocon_offset: u8,
    gpio_function: u8,
    pad_kind: LpcPadKind,
}

impl LpcPinHw {
    pub const fn bank(self) -> LpcBank {
        self.bank
    }

    pub const fn bit(self) -> u8 {
        self.bit
    }

    pub const fn iocon_offset(self) -> u8 {
        self.iocon_offset
    }

    pub const fn gpio_function(self) -> u8 {
        self.gpio_function
    }

    pub const fn pad_kind(self) -> LpcPadKind {
        self.pad_kind
    }
}

pub const LPC_IDENTITY: &[u8] = b"LPC1115 GPIO";

static BANKS: [BankInfo; 4] = [
    BankInfo::new("PIO0"),
    BankInfo::new("PIO1"),
    BankInfo::new("PIO2"),
    BankInfo::new("PIO3"),
];

macro_rules! lpc_pins {
    ($($token:literal => {
        package: $package:literal,
        bank: $bank:ident,
        bit: $bit:literal,
        iocon: $iocon:literal,
        function: $function:literal,
        kind: $kind:ident,
        caps: $caps:ident
    }),+ $(,)?) => {
        static PINS: &[PinInfo] = &[
            $(PinInfo::new(
                $token,
                Some($package),
                LpcBank::$bank.id(),
                $bit,
                Capabilities::$caps,
            )),+
        ];

        static LPC_HW: &[LpcPinHw] = &[
            $(LpcPinHw {
                bank: LpcBank::$bank,
                bit: $bit,
                iocon_offset: $iocon,
                gpio_function: $function,
                pad_kind: LpcPadKind::$kind,
            }),+
        ];
    };
}

lpc_pins! {
    "PIO0_0" => { package: 3, bank: Pio0, bit: 0, iocon: 0x0c, function: 0, kind: Standard, caps: NONE },
    "PIO0_1" => { package: 4, bank: Pio0, bit: 1, iocon: 0x10, function: 0, kind: Standard, caps: INPUT },
    "PIO0_2" => { package: 10, bank: Pio0, bit: 2, iocon: 0x1c, function: 0, kind: Standard, caps: INPUT },
    "PIO0_3" => { package: 14, bank: Pio0, bit: 3, iocon: 0x2c, function: 0, kind: Standard, caps: INPUT },
    "PIO0_4" => { package: 15, bank: Pio0, bit: 4, iocon: 0x30, function: 0, kind: I2cOpenDrain, caps: INPUT },
    "PIO0_5" => { package: 16, bank: Pio0, bit: 5, iocon: 0x34, function: 0, kind: I2cOpenDrain, caps: INPUT },
    "PIO0_6" => { package: 22, bank: Pio0, bit: 6, iocon: 0x4c, function: 0, kind: Standard, caps: INPUT },
    "PIO0_7" => { package: 23, bank: Pio0, bit: 7, iocon: 0x50, function: 0, kind: Standard, caps: INPUT },
    "PIO0_8" => { package: 27, bank: Pio0, bit: 8, iocon: 0x60, function: 0, kind: Standard, caps: INPUT },
    "PIO0_9" => { package: 28, bank: Pio0, bit: 9, iocon: 0x64, function: 0, kind: Standard, caps: INPUT },
    "PIO0_10" => { package: 29, bank: Pio0, bit: 10, iocon: 0x68, function: 0, kind: Standard, caps: NONE },
    "PIO0_11" => { package: 32, bank: Pio0, bit: 11, iocon: 0x74, function: 1, kind: Analog, caps: INPUT },
    "PIO1_0" => { package: 33, bank: Pio1, bit: 0, iocon: 0x78, function: 1, kind: Analog, caps: INPUT },
    "PIO1_1" => { package: 34, bank: Pio1, bit: 1, iocon: 0x7c, function: 1, kind: Analog, caps: INPUT },
    "PIO1_2" => { package: 35, bank: Pio1, bit: 2, iocon: 0x80, function: 1, kind: Analog, caps: INPUT },
    "PIO1_3" => { package: 39, bank: Pio1, bit: 3, iocon: 0x90, function: 0, kind: Analog, caps: NONE },
    "PIO1_4" => { package: 40, bank: Pio1, bit: 4, iocon: 0x94, function: 0, kind: Analog, caps: INPUT },
    "PIO1_5" => { package: 45, bank: Pio1, bit: 5, iocon: 0xa0, function: 0, kind: Standard, caps: INPUT },
    "PIO1_6" => { package: 46, bank: Pio1, bit: 6, iocon: 0xa4, function: 0, kind: Standard, caps: NONE },
    "PIO1_7" => { package: 47, bank: Pio1, bit: 7, iocon: 0xa8, function: 0, kind: Standard, caps: NONE },
    "PIO1_8" => { package: 9, bank: Pio1, bit: 8, iocon: 0x14, function: 0, kind: Standard, caps: INPUT },
    "PIO1_9" => { package: 17, bank: Pio1, bit: 9, iocon: 0x38, function: 0, kind: Standard, caps: INPUT },
    "PIO1_10" => { package: 30, bank: Pio1, bit: 10, iocon: 0x6c, function: 0, kind: Analog, caps: INPUT },
    "PIO1_11" => { package: 42, bank: Pio1, bit: 11, iocon: 0x98, function: 0, kind: Analog, caps: INPUT },
    "PIO2_0" => { package: 2, bank: Pio2, bit: 0, iocon: 0x08, function: 0, kind: Standard, caps: INPUT },
    "PIO2_1" => { package: 13, bank: Pio2, bit: 1, iocon: 0x28, function: 0, kind: Standard, caps: INPUT },
    "PIO2_2" => { package: 26, bank: Pio2, bit: 2, iocon: 0x5c, function: 0, kind: Standard, caps: INPUT },
    "PIO2_3" => { package: 38, bank: Pio2, bit: 3, iocon: 0x8c, function: 0, kind: Standard, caps: INPUT },
    "PIO2_4" => { package: 19, bank: Pio2, bit: 4, iocon: 0x40, function: 0, kind: Standard, caps: INPUT },
    "PIO2_5" => { package: 20, bank: Pio2, bit: 5, iocon: 0x44, function: 0, kind: Standard, caps: INPUT },
    "PIO2_6" => { package: 1, bank: Pio2, bit: 6, iocon: 0x00, function: 0, kind: Standard, caps: INPUT },
    "PIO2_7" => { package: 11, bank: Pio2, bit: 7, iocon: 0x20, function: 0, kind: Standard, caps: INPUT },
    "PIO2_8" => { package: 12, bank: Pio2, bit: 8, iocon: 0x24, function: 0, kind: Standard, caps: INPUT },
    "PIO2_9" => { package: 24, bank: Pio2, bit: 9, iocon: 0x54, function: 0, kind: Standard, caps: INPUT },
    "PIO2_10" => { package: 25, bank: Pio2, bit: 10, iocon: 0x58, function: 0, kind: Standard, caps: INPUT },
    "PIO2_11" => { package: 31, bank: Pio2, bit: 11, iocon: 0x70, function: 0, kind: Standard, caps: INPUT },
    "PIO3_0" => { package: 36, bank: Pio3, bit: 0, iocon: 0x84, function: 0, kind: Standard, caps: INPUT },
    "PIO3_1" => { package: 37, bank: Pio3, bit: 1, iocon: 0x88, function: 0, kind: Standard, caps: INPUT },
    "PIO3_2" => { package: 43, bank: Pio3, bit: 2, iocon: 0x9c, function: 0, kind: Standard, caps: INPUT },
    "PIO3_3" => { package: 48, bank: Pio3, bit: 3, iocon: 0xac, function: 0, kind: Standard, caps: INPUT },
    "PIO3_4" => { package: 18, bank: Pio3, bit: 4, iocon: 0x3c, function: 0, kind: Standard, caps: INPUT },
    "PIO3_5" => { package: 21, bank: Pio3, bit: 5, iocon: 0x48, function: 0, kind: Standard, caps: INPUT },
}

pub static LPC_PIN_MAP: PinMap = PinMap::new(&BANKS, PINS);

pub fn pin_hw(pin: PinId) -> &'static LpcPinHw {
    &LPC_HW[pin.index()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Target;

    #[test]
    fn map_covers_the_48_pin_package_conservatively() {
        assert_eq!(LPC_PIN_MAP.banks().len(), 4);
        assert_eq!(LPC_PIN_MAP.pins().len(), 42);
        assert_eq!(LPC_HW.len(), LPC_PIN_MAP.pins().len());

        let mut package_pins = [false; 49];
        for (index, pin) in LPC_PIN_MAP.pins().iter().enumerate() {
            let token = pin.token.as_bytes();
            assert!(token.starts_with(b"PIO"));
            assert_eq!(token[3] - b'0', pin.bank.index() as u8);
            assert_eq!(pin.token[5..].parse::<u8>().unwrap(), pin.bit);

            let hw = pin_hw(PinId::new(index as u8));
            assert_eq!(hw.bank().id(), pin.bank);
            assert_eq!(hw.bit(), pin.bit);

            let package_pin = pin.package_pin.unwrap() as usize;
            assert!((1..=48).contains(&package_pin));
            assert!(!package_pins[package_pin]);
            package_pins[package_pin] = true;
        }

        for token in [
            b"PIO0_0".as_slice(),
            b"PIO0_10",
            b"PIO1_3",
            b"PIO1_6",
            b"PIO1_7",
        ] {
            let Target::Pin(pin) = LPC_PIN_MAP.resolve(token).unwrap() else {
                panic!("reserved LPC target must resolve to a pin");
            };
            assert!(!LPC_PIN_MAP.pin(pin).capabilities.available());
        }

        for pin in LPC_PIN_MAP
            .pins()
            .iter()
            .filter(|pin| pin.capabilities.available())
        {
            assert!(pin.capabilities.input());
            assert!(!pin.capabilities.output());
            assert!(!pin.capabilities.pull_up());
        }
    }
}
