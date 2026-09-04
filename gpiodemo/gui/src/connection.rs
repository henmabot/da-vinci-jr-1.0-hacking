use std::collections::BTreeMap;

use da_vinci_protocol::{
    Level, MAX_PACKET_LEN, Packet, Query, QueryValue, Request, Response, ResponseError,
    WIRE_PIN_COUNT, encode_request,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Hello,
    Status,
    Ack(Request),
    PinValue {
        pin: u8,
        level: Level,
    },
    PinState {
        pin: u8,
        what: Query,
        value: QueryValue,
    },
    DeviceError {
        request: Option<Request>,
        error: ResponseError,
    },
    Unknown {
        request: Option<Request>,
    },
    Bye,
    Untracked(Packet<Response>),
}

pub struct Connection {
    next_id: u16,
    pending: BTreeMap<u16, Request>,
    listeners: [Option<u16>; WIRE_PIN_COUNT as usize],
}

impl Default for Connection {
    fn default() -> Self {
        Self::new()
    }
}

impl Connection {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            pending: BTreeMap::new(),
            listeners: [None; WIRE_PIN_COUNT as usize],
        }
    }

    pub fn send(&mut self, request: Request) -> Vec<u8> {
        let id = self.next_id;
        self.next_id = if id == 999 { 1 } else { id + 1 };
        self.pending.insert(id, request);

        let mut buffer = [0u8; MAX_PACKET_LEN];
        let len = encode_request(Packet { id, body: request }, &mut buffer)
            .expect("protocol request always fits fixed packet buffer");
        buffer[..len].to_vec()
    }

    pub fn received(&mut self, packet: Packet<Response>) -> Event {
        if packet.body == Response::Bye {
            self.clear();
            return Event::Bye;
        }

        let Some(request) = self.pending.get(&packet.id).copied() else {
            return Event::Untracked(packet);
        };

        match packet.body {
            Response::Hello => {
                self.pending.remove(&packet.id);
                Event::Hello
            }
            Response::Status => {
                self.pending.remove(&packet.id);
                Event::Status
            }
            Response::Ack => self.ack(packet.id, request),
            Response::Value { pin, level } => {
                if !self.is_listener_response(packet.id, request) {
                    self.pending.remove(&packet.id);
                }
                Event::PinValue { pin, level }
            }
            Response::State { pin, what, value } => {
                self.pending.remove(&packet.id);
                Event::PinState { pin, what, value }
            }
            Response::Error(error) => {
                self.retire(packet.id, request);
                Event::DeviceError {
                    request: Some(request),
                    error,
                }
            }
            Response::Unknown => {
                self.retire(packet.id, request);
                Event::Unknown {
                    request: Some(request),
                }
            }
            Response::Bye => unreachable!(),
        }
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.listeners.fill(None);
    }

    fn ack(&mut self, id: u16, request: Request) -> Event {
        if let Request::Listen { pin, enabled } = request {
            let slot = &mut self.listeners[pin as usize];
            if enabled {
                if let Some(previous) = slot.replace(id)
                    && previous != id
                {
                    self.pending.remove(&previous);
                }
                return Event::Ack(request);
            }
            if let Some(previous) = slot.take() {
                self.pending.remove(&previous);
            }
        }
        self.pending.remove(&id);
        Event::Ack(request)
    }

    fn is_listener_response(&self, id: u16, request: Request) -> bool {
        matches!(request, Request::Listen { pin, enabled: true } if self.listeners[pin as usize] == Some(id))
    }

    fn retire(&mut self, id: u16, request: Request) {
        if let Request::Listen { pin, .. } = request
            && self.listeners[pin as usize] == Some(id)
        {
            self.listeners[pin as usize] = None;
        }
        self.pending.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use da_vinci_protocol::decode_request;

    fn response(id: u16, body: Response) -> Packet<Response> {
        Packet { id, body }
    }

    fn sent_id(bytes: &[u8]) -> u16 {
        decode_request(bytes).unwrap().id
    }

    #[test]
    fn request_ids_wrap_from_999_to_001() {
        let mut connection = Connection::new();
        let mut last = 0;
        for _ in 0..999 {
            last = sent_id(&connection.send(Request::Hello));
        }
        assert_eq!(last, 999);
        assert_eq!(sent_id(&connection.send(Request::Hello)), 1);
    }

    #[test]
    fn ordinary_requests_are_retired_after_response() {
        let mut connection = Connection::new();
        let outgoing = connection.send(Request::Get { pin: 5 });
        let id = sent_id(&outgoing);
        assert_eq!(
            connection.received(response(
                id,
                Response::Value {
                    pin: 5,
                    level: Level::High,
                },
            )),
            Event::PinValue {
                pin: 5,
                level: Level::High,
            }
        );
        assert!(matches!(
            connection.received(response(id, Response::Ack)),
            Event::Untracked(_)
        ));
    }

    #[test]
    fn successful_listener_id_persists_for_notifications() {
        let mut connection = Connection::new();
        let listener = connection.send(Request::Listen {
            pin: 5,
            enabled: true,
        });
        let listener = sent_id(&listener);
        assert_eq!(
            connection.received(response(listener, Response::Ack)),
            Event::Ack(Request::Listen {
                pin: 5,
                enabled: true,
            })
        );
        for level in [Level::Low, Level::High] {
            assert_eq!(
                connection.received(response(listener, Response::Value { pin: 5, level },)),
                Event::PinValue { pin: 5, level }
            );
        }
    }

    #[test]
    fn reenable_replaces_old_listener_only_after_ack() {
        let mut connection = Connection::new();
        let first = connection.send(Request::Listen {
            pin: 5,
            enabled: true,
        });
        let first = sent_id(&first);
        connection.received(response(first, Response::Ack));

        let second = connection.send(Request::Listen {
            pin: 5,
            enabled: true,
        });
        let second = sent_id(&second);
        assert!(matches!(
            connection.received(response(
                first,
                Response::Value {
                    pin: 5,
                    level: Level::High,
                }
            )),
            Event::PinValue { .. }
        ));
        connection.received(response(second, Response::Ack));
        assert!(matches!(
            connection.received(response(
                first,
                Response::Value {
                    pin: 5,
                    level: Level::Low,
                }
            )),
            Event::Untracked(_)
        ));
    }

    #[test]
    fn listener_off_retires_persistent_id() {
        let mut connection = Connection::new();
        let on = connection.send(Request::Listen {
            pin: 5,
            enabled: true,
        });
        let on = sent_id(&on);
        connection.received(response(on, Response::Ack));
        let off = connection.send(Request::Listen {
            pin: 5,
            enabled: false,
        });
        let off = sent_id(&off);
        connection.received(response(off, Response::Ack));
        assert!(matches!(
            connection.received(response(
                on,
                Response::Value {
                    pin: 5,
                    level: Level::High,
                }
            )),
            Event::Untracked(_)
        ));
    }

    #[test]
    fn cya_clears_all_bookkeeping() {
        let mut connection = Connection::new();
        let listener = connection.send(Request::Listen {
            pin: 5,
            enabled: true,
        });
        let listener = sent_id(&listener);
        connection.received(response(listener, Response::Ack));
        let bye = connection.send(Request::Bye);
        let bye = sent_id(&bye);
        assert_eq!(
            connection.received(response(bye, Response::Bye)),
            Event::Bye
        );
        assert!(matches!(
            connection.received(response(
                listener,
                Response::Value {
                    pin: 5,
                    level: Level::High,
                }
            )),
            Event::Untracked(_)
        ));
    }
}
