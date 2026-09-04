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
    use da_vinci_firmware::{Firmware, Gpio};
    use da_vinci_protocol::{
        DecodeErrorKind, Level, LineBuffer, MAX_PACKET_LEN, Packet, Pin, Port, Response,
        ResponseError, decode_request, encode_response,
    };
    use panic_halt as _;
    use usb_device::{class_prelude::UsbBusAllocator, prelude::*};
    use usbd_serial::{SerialPort, USB_CLASS_CDC};

    struct SamGpio;

    macro_rules! with_port {
        ($pin:expr, |$port:ident, $mask:ident| $body:block) => {{
            let $mask = 1u32 << $pin.bit();
            // SAFETY: Firmware is the only SamGpio caller and only passes validated protocol pins.
            // Reserved clock/USB pins never reach this adapter,
            // and the firmware loop is single-threaded, so these MMIO registers are not aliased.
            unsafe {
                match $pin.port() {
                    Port::A => {
                        let $port = &*pac::PIOA::ptr();
                        $body
                    }
                    Port::B => {
                        let $port = &*pac::PIOB::ptr();
                        $body
                    }
                    Port::C => {
                        let $port = &*pac::PIOC::ptr();
                        $body
                    }
                    Port::D => {
                        let $port = &*pac::PIOD::ptr();
                        $body
                    }
                    Port::E => {
                        let $port = &*pac::PIOE::ptr();
                        $body
                    }
                }
            }
        }};
    }

    impl Gpio for SamGpio {
        fn input(&mut self, pin: Pin, pullup: bool) {
            with_port!(pin, |port, mask| {
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

        fn output(&mut self, pin: Pin, level: Level) {
            with_port!(pin, |port, mask| {
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

        fn write(&mut self, pin: Pin, level: Level) {
            with_port!(pin, |port, mask| {
                if level == Level::High {
                    port.sodr.write_with_zero(|w| w.bits(mask));
                } else {
                    port.codr.write_with_zero(|w| w.bits(mask));
                }
            });
        }

        fn read(&self, pin: Pin) -> Level {
            with_port!(pin, |port, mask| {
                if port.pdsr.read().bits() & mask == 0 {
                    Level::Low
                } else {
                    Level::High
                }
            })
        }
    }

    struct PendingTx {
        bytes: [u8; MAX_PACKET_LEN],
        len: usize,
        offset: usize,
    }

    impl PendingTx {
        const fn new() -> Self {
            Self {
                bytes: [0; MAX_PACKET_LEN],
                len: 0,
                offset: 0,
            }
        }

        fn is_empty(&self) -> bool {
            self.offset == self.len
        }

        fn queue(&mut self, packet: Packet<Response>) {
            assert!(
                self.is_empty(),
                "response queued while USB TX is still pending"
            );
            self.len = encode_response(packet, &mut self.bytes)
                .expect("protocol response always fits fixed packet buffer");
            self.offset = 0;
        }

        fn flush<B: usb_device::bus::UsbBus>(&mut self, serial: &mut SerialPort<'_, B>) {
            if self.is_empty() {
                return;
            }
            if let Ok(written) = serial.write(&self.bytes[self.offset..self.len]) {
                self.offset += written;
                if self.offset == self.len {
                    self.offset = 0;
                    self.len = 0;
                }
            }
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

        let mut firmware = Firmware::new();
        let mut gpio = SamGpio;
        let mut reader = LineBuffer::new();
        let mut tx = PendingTx::new();
        let mut rx = [0u8; 1];

        loop {
            usb.poll(&mut [&mut serial]);
            tx.flush(&mut serial);

            if tx.is_empty()
                && let Some(packet) = firmware.poll_bulk(&gpio)
            {
                tx.queue(packet);
            }

            if tx.is_empty()
                && let Ok(count) = serial.read(&mut rx)
            {
                for &byte in &rx[..count] {
                    if let Ok(Some(line)) = reader.push(byte) {
                        match decode_request(line) {
                            Ok(packet) => tx.queue(firmware.handle(packet, &mut gpio)),
                            Err(error) => {
                                if let Some(id) = error.id {
                                    let body = match error.kind {
                                        DecodeErrorKind::Malformed => {
                                            Response::Error(ResponseError::BadPacket)
                                        }
                                        DecodeErrorKind::UnknownCommand => Response::Unknown,
                                    };
                                    tx.queue(Packet { id, body });
                                }
                            }
                        }
                        if !tx.is_empty() {
                            break;
                        }
                    }
                }
            }

            if tx.is_empty()
                && let Some(packet) = firmware.poll_listener(&gpio)
            {
                tx.queue(packet);
            }
            tx.flush(&mut serial);
        }
    }
}
