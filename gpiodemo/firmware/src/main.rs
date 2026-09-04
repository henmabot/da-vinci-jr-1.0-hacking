#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

#[cfg(any(target_arch = "arm", test))]
const RX_BUFFER_CAPACITY: usize = da_vinci_protocol::MAX_PACKET_LEN * 4;

#[cfg(any(target_arch = "arm", test))]
struct RxBuffer {
    bytes: [u8; RX_BUFFER_CAPACITY],
    head: usize,
    len: usize,
}

#[cfg(any(target_arch = "arm", test))]
impl RxBuffer {
    const fn new() -> Self {
        Self {
            bytes: [0; RX_BUFFER_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    fn free(&self) -> usize {
        RX_BUFFER_CAPACITY - self.len
    }

    fn try_extend(&mut self, input: &[u8]) -> bool {
        if input.len() > self.free() {
            return false;
        }

        let tail = (self.head + self.len) % RX_BUFFER_CAPACITY;
        let first = input.len().min(RX_BUFFER_CAPACITY - tail);
        self.bytes[tail..tail + first].copy_from_slice(&input[..first]);
        self.bytes[..input.len() - first].copy_from_slice(&input[first..]);
        self.len += input.len();
        true
    }

    fn pop(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }
        let byte = self.bytes[self.head];
        self.head = (self.head + 1) % RX_BUFFER_CAPACITY;
        self.len -= 1;
        Some(byte)
    }
}

#[cfg(not(target_arch = "arm"))]
fn main() {}

#[cfg(target_arch = "arm")]
mod board {
    use super::RxBuffer;
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
        let mut rx = RxBuffer::new();
        let mut usb_rx = [0u8; MAX_PACKET_LEN];

        loop {
            usb.poll(&mut [&mut serial]);
            tx.flush(&mut serial);

            let read_len = rx.free().min(usb_rx.len());
            if read_len != 0
                && let Ok(count) = serial.read(&mut usb_rx[..read_len])
            {
                debug_assert!(rx.try_extend(&usb_rx[..count]));
            }

            if tx.is_empty()
                && let Some(packet) = firmware.poll_bulk(&gpio)
            {
                tx.queue(packet);
            }

            if tx.is_empty() {
                while let Some(byte) = rx.pop() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use da_vinci_protocol::LineBuffer;

    fn next_line(rx: &mut RxBuffer, reader: &mut LineBuffer) -> Option<Vec<u8>> {
        while let Some(byte) = rx.pop() {
            if let Ok(Some(line)) = reader.push(byte) {
                return Some(line.to_vec());
            }
        }
        None
    }

    #[test]
    fn fragmented_line_survives_multiple_usb_reads() {
        let mut rx = RxBuffer::new();
        let mut reader = LineBuffer::new();

        assert!(rx.try_extend(b"001 HA"));
        assert_eq!(next_line(&mut rx, &mut reader), None);
        assert!(rx.try_extend(b"I\n"));
        assert_eq!(next_line(&mut rx, &mut reader), Some(b"001 HAI".to_vec()));
    }

    #[test]
    fn multiple_lines_from_one_usb_read_stay_fifo() {
        let mut rx = RxBuffer::new();
        let mut reader = LineBuffer::new();

        assert!(rx.try_extend(b"001 HAI\n002 HRU\n"));
        assert_eq!(next_line(&mut rx, &mut reader), Some(b"001 HAI".to_vec()));
        assert_eq!(next_line(&mut rx, &mut reader), Some(b"002 HRU".to_vec()));
    }

    #[test]
    fn unprocessed_request_stays_buffered_while_tx_is_pending() {
        let mut rx = RxBuffer::new();
        let mut reader = LineBuffer::new();

        assert!(rx.try_extend(b"001 HAI\n002 HRU\n"));
        assert_eq!(next_line(&mut rx, &mut reader), Some(b"001 HAI".to_vec()));
        assert!(rx.len != 0);
        assert_eq!(next_line(&mut rx, &mut reader), Some(b"002 HRU".to_vec()));
    }

    #[test]
    fn overflow_rejects_new_bytes_without_corrupting_buffer() {
        let mut rx = RxBuffer::new();
        let fill = [b'x'; RX_BUFFER_CAPACITY];

        assert!(rx.try_extend(&fill));
        assert!(!rx.try_extend(b"extra"));
        assert_eq!(rx.len, RX_BUFFER_CAPACITY);
        for expected in fill {
            assert_eq!(rx.pop(), Some(expected));
        }
        assert_eq!(rx.pop(), None);
        assert!(rx.try_extend(b"ok"));
        assert_eq!(rx.pop(), Some(b'o'));
        assert_eq!(rx.pop(), Some(b'k'));
    }
}
