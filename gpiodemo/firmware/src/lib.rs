#![no_std]

use da_vinci_protocol::{
    Direction, Level, Packet, Pin, PinError, PinTable, PinTarget, Port, Query, QueryValue, Request,
    Response, ResponseError, WIRE_PIN_COUNT,
};

pub trait Gpio {
    fn input(&mut self, pin: Pin, pullup: bool);
    fn output(&mut self, pin: Pin, level: Level);
    fn write(&mut self, pin: Pin, level: Level);
    fn read_port(&self, port: Port) -> u32;

    fn read(&self, pin: Pin) -> Level {
        if self.read_port(pin.port()) & (1u32 << pin.bit()) == 0 {
            Level::Low
        } else {
            Level::High
        }
    }
}

#[derive(Clone, Copy)]
enum PinState {
    Unset,
    Input {
        pullup: bool,
        listener: Option<u16>,
        previous: Level,
    },
    Output,
}

#[derive(Clone, Copy)]
enum BulkResponse {
    Values {
        id: u16,
        target: PinTarget,
        next: u8,
    },
    States {
        id: u16,
        target: PinTarget,
        next: u8,
        what: Query,
    },
}

pub struct Firmware {
    pins: PinTable<PinState>,
    bulk: Option<BulkResponse>,
    listener_cursor: u8,
}

impl Default for Firmware {
    fn default() -> Self {
        Self::new()
    }
}

impl Firmware {
    pub const fn new() -> Self {
        Self {
            pins: PinTable::filled(PinState::Unset),
            bulk: None,
            listener_cursor: 0,
        }
    }

    pub fn handle<G: Gpio>(&mut self, packet: Packet<Request>, gpio: &mut G) -> Packet<Response> {
        let body = match packet.body {
            Request::Hello => Response::Hello,
            Request::Status => Response::Status,
            Request::Direction { target, direction } => self.set_direction(target, direction, gpio),
            Request::Get {
                target: PinTarget::Pin(pin),
            } => match self.initialized(pin) {
                Ok(()) => Response::Value {
                    pin,
                    level: gpio.read(pin),
                },
                Err(error) => error,
            },
            Request::Get { target } => {
                self.bulk = Some(BulkResponse::Values {
                    id: packet.id,
                    target,
                    next: 0,
                });
                return self
                    .poll_bulk(gpio)
                    .expect("new grouped GET always yields a response");
            }
            Request::Set { target, level } => self.set_level(target, level, gpio),
            Request::Pullup { target, enabled } => self.set_pullup(target, enabled, gpio),
            Request::Listen { target, enabled } => {
                self.set_listening(target, enabled, packet.id, gpio)
            }
            Request::Query {
                target: PinTarget::Pin(pin),
                what,
            } => match supported(pin) {
                Ok(()) => Response::State {
                    pin,
                    what,
                    value: self.query(pin, what),
                },
                Err(error) => error,
            },
            Request::Query { target, what } => {
                self.bulk = Some(BulkResponse::States {
                    id: packet.id,
                    target,
                    next: 0,
                    what,
                });
                return self
                    .poll_bulk(gpio)
                    .expect("new grouped WYD always yields a response");
            }
            Request::Bye => {
                self.reset(gpio);
                Response::Bye
            }
        };

        Packet {
            id: packet.id,
            body,
        }
    }

