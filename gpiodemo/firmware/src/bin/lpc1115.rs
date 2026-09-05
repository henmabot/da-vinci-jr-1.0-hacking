#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

#[cfg(not(target_arch = "arm"))]
fn main() {}

#[cfg(target_arch = "arm")]
mod board {
    use core::ptr::{read_volatile, write_volatile};

    use cortex_m_rt::entry;
    use da_vinci_firmware::{
        BankId, GpioHal, Node, PinId,
        lpc::{LPC_IDENTITY, LPC_PIN_MAP},
        transport::{ByteError, NonBlockingBytes},
    };
    use da_vinci_protocol::Level;
    use lpc11xx as pac;
    use panic_halt as _;

    const LOCAL_ROUTE: &[u8] = b"LPC";
    const IOCON_OFFSETS: [u8; 42] = [
        0x0c, 0x10, 0x1c, 0x2c, 0x30, 0x34, 0x4c, 0x50, 0x60, 0x64, 0x68, 0x74, 0x78, 0x7c, 0x80,
        0x90, 0x94, 0xa0, 0xa4, 0xa8, 0x14, 0x38, 0x6c, 0x98, 0x08, 0x28, 0x5c, 0x8c, 0x40, 0x44,
        0x00, 0x20, 0x24, 0x54, 0x58, 0x70, 0x84, 0x88, 0x9c, 0xac, 0x3c, 0x48,
    ];

    struct LpcGpio {
        _iocon: pac::IOCON,
        gpio0: pac::GPIO0,
        gpio1: pac::GPIO1,
        gpio2: pac::GPIO2,
        gpio3: pac::GPIO3,
    }

    impl LpcGpio {
        fn registers(&self, bank: BankId) -> &pac::gpio0::RegisterBlock {
            match bank.index() {
                0 => &self.gpio0,
                1 => &self.gpio1,
                2 => &self.gpio2,
                3 => &self.gpio3,
                _ => unreachable!("LPC pin map contains only GPIO0-3"),
            }
        }

        fn configure(&self, pin: PinId, pull_up: bool) {
            let info = LPC_PIN_MAP.pin(pin);
            let function = match (info.bank.index(), info.bit) {
                (0, 11) | (1, 0..=2) => 1,
                _ => 0,
            };
            let offset = IOCON_OFFSETS[pin.index()] as usize;
            // SAFETY: LpcGpio owns IOCON for the lifetime of this adapter. Each PinId comes from
            // LPC_PIN_MAP, whose index is the IOCON_OFFSETS index. Volatile access is required for
            // the memory-mapped IOCON register and no other code mutates GPIO IOCON entries.
            unsafe {
                let register = (pac::IOCON::ptr() as *mut u8).add(offset).cast::<u32>();
                let current = read_volatile(register);
                let next = if info.bank.index() == 0 && matches!(info.bit, 4 | 5) {
                    // PIO0_4/PIO0_5 are dedicated open-drain I2C pads. In GPIO mode they need
                    // Standard I/O mode and have no ordinary pull-up/pull-down MODE field.
                    (current & !(0x07 | (0x03 << 8))) | (1 << 8)
                } else {
                    let mode = if pull_up { 2 } else { 0 };
                    let mut bits =
                        (current & !(0x07 | (0x03 << 3))) | function | ((mode as u32) << 3);
                    if matches!(
                        (info.bank.index(), info.bit),
                        (0, 11) | (1, 0..=4) | (1, 10..=11)
                    ) {
                        bits |= 1 << 7;
                    }
                    bits
                };
                write_volatile(register, next);
            }
        }
    }

    impl GpioHal for LpcGpio {
        fn pin_map(&self) -> &'static da_vinci_firmware::PinMap {
            &LPC_PIN_MAP
        }

        fn input(&mut self, pin: PinId, pull_up: bool) {
            self.configure(pin, pull_up);
            let info = LPC_PIN_MAP.pin(pin);
            let mask = 1u32 << info.bit;
            self.registers(info.bank)
                .dir
                .modify(|r, w| unsafe { w.bits(r.bits() & !mask) });
        }

