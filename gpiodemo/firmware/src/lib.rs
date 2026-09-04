#![no_std]

use da_vinci_protocol::{
    Direction, Level, Packet, PinError, Query, QueryValue, Request, Response, ResponseError,
    WIRE_PIN_COUNT,
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
    pub port: Port,
    pub bit: u8,
}

pub const fn wire_pin(id: u8) -> Option<PinId> {
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

pub const fn available(id: u8) -> bool {
    id < WIRE_PIN_COUNT && !matches!(id, 40..=43)
}

pub trait Gpio {
    fn input(&mut self, pin: PinId, pullup: bool);
    fn output(&mut self, pin: PinId, high: bool);
    fn write(&mut self, pin: PinId, high: bool);
    fn read(&self, pin: PinId) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PinDirection {
    Unset,
    Input,
    Output,
}

#[derive(Clone, Copy)]
struct PinState {
    direction: PinDirection,
    pullup: bool,
    listener: Option<u16>,
    previous: bool,
}

impl PinState {
    const UNSET: Self = Self {
        direction: PinDirection::Unset,
        pullup: false,
        listener: None,
        previous: false,
    };
}

pub struct Firmware {
    pins: [PinState; WIRE_PIN_COUNT as usize],
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
        }
    }

    pub fn handle<G: Gpio>(&mut self, packet: Packet<Request>, gpio: &mut G) -> Packet<Response> {
        let body = match packet.body {
            Request::Hello => Response::Hello,
            Request::Status => Response::Status,
            Request::Direction { pin, direction } => {
                if let Err(error) = supported(pin) {
                    error
                } else {
                    let physical = wire_pin(pin).expect("validated wire pin");
                    match direction {
                        Direction::Input => gpio.input(physical, false),
                        Direction::Output => gpio.output(physical, false),
                    }
                    let state = &mut self.pins[pin as usize];
                    state.direction = match direction {
                        Direction::Input => PinDirection::Input,
                        Direction::Output => PinDirection::Output,
                    };
                    state.pullup = false;
                    state.previous = gpio.read(physical);
                    Response::Ack
                }
            }
            Request::Get { pin } => match self.initialized(pin) {
                Ok(physical) => Response::Value {
                    pin,
                    level: Level::from_bool(gpio.read(physical)),
                },
                Err(error) => error,
            },
            Request::Set { pin, level } => match self.initialized(pin) {
                Ok(physical) => {
                    gpio.write(physical, level.is_high());
                    Response::Ack
                }
                Err(error) => error,
            },
            Request::Pullup { pin, enabled } => match self.initialized(pin) {
                Ok(physical) => {
                    let state = &mut self.pins[pin as usize];
                    state.pullup = enabled;
                    if state.direction == PinDirection::Input {
                        gpio.input(physical, enabled);
                        state.previous = gpio.read(physical);
                    }
                    Response::Ack
                }
                Err(error) => error,
            },
            Request::Listen { pin, enabled } => match self.initialized(pin) {
                Ok(physical) => {
                    let state = &mut self.pins[pin as usize];
                    state.listener = enabled.then_some(packet.id);
                    if enabled {
                        state.previous = gpio.read(physical);
                    }
                    Response::Ack
                }
                Err(error) => error,
            },
            Request::Query { pin, what } => match supported(pin) {
                Ok(()) => Response::State {
                    pin,
                    what,
                    value: self.query(pin, what),
                },
                Err(error) => error,
            },
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

    pub fn poll_listener<G: Gpio>(&mut self, gpio: &G) -> Option<Packet<Response>> {
        for pin in 0..WIRE_PIN_COUNT {
            let state = &mut self.pins[pin as usize];
            let Some(listener) = state.listener else {
                continue;
            };
            let physical = wire_pin(pin).expect("wire pin table covers state table");
            let value = gpio.read(physical);
            if value == state.previous {
                continue;
            }
            state.previous = value;
            return Some(Packet {
                id: listener,
                body: Response::Value {
                    pin,
                    level: Level::from_bool(value),
                },
            });
        }
        None
    }

    fn initialized(&self, pin: u8) -> Result<PinId, Response> {
        supported(pin)?;
        if self.pins[pin as usize].direction == PinDirection::Unset {
            return Err(pin_error(pin, PinError::Unset));
        }
        Ok(wire_pin(pin).expect("validated wire pin"))
    }

    fn query(&self, pin: u8, what: Query) -> QueryValue {
        let state = self.pins[pin as usize];
        if state.direction == PinDirection::Unset {
            return QueryValue::Unset;
        }
        match what {
            Query::Direction => QueryValue::Direction(match state.direction {
                PinDirection::Input => Direction::Input,
                PinDirection::Output => Direction::Output,
                PinDirection::Unset => unreachable!(),
            }),
            Query::Pullup => QueryValue::Enabled(state.pullup),
            Query::Listen => QueryValue::Enabled(state.listener.is_some()),
        }
    }

    fn reset<G: Gpio>(&mut self, gpio: &mut G) {
        for pin in 0..WIRE_PIN_COUNT {
            let state = &mut self.pins[pin as usize];
            if available(pin) && state.direction != PinDirection::Unset {
                gpio.input(
                    wire_pin(pin).expect("wire pin table covers state table"),
                    false,
                );
            }
            *state = PinState::UNSET;
        }
    }
}

fn supported(pin: u8) -> Result<(), Response> {
    if available(pin) {
        Ok(())
    } else {
        Err(pin_error(pin, PinError::Unavailable))
    }
}

fn pin_error(pin: u8, reason: PinError) -> Response {
    Response::Error(ResponseError::Pin { pin, reason })
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    struct FakeGpio {
        values: [bool; WIRE_PIN_COUNT as usize],
        inputs: [bool; WIRE_PIN_COUNT as usize],
        pullups: [bool; WIRE_PIN_COUNT as usize],
        outputs: [bool; WIRE_PIN_COUNT as usize],
    }

    impl Default for FakeGpio {
        fn default() -> Self {
            Self {
                values: [false; WIRE_PIN_COUNT as usize],
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

        fn output(&mut self, pin: PinId, high: bool) {
            let i = Self::wire_index(pin);
            self.inputs[i] = false;
            self.outputs[i] = true;
            self.pullups[i] = false;
            self.values[i] = high;
        }

        fn write(&mut self, pin: PinId, high: bool) {
            self.values[Self::wire_index(pin)] = high;
        }

        fn read(&self, pin: PinId) -> bool {
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
            assert!(!available(pin));
        }
    }

    #[test]
    fn direction_initializes_and_pullup_resets() {
        let mut firmware = Firmware::new();
        let mut gpio = FakeGpio::default();
        assert_eq!(
            firmware
                .handle(packet(1, Request::Get { pin: 0 }), &mut gpio)
                .body,
            pin_error(0, PinError::Unset)
        );
        assert_eq!(
            firmware
                .handle(
                    packet(
                        2,
                        Request::Direction {
                            pin: 0,
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
                    pin: 0,
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
                    pin: 0,
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
                            pin: 0,
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
                    pin: 5,
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
                            pin: 5,
                            enabled: true,
                        },
                    ),
                    &mut gpio,
                )
                .body,
            Response::Ack
        );
        gpio.values[5] = true;
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
                    pin: 5,
                    enabled: false,
                },
            ),
            &mut gpio,
        );
        gpio.values[5] = false;
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
                    pin: 5,
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
                .handle(packet(3, Request::Get { pin: 5 }), &mut gpio)
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
                            pin: 40,
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
}