    pub fn poll_bulk<G: Gpio>(&mut self, gpio: &G) -> Option<Packet<Response>> {
        match self.bulk? {
            BulkResponse::Values {
                id,
                target,
                mut next,
            } => {
                while next < WIRE_PIN_COUNT {
                    let pin = Pin::from_wire_index(next).expect("wire pin index is in range");
                    next += 1;
                    if !target.contains(pin) {
                        continue;
                    }
                    if supported(pin).is_err() {
                        continue;
                    }
                    if matches!(self.pins[pin], PinState::Unset) {
                        continue;
                    }
                    self.bulk = Some(BulkResponse::Values { id, target, next });
                    return Some(Packet {
                        id,
                        body: Response::Value {
                            pin,
                            level: gpio.read(pin),
                        },
                    });
                }
                self.bulk = None;
                Some(Packet {
                    id,
                    body: Response::Ack,
                })
            }
            BulkResponse::States {
                id,
                target,
                mut next,
                what,
            } => {
                while next < WIRE_PIN_COUNT {
                    let pin = Pin::from_wire_index(next).expect("wire pin index is in range");
                    next += 1;
                    if !target.contains(pin) {
                        continue;
                    }
                    if supported(pin).is_err() {
                        continue;
                    }
                    self.bulk = Some(BulkResponse::States {
                        id,
                        target,
                        next,
                        what,
                    });
                    return Some(Packet {
                        id,
                        body: Response::State {
                            pin,
                            what,
                            value: self.query(pin, what),
                        },
                    });
                }
                self.bulk = None;
                Some(Packet {
                    id,
                    body: Response::Ack,
                })
            }
        }
    }

    pub fn poll_listener<G: Gpio>(&mut self, gpio: &G) -> Option<Packet<Response>> {
        let mut snapshots = [None; 5];
        for offset in 0..WIRE_PIN_COUNT {
            let index = (self.listener_cursor + offset) % WIRE_PIN_COUNT;
            let pin = Pin::from_wire_index(index).expect("listener index is in range");
            let PinState::Input {
                listener: Some(listener),
                previous,
                ..
            } = &mut self.pins[pin]
            else {
                continue;
            };
            let snapshot =
                *snapshots[port_slot(pin.port())].get_or_insert_with(|| gpio.read_port(pin.port()));
            let value = if snapshot & (1u32 << pin.bit()) == 0 {
                Level::Low
            } else {
                Level::High
            };
            if value == *previous {
                continue;
            }
            *previous = value;
            self.listener_cursor = (index + 1) % WIRE_PIN_COUNT;
            return Some(Packet {
                id: *listener,
                body: Response::Value { pin, level: value },
            });
        }
        None
    }

    fn set_direction<G: Gpio>(
        &mut self,
        target: PinTarget,
        direction: Direction,
        gpio: &mut G,
    ) -> Response {
        if let PinTarget::Pin(pin) = target
            && let Err(error) = supported(pin)
        {
            return error;
        }
        for pin in target.available_pins() {
            self.set_direction_pin(pin, direction, gpio);
        }
        Response::Ack
    }

    fn set_direction_pin<G: Gpio>(&mut self, pin: Pin, direction: Direction, gpio: &mut G) {
        match direction {
            Direction::Input => {
                let listener = match self.pins[pin] {
                    PinState::Input { listener, .. } => listener,
                    PinState::Unset | PinState::Output => None,
                };
                gpio.input(pin, false);
                self.pins[pin] = PinState::Input {
                    pullup: false,
                    listener,
                    previous: gpio.read(pin),
                };
            }
            Direction::Output => {
                gpio.output(pin, Level::Low);
                self.pins[pin] = PinState::Output;
            }
        }
    }

    fn set_level<G: Gpio>(&mut self, target: PinTarget, level: Level, gpio: &mut G) -> Response {
        if let PinTarget::Pin(pin) = target
            && let Err(error) = self.initialized(pin)
        {
            return error;
        }
        for pin in target.available_pins() {
            if matches!(self.pins[pin], PinState::Output) {
                gpio.write(pin, level);
            }
        }
        Response::Ack
    }

    fn set_pullup<G: Gpio>(&mut self, target: PinTarget, enabled: bool, gpio: &mut G) -> Response {
        if let PinTarget::Pin(pin) = target
            && let Err(error) = self.initialized(pin)
        {
            return error;
        }
        for pin in target.available_pins() {
            self.set_pullup_pin(pin, enabled, gpio);
        }
        Response::Ack
    }

