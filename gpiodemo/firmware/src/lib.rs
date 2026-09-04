#![no_std]

use da_vinci_protocol::{
    Direction, Level, Packet, PinError, PinTarget, Query, QueryValue, Request, Response,
    ResponseError, WIRE_PIN_COUNT,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    A,
    B,
    C,
    D,
    E,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinId {
    port: Port,
    bit: u8,
}

impl PinId {
    pub const fn port(self) -> Port {
        self.port
    }

    pub const fn bit(self) -> u8 {
        self.bit
    }
}

const fn wire_pin(id: u8) -> Option<PinId> {
    match id {
        0..=31 => Some(PinId {
            port: Port::A,
            bit: id,
        }),
        32..=46 => Some(PinId {
            port: Port::B,
            bit: id - 32,
        }),
        47..=78 => Some(PinId {
            port: Port::C,
            bit: id - 47,
        }),
        79..=110 => Some(PinId {
            port: Port::D,
            bit: id - 79,
        }),
        111..=116 => Some(PinId {
            port: Port::E,
            bit: id - 111,
        }),
        _ => None,
    }
}

pub trait Gpio {
    fn input(&mut self, pin: PinId, pullup: bool);
    fn output(&mut self, pin: PinId, level: Level);
    fn write(&mut self, pin: PinId, level: Level);
    fn read(&self, pin: PinId) -> Level;
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
    Values { id: u16, next: u8 },
    States { id: u16, next: u8, what: Query },
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
                Ok(physical) => Response::Value {
                    pin,
                    level: gpio.read(physical),
                },
                Err(error) => error,
            },
            Request::Get {
                target: PinTarget::All,
            } => {
                self.bulk = Some(BulkResponse::Values {
                    id: packet.id,
                    next: 0,
                });
                return self
                    .poll_bulk(gpio)
                    .expect("new bulk GET always yields a response");
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
                Ok(_) => Response::State {
                    pin,
                    what,
                    value: self.query(pin, what),
                },
                Err(error) => error,
            },
            Request::Query {
                target: PinTarget::All,
                what,
            } => {
                self.bulk = Some(BulkResponse::States {
                    id: packet.id,
                    next: 0,
                    what,
                });
                return self
                    .poll_bulk(gpio)
                    .expect("new bulk WYD always yields a response");
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
            BulkResponse::Values { id, mut next } => {
                while next < WIRE_PIN_COUNT {
                    let pin = next;
                    next += 1;
                    let Ok(physical) = supported(pin) else {
                        continue;
                    };
                    if self.pins[pin as usize].direction.is_none() {
                        continue;
                    }
                    self.bulk = Some(BulkResponse::Values { id, next });
                    return Some(Packet {
                        id,
                        body: Response::Value {
                            pin,
                            level: gpio.read(physical),
                        },
                    });
                }
                self.bulk = None;
                Some(Packet {
                    id,
                    body: Response::Ack,
                })
            }
            BulkResponse::States { id, mut next, what } => {
                while next < WIRE_PIN_COUNT {
                    let pin = next;
                    next += 1;
                    if supported(pin).is_err() {
                        continue;
                    }
                    self.bulk = Some(BulkResponse::States { id, next, what });
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
        for pin in 0..WIRE_PIN_COUNT {
            let state = &mut self.pins[pin as usize];
            let Some(listener) = state.listener else {
                continue;
            };
            let Ok(physical) = supported(pin) else {
                continue;
            };
            let value = gpio.read(physical);
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
                let physical = match supported(pin) {
                    Ok(physical) => physical,
                    Err(error) => return error,
                };
                self.set_direction_pin(pin, physical, direction, gpio);
            }
            PinTarget::All => {
                for pin in 0..WIRE_PIN_COUNT {
                    if let Ok(physical) = supported(pin) {
                        self.set_direction_pin(pin, physical, direction, gpio);
                    }
                }
            }
        }
        Response::Ack
    }

    fn set_direction_pin<G: Gpio>(
        &mut self,
        pin: u8,
        physical: PinId,
        direction: Direction,
        gpio: &mut G,
    ) {
        match direction {
            Direction::Input => gpio.input(physical, false),
            Direction::Output => gpio.output(physical, Level::Low),
        }
        let state = &mut self.pins[pin as usize];
        state.direction = Some(direction);
        state.pullup = false;
        state.previous = gpio.read(physical);
    }

    fn set_level<G: Gpio>(&mut self, target: PinTarget, level: Level, gpio: &mut G) -> Response {
        match target {
            PinTarget::Pin(pin) => match self.initialized(pin) {
                Ok(physical) => gpio.write(physical, level),
                Err(error) => return error,
            },
            PinTarget::All => {
                for pin in 0..WIRE_PIN_COUNT {
                    if self.pins[pin as usize].direction.is_some()
                        && let Ok(physical) = supported(pin)
                    {
                        gpio.write(physical, level);
                    }
                }
            }
        }
        Response::Ack
    }

    fn set_pullup<G: Gpio>(&mut self, target: PinTarget, enabled: bool, gpio: &mut G) -> Response {
        match target {
            PinTarget::Pin(pin) => {
                let physical = match self.initialized(pin) {
                    Ok(physical) => physical,
                    Err(error) => return error,
                };
                self.set_pullup_pin(pin, physical, enabled, gpio);
            }
            PinTarget::All => {
                for pin in 0..WIRE_PIN_COUNT {
                    if self.pins[pin as usize].direction.is_some()
                        && let Ok(physical) = supported(pin)
                    {
                        self.set_pullup_pin(pin, physical, enabled, gpio);
                    }
                }
            }
        }
        Response::Ack
    }

    fn set_pullup_pin<G: Gpio>(&mut self, pin: u8, physical: PinId, enabled: bool, gpio: &mut G) {
        let state = &mut self.pins[pin as usize];
        state.pullup = enabled;
        if state.direction == Some(Direction::Input) {
            gpio.input(physical, enabled);
            state.previous = gpio.read(physical);
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
                let physical = match self.initialized(pin) {
                    Ok(physical) => physical,
                    Err(error) => return error,
                };
                self.set_listener_pin(pin, physical, enabled, id, gpio);
            }
            PinTarget::All => {
                for pin in 0..WIRE_PIN_COUNT {
                    if self.pins[pin as usize].direction.is_some()
                        && let Ok(physical) = supported(pin)
                    {
                        self.set_listener_pin(pin, physical, enabled, id, gpio);
                    }
                }
            }
        }
        Response::Ack
    }

    fn set_listener_pin<G: Gpio>(
        &mut self,
        pin: u8,
        physical: PinId,
        enabled: bool,
        id: u16,
        gpio: &G,
    ) {
        let state = &mut self.pins[pin as usize];
        state.listener = enabled.then_some(id);
        if enabled {
            state.previous = gpio.read(physical);
        }
    }

    fn initialized(&self, pin: u8) -> Result<PinId, Response> {
        let physical = supported(pin)?;
        if self.pins[pin as usize].direction.is_none() {
            return Err(pin_error(pin, PinError::Unset));
        }
        Ok(physical)
    }

    fn query(&self, pin: u8, what: Query) -> QueryValue {
        let state = self.pins[pin as usize];
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
        for pin in 0..WIRE_PIN_COUNT {
            let state = &mut self.pins[pin as usize];
            if state.direction.is_some()
                && let Ok(physical) = supported(pin)
            {
                gpio.input(physical, false);
            }
            *state = PinState::UNSET;
        }
    }
}

