#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

#[cfg(not(target_arch = "arm"))]
fn main() {}

#[cfg(target_arch = "arm")]
mod board {
    use core::ptr::{read_volatile, write_volatile};

    use cortex_m_rt::entry;
    use da_vinci_firmware::{
        BankId, GpioHal, Node, PinId, PinMode,
        lpc::{LPC_IDENTITY, LPC_PIN_MAP, LpcBank, LpcPadKind, pin_hw},
        transport::{ByteError, NonBlockingBytes},
    };
    use da_vinci_protocol::Level;
    use lpc11xx as pac;
    use panic_halt as _;

    const LOCAL_ROUTE: &[u8] = b"LPC";

    struct LpcGpio {
        _iocon: pac::IOCON,
        gpio0: pac::GPIO0,
        gpio1: pac::GPIO1,
        gpio2: pac::GPIO2,
        gpio3: pac::GPIO3,
    }

    impl LpcGpio {
        fn registers(&self, bank: LpcBank) -> &pac::gpio0::RegisterBlock {
            match bank {
                LpcBank::Pio0 => &self.gpio0,
                LpcBank::Pio1 => &self.gpio1,
                LpcBank::Pio2 => &self.gpio2,
                LpcBank::Pio3 => &self.gpio3,
            }
        }

        fn configure_pad(&self, pin: PinId, pull_up: bool) {
            let hw = pin_hw(pin);
            let offset = hw.iocon_offset() as usize;
            // SAFETY: LpcGpio owns IOCON for the lifetime of this adapter. Each PinId comes from
            // LPC_PIN_MAP and therefore has matching LPC hardware metadata. The PAC exposes IOCON
            // as individually named registers rather than an indexable array, so volatile pointer
            // access is required here. No other code mutates GPIO IOCON entries.
            unsafe {
                let register = (pac::IOCON::ptr() as *mut u8).add(offset).cast::<u32>();
                let current = read_volatile(register);
                let next = match hw.pad_kind() {
                    LpcPadKind::I2cOpenDrain => {
                        // Dedicated I2C pads use the I2C-mode field instead of ordinary MODE bits.
                        (current & !(0x07 | (0x03 << 8))) | (1 << 8)
                    }
                    LpcPadKind::Standard | LpcPadKind::Analog => {
                        let mode = if pull_up { 2 } else { 0 };
                        let mut bits = (current & !(0x07 | (0x03 << 3)))
                            | u32::from(hw.gpio_function())
                            | ((mode as u32) << 3);
                        if hw.pad_kind() == LpcPadKind::Analog {
                            bits |= 1 << 7;
                        }
                        bits
                    }
                };
                write_volatile(register, next);
            }
        }
    }

    impl GpioHal for LpcGpio {
        fn pin_map(&self) -> &'static da_vinci_firmware::PinMap {
            &LPC_PIN_MAP
        }

        fn configure(&mut self, pin: PinId, mode: PinMode) {
            let hw = pin_hw(pin);
            let mask = 1u32 << hw.bit();
            match mode {
                PinMode::Input { pull_up } => {
                    self.configure_pad(pin, pull_up);
                    // The PAC models DIR as a whole-bank bitmap, so a raw masked update is the
                    // only available way to change one GPIO direction without disturbing peers.
                    self.registers(hw.bank())
                        .dir
                        .modify(|r, w| unsafe { w.bits(r.bits() & !mask) });
                }
                PinMode::Output { initial } => {
                    self.configure_pad(pin, false);
                    self.write(pin, initial);
                    self.registers(hw.bank())
                        .dir
                        .modify(|r, w| unsafe { w.bits(r.bits() | mask) });
                }
            }
        }

        fn write(&mut self, pin: PinId, level: Level) {
            let hw = pin_hw(pin);
            let mask = 1u32 << hw.bit();
            // DATA is also exposed as a whole-bank bitmap by this PAC.
            self.registers(hw.bank()).data.modify(|r, w| unsafe {
                w.bits(match level {
                    Level::Low => r.bits() & !mask,
                    Level::High => r.bits() | mask,
                })
            });
        }

        fn read_bank(&self, bank: BankId) -> u32 {
            self.registers(LpcBank::from_id(bank).expect("LPC pin map contains only GPIO0-3"))
                .data
                .read()
                .bits()
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
        uart.lcr.write(|w| w.wls().eight().dlab().enable());
        uart.dll().write(|w| w.dllsb().bits(4));
        uart.dlm().write(|w| w.dlmsb().bits(0));
        uart.fdr.write(|w| w.divaddval().bits(5).mulval().bits(8));
        uart.lcr.write(|w| w.wls().eight().dlab().disable());
        uart.fcr()
            .write(|w| w.fifoen().enable().rxfifores().clear().txfifores().clear());
        uart.ter.write(|w| w.txen().set_bit());
    }
}
