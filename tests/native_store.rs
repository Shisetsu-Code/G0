use g0::{
    authority::{Action, PolicyExpr, PolicyRule, Principal},
    data_format::{DataSchema, FieldRequirement, SchemaField},
    gir::{Capability, CapabilityClass, SemanticType},
    storage::{FieldProtection, FieldSchema, ResourceSchema, StoreSchema},
    store_engine::{NativeStore, StoreError, StorePermit},
    value::Value,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "g0-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn permit(&self) -> StorePermit {
        let path = self.0.canonicalize().unwrap();
        let grants = BTreeSet::from([
            Capability::new(
                CapabilityClass::Storage,
                "open",
                path.to_string_lossy(),
                "store",
            ),
            Capability::new(
                CapabilityClass::Entropy,
                "generate",
                "storage-nonce",
                "store",
            ),
        ]);
        StorePermit::authorize(&path, &grants).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn schema() -> (StoreSchema, Vec<DataSchema>) {
    let mut resource = ResourceSchema::new("Message");
    resource.fields.push(FieldSchema {
        name: "body".into(),
        ty: SemanticType::Text,
        protection: FieldProtection::Public,
        mutable: true,
    });
    for action in [
        Action::read(),
        Action::create(),
        Action::update(),
        Action::delete(),
    ] {
        resource.policies.rules.push(PolicyRule {
            action,
            allow_if: PolicyExpr::PrincipalOwnsResource,
        });
    }
    (
        StoreSchema {
            resources: vec![resource],
        },
        vec![DataSchema {
            name: "Message".into(),
            version: 1,
            fields: vec![SchemaField {
                tag: 1,
                name: "body".into(),
                ty: SemanticType::Text,
                requirement: FieldRequirement::Required,
            }],
        }],
    )
}
fn value(text: &str) -> Value {
    Value::Record {
        schema: "Message".into(),
        fields: Arc::new(BTreeMap::from([("body".into(), Value::Text(text.into()))])),
    }
}
fn open(temp: &Temp) -> NativeStore {
    let (schema, registry) = schema();
    NativeStore::open(temp.permit(), schema, registry, [42; 32]).unwrap()
}

#[test]
fn graph_storage_effects_set_and_traverse_persistent_relations() {
    use g0::{
        execution::{ExecutionLimits, Executor},
        gir::*,
        storage::{Cardinality, DeleteRule, RelationSchema},
        storage_host::StorageHost,
    };
    let temp = Temp::new();
    let (mut schema, registry) = schema();
    schema.resources[0].relations.push(RelationSchema {
        name: "parent".into(),
        target_resource: "Message".into(),
        cardinality: Cardinality::OptionalOne,
        on_delete: DeleteRule::Detach,
    });
    let mut store = NativeStore::open(temp.permit(), schema, registry.clone(), [42; 32]).unwrap();
    let alice = Principal::new("alice", "tenant-a");
    let mut tx = store.begin(alice.clone());
    store.create(&mut tx, "source", value("source")).unwrap();
    store.create(&mut tx, "target", value("target")).unwrap();
    store.commit(tx).unwrap();
    let version = SemanticType::Integer(IntegerType {
        min: 0,
        max: u64::MAX as i128,
    });
    let ids = SemanticType::Slice(Box::new(SemanticType::Text));
    let records = SemanticType::Slice(Box::new(SemanticType::Record("Message".into())));
    let ports = |types: Vec<SemanticType>| {
        types
            .into_iter()
            .enumerate()
            .map(|(id, ty)| Port {
                id: id as u16,
                name: format!("p{id}"),
                ty,
            })
            .collect()
    };
    let node = |id, operation, inputs, outputs, action: Option<&str>| Node {
        id,
        operation,
        inputs: ports(inputs),
        outputs: ports(outputs),
        effects: if action.is_some() {
            BTreeSet::from([Effect::Storage])
        } else {
            BTreeSet::new()
        },
        required_capabilities: action
            .map(|action| {
                BTreeSet::from([Capability::new(
                    CapabilityClass::Storage,
                    action,
                    "Message",
                    "tenant-a",
                )])
            })
            .unwrap_or_default(),
    };
    let mut graph = Graph::new("relations");
    graph.inputs = ports(vec![SemanticType::Text]);
    graph.outputs = ports(vec![records.clone()]);
    graph.nodes = vec![
        node(
            1,
            Operation::Const(Literal::Text("target".into())),
            vec![],
            vec![SemanticType::Text],
            None,
        ),
        node(
            2,
            Operation::MakeArray,
            vec![SemanticType::Text],
            vec![ids.clone()],
            None,
        ),
        node(
            3,
            Operation::StoreSetRelation {
                resource: "Message".into(),
                relation: "parent".into(),
            },
            vec![SemanticType::Text, ids],
            vec![version.clone()],
            Some("link"),
        ),
        node(
            4,
            Operation::StoreTraverse {
                resource: "Message".into(),
                relation: "parent".into(),
            },
            vec![SemanticType::Text, version],
            vec![records],
            Some("traverse"),
        ),
    ];
    graph.edges = vec![
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::NodeInput { node: 2, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
            to: TargetEndpoint::NodeInput { node: 3, port: 1 },
        },
        Edge {
            from: SourceEndpoint::GraphInput(0),
            to: TargetEndpoint::NodeInput { node: 3, port: 0 },
        },
        Edge {
            from: SourceEndpoint::GraphInput(0),
            to: TargetEndpoint::NodeInput { node: 4, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 3, port: 0 },
            to: TargetEndpoint::NodeInput { node: 4, port: 1 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 4, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    let encoded = g0::graph_binary::encode_graph(&graph).unwrap();
    let graph = g0::graph_binary_decode::decode_graph(&encoded).unwrap();
    let grants: BTreeSet<_> = graph
        .nodes
        .iter()
        .flat_map(|n| n.required_capabilities.clone())
        .collect();
    let program = g0::program::ProgramContract {
        entry_graph: Some(graph.name.clone()),
        graphs: vec![graph],
        schemas: registry,
        ..Default::default()
    };
    let mut runtime = Executor::new(&program, ExecutionLimits::default()).unwrap();
    for cap in grants {
        runtime.grant(cap);
    }
    let mut host = StorageHost::new(&mut store, alice.clone());
    assert_eq!(
        runtime
            .run_with_host("relations", vec![Value::Text("source".into())], &mut host)
            .unwrap(),
        vec![Value::Array(Arc::from([value("target")]))]
    );
    host.commit().unwrap();
    assert_eq!(
        store
            .traverse(&store.begin(alice), "Message", "source", "parent")
            .unwrap()[0]
            .value,
        value("target")
    );
}

#[test]
fn relation_cascades_authorize_every_affected_row_and_bound_recursive_planning() {
    use g0::storage::{Cardinality, DeleteRule, RelationSchema};
    for rule in [DeleteRule::Detach, DeleteRule::Cascade] {
        let temp = Temp::new();
        let (mut schema, registry) = schema();
        schema.resources[0]
            .policies
            .rules
            .iter_mut()
            .find(|r| r.action == Action::read())
            .unwrap()
            .allow_if = PolicyExpr::Public;
        schema.resources[0].relations.push(RelationSchema {
            name: "parent".into(),
            target_resource: "Message".into(),
            cardinality: Cardinality::OptionalOne,
            on_delete: rule,
        });
        let mut store = NativeStore::open(temp.permit(), schema, registry, [42; 32]).unwrap();
        let alice = Principal::new("alice", "tenant-a");
        let bob = Principal::new("bob", "tenant-a");
        let mut tx = store.begin(alice.clone());
        store.create(&mut tx, "target", value("target")).unwrap();
        store.commit(tx).unwrap();
        let mut tx = store.begin(bob.clone());
        store.create(&mut tx, "source", value("source")).unwrap();
        store
            .set_relation(
                &mut tx,
                "Message",
                "source",
                "parent",
                vec!["target".into()],
            )
            .unwrap();
        store.commit(tx).unwrap();
        let mut deletion = store.begin(alice.clone());
        assert!(matches!(
            store.delete(&mut deletion, "Message", "target"),
            Err(StoreError::Denied)
        ));
        store.commit(deletion).unwrap();
        assert!(store.read(&store.begin(alice), "Message", "target").is_ok());
        assert_eq!(
            store
                .traverse(&store.begin(bob), "Message", "source", "parent")
                .unwrap()
                .len(),
            1
        );
    }
    let temp = Temp::new();
    let (mut schema, registry) = schema();
    schema.resources[0].relations.push(RelationSchema {
        name: "parent".into(),
        target_resource: "Message".into(),
        cardinality: Cardinality::OptionalOne,
        on_delete: DeleteRule::Cascade,
    });
    let mut store = NativeStore::open(temp.permit(), schema, registry, [42; 32]).unwrap();
    let alice = Principal::new("alice", "tenant-a");
    let mut tx = store.begin(alice.clone());
    for index in 0..129 {
        store
            .create(&mut tx, &format!("m{index}"), value("bounded"))
            .unwrap();
        if index > 0 {
            store
                .set_relation(
                    &mut tx,
                    "Message",
                    &format!("m{index}"),
                    "parent",
                    vec![format!("m{}", index - 1)],
                )
                .unwrap();
        }
    }
    store.commit(tx).unwrap();
    let mut deletion = store.begin(alice.clone());
    assert!(matches!(
        store.delete(&mut deletion, "Message", "m0"),
        Err(StoreError::Limit)
    ));
    store.commit(deletion).unwrap();
    assert!(store.read(&store.begin(alice), "Message", "m0").is_ok());
}

#[test]
fn legacy_encrypted_snapshot_is_readable_and_upgraded_on_commit() {
    use ring::aead;
    let temp = Temp::new();
    let mut plaintext = Vec::new();
    let blob = |bytes: &mut Vec<u8>, value: &[u8]| {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value);
    };
    plaintext.extend_from_slice(&1u64.to_le_bytes());
    plaintext.extend_from_slice(&1u32.to_le_bytes());
    for field in ["tenant-a", "Message", "m1"] {
        blob(&mut plaintext, field.as_bytes());
    }
    plaintext.extend_from_slice(&1u64.to_le_bytes());
    blob(&mut plaintext, b"alice");
    let (_, registry) = schema();
    let encoded = g0::value_codec::encode_value(
        &value("legacy"),
        &SemanticType::Record("Message".into()),
        &registry,
        Default::default(),
    )
    .unwrap();
    blob(&mut plaintext, &encoded);
    let cipher =
        aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &[42; 32]).unwrap());
    let mut aad = b"g0.store.snapshot.v1\0".to_vec();
    aad.extend_from_slice(temp.0.canonicalize().unwrap().to_string_lossy().as_bytes());
    let nonce = [7u8; 12];
    cipher
        .seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(aad),
            &mut plaintext,
        )
        .unwrap();
    let mut framed = b"G0S\0\0\0\x01\0".to_vec();
    framed.extend_from_slice(&nonce);
    framed.extend_from_slice(&plaintext);
    std::fs::write(temp.0.join("snapshot.g0s"), framed).unwrap();
    let mut store = open(&temp);
    let alice = Principal::new("alice", "tenant-a");
    assert_eq!(
        store
            .read(&store.begin(alice.clone()), "Message", "m1")
            .unwrap()
            .value,
        value("legacy")
    );
    let mut tx = store.begin(alice.clone());
    store
        .update(
            &mut tx,
            "Message",
            "m1",
            BTreeMap::from([("body".into(), Value::Text("upgraded".into()))]),
        )
        .unwrap();
    store.commit(tx).unwrap();
    drop(store);
    assert_eq!(std::fs::read(temp.0.join("snapshot.g0s")).unwrap()[6], 2);
    let store = open(&temp);
    assert_eq!(
        store
            .read(&store.begin(alice), "Message", "m1")
            .unwrap()
            .value,
        value("upgraded")
    );
}

