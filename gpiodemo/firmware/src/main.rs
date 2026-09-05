#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

#[cfg(not(target_arch = "arm"))]
fn main() {}

#[cfg(target_arch = "arm")]
mod board {
    use atsam4_hal::{
        clock::{ClockController, MainClock, SlowClock},
        gpio::{GpioExt, Ports},
        pac,
        udp::{UdpBus, usb_device},
        watchdog::{Watchdog, WatchdogDisable},
    };
    use cortex_m_rt::entry;
    use da_vinci_firmware::{
        BankId, GpioHal, Node, PinId,
        sam::{SAM_IDENTITY, SAM_PIN_MAP},
        transport::{ByteError, NonBlockingBytes},
    };
    use da_vinci_protocol::Level;
    use panic_halt as _;
    use usb_device::{class_prelude::UsbBusAllocator, prelude::*};
    use usbd_serial::{SerialPort, USB_CLASS_CDC};

    const LOCAL_ROUTE: &[u8] = b"SAM";

    struct SamGpio;

    macro_rules! with_pin {
        ($pin:expr, |$port:ident, $mask:ident| $body:block) => {{
            let info = SAM_PIN_MAP.pin($pin);
            let $mask = 1u32 << info.bit;
            // SAFETY: Firmware is the only SamGpio caller and only passes IDs from SAM_PIN_MAP.
            // Reserved clock/USB pins never reach this adapter,
            // and the firmware loop is single-threaded, so these MMIO registers are not aliased.
            unsafe {
                match info.bank.index() {
                    0 => {
                        let $port = &*pac::PIOA::ptr();
                        $body
                    }
                    1 => {
                        let $port = &*pac::PIOB::ptr();
                        $body
                    }
                    2 => {
                        let $port = &*pac::PIOC::ptr();
                        $body
                    }
                    3 => {
                        let $port = &*pac::PIOD::ptr();
                        $body
                    }
                    4 => {
                        let $port = &*pac::PIOE::ptr();
                        $body
                    }
                    _ => unreachable!("SAM pin map contains only PIOA-E"),
                }
            }
        }};
    }

    impl GpioHal for SamGpio {
        fn pin_map(&self) -> &'static da_vinci_firmware::PinMap {
            &SAM_PIN_MAP
        }

        fn input(&mut self, pin: PinId, pullup: bool) {
            with_pin!(pin, |port, mask| {
                if pullup {
                    port.ppddr.write_with_zero(|w| w.bits(mask));
                    port.puer.write_with_zero(|w| w.bits(mask));
                } else {
                    port.pudr.write_with_zero(|w| w.bits(mask));
                    port.ppddr.write_with_zero(|w| w.bits(mask));
                }
                port.odr.write_with_zero(|w| w.bits(mask));
                port.per.write_with_zero(|w| w.bits(mask));
            });
        }

        fn output(&mut self, pin: PinId, level: Level) {
            with_pin!(pin, |port, mask| {
                port.pudr.write_with_zero(|w| w.bits(mask));
                port.ppddr.write_with_zero(|w| w.bits(mask));
                if level == Level::High {
                    port.sodr.write_with_zero(|w| w.bits(mask));
                } else {
                    port.codr.write_with_zero(|w| w.bits(mask));
                }
                port.oer.write_with_zero(|w| w.bits(mask));
                port.ower.write_with_zero(|w| w.bits(mask));
                port.per.write_with_zero(|w| w.bits(mask));
            });
        }

        fn write(&mut self, pin: PinId, level: Level) {
            with_pin!(pin, |port, mask| {
                if level == Level::High {
                    port.sodr.write_with_zero(|w| w.bits(mask));
                } else {
                    port.codr.write_with_zero(|w| w.bits(mask));
                }
            });
        }

        fn read_bank(&self, bank: BankId) -> u32 {
            // SAFETY: Firmware is the only SamGpio caller, the loop is single-threaded,
            // and reading PDSR does not mutate or alias the PIO registers.
            unsafe {
                match bank.index() {
                    0 => (&*pac::PIOA::ptr()).pdsr.read().bits(),
                    1 => (&*pac::PIOB::ptr()).pdsr.read().bits(),
                    2 => (&*pac::PIOC::ptr()).pdsr.read().bits(),
                    3 => (&*pac::PIOD::ptr()).pdsr.read().bits(),
                    4 => (&*pac::PIOE::ptr()).pdsr.read().bits(),
                    _ => unreachable!("SAM pin map contains only PIOA-E"),
                }
            }
        }
    }

    struct UsbBytes<'a, 'bus, B: usb_device::bus::UsbBus>(&'a mut SerialPort<'bus, B>);

    impl<B: usb_device::bus::UsbBus> NonBlockingBytes for UsbBytes<'_, '_, B> {
        fn try_read(&mut self, out: &mut [u8]) -> Result<usize, ByteError> {
            self.0.read(out).map_err(byte_error)
        }

        fn try_write(&mut self, bytes: &[u8]) -> Result<usize, ByteError> {
            self.0.write(bytes).map_err(byte_error)
        }
    }

    fn byte_error(error: usb_device::UsbError) -> ByteError {
        match error {
            usb_device::UsbError::WouldBlock => ByteError::WouldBlock,
            _ => ByteError::Down,
        }
    }

    #[entry]
    fn main() -> ! {
        let peripherals = pac::Peripherals::take().unwrap();
        let mut watchdog = Watchdog::new(peripherals.WDT);
        watchdog.disable();

        let mut clocks = ClockController::new(
            peripherals.PMC,
            &peripherals.SUPC,
            &peripherals.EFC,
            MainClock::Crystal12Mhz,
            SlowClock::RcOscillator32Khz,
        );
        let pio_a = clocks.peripheral_clocks.pio_a.into_enabled_clock();
        let pio_b = clocks.peripheral_clocks.pio_b.into_enabled_clock();
        let pio_c = clocks.peripheral_clocks.pio_c.into_enabled_clock();
        let pio_d = clocks.peripheral_clocks.pio_d.into_enabled_clock();
        let pio_e = clocks.peripheral_clocks.pio_e.into_enabled_clock();
        let udp_clock = clocks.peripheral_clocks.udp;

        let pins = Ports::new(
            (peripherals.PIOA, pio_a),
            (peripherals.PIOB, pio_b),
            (peripherals.PIOC, pio_c),
            (peripherals.PIOD, pio_d),
            (peripherals.PIOE, pio_e),
        )
        .split();
        let ddm = pins.pb10.into_system_function(&peripherals.MATRIX);
        let ddp = pins.pb11.into_system_function(&peripherals.MATRIX);

        let usb_bus = UsbBusAllocator::new(UdpBus::new(peripherals.UDP, udp_clock, ddm, ddp));
        let mut serial = SerialPort::new(&usb_bus);
        let mut usb = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x1d50, 0x614e))
            .device_class(USB_CLASS_CDC)
            .build();

        let mut gpio = SamGpio;
        let mut node = Node::new(SAM_IDENTITY, LOCAL_ROUTE, []);

        loop {
            usb.poll(&mut [&mut serial]);
            let _ = node.poll(&mut UsbBytes(&mut serial), &mut gpio);
        }
    }
}