    fn set_pullup_pin<G: Gpio>(&mut self, pin: Pin, enabled: bool, gpio: &mut G) {
        let PinState::Input {
            pullup, previous, ..
        } = &mut self.pins[pin]
        else {
            return;
        };
        gpio.input(pin, enabled);
        *pullup = enabled;
        *previous = gpio.read(pin);
    }

    fn set_listening<G: Gpio>(
        &mut self,
        target: PinTarget,
        enabled: bool,
        id: u16,
        gpio: &G,
    ) -> Response {
        if let PinTarget::Pin(pin) = target
            && let Err(error) = self.initialized(pin)
        {
            return error;
        }
        for pin in target.available_pins() {
            self.set_listener_pin(pin, enabled, id, gpio);
        }
        Response::Ack
    }

    fn set_listener_pin<G: Gpio>(&mut self, pin: Pin, enabled: bool, id: u16, gpio: &G) {
        let PinState::Input {
            listener, previous, ..
        } = &mut self.pins[pin]
        else {
            return;
        };
        *listener = enabled.then_some(id);
        if enabled {
            *previous = gpio.read(pin);
        }
    }

    fn initialized(&self, pin: Pin) -> Result<(), Response> {
        supported(pin)?;
        if matches!(self.pins[pin], PinState::Unset) {
            return Err(pin_error(pin, PinError::Unset));
        }
        Ok(())
    }

    fn query(&self, pin: Pin, what: Query) -> QueryValue {
        match (self.pins[pin], what) {
            (PinState::Unset, _) => QueryValue::Unset,
            (PinState::Input { .. }, Query::Direction) => QueryValue::Direction(Direction::Input),
            (PinState::Output, Query::Direction) => QueryValue::Direction(Direction::Output),
            (PinState::Input { pullup, .. }, Query::Pullup) => QueryValue::Enabled(pullup),
            (PinState::Input { listener, .. }, Query::Listen) => {
                QueryValue::Enabled(listener.is_some())
            }
            (PinState::Output, Query::Pullup | Query::Listen) => QueryValue::Enabled(false),
        }
    }

    fn reset<G: Gpio>(&mut self, gpio: &mut G) {
        self.bulk = None;
        self.listener_cursor = 0;
        for pin in Pin::all() {
            let state = &mut self.pins[pin];
            if !matches!(state, PinState::Unset) && supported(pin).is_ok() {
                gpio.input(pin, false);
            }
            *state = PinState::Unset;
        }
    }
}

const fn port_slot(port: Port) -> usize {
    match port {
        Port::A => 0,
        Port::B => 1,
        Port::C => 2,
        Port::D => 3,
        Port::E => 4,
    }
}

fn supported(pin: Pin) -> Result<(), Response> {
    pin.is_available()
        .then_some(())
        .ok_or_else(|| pin_error(pin, PinError::Unavailable))
}

