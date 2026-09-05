use da_vinci_protocol::{
    DecodedRequest, Direction, Level, Packet, Query, QueryValue, Response, ResponseError,
    TargetError,
};

pub const MAX_PINS: usize = 128;
pub const MAX_BANKS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinId(u8);

impl PinId {
    pub const fn new(index: u8) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BankId(u8);

impl BankId {
    pub const fn new(index: u8) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities(u8);

impl Capabilities {
    const INPUT_BIT: u8 = 1 << 0;
    const OUTPUT_BIT: u8 = 1 << 1;
    const PULL_UP_BIT: u8 = 1 << 2;

    pub const NONE: Self = Self(0);
    pub const GPIO: Self = Self(Self::INPUT_BIT | Self::OUTPUT_BIT | Self::PULL_UP_BIT);
    pub(crate) const INPUT: Self = Self(Self::INPUT_BIT);
    pub const INPUT_ONLY: Self = Self(Self::INPUT_BIT | Self::PULL_UP_BIT);

    pub const fn new(input: bool, output: bool, pull_up: bool) -> Self {
        Self(
            (if input { Self::INPUT_BIT } else { 0 })
                | (if output { Self::OUTPUT_BIT } else { 0 })
                | (if pull_up { Self::PULL_UP_BIT } else { 0 }),
        )
    }

    pub const fn available(self) -> bool {
        self.0 != 0
    }

    pub const fn input(self) -> bool {
        self.0 & Self::INPUT_BIT != 0
    }

    pub const fn output(self) -> bool {
        self.0 & Self::OUTPUT_BIT != 0
    }

    pub const fn pull_up(self) -> bool {
        self.0 & Self::PULL_UP_BIT != 0
    }