#[test]
fn persistent_relations_enforce_cardinality_authority_and_delete_rules() {
    use g0::storage::{Cardinality, DeleteRule, RelationSchema};
    for rule in [
        DeleteRule::Restrict,
        DeleteRule::Detach,
        DeleteRule::Cascade,
    ] {
        let temp = Temp::new();
        let (mut schema, registry) = schema();
        schema.resources[0].relations.push(RelationSchema {
            name: "parent".into(),
            target_resource: "Message".into(),
            cardinality: Cardinality::OptionalOne,
            on_delete: rule,
        });
        let mut store =
            NativeStore::open(temp.permit(), schema.clone(), registry.clone(), [42; 32]).unwrap();
        let alice = Principal::new("alice", "tenant-a");
        let mut tx = store.begin(alice.clone());
        store.create(&mut tx, "source", value("source")).unwrap();
        store.create(&mut tx, "target", value("target")).unwrap();
        store
            .set_relation(
                &mut tx,
                "Message",
                "source",
                "parent",
                vec!["target".into()],
            )
            .unwrap();
        store.commit(tx).unwrap();
        drop(store);
        let mut store = NativeStore::open(temp.permit(), schema, registry, [42; 32]).unwrap();
        assert_eq!(
            store
                .traverse(&store.begin(alice.clone()), "Message", "source", "parent")
                .unwrap()[0]
                .value,
            value("target")
        );
        assert!(matches!(
            store.traverse(
                &store.begin(Principal::new("bob", "tenant-a")),
                "Message",
                "source",
                "parent"
            ),
            Err(StoreError::Denied)
        ));
        let mut invalid = store.begin(alice.clone());
        assert!(matches!(
            store.set_relation(
                &mut invalid,
                "Message",
                "source",
                "parent",
                vec!["missing".into()]
            ),
            Err(StoreError::NotFound)
        ));
        store
            .set_relation(
                &mut invalid,
                "Message",
                "source",
                "parent",
                vec!["source".into(), "target".into()],
            )
            .unwrap();
        assert!(matches!(
            store.commit(invalid),
            Err(StoreError::RelationConstraint)
        ));
        let mut deletion = store.begin(alice.clone());
        let result = store.delete(&mut deletion, "Message", "target");
        if rule == DeleteRule::Restrict {
            assert!(matches!(result, Err(StoreError::RelationConstraint)));
            store.commit(deletion).unwrap();
            assert!(store.read(&store.begin(alice), "Message", "target").is_ok());
        } else {
            result.unwrap();
            store.commit(deletion).unwrap();
            assert!(matches!(
                store.read(&store.begin(alice.clone()), "Message", "target"),
                Err(StoreError::NotFound)
            ));
            if rule == DeleteRule::Cascade {
                assert!(matches!(
                    store.read(&store.begin(alice), "Message", "source"),
                    Err(StoreError::NotFound)
                ));
            } else {
                assert!(
                    store
                        .traverse(&store.begin(alice), "Message", "source", "parent")
                        .unwrap()
                        .is_empty()
                );
            }
        }
    }
}

