use g0::{
    gir::{Capability, CapabilityClass, SemanticType},
    native_transport::{
        Identity, NetworkPermit, SecureChannel, SecureListener, TransportError, TransportLimits,
    },
    value::Value,
};
use std::{collections::BTreeSet, net::SocketAddr, thread, time::Duration};
const CA: &[u8] = include_bytes!("support/tls/ca.der");
fn identity(kind: &str) -> Identity {
    let (cert, key) = if kind == "server" {
        (
            include_bytes!("support/tls/server.der").as_slice(),
            include_bytes!("support/tls/server-key.der").as_slice(),
        )
    } else {
        (
            include_bytes!("support/tls/client.der").as_slice(),
            include_bytes!("support/tls/client-key.der").as_slice(),
        )
    };
    Identity::new(vec![cert.to_vec()], key.to_vec()).unwrap()
}
fn permit(action: &str, addr: SocketAddr) -> NetworkPermit {
    NetworkPermit::authorize(
        action,
        addr,
        &BTreeSet::from([
            Capability::new(
                CapabilityClass::Network,
                action,
                addr.to_string(),
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
fn limits() -> TransportLimits {
    TransportLimits {
        timeout: Duration::from_secs(3),
        ..Default::default()
    }
}

#[test]
fn native_tls_mutual_identity_transports_the_same_typed_value() {
    let listener = SecureListener::bind(
        permit("listen", "127.0.0.1:0".parse().unwrap()),
        identity("server"),
        vec![CA.to_vec()],
        limits(),
    )
    .unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut channel = listener.accept().unwrap();
        assert_ne!(channel.peer_identity().certificate_sha256, [0; 32]);
        assert_eq!(
            channel.receive(&SemanticType::Text, &[]).unwrap(),
            Value::Text("ñ🙂 native".into())
        );
        channel
            .send(&Value::Text("reply".into()), &SemanticType::Text, &[])
            .unwrap();
    });
    let mut channel = SecureChannel::connect(
        permit("connect", address),
        "server.g0.test".into(),
        identity("client"),
        vec![CA.to_vec()],
        limits(),
    )
    .unwrap();
    channel
        .send(&Value::Text("ñ🙂 native".into()), &SemanticType::Text, &[])
        .unwrap();
    assert_eq!(
        channel.receive(&SemanticType::Text, &[]).unwrap(),
        Value::Text("reply".into())
    );
    server.join().unwrap();
}

#[test]
fn authority_wrong_identity_and_oversized_values_are_rejected() {
    assert!(matches!(
        NetworkPermit::authorize("connect", "127.0.0.1:1".parse().unwrap(), &BTreeSet::new()),
        Err(TransportError::Denied)
    ));
    let listener = SecureListener::bind(
        permit("listen", "127.0.0.1:0".parse().unwrap()),
        identity("server"),
        vec![CA.to_vec()],
        limits(),
    )
    .unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || listener.accept().is_err());
    assert!(
        SecureChannel::connect(
            permit("connect", address),
            "wrong.g0.test".into(),
            identity("client"),
            vec![CA.to_vec()],
            limits()
        )
        .is_err()
    );
    assert!(server.join().unwrap());
}

#[test]
fn native_messages_reject_type_mismatch_and_bound_allocations() {
    let listener = SecureListener::bind(
        permit("listen", "127.0.0.1:0".parse().unwrap()),
        identity("server"),
        vec![CA.to_vec()],
        limits(),
    )
    .unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut channel = listener.accept().unwrap();
        assert!(channel.receive(&SemanticType::Bytes, &[]).is_err());
        assert!(matches!(
            channel.receive(&SemanticType::Text, &[]),
            Err(TransportError::Closed)
        ));
    });
    let mut channel = SecureChannel::connect(
        permit("connect", address),
        "server.g0.test".into(),
        identity("client"),
        vec![CA.to_vec()],
        limits(),
    )
    .unwrap();
    channel
        .send(&Value::Text("typed".into()), &SemanticType::Text, &[])
        .unwrap();
    server.join().unwrap();
}

