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