#[test]
fn encrypted_transactions_survive_reopen_and_isolate_principals() {
    let temp = Temp::new();
    let mut store = open(&temp);
    let alice = Principal::new("alice", "tenant-a");
    let mut tx = store.begin(alice.clone());
    store
        .create(&mut tx, "m1", value("persistent secret marker"))
        .unwrap();
    assert_eq!(
        store.read(&tx, "Message", "m1").unwrap().value,
        value("persistent secret marker")
    );
    store.commit(tx).unwrap();
    assert!(
        !std::fs::read(temp.0.join("snapshot.g0s"))
            .unwrap()
            .windows(24)
            .any(|s| s == b"persistent secret marker")
    );
    assert!(matches!(
        NativeStore::open(temp.permit(), schema().0, schema().1, [42; 32]),
        Err(StoreError::Busy)
    ));
    let bob = store.begin(Principal::new("bob", "tenant-a"));
    assert!(matches!(
        store.read(&bob, "Message", "m1"),
        Err(StoreError::Denied)
    ));
    let other = store.begin(Principal::new("alice", "tenant-b"));
    assert!(matches!(
        store.read(&other, "Message", "m1"),
        Err(StoreError::NotFound)
    ));
    drop(store);
    let store = open(&temp);
    let tx = store.begin(alice);
    assert_eq!(
        store.read(&tx, "Message", "m1").unwrap().value,
        value("persistent secret marker")
    );
}

