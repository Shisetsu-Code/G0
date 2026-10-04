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