fn supported(pin: u8) -> Result<PinId, Response> {
    wire_pin(pin)
        .filter(|_| !matches!(pin, 40..=43))
        .ok_or_else(|| pin_error(pin, PinError::Unavailable))
}

fn pin_error(pin: u8, reason: PinError) -> Response {
    Response::Error(ResponseError::Pin { pin, reason })
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

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

    impl FakeGpio {
        fn wire_index(pin: PinId) -> usize {
            match pin.port {
                Port::A => pin.bit as usize,
                Port::B => 32 + pin.bit as usize,
                Port::C => 47 + pin.bit as usize,
                Port::D => 79 + pin.bit as usize,
                Port::E => 111 + pin.bit as usize,
            }
        }
    }

    impl Gpio for FakeGpio {
        fn input(&mut self, pin: PinId, pullup: bool) {
            let i = Self::wire_index(pin);
            self.inputs[i] = true;
            self.outputs[i] = false;
            self.pullups[i] = pullup;
        }

        fn output(&mut self, pin: PinId, level: Level) {
            let i = Self::wire_index(pin);
            self.inputs[i] = false;
            self.outputs[i] = true;
            self.pullups[i] = false;
            self.values[i] = level;
        }

        fn write(&mut self, pin: PinId, level: Level) {
            self.values[Self::wire_index(pin)] = level;
        }

        fn read(&self, pin: PinId) -> Level {
            self.values[Self::wire_index(pin)]
        }
    }

    fn packet(id: u16, body: Request) -> Packet<Request> {
        Packet { id, body }
    }

    #[test]
    fn wire_mapping_and_reserved_pins_match_current_firmware() {
        assert_eq!(
            wire_pin(0),
            Some(PinId {
                port: Port::A,
                bit: 0
            })
        );
        assert_eq!(
            wire_pin(31),
            Some(PinId {
                port: Port::A,
                bit: 31
            })
        );
        assert_eq!(
            wire_pin(32),
            Some(PinId {
                port: Port::B,
                bit: 0
            })
        );
        assert_eq!(
            wire_pin(46),
            Some(PinId {
                port: Port::B,
                bit: 14
            })
        );
        assert_eq!(
            wire_pin(47),
            Some(PinId {
                port: Port::C,
                bit: 0
            })
        );
        assert_eq!(
            wire_pin(79),
            Some(PinId {
                port: Port::D,
                bit: 0
            })
        );
        assert_eq!(
            wire_pin(111),
            Some(PinId {
                port: Port::E,
                bit: 0
            })
        );
        assert_eq!(
            wire_pin(116),
            Some(PinId {
                port: Port::E,
                bit: 5
            })
        );
        assert_eq!(wire_pin(117), None);
        for pin in 40..=43 {
            assert!(supported(pin).is_err());
        }
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
                            target: PinTarget::Pin(0)
                        }
                    ),
                    &mut gpio
                )
                .body,
            pin_error(0, PinError::Unset)
        );
        assert_eq!(
            firmware
                .handle(
                    packet(
                        2,
                        Request::Direction {
                            target: PinTarget::Pin(0),
                            direction: Direction::Input,
                        },
                    ),
                    &mut gpio,
                )
                .body,
            Response::Ack
        );
        firmware.handle(
            packet(
                3,
                Request::Pullup {
                    target: PinTarget::Pin(0),
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
                    target: PinTarget::Pin(0),
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
                            target: PinTarget::Pin(0),
                            what: Query::Pullup,
                        },
                    ),
                    &mut gpio,
                )
                .body,
            Response::State {
                pin: 0,
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
                    target: PinTarget::Pin(5),
                    direction: Direction::Input,
                },
            ),
            &mut gpio,
        );
        assert_eq!(
            firmware
                .handle(
                    packet(
                        27,
                        Request::Listen {
                            target: PinTarget::Pin(5),
                            enabled: true,
                        },
                    ),
                    &mut gpio,
                )
                .body,
            Response::Ack
        );
        gpio.values[5] = Level::High;
        assert_eq!(
            firmware.poll_listener(&gpio),
            Some(Packet {
                id: 27,
                body: Response::Value {
                    pin: 5,
                    level: Level::High,
                },
            })
        );
        firmware.handle(
            packet(
                28,
                Request::Listen {
                    target: PinTarget::Pin(5),
                    enabled: false,
                },
            ),
            &mut gpio,
        );
        gpio.values[5] = Level::Low;
        assert_eq!(firmware.poll_listener(&gpio), None);
    }

    #[test]
    fn bye_releases_initialized_pins_and_clears_state() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        firmware.handle(
            packet(
                1,
                Request::Direction {
                    target: PinTarget::Pin(5),
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
        assert!(!gpio.pullups[5]);
        assert_eq!(
            firmware
                .handle(
                    packet(
                        3,
                        Request::Get {
                            target: PinTarget::Pin(5)
                        }
                    ),
                    &mut gpio
                )
                .body,
            pin_error(5, PinError::Unset)
        );
    }

    #[test]
    fn reserved_pin_is_never_touched() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        assert_eq!(
            firmware
                .handle(
                    packet(
                        1,
                        Request::Direction {
                            target: PinTarget::Pin(40),
                            direction: Direction::Input,
                        },
                    ),
                    &mut gpio,
                )
                .body,
            pin_error(40, PinError::Unavailable)
        );
        assert!(!gpio.inputs[40]);
    }

    #[test]
    fn all_mutations_apply_to_supported_initialized_pins() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();

        assert_eq!(
            firmware
                .handle(
                    packet(
                        10,
                        Request::Direction {
                            target: PinTarget::All,
                            direction: Direction::Input,
                        },
                    ),
                    &mut gpio,
                )
                .body,
            Response::Ack
        );
        assert_eq!(
            firmware
                .handle(
                    packet(
                        11,
                        Request::Pullup {
                            target: PinTarget::All,
                            enabled: true,
                        },
                    ),
                    &mut gpio,
                )
                .body,
            Response::Ack
        );
        for pin in 0..WIRE_PIN_COUNT as usize {
            if matches!(pin, 40..=43) {
                assert!(!gpio.inputs[pin]);
                assert!(!gpio.pullups[pin]);
            } else {
                assert!(gpio.inputs[pin]);
                assert!(gpio.pullups[pin]);
            }
        }

        firmware.handle(
            packet(
                12,
                Request::Listen {
                    target: PinTarget::All,
                    enabled: true,
                },
            ),
            &mut gpio,
        );
        gpio.values[7] = Level::High;
        assert_eq!(
            firmware.poll_listener(&gpio),
            Some(Packet {
                id: 12,
                body: Response::Value {
                    pin: 7,
                    level: Level::High,
                },
            })
        );
        firmware.handle(
            packet(
                13,
                Request::Listen {
                    target: PinTarget::All,
                    enabled: false,
                },
            ),
            &mut gpio,
        );

        firmware.handle(
            packet(
                14,
                Request::Direction {
                    target: PinTarget::All,
                    direction: Direction::Output,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            packet(
                15,
                Request::Set {
                    target: PinTarget::All,
                    level: Level::High,
                },
            ),
            &mut gpio,
        );
        for pin in 0..WIRE_PIN_COUNT as usize {
            if !matches!(pin, 40..=43) {
                assert_eq!(gpio.values[pin], Level::High);
            }
        }
    }

    #[test]
    fn get_all_streams_configured_pins_then_acknowledges() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        for pin in [0, 5] {
            firmware.handle(
                packet(
                    pin as u16 + 1,
                    Request::Direction {
                        target: PinTarget::Pin(pin),
                        direction: Direction::Input,
                    },
                ),
                &mut gpio,
            );
        }
        gpio.values[0] = Level::High;

        assert_eq!(
            firmware.handle(
                packet(
                    30,
                    Request::Get {
                        target: PinTarget::All,
                    },
                ),
                &mut gpio,
            ),
            Packet {
                id: 30,
                body: Response::Value {
                    pin: 0,
                    level: Level::High,
                },
            }
        );
        assert_eq!(
            firmware.poll_bulk(&gpio),
            Some(Packet {
                id: 30,
                body: Response::Value {
                    pin: 5,
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
        assert_eq!(firmware.poll_bulk(&gpio), None);
    }

    #[test]
    fn query_all_streams_every_supported_pin_then_acknowledges() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        let first = firmware.handle(
            packet(
                40,
                Request::Query {
                    target: PinTarget::All,
                    what: Query::Direction,
                },
            ),
            &mut gpio,
        );
        assert_eq!(
            first.body,
            Response::State {
                pin: 0,
                what: Query::Direction,
                value: QueryValue::Unset,
            }
        );

        let mut states = 1;
        loop {
            let packet = firmware.poll_bulk(&gpio).unwrap();
            match packet.body {
                Response::State { pin, .. } => {
                    assert!(!matches!(pin, 40..=43));
                    states += 1;
                }
                Response::Ack => break,
                other => panic!("unexpected bulk response: {other:?}"),
            }
        }
        assert_eq!(states, WIRE_PIN_COUNT as usize - 4);
        assert_eq!(firmware.poll_bulk(&gpio), None);
    }
}
