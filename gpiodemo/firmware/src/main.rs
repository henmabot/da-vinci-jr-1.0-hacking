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
        Firmware, Gpio,
        router::Router,
        transport::{ByteError, FramedTransport, NonBlockingBytes},
    };
    use da_vinci_protocol::{
        DecodeErrorKind, Level, MAX_PACKET_LEN, Packet, Pin, Port, Response, ResponseError,
        decode_request, decode_request_envelope, encode_response,
    };
    use panic_halt as _;
    use usb_device::{class_prelude::UsbBusAllocator, prelude::*};
    use usbd_serial::{SerialPort, USB_CLASS_CDC};

    const LOCAL_ROUTE: &[u8] = b"SAM";

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

        fn read_port(&self, target: Port) -> u32 {
            // SAFETY: Firmware is the only SamGpio caller, the loop is single-threaded,
            // and reading PDSR does not mutate or alias the PIO registers.
            unsafe {
                match target {
                    Port::A => (&*pac::PIOA::ptr()).pdsr.read().bits(),
                    Port::B => (&*pac::PIOB::ptr()).pdsr.read().bits(),
                    Port::C => (&*pac::PIOC::ptr()).pdsr.read().bits(),
                    Port::D => (&*pac::PIOD::ptr()).pdsr.read().bits(),
                    Port::E => (&*pac::PIOE::ptr()).pdsr.read().bits(),
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

        let mut firmware = Firmware::new();
        let mut gpio = SamGpio;
        let mut router = Router::new(LOCAL_ROUTE, []);
        let mut transport = FramedTransport::new();
        let mut frame = [0; MAX_PACKET_LEN];

        loop {
            usb.poll(&mut [&mut serial]);
            let _ = transport.poll(&mut UsbBytes(&mut serial));

            if transport.tx_idle()
                && let Some(packet) = firmware.poll_bulk(&gpio)
            {
                queue_response(&mut transport, router.local_route(), packet);
            }

            if transport.tx_idle()
                && let Ok(Some(len)) = transport.next_frame(&mut frame)
            {
                match decode_request_envelope(&frame[..len]) {
                    Ok(envelope) => {
                        let response = router.dispatch(&frame[..len], envelope, |body| {
                            decode_request(body)
                                .map(|packet| firmware.handle(packet, &mut gpio))
                                .unwrap_or_else(|error| {
                                    decode_error_response(error)
                                        .expect("local command decode errors keep their ID")
                                })
                        });
                        if let Some(response) = response {
                            queue_response(&mut transport, router.local_route(), response);
                        }
                    }
                    Err(error) => {
                        if let Some(response) = decode_error_response(error) {
                            queue_response(&mut transport, router.local_route(), response);
                        }
                    }
                }
            }

            if transport.tx_idle()
                && let Some(packet) = firmware.poll_listener(&gpio)
            {
                queue_response(&mut transport, router.local_route(), packet);
            }
            let _ = transport.poll(&mut UsbBytes(&mut serial));
        }
    }

    fn queue_response<R: AsRef<[u8]>>(
        transport: &mut FramedTransport,
        source: &[u8],
        packet: Packet<Response<R>>,
    ) {
        let mut frame = [0; MAX_PACKET_LEN];
        let len = encode_response(packet, source, &mut frame)
            .expect("protocol response always fits fixed packet buffer");
        transport
            .enqueue(&frame[..len])
            .expect("response queued only while transport TX is idle");
    }

    fn decode_error_response(error: da_vinci_protocol::DecodeError) -> Option<Packet<Response>> {
        let id = error.id?;
        let body = match error.kind {
            DecodeErrorKind::Malformed => Response::Error(ResponseError::BadPacket),
            DecodeErrorKind::UnknownCommand => Response::Unknown,
        };
        Some(Packet { id, body })
    }
}
