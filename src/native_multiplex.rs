//! Explicitly provisioned typed streams over one authenticated native channel.
use super::{SecureChannel, TransportError};
use crate::{
    data_format::{DataSchema, FieldRequirement, SchemaField},
    gir::{IntegerType, SemanticType},
    value::Value,
    value_codec::{CodecLimits, decode_value, encode_value},
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone)]
pub struct StreamDefinition {
    pub id: u32,
    pub ty: SemanticType,
    pub max_message_bytes: usize,
    pub max_messages: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    Value { stream: u32, value: Value },
    Finished { stream: u32 },
}
struct StreamState {
    definition: StreamDefinition,
    sent: u64,
    received: u64,
    sent_values: u64,
    received_values: u64,
    send_closed: bool,
    receive_closed: bool,
}
pub struct MultiplexedChannel {
    channel: SecureChannel,
    streams: BTreeMap<u32, StreamState>,
    schemas: Vec<DataSchema>,
    closed: bool,
}
fn envelope_schema() -> DataSchema {
    DataSchema {
        name: "g0.native.multiplex.v1".into(),
        version: 1,
        fields: [
            (
                1,
                "stream",
                SemanticType::Integer(IntegerType {
                    min: 1,
                    max: u32::MAX as i128,
                }),
            ),
            (
                2,
                "sequence",
                SemanticType::Integer(IntegerType {
                    min: 0,
                    max: u64::MAX as i128,
                }),
            ),
            (
                3,
                "kind",
                SemanticType::Integer(IntegerType { min: 0, max: 1 }),
            ),
            (4, "payload", SemanticType::Bytes),
        ]
        .into_iter()
        .map(|(tag, name, ty)| SchemaField {
            tag,
            name: name.into(),
            ty,
            requirement: FieldRequirement::Required,
        })
        .collect(),
    }
}
impl MultiplexedChannel {
    /// Both hosts must provision matching stream types. No peer can open a stream
    /// or gain authority by sending a new identifier or schema declaration.
    pub fn new(
        channel: SecureChannel,
        definitions: Vec<StreamDefinition>,
        schemas: Vec<DataSchema>,
    ) -> Result<Self, TransportError> {
        if definitions.is_empty() || definitions.len() > 128 || schemas.len() > 4096 {
            return Err(TransportError::Limit);
        }
        let mut streams = BTreeMap::new();
        for definition in definitions {
            if definition.id == 0
                || definition.max_message_bytes == 0
                || definition.max_message_bytes > channel.limits.max_frame_bytes / 2
                || definition.max_messages == 0
                || definition.max_messages > channel.limits.max_messages
            {
                return Err(TransportError::Limit);
            }
            let id = definition.id;
            if streams
                .insert(
                    id,
                    StreamState {
                        definition,
                        sent: 0,
                        received: 0,
                        sent_values: 0,
                        received_values: 0,
                        send_closed: false,
                        receive_closed: false,
                    },
                )
                .is_some()
            {
                return Err(TransportError::Protocol);
            }
        }
        Ok(Self {
            channel,
            streams,
            schemas,
            closed: false,
        })
    }
    pub fn peer_identity(&self) -> &super::PeerIdentity {
        self.channel.peer_identity()
    }
    pub fn send(&mut self, stream: u32, value: &Value) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::Closed);
        }
        let state = self.streams.get(&stream).ok_or(TransportError::Protocol)?;
        if state.send_closed {
            return Err(TransportError::Closed);
        }
        if state.sent_values >= state.definition.max_messages {
            return Err(TransportError::Limit);
        }
        if value
            .resident_bytes()
            .is_none_or(|n| n > self.channel.limits.max_value_bytes as u64 / 4)
        {
            return Err(TransportError::Limit);
        }
        let payload = encode_value(
            value,
            &state.definition.ty,
            &self.schemas,
            CodecLimits {
                max_bytes: state.definition.max_message_bytes,
                ..Default::default()
            },
        )?;
        self.send_frame(stream, 0, payload)
    }
    /// Half-close one stream; other streams and the reverse direction remain live.
    pub fn finish(&mut self, stream: u32) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::Closed);
        }
        let state = self.streams.get(&stream).ok_or(TransportError::Protocol)?;
        if state.send_closed {
            return Err(TransportError::Closed);
        }
        self.send_frame(stream, 1, vec![])?;
        self.streams.get_mut(&stream).unwrap().send_closed = true;
        Ok(())
    }
    fn send_frame(
        &mut self,
        stream: u32,
        kind: i128,
        payload: Vec<u8>,
    ) -> Result<(), TransportError> {
        let sequence = self.streams[&stream].sent;
        let envelope = Value::Record {
            schema: "g0.native.multiplex.v1".into(),
            fields: Arc::new(BTreeMap::from([
                ("stream".into(), Value::Integer(stream as i128)),
                ("sequence".into(), Value::Integer(sequence as i128)),
                ("kind".into(), Value::Integer(kind)),
                ("payload".into(), Value::Bytes(payload.into())),
            ])),
        };
        if let Err(error) = self.channel.send(
            &envelope,
            &SemanticType::Record(envelope_schema().name),
            &[envelope_schema()],
        ) {
            // A failed wire write can leave a partial frame; preserve fail-closed behavior.
            self.closed = self.channel.closed;
            return Err(error);
        }
        self.streams.get_mut(&stream).unwrap().sent =
            sequence.checked_add(1).ok_or(TransportError::Sequence)?;
        if kind == 0 {
            let state = self.streams.get_mut(&stream).unwrap();
            state.sent_values = state
                .sent_values
                .checked_add(1)
                .ok_or(TransportError::Sequence)?;
        }
        Ok(())
    }
    pub fn receive(&mut self) -> Result<StreamEvent, TransportError> {
        if self.closed {
            return Err(TransportError::Closed);
        }
        let result = self.receive_frame();
        if result.is_err() {
            self.closed = true;
            self.channel.closed = true;
        }
        result
    }
    fn receive_frame(&mut self) -> Result<StreamEvent, TransportError> {
        let envelope = self.channel.receive(
            &SemanticType::Record(envelope_schema().name),
            &[envelope_schema()],
        )?;
        let Value::Record { fields, .. } = envelope else {
            return Err(TransportError::Protocol);
        };
        let get_integer = |name: &str| match fields.get(name) {
            Some(Value::Integer(n)) => Ok(*n),
            _ => Err(TransportError::Protocol),
        };
        let stream = u32::try_from(get_integer("stream")?).map_err(|_| TransportError::Protocol)?;
        let sequence =
            u64::try_from(get_integer("sequence")?).map_err(|_| TransportError::Sequence)?;
        let kind = get_integer("kind")?;
        let Some(Value::Bytes(payload)) = fields.get("payload") else {
            return Err(TransportError::Protocol);
        };
        let state = self
            .streams
            .get_mut(&stream)
            .ok_or(TransportError::Protocol)?;
        if state.receive_closed || sequence != state.received {
            return Err(TransportError::Sequence);
        }
        if (kind == 0 && state.received_values >= state.definition.max_messages)
            || payload.len() > state.definition.max_message_bytes
        {
            return Err(TransportError::Limit);
        }
        let event = match kind {
            0 => StreamEvent::Value {
                stream,
                value: decode_value(
                    payload,
                    &state.definition.ty,
                    &self.schemas,
                    CodecLimits {
                        max_bytes: self.channel.limits.max_value_bytes / 2,
                        ..Default::default()
                    },
                )?,
            },
            1 if payload.is_empty() => {
                state.receive_closed = true;
                StreamEvent::Finished { stream }
            }
            _ => return Err(TransportError::Protocol),
        };
        state.received = state
            .received
            .checked_add(1)
            .ok_or(TransportError::Sequence)?;
        if kind == 0 {
            state.received_values = state
                .received_values
                .checked_add(1)
                .ok_or(TransportError::Sequence)?;
        }
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gir::{Capability, CapabilityClass},
        native_transport::{Identity, NetworkPermit, SecureListener, TransportLimits},
    };
    use std::{collections::BTreeSet, net::SocketAddr};
    const CA: &[u8] = include_bytes!("../tests/support/tls/ca.der");
    fn identity(server: bool) -> Identity {
        let (cert, key) = if server {
            (
                include_bytes!("../tests/support/tls/server.der").as_slice(),
                include_bytes!("../tests/support/tls/server-key.der").as_slice(),
            )
        } else {
            (
                include_bytes!("../tests/support/tls/client.der").as_slice(),
                include_bytes!("../tests/support/tls/client-key.der").as_slice(),
            )
        };
        Identity::new(vec![cert.to_vec()], key.to_vec()).unwrap()
    }
    fn permit(action: &str, address: SocketAddr) -> NetworkPermit {
        NetworkPermit::authorize(
            action,
            address,
            &BTreeSet::from([
                Capability::new(
                    CapabilityClass::Network,
                    action,
                    address.to_string(),
                    "transport",
                ),
                Capability::new(
                    CapabilityClass::Entropy,
                    "generate",
                    "tls-entropy",
                    "transport",
                ),
            ]),
        )
        .unwrap()
    }
    fn frame(sequence: i128, kind: i128, payload: Vec<u8>) -> Value {
        Value::Record {
            schema: envelope_schema().name,
            fields: Arc::new(BTreeMap::from([
                ("stream".into(), Value::Integer(1)),
                ("sequence".into(), Value::Integer(sequence)),
                ("kind".into(), Value::Integer(kind)),
                ("payload".into(), Value::Bytes(payload.into())),
            ])),
        }
    }
    #[test]
    fn actual_encrypted_frames_reject_replay_post_close_and_stream_exhaustion() {
        for scenario in 0..3 {
            let listener = SecureListener::bind(
                permit("listen", "127.0.0.1:0".parse().unwrap()),
                identity(true),
                vec![CA.to_vec()],
                TransportLimits::default(),
            )
            .unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let mut mux = MultiplexedChannel::new(
                    listener.accept().unwrap(),
                    vec![StreamDefinition {
                        id: 1,
                        ty: SemanticType::Text,
                        max_message_bytes: 1024,
                        max_messages: if scenario == 2 { 1 } else { 3 },
                    }],
                    vec![],
                )
                .unwrap();
                mux.receive().unwrap();
                let result = mux.receive();
                if scenario == 2 {
                    assert!(matches!(result, Err(TransportError::Limit)));
                } else {
                    assert!(matches!(result, Err(TransportError::Sequence)));
                }
                assert!(matches!(mux.receive(), Err(TransportError::Closed)));
            });
            let mut channel = SecureChannel::connect(
                permit("connect", address),
                "server.g0.test".into(),
                identity(false),
                vec![CA.to_vec()],
                TransportLimits::default(),
            )
            .unwrap();
            let payload = encode_value(
                &Value::Text("typed".into()),
                &SemanticType::Text,
                &[],
                CodecLimits::default(),
            )
            .unwrap();
            let first = if scenario == 1 {
                frame(0, 1, vec![])
            } else {
                frame(0, 0, payload.clone())
            };
            channel
                .send(
                    &first,
                    &SemanticType::Record(envelope_schema().name),
                    &[envelope_schema()],
                )
                .unwrap();
            let second = frame(if scenario == 0 { 0 } else { 1 }, 0, payload);
            channel
                .send(
                    &second,
                    &SemanticType::Record(envelope_schema().name),
                    &[envelope_schema()],
                )
                .unwrap();
            server.join().unwrap();
        }
    }
}