        fn output(&mut self, pin: PinId, level: Level) {
            self.configure(pin, false);
            self.write(pin, level);
            let info = LPC_PIN_MAP.pin(pin);
            let mask = 1u32 << info.bit;
            self.registers(info.bank)
                .dir
                .modify(|r, w| unsafe { w.bits(r.bits() | mask) });
        }

        fn write(&mut self, pin: PinId, level: Level) {
            let info = LPC_PIN_MAP.pin(pin);
            let mask = 1u32 << info.bit;
            self.registers(info.bank).data.modify(|r, w| unsafe {
                w.bits(match level {
                    Level::Low => r.bits() & !mask,
                    Level::High => r.bits() | mask,
                })
            });
        }

        fn read_bank(&self, bank: BankId) -> u32 {
            self.registers(bank).data.read().bits()
        }
    }

    struct LpcUart(pac::UART);

    impl NonBlockingBytes for LpcUart {
        fn try_read(&mut self, out: &mut [u8]) -> Result<usize, ByteError> {
            let mut read = 0;
            while read < out.len() && self.0.lsr.read().rdr().bit_is_set() {
                out[read] = self.0.rbr().read().rbr().bits();
                read += 1;
            }
            if read == 0 {
                Err(ByteError::WouldBlock)
            } else {
                Ok(read)
            }
        }

        fn try_write(&mut self, bytes: &[u8]) -> Result<usize, ByteError> {
            if bytes.is_empty() || !self.0.lsr.read().thre().bit_is_set() {
                return Err(ByteError::WouldBlock);
            }
            self.0.thr().write(|w| w.thr().bits(bytes[0]));
            Ok(1)
        }
    }

    #[entry]
    fn main() -> ! {
        let peripherals = pac::Peripherals::take().unwrap();
        configure_clocks_and_uart(&peripherals.SYSCON, &peripherals.IOCON, &peripherals.UART);

        let mut gpio = LpcGpio {
            _iocon: peripherals.IOCON,
            gpio0: peripherals.GPIO0,
            gpio1: peripherals.GPIO1,
            gpio2: peripherals.GPIO2,
            gpio3: peripherals.GPIO3,
        };
        let mut uart = LpcUart(peripherals.UART);
        let mut node = Node::new(LPC_IDENTITY, LOCAL_ROUTE, []);

        loop {
            let _ = node.poll(&mut uart, &mut gpio);
        }
    }

    fn configure_clocks_and_uart(syscon: &pac::SYSCON, iocon: &pac::IOCON, uart: &pac::UART) {
        syscon
            .sysahbclkctrl
            .modify(|_, w| w.gpio().set_bit().uart().set_bit().iocon().set_bit());
        // SAFETY: UARTCLKDIV accepts any non-zero divider value in this field; 1 selects the
        // undivided main clock for the UART peripheral.
        syscon.uartclkdiv.write(|w| unsafe { w.div().bits(1) });

        iocon
            .iocon_pio1_6
            .modify(|_, w| w.func().rxd().mode().inactive_no_pull_do());
        iocon
            .iocon_pio1_7
            .modify(|_, w| w.func().txd().mode().inactive_no_pull_do());
        iocon.iocon_rxd_loc.write(|w| w.rxdloc().pio1_6());

        // 12 MHz IRC / 16 / 4 / (1 + 5/8) = 115384.6 baud (0.16% high).
        uart.lcr.write(|w| unsafe { w.bits(0x83) });
        uart.dll().write(|w| w.dllsb().bits(4));
        uart.dlm().write(|w| w.dlmsb().bits(0));
        uart.fdr.write(|w| w.divaddval().bits(5).mulval().bits(8));
        uart.lcr.write(|w| unsafe { w.bits(0x03) });
        uart.fcr().write(|w| unsafe { w.bits(0x07) });
        uart.ter.write(|w| w.txen().set_bit());
    }
}