#[test]
fn stale_and_foreign_transactions_cannot_commit_and_rollback_is_inert() {
    let temp = Temp::new();
    let mut store = open(&temp);
    let principal = Principal::new("alice", "tenant");
    let mut first = store.begin(principal.clone());
    let mut stale = store.begin(principal.clone());
    store.create(&mut first, "one", value("first")).unwrap();
    store.create(&mut stale, "two", value("stale")).unwrap();
    store.commit(first).unwrap();
    assert!(matches!(store.commit(stale), Err(StoreError::Conflict)));
    let mut rollback = store.begin(principal.clone());
    store
        .create(&mut rollback, "discard", value("never committed"))
        .unwrap();
    drop(rollback);
    assert!(matches!(
        store.read(&store.begin(principal.clone()), "Message", "discard"),
        Err(StoreError::NotFound)
    ));
    let foreign_temp = Temp::new();
    let foreign = open(&foreign_temp).begin(principal);
    assert!(matches!(
        store.commit(foreign),
        Err(StoreError::ForeignTransaction)
    ));
}

#[test]
fn malformed_storage_wrong_keys_and_missing_capabilities_fail_closed() {
    let temp = Temp::new();
    assert!(matches!(
        StorePermit::authorize(&temp.0, &BTreeSet::new()),
        Err(StoreError::Denied)
    ));
    let mut store = open(&temp);
    let mut tx = store.begin(Principal::new("alice", "tenant"));
    store.create(&mut tx, "m1", value("message")).unwrap();
    store.commit(tx).unwrap();
    drop(store);
    let (store_schema, registry) = schema();
    assert!(matches!(
        NativeStore::open(temp.permit(), store_schema, registry, [7; 32]),
        Err(StoreError::Integrity)
    ));
    let file = temp.0.join("snapshot.g0s");
    let mut bytes = std::fs::read(&file).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    std::fs::write(&file, &bytes).unwrap();
    assert!(matches!(
        NativeStore::open(temp.permit(), schema().0, schema().1, [42; 32]),
        Err(StoreError::Integrity)
    ));
}

#[test]
fn update_delete_and_policy_revocation_are_commit_boundaries() {
    let temp = Temp::new();
    let mut store = open(&temp);
    let alice = Principal::new("alice", "tenant");
    let mut tx = store.begin(alice.clone());
    store.create(&mut tx, "m1", value("old")).unwrap();
    store.commit(tx).unwrap();
    let mut tx = store.begin(alice.clone());
    store
        .update(
            &mut tx,
            "Message",
            "m1",
            BTreeMap::from([("body".into(), Value::Text("new".into()))]),
        )
        .unwrap();
    store.commit(tx).unwrap();
    assert_eq!(
        store
            .read(&store.begin(alice.clone()), "Message", "m1")
            .unwrap()
            .value,
        value("new")
    );
    let mut stale = store.begin(alice.clone());
    store.delete(&mut stale, "Message", "m1").unwrap();
    store.advance_security_epoch();
    assert!(matches!(
        store.commit(stale),
        Err(StoreError::AuthorityChanged)
    ));
    let mut tx = store.begin(alice);
    store.delete(&mut tx, "Message", "m1").unwrap();
    store.commit(tx).unwrap();
}
