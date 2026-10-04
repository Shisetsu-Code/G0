use g0::{
    authority::{Action, PolicyExpr, PolicyRule, Principal},
    data_format::{DataSchema, FieldRequirement, SchemaField},
    gir::*,
    native_application::GraphService,
    native_transport::{Identity, NetworkPermit, SecureChannel, SecureListener, TransportLimits},
    program::ProgramContract,
    storage::{FieldProtection, FieldSchema, ResourceSchema, StoreSchema},
    store_engine::{NativeStore, StorePermit},
    value::Value,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
const CA: &[u8] = include_bytes!("support/tls/ca.der");
fn registry() -> Vec<DataSchema> {
    vec![DataSchema {
        name: "Message".into(),
        version: 1,
        fields: ["id", "body"]
            .into_iter()
            .enumerate()
            .map(|(i, name)| SchemaField {
                tag: i as u32 + 1,
                name: name.into(),
                ty: SemanticType::Text,
                requirement: FieldRequirement::Required,
            })
            .collect(),
    }]
}
fn policies() -> StoreSchema {
    let mut resource = ResourceSchema::new("Message");
    resource.fields = registry()[0]
        .fields
        .iter()
        .map(|f| FieldSchema {
            name: f.name.clone(),
            ty: f.ty.clone(),
            protection: FieldProtection::Public,
            mutable: f.name != "id",
        })
        .collect();
    resource.policies.rules = [Action::read(), Action::create()]
        .into_iter()
        .map(|action| PolicyRule {
            action,
            allow_if: PolicyExpr::PrincipalOwnsResource,
        })
        .collect();
    StoreSchema {
        resources: vec![resource],
    }
}
fn port(id: u16, name: &str, ty: SemanticType) -> Port {
    Port {
        id,
        name: name.into(),
        ty,
    }
}
fn capability(action: &str) -> Capability {
    Capability::new(CapabilityClass::Storage, action, "Message", "tenant-a")
}
fn program() -> ProgramContract {
    let record = SemanticType::Record("Message".into());
    let version = SemanticType::Integer(IntegerType::new(0, u64::MAX as i128).unwrap());
    let mut graph = Graph::new("put-message");
    graph.inputs = vec![port(0, "message", record.clone())];
    graph.outputs = graph.inputs.clone();
    graph.nodes = vec![
        Node {
            id: 1,
            operation: Operation::Field { name: "id".into() },
            inputs: graph.inputs.clone(),
            outputs: vec![port(0, "id", SemanticType::Text)],
            effects: Default::default(),
            required_capabilities: Default::default(),
        },
        Node {
            id: 2,
            operation: Operation::StoreCreate {
                resource: "Message".into(),
                fields: vec!["id".into(), "body".into()],
            },
            inputs: vec![
                port(0, "id", SemanticType::Text),
                port(1, "message", record.clone()),
            ],
            outputs: vec![port(0, "version", version.clone())],
            effects: [Effect::Storage].into(),
            required_capabilities: [capability("create")].into(),
        },
        Node {
            id: 3,
            operation: Operation::StoreRead {
                resource: "Message".into(),
                fields: vec!["id".into(), "body".into()],
            },
            inputs: vec![port(0, "id", SemanticType::Text), port(1, "after", version)],
            outputs: vec![port(0, "message", record)],
            effects: [Effect::Storage].into(),
            required_capabilities: [capability("read")].into(),
        },
    ];
    let source = |node| SourceEndpoint::NodeOutput { node, port: 0 };
    let target = |node, port| TargetEndpoint::NodeInput { node, port };
    graph.edges = vec![
        Edge {
            from: SourceEndpoint::GraphInput(0),
            to: target(1, 0),
        },
        Edge {
            from: SourceEndpoint::GraphInput(0),
            to: target(2, 1),
        },
        Edge {
            from: source(1),
            to: target(2, 0),
        },
        Edge {
            from: source(1),
            to: target(3, 0),
        },
        Edge {
            from: source(2),
            to: target(3, 1),
        },
        Edge {
            from: source(3),
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    ProgramContract {
        graphs: vec![graph],
        schemas: registry(),
        store: policies(),
        ..Default::default()
    }
}
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
fn permit(action: &str, addr: std::net::SocketAddr) -> NetworkPermit {
    NetworkPermit::authorize(
        action,
        addr,
        &[
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
        ]
        .into(),
    )
    .unwrap()
}

#[test]
fn one_record_survives_graph_storage_native_transport_and_client() {
    let directory = std::env::temp_dir().join(format!("g0-application-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let root = directory.canonicalize().unwrap();
    let grants = BTreeSet::from([
        Capability::new(
            CapabilityClass::Storage,
            "open",
            root.to_string_lossy(),
            "store",
        ),
        Capability::new(
            CapabilityClass::Entropy,
            "generate",
            "storage-nonce",
            "store",
        ),
    ]);
    let mut store = NativeStore::open(
        StorePermit::authorize(&root, &grants).unwrap(),
        policies(),
        registry(),
        [7; 32],
    )
    .unwrap();
    let client_hash: [u8; 32] = ring::digest::digest(
        &ring::digest::SHA256,
        include_bytes!("support/tls/client.der"),
    )
    .as_ref()
    .try_into()
    .unwrap();
    let service = GraphService::new(
        Arc::new(program()),
        "put-message".into(),
        BTreeMap::from([(client_hash, Principal::new("alice", "tenant-a"))]),
        [capability("create"), capability("read")].into(),
        Default::default(),
    )
    .unwrap();
    let listener = SecureListener::bind(
        permit("listen", "127.0.0.1:0".parse().unwrap()),
        identity("server"),
        vec![CA.to_vec()],
        TransportLimits::default(),
    )
    .unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let outcome = service.serve_one(&listener, &mut store).unwrap();
        assert!(outcome.steps > 0);
    });
    let mut client = SecureChannel::connect(
        permit("connect", address),
        "server.g0.test".into(),
        identity("client"),
        vec![CA.to_vec()],
        TransportLimits::default(),
    )
    .unwrap();
    let message = Value::Record {
        schema: "Message".into(),
        fields: Arc::new(BTreeMap::from([
            ("id".into(), Value::Text("m1".into())),
            (
                "body".into(),
                Value::Text("G0 completo de extremo a extremo".into()),
            ),
        ])),
    };
    let ty = SemanticType::Record("Message".into());
    client.send(&message, &ty, &registry()).unwrap();
    assert_eq!(client.receive(&ty, &registry()).unwrap(), message);
    server.join().unwrap();
    let store = NativeStore::open(
        StorePermit::authorize(&root, &grants).unwrap(),
        policies(),
        registry(),
        [7; 32],
    )
    .unwrap();
    assert_eq!(
        store
            .read(
                &store.begin(Principal::new("alice", "tenant-a")),
                "Message",
                "m1"
            )
            .unwrap()
            .value,
        message
    );
    drop(store);
    drop(client);
    std::fs::remove_dir_all(directory).unwrap();
}