    const fn supports_direction(self, direction: Direction) -> bool {
        match direction {
            Direction::Input => self.input(),
            Direction::Output => self.output(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BankInfo {
    pub token: &'static str,
}

impl BankInfo {
    pub const fn new(token: &'static str) -> Self {
        Self { token }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinInfo {
    pub token: &'static str,
    pub package_pin: Option<u16>,
    pub bank: BankId,
    pub bit: u8,
    pub capabilities: Capabilities,
}

impl PinInfo {
    pub const fn new(
        token: &'static str,
        package_pin: Option<u16>,
        bank: BankId,
        bit: u8,
        capabilities: Capabilities,
    ) -> Self {
        Self {
            token,
            package_pin,
            bank,
            bit,
            capabilities,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Pin(PinId),
    Bank(BankId),
    All,
}

pub struct PinMap {
    banks: &'static [BankInfo],
    pins: &'static [PinInfo],
}

impl PinMap {
    pub const fn new(banks: &'static [BankInfo], pins: &'static [PinInfo]) -> Self {
        assert!(banks.len() <= MAX_BANKS);
        assert!(pins.len() <= MAX_PINS);
        assert!(pins.len() <= u8::MAX as usize);
        Self { banks, pins }
    }

    pub const fn banks(&self) -> &'static [BankInfo] {
        self.banks
    }

    pub const fn pins(&self) -> &'static [PinInfo] {
        self.pins
    }

    pub fn bank(&self, id: BankId) -> &'static BankInfo {
        &self.banks[id.index()]
    }

    pub fn pin(&self, id: PinId) -> &'static PinInfo {
        &self.pins[id.index()]
    }

    pub fn resolve(&self, token: &[u8]) -> Option<Target> {
        if token == b"ALL" {
            return Some(Target::All);
        }
        if let Some(index) = self
            .banks
            .iter()
            .position(|bank| bank.token.as_bytes() == token)
        {
            return Some(Target::Bank(BankId(index as u8)));
        }
        self.pins
            .iter()
            .position(|pin| pin.token.as_bytes() == token)
            .map(|index| Target::Pin(PinId(index as u8)))
    }

    pub fn pins_for(&self, target: Target) -> impl Iterator<Item = PinId> + '_ {
        self.pins
            .iter()
            .enumerate()
            .filter_map(move |(index, pin)| {
                let id = PinId(index as u8);
                match target {
                    Target::Pin(target) if target == id => Some(id),
                    Target::Bank(bank) if pin.bank == bank => Some(id),
                    Target::All => Some(id),
                    _ => None,
                }
            })
    }
}

pub trait GpioHal {
    fn pin_map(&self) -> &'static PinMap;
    fn input(&mut self, pin: PinId, pull_up: bool);
    fn output(&mut self, pin: PinId, level: Level);
    fn write(&mut self, pin: PinId, level: Level);
    fn read_bank(&self, bank: BankId) -> u32;
}

type FirmwareResponse = Response<&'static [u8], &'static [u8]>;

#[derive(Clone, Copy)]
enum PinState {
    Unset,
    Configured {
        direction: Direction,
        pull_up: bool,
        listener: Option<u16>,
        previous: Level,
    },
}

#[derive(Clone, Copy)]
enum BulkKind {
    Values,
    States(Query),
}

#[derive(Clone, Copy)]
struct BulkResponse {
    id: u16,
    target: Target,
    next: usize,
    kind: BulkKind,
}

pub struct Firmware {
    identity: &'static [u8],
    pins: [PinState; MAX_PINS],
    bulk: Option<BulkResponse>,
    listener_cursor: usize,
}

impl Firmware {
    pub const fn new(identity: &'static [u8]) -> Self {
        Self {
            identity,
            pins: [PinState::Unset; MAX_PINS],
            bulk: None,
            listener_cursor: 0,
        }
    }

    pub fn handle<G: GpioHal>(
        &mut self,
        packet: Packet<DecodedRequest<'_>>,
        gpio: &mut G,
    ) -> Packet<FirmwareResponse> {
        let map = gpio.pin_map();
        let body = match packet.body {
            DecodedRequest::Hello => Response::Hello,
            DecodedRequest::Status => Response::Status {
                identity: self.identity,
            },
            DecodedRequest::Direction { target, direction } => {
                self.resolve(map, target).map_or_else(
                    |error| error,
                    |target| self.set_direction(map, target, direction, gpio),
                )
            }
            DecodedRequest::Get { target } => {
                let Ok(target) = self.resolve(map, target) else {
                    return Packet {
                        id: packet.id,
                        body: bad_packet(),
                    };
                };
                if let Target::Pin(pin) = target {
                    match self.initialized(map, pin) {
                        Ok(()) => Response::Value {
                            target: map.pin(pin).token.as_bytes(),
                            level: read_pin(map, gpio, pin),
                        },
                        Err(error) => error,
                    }
                } else {
                    return self.begin_bulk(packet.id, target, BulkKind::Values, gpio);
                }
            }
            DecodedRequest::Set { target, level } => self.resolve(map, target).map_or_else(
                |error| error,
                |target| self.set_level(map, target, level, gpio),
            ),
            DecodedRequest::Pullup { target, enabled } => self.resolve(map, target).map_or_else(
                |error| error,
                |target| self.set_pull_up(map, target, enabled, gpio),
            ),
            DecodedRequest::Listen { target, enabled } => self.resolve(map, target).map_or_else(
                |error| error,
                |target| self.set_listening(map, target, enabled, packet.id, gpio),
            ),
            DecodedRequest::Query { target, what } => {
                let Ok(target) = self.resolve(map, target) else {
                    return Packet {
                        id: packet.id,
                        body: bad_packet(),
                    };
                };
                if let Target::Pin(pin) = target {
                    match supported(map, pin) {
                        Ok(()) => Response::State {
                            target: map.pin(pin).token.as_bytes(),
                            what,
                            value: self.query(pin, what),
                        },
                        Err(error) => error,
                    }
                } else {
                    return self.begin_bulk(packet.id, target, BulkKind::States(what), gpio);
                }
            }
            DecodedRequest::Bye => {
                self.reset(gpio);
                Response::Bye
            }
        };
        Packet {
            id: packet.id,
            body,
        }
    }

    fn resolve(&self, map: &PinMap, token: &[u8]) -> Result<Target, FirmwareResponse> {
        map.resolve(token).ok_or_else(bad_packet)
    }

    fn begin_bulk<G: GpioHal>(
        &mut self,
        id: u16,
        target: Target,
        kind: BulkKind,
        gpio: &G,
    ) -> Packet<FirmwareResponse> {
        self.bulk = Some(BulkResponse {
            id,
            target,
            next: 0,
            kind,
        });
        self.poll_bulk(gpio)
            .expect("new bulk response always yields a packet")
    }

    pub fn poll_bulk<G: GpioHal>(&mut self, gpio: &G) -> Option<Packet<FirmwareResponse>> {
        let map = gpio.pin_map();
        let BulkResponse {
            id,
            target,
            mut next,
            kind,
        } = self.bulk?;

        while next < map.pins().len() {
            let pin = PinId(next as u8);
            next += 1;
            let info = map.pin(pin);
            if !target_contains(map, target, pin) || !info.capabilities.available() {
                continue;
            }

            let body = match kind {
                BulkKind::Values => {
                    if matches!(self.state(pin), PinState::Unset) {
                        continue;
                    }
                    Response::Value {
                        target: info.token.as_bytes(),
                        level: read_pin(map, gpio, pin),
                    }
                }
                BulkKind::States(what) => Response::State {
                    target: info.token.as_bytes(),
                    what,
                    value: self.query(pin, what),
                },
            };
            self.bulk = Some(BulkResponse {
                id,
                target,
                next,
                kind,
            });
            return Some(Packet { id, body });
        }

        self.bulk = None;
        Some(Packet {
            id,
            body: Response::Ack,
        })
    }

    pub fn poll_listener<G: GpioHal>(&mut self, gpio: &G) -> Option<Packet<FirmwareResponse>> {
        let map = gpio.pin_map();
        let pin_count = map.pins().len();
        if pin_count == 0 {
            return None;
        }

        let mut snapshots = [None; MAX_BANKS];
        for offset in 0..pin_count {
            let index = (self.listener_cursor + offset) % pin_count;
            let pin = PinId(index as u8);
            let PinState::Configured {
                listener: Some(listener),
                previous,
                ..
            } = self.state(pin)
            else {
                continue;
            };
            let info = map.pin(pin);
            let snapshot =
                *snapshots[info.bank.index()].get_or_insert_with(|| gpio.read_bank(info.bank));
            let value = level_from_bank(snapshot, info.bit);
            if value == previous {
                continue;
            }
            if let PinState::Configured { previous, .. } = self.state_mut(pin) {
                *previous = value;
            }
            self.listener_cursor = (index + 1) % pin_count;
            return Some(Packet {
                id: listener,
                body: Response::Value {
                    target: info.token.as_bytes(),
                    level: value,
                },
            });
        }
        None
    }

    fn set_direction<G: GpioHal>(
        &mut self,
        map: &PinMap,
        target: Target,
        direction: Direction,
        gpio: &mut G,
    ) -> FirmwareResponse {
        if let Target::Pin(pin) = target
            && !map.pin(pin).capabilities.supports_direction(direction)
        {
            return pin_error(map, pin, TargetError::Unavailable);
        }
        for pin in map
            .pins_for(target)
            .filter(|pin| map.pin(*pin).capabilities.supports_direction(direction))
        {
            self.set_direction_pin(map, pin, direction, gpio);
        }
        Response::Ack
    }

    fn set_direction_pin<G: GpioHal>(
        &mut self,
        map: &PinMap,
        pin: PinId,
        direction: Direction,
        gpio: &mut G,
    ) {
        let listener = match self.state(pin) {
            PinState::Configured { listener, .. } => listener,
            PinState::Unset => None,
        };
        match direction {
            Direction::Input => gpio.input(pin, false),
            Direction::Output => gpio.output(pin, Level::Low),
        }
        self.pins[pin.index()] = PinState::Configured {
            direction,
            pull_up: false,
            listener,
            previous: read_pin(map, gpio, pin),
        };
    }

    fn set_level<G: GpioHal>(
        &mut self,
        map: &PinMap,
        target: Target,
        level: Level,
        gpio: &mut G,
    ) -> FirmwareResponse {
        if let Target::Pin(pin) = target
            && let Err(error) = self.initialized(map, pin)
        {
            return error;
        }
        let direct = matches!(target, Target::Pin(_));
        for pin in map
            .pins_for(target)
            .filter(|pin| map.pin(*pin).capabilities.output())
        {
            if direct
                || matches!(
                    self.state(pin),
                    PinState::Configured {
                        direction: Direction::Output,
                        ..
                    }
                )
            {
                gpio.write(pin, level);
            }
        }
        Response::Ack
    }

    fn set_pull_up<G: GpioHal>(
        &mut self,
        map: &PinMap,
        target: Target,
        enabled: bool,
        gpio: &mut G,
    ) -> FirmwareResponse {
        if let Target::Pin(pin) = target
            && let Err(error) = self.initialized(map, pin)
        {
            return error;
        }
        for pin in map.pins_for(target) {
            let info = map.pin(pin);
            let needs_pull_up = matches!(
                self.state(pin),
                PinState::Configured {
                    direction: Direction::Input,
                    ..
                }
            );
            if !info.capabilities.available() || (needs_pull_up && !info.capabilities.pull_up()) {
                continue;
            }
            self.set_pull_up_pin(map, pin, enabled, gpio);
        }
        Response::Ack
    }

    fn set_pull_up_pin<G: GpioHal>(
        &mut self,
        map: &PinMap,
        pin: PinId,
        enabled: bool,
        gpio: &mut G,
    ) {
        let PinState::Configured {
            direction,
            pull_up,
            previous,
            ..
        } = self.state_mut(pin)
        else {
            return;
        };
        *pull_up = enabled;
        if *direction == Direction::Input {
            gpio.input(pin, enabled);
            *previous = read_pin(map, gpio, pin);
        }
    }

    fn set_listening<G: GpioHal>(
        &mut self,
        map: &PinMap,
        target: Target,
        enabled: bool,
        id: u16,
        gpio: &G,
    ) -> FirmwareResponse {
        if let Target::Pin(pin) = target
            && let Err(error) = self.initialized(map, pin)
        {
            return error;
        }
        let direct = matches!(target, Target::Pin(_));
        for pin in map
            .pins_for(target)
            .filter(|pin| map.pin(*pin).capabilities.input())
        {
            if direct
                || matches!(
                    self.state(pin),
                    PinState::Configured {
                        direction: Direction::Input,
                        ..
                    }
                )
            {
                self.set_listener_pin(map, pin, enabled, id, gpio);
            }
        }
        Response::Ack
    }

    fn set_listener_pin<G: GpioHal>(
        &mut self,
        map: &PinMap,
        pin: PinId,
        enabled: bool,
        id: u16,
        gpio: &G,
    ) {
        let PinState::Configured {
            listener, previous, ..
        } = self.state_mut(pin)
        else {
            return;
        };
        *listener = enabled.then_some(id);
        if enabled {
            *previous = read_pin(map, gpio, pin);
        }
    }

    fn initialized(&self, map: &PinMap, pin: PinId) -> Result<(), FirmwareResponse> {
        supported(map, pin)?;
        if matches!(self.state(pin), PinState::Unset) {
            return Err(pin_error(map, pin, TargetError::Unset));
        }
        Ok(())
    }

    fn query(&self, pin: PinId, what: Query) -> QueryValue {
        match (self.state(pin), what) {
            (PinState::Unset, _) => QueryValue::Unset,
            (PinState::Configured { direction, .. }, Query::Direction) => {
                QueryValue::Direction(direction)
            }
            (PinState::Configured { pull_up, .. }, Query::Pullup) => QueryValue::Enabled(pull_up),
            (PinState::Configured { listener, .. }, Query::Listen) => {
                QueryValue::Enabled(listener.is_some())
            }
        }
    }

    fn reset<G: GpioHal>(&mut self, gpio: &mut G) {
        self.bulk = None;
        self.listener_cursor = 0;
        let map = gpio.pin_map();
        for index in 0..map.pins().len() {
            let pin = PinId(index as u8);
            let state = self.state_mut(pin);
            if !matches!(state, PinState::Unset) && map.pin(pin).capabilities.input() {
                gpio.input(pin, false);
            }
            *state = PinState::Unset;
        }
    }

    fn state(&self, pin: PinId) -> PinState {
        self.pins[pin.index()]
    }

    fn state_mut(&mut self, pin: PinId) -> &mut PinState {
        &mut self.pins[pin.index()]
    }
}

fn target_contains(map: &PinMap, target: Target, pin: PinId) -> bool {
    match target {
        Target::Pin(target) => target == pin,
        Target::Bank(bank) => map.pin(pin).bank == bank,
        Target::All => true,
    }
}

fn supported(map: &PinMap, pin: PinId) -> Result<(), FirmwareResponse> {
    map.pin(pin)
        .capabilities
        .available()
        .then_some(())
        .ok_or_else(|| pin_error(map, pin, TargetError::Unavailable))
}

fn pin_error(map: &PinMap, pin: PinId, reason: TargetError) -> FirmwareResponse {
    Response::Error(ResponseError::Target {
        target: map.pin(pin).token.as_bytes(),
        reason,
    })
}

fn bad_packet() -> FirmwareResponse {
    Response::Error(ResponseError::BadPacket)
}

fn read_pin<G: GpioHal>(map: &PinMap, gpio: &G, pin: PinId) -> Level {
    let info = map.pin(pin);
    level_from_bank(gpio.read_bank(info.bank), info.bit)
}

fn level_from_bank(bits: u32, bit: u8) -> Level {
    if bits & (1u32 << bit) == 0 {
        Level::Low
    } else {
        Level::High
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::cell::Cell;

    use super::*;
    use crate::sam::{BANK_A, BANK_B, BANK_C, SAM_IDENTITY, SAM_PIN_MAP};
    use da_vinci_protocol::Request;

    const BANK_0: BankId = BankId::new(0);
    const BANK_1: BankId = BankId::new(1);

    static SYNTH_BANKS: [BankInfo; 2] = [BankInfo::new("PIO0"), BankInfo::new("PORTX")];
    static SYNTH_PINS: [PinInfo; 4] = [
        PinInfo::new("PIO0_0", Some(1), BANK_0, 0, Capabilities::GPIO),
        PinInfo::new("PIO0_1", Some(2), BANK_0, 1, Capabilities::NONE),
        PinInfo::new("PX07", None, BANK_1, 7, Capabilities::INPUT_ONLY),
        PinInfo::new("PX08", Some(8), BANK_1, 8, Capabilities::GPIO),
    ];
    static SYNTH_MAP: PinMap = PinMap::new(&SYNTH_BANKS, &SYNTH_PINS);

    struct FakeHal {
        map: &'static PinMap,
        values: [Level; MAX_PINS],
        inputs: [bool; MAX_PINS],
        outputs: [bool; MAX_PINS],
        pull_ups: [bool; MAX_PINS],
        bank_reads: Cell<[u16; MAX_BANKS]>,
    }

    impl FakeHal {
        fn new(map: &'static PinMap) -> Self {
            Self {
                map,
                values: [Level::Low; MAX_PINS],
                inputs: [false; MAX_PINS],
                outputs: [false; MAX_PINS],
                pull_ups: [false; MAX_PINS],
                bank_reads: Cell::new([0; MAX_BANKS]),
            }
        }

        fn reset_reads(&self) {
            self.bank_reads.set([0; MAX_BANKS]);
        }
    }

    impl GpioHal for FakeHal {
        fn pin_map(&self) -> &'static PinMap {
            self.map
        }

        fn input(&mut self, pin: PinId, pull_up: bool) {
            self.inputs[pin.index()] = true;
            self.outputs[pin.index()] = false;
            self.pull_ups[pin.index()] = pull_up;
        }

        fn output(&mut self, pin: PinId, level: Level) {
            self.inputs[pin.index()] = false;
            self.outputs[pin.index()] = true;
            self.pull_ups[pin.index()] = false;
            self.values[pin.index()] = level;
        }

        fn write(&mut self, pin: PinId, level: Level) {
            self.values[pin.index()] = level;
        }

        fn read_bank(&self, bank: BankId) -> u32 {
            let mut reads = self.bank_reads.get();
            reads[bank.index()] += 1;
            self.bank_reads.set(reads);

            self.map.pins_for(Target::Bank(bank)).fold(0, |bits, pin| {
                let info = self.map.pin(pin);
                if self.values[pin.index()] == Level::High {
                    bits | (1u32 << info.bit)
                } else {
                    bits
                }
            })
        }
    }

    fn request(id: u16, body: Request<&'static [u8]>) -> Packet<DecodedRequest<'static>> {
        Packet { id, body }
    }

    fn firmware() -> Firmware {
        Firmware::new(b"SYNTH GPIO")
    }

    #[test]
    fn sam_map_preserves_native_names_package_pins_and_reservations() {
        assert_eq!(SAM_PIN_MAP.banks().len(), 5);
        assert_eq!(SAM_PIN_MAP.pins().len(), 117);
        assert_eq!(SAM_PIN_MAP.bank(BANK_C).token, "PIOC");

        let Target::Pin(pb12) = SAM_PIN_MAP.resolve(b"PB12").unwrap() else {
            panic!("PB12 must resolve to a pin");
        };
        assert_eq!(SAM_PIN_MAP.pin(pb12).package_pin, Some(87));

        for token in [b"PA05".as_slice(), b"PA06"] {
            let Target::Pin(pin) = SAM_PIN_MAP.resolve(token).unwrap() else {
                panic!("reserved SAM UART target must still be present in metadata");
            };
            assert!(!SAM_PIN_MAP.pin(pin).capabilities.available());
            assert_eq!(SAM_PIN_MAP.pin(pin).bank, BANK_A);
        }

        for token in [b"PB08".as_slice(), b"PB09", b"PB10", b"PB11"] {
            let Target::Pin(pin) = SAM_PIN_MAP.resolve(token).unwrap() else {
                panic!("reserved SAM target must still be present in metadata");
            };
            assert!(!SAM_PIN_MAP.pin(pin).capabilities.available());
            assert_eq!(SAM_PIN_MAP.pin(pin).bank, BANK_B);
        }
        assert_eq!(SAM_IDENTITY, b"SAM4E8E GPIO");
    }

    #[test]
    fn pin_map_resolves_native_pin_bank_and_all_without_mcu_branches() {
        assert_eq!(SYNTH_MAP.resolve(b"PIO0"), Some(Target::Bank(BANK_0)));
        assert_eq!(SYNTH_MAP.resolve(b"ALL"), Some(Target::All));
        assert_eq!(
            SYNTH_MAP.resolve(b"PIO0_0"),
            Some(Target::Pin(PinId::new(0)))
        );
        assert_eq!(SYNTH_MAP.resolve(b"PX08"), Some(Target::Pin(PinId::new(3))));
        assert_eq!(SYNTH_MAP.resolve(b"PA00"), None);
    }

    #[test]
    fn direction_pullup_read_and_identity_use_map_metadata() {
        let mut firmware = firmware();
        let mut gpio = FakeHal::new(&SYNTH_MAP);

        assert_eq!(
            firmware.handle(request(1, Request::Status), &mut gpio).body,
            Response::Status {
                identity: b"SYNTH GPIO".as_slice(),
            }
        );
        assert_eq!(
            firmware
                .handle(request(2, Request::Get { target: b"PIO0_0" }), &mut gpio)
                .body,
            pin_error(&SYNTH_MAP, PinId::new(0), TargetError::Unset)
        );

        firmware.handle(
            request(
                3,
                Request::Direction {
                    target: b"PIO0_0",
                    direction: Direction::Input,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            request(
                4,
                Request::Pullup {
                    target: b"PIO0_0",
                    enabled: true,
                },
            ),
            &mut gpio,
        );
        assert!(gpio.inputs[0]);
        assert!(gpio.pull_ups[0]);
        gpio.values[0] = Level::High;
        assert_eq!(
            firmware
                .handle(request(5, Request::Get { target: b"PIO0_0" }), &mut gpio)
                .body,
            Response::Value {
                target: b"PIO0_0".as_slice(),
                level: Level::High,
            }
        );
    }

    #[test]
    fn capability_and_unknown_target_errors_are_local_to_the_map() {
        let mut firmware = firmware();
        let mut gpio = FakeHal::new(&SYNTH_MAP);

        assert_eq!(
            firmware
                .handle(
                    request(
                        1,
                        Request::Direction {
                            target: b"PIO0_1",
                            direction: Direction::Input,
                        },
                    ),
                    &mut gpio,
                )
                .body,
            pin_error(&SYNTH_MAP, PinId::new(1), TargetError::Unavailable)
        );
        assert_eq!(
            firmware
                .handle(request(2, Request::Get { target: b"PA00" }), &mut gpio)
                .body,
            bad_packet()
        );
        assert_eq!(
            firmware
                .handle(
                    request(
                        3,
                        Request::Direction {
                            target: b"PX07",
                            direction: Direction::Output,
                        },
                    ),
                    &mut gpio,
                )
                .body,
            pin_error(&SYNTH_MAP, PinId::new(2), TargetError::Unavailable)
        );
    }

    #[test]
    fn grouped_mutations_follow_active_map_and_skip_unavailable_pins() {
        let mut firmware = firmware();
        let mut gpio = FakeHal::new(&SYNTH_MAP);
        firmware.handle(
            request(
                1,
                Request::Direction {
                    target: b"ALL",
                    direction: Direction::Output,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            request(
                2,
                Request::Set {
                    target: b"ALL",
                    level: Level::High,
                },
            ),
            &mut gpio,
        );

        assert!(gpio.outputs[0]);
        assert!(!gpio.outputs[1]);
        assert!(!gpio.outputs[2]);
        assert!(gpio.outputs[3]);
        assert_eq!(gpio.values[0], Level::High);
        assert_eq!(gpio.values[3], Level::High);
    }

    #[test]
    fn grouped_get_and_query_stream_until_terminal_ack() {
        let mut firmware = firmware();
        let mut gpio = FakeHal::new(&SYNTH_MAP);
        for target in [b"PIO0_0".as_slice(), b"PX08"] {
            firmware.handle(
                request(
                    1,
                    Request::Direction {
                        target,
                        direction: Direction::Input,
                    },
                ),
                &mut gpio,
            );
        }
        gpio.values[3] = Level::High;

        assert_eq!(
            firmware.handle(request(20, Request::Get { target: b"ALL" }), &mut gpio),
            Packet {
                id: 20,
                body: Response::Value {
                    target: b"PIO0_0".as_slice(),
                    level: Level::Low,
                },
            }
        );
        assert_eq!(
            firmware.poll_bulk(&gpio),
            Some(Packet {
                id: 20,
                body: Response::Value {
                    target: b"PX08".as_slice(),
                    level: Level::High,
                },
            })
        );
        assert_eq!(
            firmware.poll_bulk(&gpio),
            Some(Packet {
                id: 20,
                body: Response::Ack,
            })
        );

        let first = firmware.handle(
            request(
                21,
                Request::Query {
                    target: b"PORTX",
                    what: Query::Direction,
                },
            ),
            &mut gpio,
        );
        assert_eq!(
            first.body,
            Response::State {
                target: b"PX07".as_slice(),
                what: Query::Direction,
                value: QueryValue::Unset,
            }
        );
        assert!(matches!(
            firmware.poll_bulk(&gpio),
            Some(Packet {
                id: 21,
                body: Response::State {
                    target: b"PX08",
                    ..
                },
            })
        ));
        assert_eq!(firmware.poll_bulk(&gpio).unwrap().body, Response::Ack);
    }

    #[test]
    fn listeners_keep_request_ids_read_each_bank_once_and_rotate_fairly() {
        let mut firmware = firmware();
        let mut gpio = FakeHal::new(&SYNTH_MAP);
        for (id, target) in [(10, b"PIO0_0".as_slice()), (11, b"PX08")] {
            firmware.handle(
                request(
                    id,
                    Request::Direction {
                        target,
                        direction: Direction::Input,
                    },
                ),
                &mut gpio,
            );
            firmware.handle(
                request(
                    id + 100,
                    Request::Listen {
                        target,
                        enabled: true,
                    },
                ),
                &mut gpio,
            );
        }

        gpio.reset_reads();
        assert_eq!(firmware.poll_listener(&gpio), None);
        let reads = gpio.bank_reads.get();
        assert_eq!(reads[BANK_0.index()], 1);
        assert_eq!(reads[BANK_1.index()], 1);

        gpio.values[0] = Level::High;
        gpio.values[3] = Level::High;
        assert_eq!(
            firmware.poll_listener(&gpio),
            Some(Packet {
                id: 110,
                body: Response::Value {
                    target: b"PIO0_0".as_slice(),
                    level: Level::High,
                },
            })
        );
        gpio.values[0] = Level::Low;
        assert_eq!(
            firmware.poll_listener(&gpio),
            Some(Packet {
                id: 111,
                body: Response::Value {
                    target: b"PX08".as_slice(),
                    level: Level::High,
                },
            })
        );
    }

    #[test]
    fn bye_releases_initialized_pins_and_listener_state() {
        let mut firmware = firmware();
        let mut gpio = FakeHal::new(&SYNTH_MAP);
        firmware.handle(
            request(
                1,
                Request::Direction {
                    target: b"PIO0_0",
                    direction: Direction::Output,
                },
            ),
            &mut gpio,
        );
        firmware.handle(
            request(
                2,
                Request::Listen {
                    target: b"PIO0_0",
                    enabled: true,
                },
            ),
            &mut gpio,
        );
        assert_eq!(
            firmware.handle(request(3, Request::Bye), &mut gpio).body,
            Response::Bye
        );
        assert!(gpio.inputs[0]);
        assert_eq!(firmware.poll_listener(&gpio), None);
        assert_eq!(
            firmware
                .handle(request(4, Request::Get { target: b"PIO0_0" }), &mut gpio)
                .body,
            pin_error(&SYNTH_MAP, PinId::new(0), TargetError::Unset)
        );
    }
}
