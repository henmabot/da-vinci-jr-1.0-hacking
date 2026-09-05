use crate::gpio::{BankId, BankInfo, Capabilities, PinInfo, PinMap};

const BANK_0: BankId = BankId::new(0);
const BANK_1: BankId = BankId::new(1);
const BANK_2: BankId = BankId::new(2);
const BANK_3: BankId = BankId::new(3);

pub const LPC_IDENTITY: &[u8] = b"LPC1115 GPIO";

static BANKS: [BankInfo; 4] = [
    BankInfo::new("PIO0"),
    BankInfo::new("PIO1"),
    BankInfo::new("PIO2"),
    BankInfo::new("PIO3"),
];

static PINS: [PinInfo; 42] = [
    PinInfo::new("PIO0_0", Some(3), BANK_0, 0, Capabilities::NONE),
    PinInfo::new("PIO0_1", Some(4), BANK_0, 1, Capabilities::INPUT),
    PinInfo::new("PIO0_2", Some(10), BANK_0, 2, Capabilities::INPUT),
    PinInfo::new("PIO0_3", Some(14), BANK_0, 3, Capabilities::INPUT),
    PinInfo::new("PIO0_4", Some(15), BANK_0, 4, Capabilities::INPUT),
    PinInfo::new("PIO0_5", Some(16), BANK_0, 5, Capabilities::INPUT),
    PinInfo::new("PIO0_6", Some(22), BANK_0, 6, Capabilities::INPUT),
    PinInfo::new("PIO0_7", Some(23), BANK_0, 7, Capabilities::INPUT),
    PinInfo::new("PIO0_8", Some(27), BANK_0, 8, Capabilities::INPUT),
    PinInfo::new("PIO0_9", Some(28), BANK_0, 9, Capabilities::INPUT),
    PinInfo::new("PIO0_10", Some(29), BANK_0, 10, Capabilities::NONE),
    PinInfo::new("PIO0_11", Some(32), BANK_0, 11, Capabilities::INPUT),
    PinInfo::new("PIO1_0", Some(33), BANK_1, 0, Capabilities::INPUT),
    PinInfo::new("PIO1_1", Some(34), BANK_1, 1, Capabilities::INPUT),
    PinInfo::new("PIO1_2", Some(35), BANK_1, 2, Capabilities::INPUT),
    PinInfo::new("PIO1_3", Some(39), BANK_1, 3, Capabilities::NONE),
    PinInfo::new("PIO1_4", Some(40), BANK_1, 4, Capabilities::INPUT),
    PinInfo::new("PIO1_5", Some(45), BANK_1, 5, Capabilities::INPUT),
    PinInfo::new("PIO1_6", Some(46), BANK_1, 6, Capabilities::NONE),
    PinInfo::new("PIO1_7", Some(47), BANK_1, 7, Capabilities::NONE),
    PinInfo::new("PIO1_8", Some(9), BANK_1, 8, Capabilities::INPUT),
    PinInfo::new("PIO1_9", Some(17), BANK_1, 9, Capabilities::INPUT),
    PinInfo::new("PIO1_10", Some(30), BANK_1, 10, Capabilities::INPUT),
    PinInfo::new("PIO1_11", Some(42), BANK_1, 11, Capabilities::INPUT),
    PinInfo::new("PIO2_0", Some(2), BANK_2, 0, Capabilities::INPUT),
    PinInfo::new("PIO2_1", Some(13), BANK_2, 1, Capabilities::INPUT),
    PinInfo::new("PIO2_2", Some(26), BANK_2, 2, Capabilities::INPUT),
    PinInfo::new("PIO2_3", Some(38), BANK_2, 3, Capabilities::INPUT),
    PinInfo::new("PIO2_4", Some(19), BANK_2, 4, Capabilities::INPUT),
    PinInfo::new("PIO2_5", Some(20), BANK_2, 5, Capabilities::INPUT),
    PinInfo::new("PIO2_6", Some(1), BANK_2, 6, Capabilities::INPUT),
    PinInfo::new("PIO2_7", Some(11), BANK_2, 7, Capabilities::INPUT),
    PinInfo::new("PIO2_8", Some(12), BANK_2, 8, Capabilities::INPUT),
    PinInfo::new("PIO2_9", Some(24), BANK_2, 9, Capabilities::INPUT),
    PinInfo::new("PIO2_10", Some(25), BANK_2, 10, Capabilities::INPUT),
    PinInfo::new("PIO2_11", Some(31), BANK_2, 11, Capabilities::INPUT),
    PinInfo::new("PIO3_0", Some(36), BANK_3, 0, Capabilities::INPUT),
    PinInfo::new("PIO3_1", Some(37), BANK_3, 1, Capabilities::INPUT),
    PinInfo::new("PIO3_2", Some(43), BANK_3, 2, Capabilities::INPUT),
    PinInfo::new("PIO3_3", Some(48), BANK_3, 3, Capabilities::INPUT),
    PinInfo::new("PIO3_4", Some(18), BANK_3, 4, Capabilities::INPUT),
    PinInfo::new("PIO3_5", Some(21), BANK_3, 5, Capabilities::INPUT),
];

pub static LPC_PIN_MAP: PinMap = PinMap::new(&BANKS, &PINS);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Target;

    #[test]
    fn map_covers_the_48_pin_package_conservatively() {
        assert_eq!(LPC_PIN_MAP.banks().len(), 4);
        assert_eq!(LPC_PIN_MAP.pins().len(), 42);

        let mut package_pins = [false; 49];
        for pin in LPC_PIN_MAP.pins() {
            let token = pin.token.as_bytes();
            assert!(token.starts_with(b"PIO"));
            assert_eq!(token[3] - b'0', pin.bank.index() as u8);
            assert_eq!(pin.token[5..].parse::<u8>().unwrap(), pin.bit);

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
