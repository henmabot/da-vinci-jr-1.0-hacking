#![no_std]

use da_vinci_protocol::{
    Direction, Level, Packet, Pin, PinError, PinTarget, Query, QueryValue, Request, Response,
    ResponseError, WIRE_PIN_COUNT,
};

pub trait Gpio {
    fn input(&mut self, pin: Pin, pullup: bool);
    fn output(&mut self, pin: Pin, level: Level);
    fn write(&mut self, pin: Pin, level: Level);
    fn read(&self, pin: Pin) -> Level;
}

#[derive(Clone, Copy)]
struct PinState {
    direction: Option<Direction>,
    pullup: bool,
    listener: Option<u16>,
    previous: Level,
}

impl PinState {
    const UNSET: Self = Self {
        direction: None,
        pullup: false,
        listener: None,
        previous: Level::Low,
    };
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
    pins: [PinState; WIRE_PIN_COUNT as usize],
    bulk: Option<BulkResponse>,
}

impl Default for Firmware {
    fn default() -> Self {
        Self::new()
    }
}

impl Firmware {
    pub const fn new() -> Self {
        Self {
            pins: [PinState::UNSET; WIRE_PIN_COUNT as usize],
            bulk: None,
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
                    let pin = Pin::try_from(next).expect("wire pin index is in range");
                    next += 1;
                    if !target.contains(pin) {
                        continue;
                    }
                    if supported(pin).is_err() {
                        continue;
                    }
                    if self.pins[pin.index() as usize].direction.is_none() {
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
                    let pin = Pin::try_from(next).expect("wire pin index is in range");
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
        for (index, pin) in Pin::all().enumerate() {
            let state = &mut self.pins[index];
            let Some(listener) = state.listener else {
                continue;
            };
            if supported(pin).is_err() {
                continue;
            }
            let value = gpio.read(pin);
            if value == state.previous {
                continue;
            }
            state.previous = value;
            return Some(Packet {
                id: listener,
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
        match target {
            PinTarget::Pin(pin) => {
                if let Err(error) = supported(pin) {
                    return error;
                }
                self.set_direction_pin(pin, direction, gpio);
            }
            PinTarget::Bank(_) | PinTarget::All => for_each_group_pin(target, |pin| {
                self.set_direction_pin(pin, direction, gpio);
            }),
        }
        Response::Ack
    }

    fn set_direction_pin<G: Gpio>(&mut self, pin: Pin, direction: Direction, gpio: &mut G) {
        match direction {
            Direction::Input => gpio.input(pin, false),
            Direction::Output => gpio.output(pin, Level::Low),
        }
        let state = &mut self.pins[pin.index() as usize];
        state.direction = Some(direction);
        state.pullup = false;
        state.previous = gpio.read(pin);
    }

    fn set_level<G: Gpio>(&mut self, target: PinTarget, level: Level, gpio: &mut G) -> Response {
        match target {
            PinTarget::Pin(pin) => {
                if let Err(error) = self.initialized(pin) {
                    return error;
                }
                gpio.write(pin, level);
            }
            PinTarget::Bank(_) | PinTarget::All => for_each_group_pin(target, |pin| {
                if self.pins[pin.index() as usize].direction == Some(Direction::Output) {
                    gpio.write(pin, level);
                }
            }),
        }
        Response::Ack
    }

    fn set_pullup<G: Gpio>(&mut self, target: PinTarget, enabled: bool, gpio: &mut G) -> Response {
        match target {
            PinTarget::Pin(pin) => {
                if let Err(error) = self.initialized(pin) {
                    return error;
                }
                self.set_pullup_pin(pin, enabled, gpio);
            }
            PinTarget::Bank(_) | PinTarget::All => for_each_group_pin(target, |pin| {
                if self.pins[pin.index() as usize].direction.is_some() {
                    self.set_pullup_pin(pin, enabled, gpio);
                }
            }),
        }
        Response::Ack
    }

    fn set_pullup_pin<G: Gpio>(&mut self, pin: Pin, enabled: bool, gpio: &mut G) {
        let state = &mut self.pins[pin.index() as usize];
        state.pullup = enabled;
        if state.direction == Some(Direction::Input) {
            gpio.input(pin, enabled);
            state.previous = gpio.read(pin);
        }
    }

    fn set_listening<G: Gpio>(
        &mut self,
        target: PinTarget,
        enabled: bool,
        id: u16,
        gpio: &G,
    ) -> Response {
        match target {
            PinTarget::Pin(pin) => {
                if let Err(error) = self.initialized(pin) {
                    return error;
                }
                self.set_listener_pin(pin, enabled, id, gpio);
            }
            PinTarget::Bank(_) | PinTarget::All => for_each_group_pin(target, |pin| {
                if self.pins[pin.index() as usize].direction == Some(Direction::Input) {
                    self.set_listener_pin(pin, enabled, id, gpio);
                }
            }),
        }
        Response::Ack
    }

    fn set_listener_pin<G: Gpio>(&mut self, pin: Pin, enabled: bool, id: u16, gpio: &G) {
        let state = &mut self.pins[pin.index() as usize];
        state.listener = enabled.then_some(id);
        if enabled {
            state.previous = gpio.read(pin);
        }
    }

    fn initialized(&self, pin: Pin) -> Result<(), Response> {
        supported(pin)?;
        if self.pins[pin.index() as usize].direction.is_none() {
            return Err(pin_error(pin, PinError::Unset));
        }
        Ok(())
    }

    fn query(&self, pin: Pin, what: Query) -> QueryValue {
        let state = self.pins[pin.index() as usize];
        let Some(direction) = state.direction else {
            return QueryValue::Unset;
        };
        match what {
            Query::Direction => QueryValue::Direction(direction),
            Query::Pullup => QueryValue::Enabled(state.pullup),
            Query::Listen => QueryValue::Enabled(state.listener.is_some()),
        }
    }

    fn reset<G: Gpio>(&mut self, gpio: &mut G) {
        self.bulk = None;
        for (index, pin) in Pin::all().enumerate() {
            let state = &mut self.pins[index];
            if state.direction.is_some() && supported(pin).is_ok() {
                gpio.input(pin, false);
            }
            *state = PinState::UNSET;
        }
    }
}

fn for_each_group_pin(target: PinTarget, mut f: impl FnMut(Pin)) {
    for pin in Pin::all() {
        if target.contains(pin) && pin.is_available() {
            f(pin);
        }
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

    use super::*;
    use da_vinci_protocol::Port;

    struct FakeGpio {
        values: [Level; WIRE_PIN_COUNT as usize],
        inputs: [bool; WIRE_PIN_COUNT as usize],
        pullups: [bool; WIRE_PIN_COUNT as usize],
        outputs: [bool; WIRE_PIN_COUNT as usize],
    }

    impl Default for FakeGpio {
        fn default() -> Self {
            Self {
                values: [Level::Low; WIRE_PIN_COUNT as usize],
                inputs: [false; WIRE_PIN_COUNT as usize],
                pullups: [false; WIRE_PIN_COUNT as usize],
                outputs: [false; WIRE_PIN_COUNT as usize],
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

        fn read(&self, pin: Pin) -> Level {
            self.values[pin.index() as usize]
        }
    }

    fn pin(index: u8) -> Pin {
        Pin::try_from(index).unwrap()
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
