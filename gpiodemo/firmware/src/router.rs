use da_vinci_protocol::{Packet, RequestEnvelope, Response, ResponseError};

pub struct Router {
    local_route: &'static [u8],
}

impl Router {
    pub const fn new(local_route: &'static [u8]) -> Self {
        Self { local_route }
    }

    pub const fn local_route(&self) -> &'static [u8] {
        self.local_route
    }

    pub fn dispatch<'a, F>(
        &self,
        envelope: RequestEnvelope<'a>,
        local: F,
    ) -> Packet<Response<&'a [u8]>>
    where
        F: FnOnce(Packet<&'a [u8]>) -> Packet<Response<&'a [u8]>>,
    {
        if envelope.destination == self.local_route {
            return local(Packet {
                id: envelope.id,
                body: envelope.body,
            });
        }

        Packet {
            id: envelope.id,
            body: Response::Error(ResponseError::NoRoute {
                destination: envelope.destination,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_local_body_without_route_knowledge_in_handler() {
        let router = Router::new(b"SAM");
        let envelope = RequestEnvelope {
            id: 10,
            destination: b"SAM",
            body: b"HAI",
        };

        let response = router.dispatch(envelope, |packet| {
            assert_eq!(packet.id, 10);
            assert_eq!(packet.body, b"HAI");
            Packet {
                id: packet.id,
                body: Response::Hello,
            }
        });

        assert_eq!(response.id, 10);
        assert_eq!(response.body, Response::Hello);
    }

    #[test]
    fn missing_routes_return_local_no_route_errors() {
        let router = Router::new(b"SAM");
        for destination in [b"LPC".as_slice(), b"ABC"] {
            let response = router.dispatch(
                RequestEnvelope {
                    id: 11,
                    destination,
                    body: b"HAI",
                },
                |_| panic!("missing route must not reach local handler"),
            );

            assert_eq!(response.id, 11);
            assert_eq!(
                response.body,
                Response::Error(ResponseError::NoRoute { destination })
            );
        }
    }
}