#[test]
fn explicitly_required_hybrid_exchange_round_trips_without_classical_fallback() {
    use g0::native_transport::TlsProfile;
    let listener = SecureListener::bind_with_profile(
        permit("listen", "127.0.0.1:0".parse().unwrap()),
        identity("server"),
        vec![CA.to_vec()],
        limits(),
        TlsProfile::HybridRequired,
    )
    .unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut channel = listener.accept().unwrap();
        assert_eq!(
            channel.key_exchange_group(),
            rustls::NamedGroup::X25519MLKEM768
        );
        let value = channel.receive(&SemanticType::Text, &[]).unwrap();
        channel.send(&value, &SemanticType::Text, &[]).unwrap();
    });
    let mut channel = SecureChannel::connect_with_profile(
        permit("connect", address),
        "server.g0.test".into(),
        identity("client"),
        vec![CA.to_vec()],
        limits(),
        TlsProfile::HybridRequired,
    )
    .unwrap();
    assert_eq!(
        channel.key_exchange_group(),
        rustls::NamedGroup::X25519MLKEM768
    );
    channel
        .send(&Value::Text("hybrid".into()), &SemanticType::Text, &[])
        .unwrap();
    assert_eq!(
        channel.receive(&SemanticType::Text, &[]).unwrap(),
        Value::Text("hybrid".into())
    );
    server.join().unwrap();
    let listener = SecureListener::bind_with_profile(
        permit("listen", "127.0.0.1:0".parse().unwrap()),
        identity("server"),
        vec![CA.to_vec()],
        limits(),
        TlsProfile::HybridRequired,
    )
    .unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || listener.accept().is_err());
    assert!(
        SecureChannel::connect(
            permit("connect", address),
            "server.g0.test".into(),
            identity("client"),
            vec![CA.to_vec()],
            limits()
        )
        .is_err()
    );
    assert!(server.join().unwrap());
}

#[test]
fn multiplexed_typed_streams_interleave_and_close_independently() {
    use g0::native_transport::multiplex::{MultiplexedChannel, StreamDefinition, StreamEvent};
    let definitions = || {
        vec![
            StreamDefinition {
                id: 1,
                ty: SemanticType::Text,
                max_message_bytes: 1024,
                max_messages: 1,
            },
            StreamDefinition {
                id: 2,
                ty: SemanticType::Bytes,
                max_message_bytes: 1024,
                max_messages: 3,
            },
        ]
    };
    let listener = SecureListener::bind(
        permit("listen", "127.0.0.1:0".parse().unwrap()),
        identity("server"),
        vec![CA.to_vec()],
        limits(),
    )
    .unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut mux =
            MultiplexedChannel::new(listener.accept().unwrap(), definitions(), vec![]).unwrap();
        assert_eq!(
            mux.receive().unwrap(),
            StreamEvent::Value {
                stream: 2,
                value: Value::Bytes(vec![0, 255].into())
            }
        );
        assert_eq!(
            mux.receive().unwrap(),
            StreamEvent::Value {
                stream: 1,
                value: Value::Text("one".into())
            }
        );
        assert_eq!(mux.receive().unwrap(), StreamEvent::Finished { stream: 1 });
        assert_eq!(
            mux.receive().unwrap(),
            StreamEvent::Value {
                stream: 2,
                value: Value::Bytes(vec![42].into())
            }
        );
        mux.send(1, &Value::Text("reverse still live".into()))
            .unwrap();
    });
    let channel = SecureChannel::connect(
        permit("connect", address),
        "server.g0.test".into(),
        identity("client"),
        vec![CA.to_vec()],
        limits(),
    )
    .unwrap();
    let mut mux = MultiplexedChannel::new(channel, definitions(), vec![]).unwrap();
    mux.send(2, &Value::Bytes(vec![0, 255].into())).unwrap();
    mux.send(1, &Value::Text("one".into())).unwrap();
    mux.finish(1).unwrap();
    assert!(matches!(
        mux.send(1, &Value::Text("closed".into())),
        Err(TransportError::Closed)
    ));
    assert!(mux.send(2, &Value::Text("wrong type".into())).is_err());
    mux.send(2, &Value::Bytes(vec![42].into())).unwrap();
    assert_eq!(
        mux.receive().unwrap(),
        StreamEvent::Value {
            stream: 1,
            value: Value::Text("reverse still live".into())
        }
    );
    server.join().unwrap();
}

#[test]
fn multiplexed_unknown_stream_fails_closed_without_peer_provisioning() {
    use g0::native_transport::multiplex::{MultiplexedChannel, StreamDefinition};
    let definition = |id| StreamDefinition {
        id,
        ty: SemanticType::Text,
        max_message_bytes: 1024,
        max_messages: 2,
    };
    let listener = SecureListener::bind(
        permit("listen", "127.0.0.1:0".parse().unwrap()),
        identity("server"),
        vec![CA.to_vec()],
        limits(),
    )
    .unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut mux =
            MultiplexedChannel::new(listener.accept().unwrap(), vec![definition(1)], vec![])
                .unwrap();
        assert!(matches!(mux.receive(), Err(TransportError::Protocol)));
        assert!(matches!(mux.receive(), Err(TransportError::Closed)));
    });
    let channel = SecureChannel::connect(
        permit("connect", address),
        "server.g0.test".into(),
        identity("client"),
        vec![CA.to_vec()],
        limits(),
    )
    .unwrap();
    let mut mux = MultiplexedChannel::new(channel, vec![definition(2)], vec![]).unwrap();
    mux.send(2, &Value::Text("unprovisioned".into())).unwrap();
    server.join().unwrap();
}