fn pin_error(pin: Pin, reason: PinError) -> Response {
    Response::Error(ResponseError::Pin { pin, reason })
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::cell::Cell;

    use super::*;

    struct FakeGpio {
        values: [Level; WIRE_PIN_COUNT as usize],
        inputs: [bool; WIRE_PIN_COUNT as usize],
        pullups: [bool; WIRE_PIN_COUNT as usize],
        outputs: [bool; WIRE_PIN_COUNT as usize],
        port_reads: Cell<[u16; 5]>,
    }

    impl Default for FakeGpio {
        fn default() -> Self {
            Self {
                values: [Level::Low; WIRE_PIN_COUNT as usize],
                inputs: [false; WIRE_PIN_COUNT as usize],
                pullups: [false; WIRE_PIN_COUNT as usize],
                outputs: [false; WIRE_PIN_COUNT as usize],
                port_reads: Cell::new([0; 5]),
            }
        }
    }

    impl Gpio for FakeGpio {
        fn input(&mut self, pin: Pin, pullup: bool) {
            let i = pin.index() as usize;
            self.inputs[i] = true;
            self.outputs[i] = false;
            self.pullups[i] = pullup;
        }

        fn output(&mut self, pin: Pin, level: Level) {
            let i = pin.index() as usize;
            self.inputs[i] = false;
            self.outputs[i] = true;
            self.pullups[i] = false;
            self.values[i] = level;
        }

        fn write(&mut self, pin: Pin, level: Level) {
            self.values[pin.index() as usize] = level;
        }

        fn read_port(&self, port: Port) -> u32 {
            let mut reads = self.port_reads.get();
            reads[port_slot(port)] += 1;
            self.port_reads.set(reads);

            port.pins().fold(0, |bits, pin| {
                if self.values[pin.index() as usize] == Level::High {
                    bits | (1u32 << pin.bit())
                } else {
                    bits
                }
            })
        }
    }

    impl FakeGpio {
        fn reset_port_reads(&self) {
            self.port_reads.set([0; 5]);
        }
    }

    fn pin(index: u8) -> Pin {
        Pin::from_wire_index(index).unwrap()
    }

    fn packet(id: u16, body: Request) -> Packet<Request> {
        Packet { id, body }
    }

    #[test]
    fn direction_initializes_and_pullup_resets() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        assert_eq!(
            firmware
                .handle(
                    packet(
                        1,
                        Request::Get {
                            target: PinTarget::Pin(pin(0))
                        }
                    ),
                    &mut gpio
                )
                .body,
            pin_error(pin(0), PinError::Unset)
        );

        firmware.handle(
            packet(
                2,
                Request::Direction {
                    target: PinTarget::Pin(pin(0)),
                    direction: Direction::Input,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            packet(
                3,
                Request::Pullup {
                    target: PinTarget::Pin(pin(0)),
                    enabled: true,
                },
            ),
            &mut gpio,
        );
        assert!(gpio.pullups[0]);

        firmware.handle(
            packet(
                4,
                Request::Direction {
                    target: PinTarget::Pin(pin(0)),
                    direction: Direction::Output,
                },
            ),
            &mut gpio,
        );
        assert_eq!(
            firmware
                .handle(
                    packet(
                        5,
                        Request::Query {
                            target: PinTarget::Pin(pin(0)),
                            what: Query::Pullup,
                        },
                    ),
                    &mut gpio,
                )
                .body,
            Response::State {
                pin: pin(0),
                what: Query::Pullup,
                value: QueryValue::Enabled(false),
            }
        );
    }

    #[test]
    fn output_rejects_input_only_operations() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        let input = pin(0);
        let output = pin(1);

        firmware.handle(
            packet(
                1,
                Request::Direction {
                    target: PinTarget::Pin(input),
                    direction: Direction::Input,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            packet(
                2,
                Request::Set {
                    target: PinTarget::Pin(input),
                    level: Level::High,
                },
            ),
            &mut gpio,
        );
        assert_eq!(gpio.values[input.index() as usize], Level::Low);

        firmware.handle(
            packet(
                3,
                Request::Direction {
                    target: PinTarget::Pin(output),
                    direction: Direction::Output,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            packet(
                4,
                Request::Pullup {
                    target: PinTarget::Pin(output),
                    enabled: true,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            packet(
                5,
                Request::Listen {
                    target: PinTarget::Pin(output),
                    enabled: true,
                },
            ),
            &mut gpio,
        );

        assert!(!gpio.pullups[output.index() as usize]);
        assert!(matches!(firmware.pins[output], PinState::Output));
        assert_eq!(
            firmware.query(output, Query::Pullup),
            QueryValue::Enabled(false)
        );
        assert_eq!(
            firmware.query(output, Query::Listen),
            QueryValue::Enabled(false)
        );
        gpio.values[output.index() as usize] = Level::High;
        assert_eq!(firmware.poll_listener(&gpio), None);
    }

    #[test]
    fn listener_uses_original_request_id_until_disabled() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        firmware.handle(
            packet(
                1,
                Request::Direction {
                    target: PinTarget::Pin(pin(5)),
                    direction: Direction::Input,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            packet(
                27,
                Request::Listen {
                    target: PinTarget::Pin(pin(5)),
                    enabled: true,
                },
            ),
            &mut gpio,
        );
        gpio.values[5] = Level::High;
        assert_eq!(
            firmware.poll_listener(&gpio),
            Some(Packet {
                id: 27,
                body: Response::Value {
                    pin: pin(5),
                    level: Level::High,
                },
            })
        );
        firmware.handle(
            packet(
                28,
                Request::Listen {
                    target: PinTarget::Pin(pin(5)),
                    enabled: false,
                },
            ),
            &mut gpio,
        );
        gpio.values[5] = Level::Low;
        assert_eq!(firmware.poll_listener(&gpio), None);
    }

    #[test]
    fn listener_reads_each_bank_once() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        firmware.handle(
            packet(
                1,
                Request::Direction {
                    target: PinTarget::All,
                    direction: Direction::Input,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            packet(
                2,
                Request::Listen {
                    target: PinTarget::All,
                    enabled: true,
                },
            ),
            &mut gpio,
        );

        gpio.reset_port_reads();
        assert_eq!(firmware.poll_listener(&gpio), None);
        assert_eq!(gpio.port_reads.get(), [1, 1, 1, 1, 1]);
    }

    #[test]
    fn listener_delivery_is_round_robin() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        let first = pin(0);
        let second = pin(1);
        for (id, target) in [(1, first), (2, second)] {
            firmware.handle(
                packet(
                    id,
                    Request::Direction {
                        target: PinTarget::Pin(target),
                        direction: Direction::Input,
                    },
                ),
                &mut gpio,
            );
            firmware.handle(
                packet(
                    id + 10,
                    Request::Listen {
                        target: PinTarget::Pin(target),
                        enabled: true,
                    },
                ),
                &mut gpio,
            );
        }

        gpio.values[first.index() as usize] = Level::High;
        gpio.values[second.index() as usize] = Level::High;
        assert_eq!(
            firmware.poll_listener(&gpio),
            Some(Packet {
                id: 11,
                body: Response::Value {
                    pin: first,
                    level: Level::High,
                },
            })
        );

        gpio.values[first.index() as usize] = Level::Low;
        assert_eq!(
            firmware.poll_listener(&gpio),
            Some(Packet {
                id: 12,
                body: Response::Value {
                    pin: second,
                    level: Level::High,
                },
            })
        );
    }

    #[test]
    fn unavailable_pin_is_never_touched() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        assert_eq!(
            firmware
                .handle(
                    packet(
                        1,
                        Request::Direction {
                            target: PinTarget::Pin(pin(40)),
                            direction: Direction::Input,
                        },
                    ),
                    &mut gpio,
                )
                .body,
            pin_error(pin(40), PinError::Unavailable)
        );
        assert!(!gpio.inputs[40]);
    }

    #[test]
    fn all_mutations_skip_unavailable_pins() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        firmware.handle(
            packet(
                10,
                Request::Direction {
                    target: PinTarget::All,
                    direction: Direction::Input,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            packet(
                11,
                Request::Pullup {
                    target: PinTarget::All,
                    enabled: true,
                },
            ),
            &mut gpio,
        );
        for i in 0..WIRE_PIN_COUNT as usize {
            if matches!(i, 40..=43) {
                assert!(!gpio.inputs[i]);
                assert!(!gpio.pullups[i]);
            } else {
                assert!(gpio.inputs[i]);
                assert!(gpio.pullups[i]);
            }
        }

        firmware.handle(
            packet(
                12,
                Request::Direction {
                    target: PinTarget::All,
                    direction: Direction::Output,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            packet(
                13,
                Request::Set {
                    target: PinTarget::All,
                    level: Level::High,
                },
            ),
            &mut gpio,
        );
        for i in 0..WIRE_PIN_COUNT as usize {
            if !matches!(i, 40..=43) {
                assert_eq!(gpio.values[i], Level::High);
            }
        }
    }

    #[test]
    fn bank_mutations_stay_inside_selected_port() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        firmware.handle(
            packet(
                20,
                Request::Direction {
                    target: PinTarget::Bank(Port::C),
                    direction: Direction::Output,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            packet(
                21,
                Request::Set {
                    target: PinTarget::Bank(Port::C),
                    level: Level::High,
                },
            ),
            &mut gpio,
        );

        for i in 0..WIRE_PIN_COUNT as usize {
            let in_c = (47..=78).contains(&i);
            assert_eq!(gpio.outputs[i], in_c);
            assert_eq!(gpio.values[i], if in_c { Level::High } else { Level::Low });
        }
    }

    #[test]
    fn bank_listener_stays_inside_selected_port() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        firmware.handle(
            packet(
                24,
                Request::Direction {
                    target: PinTarget::All,
                    direction: Direction::Input,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            packet(
                25,
                Request::Listen {
                    target: PinTarget::Bank(Port::C),
                    enabled: true,
                },
            ),
            &mut gpio,
        );

        gpio.values[0] = Level::High;
        gpio.values[47] = Level::High;
        assert_eq!(
            firmware.poll_listener(&gpio),
            Some(Packet {
                id: 25,
                body: Response::Value {
                    pin: pin(47),
                    level: Level::High,
                },
            })
        );
    }

    #[test]
    fn get_bank_streams_only_initialized_pins_in_bank() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        for index in [0, 47, 72] {
            firmware.handle(
                packet(
                    index as u16 + 1,
                    Request::Direction {
                        target: PinTarget::Pin(pin(index)),
                        direction: Direction::Input,
                    },
                ),
                &mut gpio,
            );
        }
        gpio.values[47] = Level::High;

        assert_eq!(
            firmware.handle(
                packet(
                    30,
                    Request::Get {
                        target: PinTarget::Bank(Port::C),
                    },
                ),
                &mut gpio,
            ),
            Packet {
                id: 30,
                body: Response::Value {
                    pin: pin(47),
                    level: Level::High,
                },
            }
        );
        assert_eq!(
            firmware.poll_bulk(&gpio),
            Some(Packet {
                id: 30,
                body: Response::Value {
                    pin: pin(72),
                    level: Level::Low,
                },
            })
        );
        assert_eq!(
            firmware.poll_bulk(&gpio),
            Some(Packet {
                id: 30,
                body: Response::Ack,
            })
        );
    }

    #[test]
    fn query_bank_streams_every_available_pin_in_bank() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        let first = firmware.handle(
            packet(
                40,
                Request::Query {
                    target: PinTarget::Bank(Port::B),
                    what: Query::Direction,
                },
            ),
            &mut gpio,
        );
        assert_eq!(
            first.body,
            Response::State {
                pin: pin(32),
                what: Query::Direction,
                value: QueryValue::Unset,
            }
        );

        let mut seen = 1;
        loop {
            match firmware.poll_bulk(&gpio).unwrap().body {
                Response::State { pin, .. } => {
                    assert_eq!(pin.port(), Port::B);
                    assert!(pin.is_available());
                    seen += 1;
                }
                Response::Ack => break,
                other => panic!("unexpected grouped response: {other:?}"),
            }
        }
        assert_eq!(seen, Port::B.pin_count() as usize - 4);
    }

    #[test]
    fn bye_releases_initialized_pins_and_clears_state() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        firmware.handle(
            packet(
                1,
                Request::Direction {
                    target: PinTarget::Pin(pin(5)),
                    direction: Direction::Output,
                },
            ),
            &mut gpio,
        );
        assert!(gpio.outputs[5]);
        assert_eq!(
            firmware.handle(packet(2, Request::Bye), &mut gpio).body,
            Response::Bye
        );
        assert!(gpio.inputs[5]);
        assert_eq!(
            firmware
                .handle(
                    packet(
                        3,
                        Request::Get {
                            target: PinTarget::Pin(pin(5))
                        }
                    ),
                    &mut gpio
                )
                .body,
            pin_error(pin(5), PinError::Unset)
        );
    }
}
