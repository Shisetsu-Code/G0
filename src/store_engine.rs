//! Encrypted native snapshot store. Serializable optimistic transactions,
//! explicit host authority, storage-layer policy checks, no automatic retry.
use crate::{
    authority::{Principal, PrincipalId, ResourceContext},
    data_format::{DataSchema, validate_schema},
    gir::{Capability, CapabilityClass, SemanticType},
    storage::{
        FieldProtection, ManagedFieldSource, StoreOperation, StoreSchema, TenantIsolation,
        authorize_store_operation, validate_store_schema,
    },
    value::Value,
    value_codec::{CodecError, CodecLimits, decode_value, encode_value},
};
use ring::{
    aead, hkdf,
    rand::{SecureRandom, SystemRandom},
};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU64},
};
use zeroize::Zeroizing;

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ROWS: usize = 100_000;
const MAX_LINKS: usize = 100_000;
static INSTANCE: AtomicU64 = AtomicU64::new(1);
type Key = (String, String, String); // scope, resource kind, ID
#[derive(Debug)]
pub enum StoreError {
    Denied,
    Busy,
    Conflict,
    ForeignTransaction,
    AuthorityChanged,
    NotFound,
    AlreadyExists,
    InvalidSchema,
    TypeMismatch,
    Integrity,
    Entropy,
    Limit,
    UniqueConstraint,
    RelationConstraint,
    UnsupportedProfile,
    RecoveryRequired,
    CommitUncertain,
    Io(std::io::Error),
    Codec(CodecError),
    Migration(crate::execution::RuntimeError),
}
impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<CodecError> for StoreError {
    fn from(e: CodecError) -> Self {
        Self::Codec(e)
    }
}

pub struct StorePermit {
    root: PathBuf,
}
impl StorePermit {
    pub fn authorize(root: &Path, grants: &BTreeSet<Capability>) -> Result<Self, StoreError> {
        let root = root.canonicalize()?;
        if !grants.contains(&Capability::new(
            CapabilityClass::Storage,
            "open",
            root.to_string_lossy(),
            "store",
        )) || !grants.contains(&Capability::new(
            CapabilityClass::Entropy,
            "generate",
            "storage-nonce",
            "store",
        )) {
            return Err(StoreError::Denied);
        }
        if !root.is_dir() {
            return Err(StoreError::Denied);
        }
        Ok(Self { root })
    }
}
#[derive(Clone)]
struct Row {
    context: ResourceContext,
    value: Value,
    version: u64,
    relations: BTreeMap<String, BTreeSet<Key>>,
}
#[derive(Clone, Default)]
struct Snapshot {
    generation: u64,
    rows: BTreeMap<Key, Row>,
}
pub struct Transaction {
    store: u64,
    principal: Principal,
    epoch: u64,
    snapshot: Arc<Snapshot>,
    writes: BTreeMap<Key, Option<Row>>,
    credential_work: Cell<u32>,
}

impl Transaction {
    pub fn prospective_version(&self) -> Option<u64> {
        self.snapshot.generation.checked_add(1)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredValue {
    pub value: Value,
    pub version: u64,
}

/// Durable monotonic anchor kept outside the snapshot's rollback domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreCheckpoint {
    pub generation: u64,
    pub digest: [u8; 32],
}
#[derive(Clone, Copy, Debug)]
pub struct WitnessError;
/// One instance is namespaced to one store. Implementations must supply
/// durable, atomic CAS and resist rollback independently of store files.
/// A local file in the same backup/restore domain is not a trusted witness.
pub trait RollbackWitness: Send + Sync {
    fn load(&self) -> Result<Option<StoreCheckpoint>, WitnessError>;
    fn compare_exchange(
        &self,
        expected: Option<&StoreCheckpoint>,
        next: &StoreCheckpoint,
    ) -> Result<(), WitnessError>;
}

pub struct MigrationPermit {
    root: PathBuf,
    resource: String,
    from: u32,
    to: u32,
}
impl MigrationPermit {
    pub fn authorize(
        root: &Path,
        resource: &str,
        from: u32,
        to: u32,
        grants: &BTreeSet<Capability>,
    ) -> Result<Self, StoreError> {
        let root = root.canonicalize()?;
        if !grants.contains(&Capability::new(
            CapabilityClass::Storage,
            format!("migrate:{resource}:{from}:{to}"),
            root.to_string_lossy(),
            "store",
        )) {
            return Err(StoreError::Denied);
        }
        Ok(Self {
            root,
            resource: resource.into(),
            from,
            to,
        })
    }
}
#[derive(Clone)]
pub struct StoreMigration {
    pub resource: crate::storage::ResourceSchema,
    pub data_schema: DataSchema,
    pub source_schema: String,
    pub plan: crate::migration::MigrationPlan,
    pub transform: crate::program_binary::ProgramDocument,
    pub limits: crate::execution::ExecutionLimits,
    pub cancellation: crate::execution::Cancellation,
}

pub struct NativeStore {
    root: PathBuf,
    _lock: File,
    schema: StoreSchema,
    registry: Vec<DataSchema>,
    cipher: aead::LessSafeKey,
    field_keys: hkdf::Prk,
    snapshot: Arc<Snapshot>,
    instance: u64,
    epoch: u64,
    recovery: bool,
    checkpoint: StoreCheckpoint,
    witness: Option<Arc<dyn RollbackWitness>>,
}
impl NativeStore {
    fn validate_definition(
        schema: &StoreSchema,
        registry: &[DataSchema],
    ) -> Result<(), StoreError> {
        validate_store_schema(schema).map_err(|_| StoreError::InvalidSchema)?;
        let mut names = BTreeSet::new();
        for s in registry {
            if s.name.is_empty() || !names.insert(&s.name) || validate_schema(s).is_err() {
                return Err(StoreError::InvalidSchema);
            }
        }
        for resource in &schema.resources {
            let data = registry
                .iter()
                .find(|s| s.name == resource.name)
                .ok_or(StoreError::InvalidSchema)?;
            if data.fields.len() != resource.fields.len()
                || !resource
                    .fields
                    .iter()
                    .all(|f| data.fields.iter().any(|d| d.name == f.name && d.ty == f.ty))
            {
                return Err(StoreError::InvalidSchema);
            }
            // Profiles not implemented by this engine are rejected rather than
            // silently turning off field protection or referential integrity.
            if resource.managed_fields.iter().any(|m| {
                matches!(
                    m.source,
                    ManagedFieldSource::Generated | ManagedFieldSource::StoreClock
                )
            }) {
                return Err(StoreError::UnsupportedProfile);
            }
            for field in &resource.fields {
                let ty = match (&field.protection, &field.ty) {
                    (FieldProtection::Credential, SemanticType::Credential(inner))
                        if matches!(inner.as_ref(), SemanticType::Text | SemanticType::Bytes) =>
                    {
                        continue;
                    }
                    (FieldProtection::Credential, _) => return Err(StoreError::UnsupportedProfile),
                    (FieldProtection::Secret, SemanticType::Secret(inner)) => inner,
                    (_, ty) => ty,
                };
                crate::value_codec::validate_public_type(ty, registry)?;
            }
        }
        Ok(())
    }
    pub fn open(
        permit: StorePermit,
        schema: StoreSchema,
        registry: Vec<DataSchema>,
        key: [u8; 32],
    ) -> Result<Self, StoreError> {
        let key = Zeroizing::new(key);
        Self::validate_definition(&schema, &registry)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(permit.root.join("store.lock"))?;
        lock.try_lock().map_err(|_| StoreError::Busy)?;
        let cipher = aead::UnboundKey::new(&aead::AES_256_GCM, key.as_ref())
            .map_err(|_| StoreError::Integrity);
        let field_keys =
            hkdf::Salt::new(hkdf::HKDF_SHA256, b"g0.store.fields.v1").extract(key.as_ref());
        let cipher = aead::LessSafeKey::new(cipher?);
        let instance =
            crate::runtime_resources::fresh_identity(&INSTANCE).ok_or(StoreError::Limit)?;
        let checkpoint = Self::genesis(&permit.root);
        let mut store = Self {
            root: permit.root,
            _lock: lock,
            schema,
            registry,
            cipher,
            field_keys,
            snapshot: Arc::new(Snapshot::default()),
            instance,
            epoch: 0,
            recovery: false,
            checkpoint,
            witness: None,
        };
        let path = store.root.join("snapshot.g0s");
        match File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
                if bytes.len() > MAX_BYTES {
                    return Err(StoreError::Limit);
                }
                let digest = ring::digest::digest(&ring::digest::SHA256, &bytes)
                    .as_ref()
                    .try_into()
                    .unwrap();
                store.snapshot = Arc::new(store.decode_snapshot(bytes)?);
                store.checkpoint = StoreCheckpoint {
                    generation: store.snapshot.generation,
                    digest,
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        Ok(store)
    }
    fn genesis(root: &Path) -> StoreCheckpoint {
        let mut input = b"g0.store.genesis.v1\0".to_vec();
        input.extend_from_slice(root.to_string_lossy().as_bytes());
        StoreCheckpoint {
            generation: 0,
            digest: ring::digest::digest(&ring::digest::SHA256, &input)
                .as_ref()
                .try_into()
                .unwrap(),
        }
    }
    pub fn open_with_witness(
        permit: StorePermit,
        schema: StoreSchema,
        registry: Vec<DataSchema>,
        key: [u8; 32],
        witness: Arc<dyn RollbackWitness>,
    ) -> Result<Self, StoreError> {
        let mut store = Self::open(permit, schema, registry, key)?;
        match witness.load().map_err(|_| StoreError::RecoveryRequired)? {
            Some(expected) if expected == store.checkpoint => {}
            None if store.checkpoint == Self::genesis(&store.root) => witness
                .compare_exchange(None, &store.checkpoint)
                .map_err(|_| StoreError::RecoveryRequired)?,
            _ => return Err(StoreError::RecoveryRequired),
        }
        store.witness = Some(witness);
        Ok(store)
    }
    pub fn begin(&self, principal: Principal) -> Transaction {
        Transaction {
            store: self.instance,
            principal,
            epoch: self.epoch,
            snapshot: self.snapshot.clone(),
            writes: BTreeMap::new(),
            credential_work: Cell::new(0),
        }
    }
    pub fn advance_security_epoch(&mut self) {
        self.epoch = self.epoch.saturating_add(1);
    }
    fn check(&self, tx: &Transaction) -> Result<(), StoreError> {
        if self.recovery {
            return Err(StoreError::RecoveryRequired);
        }
        if tx.store != self.instance {
            return Err(StoreError::ForeignTransaction);
        }
        if tx.epoch != self.epoch {
            return Err(StoreError::AuthorityChanged);
        }
        if tx.principal.id.0.is_empty() || tx.principal.scope.0.is_empty() {
            return Err(StoreError::Denied);
        }
        Ok(())
    }
    fn charge_credential(&self, tx: &Transaction) -> Result<(), StoreError> {
        let used = tx.credential_work.get();
        if used >= 16 {
            return Err(StoreError::Limit);
        }
        tx.credential_work.set(used + 1);
        Ok(())
    }
    fn key(&self, principal: &Principal, kind: &str, id: &str) -> Result<Key, StoreError> {
        let resource = self
            .schema
            .resource(kind)
            .ok_or(StoreError::InvalidSchema)?;
        if id.is_empty()
            || id.len() > 1024
            || principal.scope.0.len() > 1024
            || principal.id.0.len() > 1024
        {
            return Err(StoreError::Limit);
        }
        Ok((
            if resource.tenant_isolation == TenantIsolation::Global {
                String::new()
            } else {
                principal.scope.0.clone()
            },
            kind.into(),
            id.into(),
        ))
    }
    fn visible<'a>(&self, tx: &'a Transaction, key: &Key) -> Option<&'a Row> {
        match tx.writes.get(key) {
            Some(value) => value.as_ref(),
            None => tx.snapshot.rows.get(key),
        }
    }
    fn authorize(
        &self,
        principal: &Principal,
        row: &Row,
        operation: StoreOperation,
    ) -> Result<(), StoreError> {
        let schema = self
            .schema
            .resource(&row.context.kind.0)
            .ok_or(StoreError::InvalidSchema)?;
        authorize_store_operation(schema, principal, &row.context, &operation)
            .map_err(|_| StoreError::Denied)
    }
    pub fn read(&self, tx: &Transaction, kind: &str, id: &str) -> Result<StoredValue, StoreError> {
        self.check(tx)?;
        let key = self.key(&tx.principal, kind, id)?;
        let row = self.visible(tx, &key).ok_or(StoreError::NotFound)?;
        let Value::Record { .. } = &row.value else {
            return Err(StoreError::Integrity);
        };
        self.authorize(
            &tx.principal,
            row,
            StoreOperation::Read {
                fields: self.resource_fields(kind)?,
            },
        )?;
        Ok(StoredValue {
            value: row.value.clone(),
            version: row.version,
        })
    }
    pub fn resource_fields(&self, kind: &str) -> Result<Vec<String>, StoreError> {
        Ok(self
            .schema
            .resource(kind)
            .ok_or(StoreError::InvalidSchema)?
            .fields
            .iter()
            .map(|f| f.name.clone())
            .collect())
    }
    pub fn read_fields(
        &self,
        tx: &Transaction,
        kind: &str,
        id: &str,
        requested: &[String],
    ) -> Result<Vec<Value>, StoreError> {
        self.check(tx)?;
        let key = self.key(&tx.principal, kind, id)?;
        let row = self.visible(tx, &key).ok_or(StoreError::NotFound)?;
        if requested.iter().collect::<BTreeSet<_>>().len() != requested.len() {
            return Err(StoreError::Denied);
        }
        self.authorize(
            &tx.principal,
            row,
            StoreOperation::Read {
                fields: requested.to_vec(),
            },
        )?;
        let Value::Record { fields, .. } = &row.value else {
            return Err(StoreError::Integrity);
        };
        let schema = self
            .registry
            .iter()
            .find(|s| s.name == kind)
            .ok_or(StoreError::InvalidSchema)?;
        requested
            .iter()
            .map(|name| {
                let field = schema
                    .fields
                    .iter()
                    .find(|f| &f.name == name)
                    .ok_or(StoreError::InvalidSchema)?;
                if field.requirement == crate::data_format::FieldRequirement::Optional {
                    Ok(Value::Option(fields.get(name).cloned().map(Arc::new)))
                } else {
                    fields.get(name).cloned().ok_or(StoreError::Integrity)
                }
            })
            .collect()
    }
    pub fn create(&self, tx: &mut Transaction, id: &str, value: Value) -> Result<(), StoreError> {
        self.check(tx)?;
        let Value::Record { schema, fields } = value else {
            return Err(StoreError::TypeMismatch);
        };
        let key = self.key(&tx.principal, &schema, id)?;
        if self.visible(tx, &key).is_some() {
            return Err(StoreError::AlreadyExists);
        }
        let mut context = ResourceContext::new(&schema, id, &key.0);
        context.owner = Some(tx.principal.id.clone());
        let original = fields.keys().cloned().collect();
        let mut fields = (*fields).clone();
        let resource = self.schema.resource(&schema).unwrap();
        for managed in &resource.managed_fields {
            if fields.contains_key(&managed.field) {
                return Err(StoreError::Denied);
            }
            fields.insert(
                managed.field.clone(),
                Value::Text(match managed.source {
                    ManagedFieldSource::CurrentPrincipal => tx.principal.id.0.as_str().into(),
                    ManagedFieldSource::CurrentScope => tx.principal.scope.0.as_str().into(),
                    _ => return Err(StoreError::UnsupportedProfile),
                }),
            );
        }
        let value = Value::Record {
            schema: schema.clone(),
            fields: Arc::new(fields),
        };
        if !value.fits(&SemanticType::Record(schema), &self.registry) {
            return Err(StoreError::TypeMismatch);
        }
        let mut row = Row {
            context,
            value,
            relations: BTreeMap::new(),
            version: tx
                .snapshot
                .generation
                .checked_add(1)
                .ok_or(StoreError::Limit)?,
        };
        self.authorize(
            &tx.principal,
            &row,
            StoreOperation::Create { fields: original },
        )?;
        if tx.writes.len() >= MAX_ROWS {
            return Err(StoreError::Limit);
        }
        // Originals are replaced only after resource and field authorization.
        let Value::Record { fields, .. } = &mut row.value else {
            unreachable!()
        };
        for field in &resource.fields {
            if field.protection == FieldProtection::Credential
                && let Some(value) = Arc::make_mut(fields).get_mut(&field.name)
            {
                self.charge_credential(tx)?;
                *value = Value::CredentialVerifier(crate::credential::Verifier::create(value)?);
            }
        }
        if tx.writes.len() >= MAX_ROWS {
            return Err(StoreError::Limit);
        }
        tx.writes.insert(key, Some(row));
        Ok(())
    }
    pub fn verify_credential(
        &self,
        tx: &Transaction,
        kind: &str,
        id: &str,
        field: &str,
        candidate: &Value,
    ) -> Result<bool, StoreError> {
        self.check(tx)?;
        let key = self.key(&tx.principal, kind, id)?;
        let row = self.visible(tx, &key).ok_or(StoreError::NotFound)?;
        self.authorize(
            &tx.principal,
            row,
            StoreOperation::VerifyCredential {
                field: field.into(),
            },
        )?;
        let Value::Record { fields, .. } = &row.value else {
            return Err(StoreError::Integrity);
        };
        let Value::CredentialVerifier(verifier) = fields.get(field).ok_or(StoreError::NotFound)?
        else {
            return Err(StoreError::Integrity);
        };
        self.charge_credential(tx)?;
        verifier.verify(candidate)
    }
    pub fn set_credential(
        &self,
        tx: &mut Transaction,
        kind: &str,
        id: &str,
        field: &str,
        candidate: &Value,
    ) -> Result<(), StoreError> {
        self.check(tx)?;
        let key = self.key(&tx.principal, kind, id)?;
        let original = self.visible(tx, &key).ok_or(StoreError::NotFound)?;
        self.authorize(
            &tx.principal,
            original,
            StoreOperation::SetCredential {
                field: field.into(),
            },
        )?;
        let ty = &self
            .schema
            .resource(kind)
            .and_then(|r| r.field(field))
            .ok_or(StoreError::InvalidSchema)?
            .ty;
        if !candidate.fits(ty, &self.registry) {
            return Err(StoreError::TypeMismatch);
        }
        if tx.writes.len() >= MAX_ROWS && !tx.writes.contains_key(&key) {
            return Err(StoreError::Limit);
        }
        self.charge_credential(tx)?;
        let verifier = crate::credential::Verifier::create(candidate)?;
        let mut row = original.clone();
        let Value::Record { fields, .. } = &mut row.value else {
            return Err(StoreError::Integrity);
        };
        Arc::make_mut(fields).insert(field.into(), Value::CredentialVerifier(verifier));
        row.version = tx.prospective_version().ok_or(StoreError::Limit)?;
        tx.writes.insert(key, Some(row));
        Ok(())
    }
    pub fn update(
        &self,
        tx: &mut Transaction,
        kind: &str,
        id: &str,
        changes: BTreeMap<String, Value>,
    ) -> Result<(), StoreError> {
        self.check(tx)?;
        let key = self.key(&tx.principal, kind, id)?;
        let row = self.visible(tx, &key).ok_or(StoreError::NotFound)?;
        self.authorize(
            &tx.principal,
            row,
            StoreOperation::Update {
                fields: changes.keys().cloned().collect(),
            },
        )?;
        let Value::Record { schema, fields } = &row.value else {
            return Err(StoreError::Integrity);
        };
        let mut fields = (**fields).clone();
        fields.extend(changes);
        let value = Value::Record {
            schema: schema.clone(),
            fields: Arc::new(fields),
        };
        if !value.fits(&SemanticType::Record(kind.into()), &self.registry) {
            return Err(StoreError::TypeMismatch);
        }
        let row = Row {
            value,
            context: row.context.clone(),
            relations: row.relations.clone(),
            version: tx
                .snapshot
                .generation
                .checked_add(1)
                .ok_or(StoreError::Limit)?,
        };
        if tx.writes.len() >= MAX_ROWS {
            return Err(StoreError::Limit);
        }
        tx.writes.insert(key, Some(row));
        Ok(())
    }
    fn materialize(&self, tx: &Transaction) -> Snapshot {
        let mut next = (*tx.snapshot).clone();
        for (key, row) in &tx.writes {
            if let Some(row) = row {
                next.rows.insert(key.clone(), row.clone());
            } else {
                next.rows.remove(key);
            }
        }
        next
    }
    fn relation_snapshot(&self, snapshot: &Snapshot) -> crate::storage_integrity::StoreSnapshot {
        use crate::storage_integrity::{EntityInstance, RelationInstance, StoreSnapshot};
        StoreSnapshot {
            entities: snapshot
                .rows
                .iter()
                .map(|(key, row)| {
                    let key = entity_key(key);
                    let entity = EntityInstance {
                        key: key.clone(),
                        relations: row
                            .relations
                            .iter()
                            .map(|(name, targets)| RelationInstance {
                                name: name.clone(),
                                targets: targets.iter().map(entity_key).collect(),
                            })
                            .collect(),
                    };
                    (key, entity)
                })
                .collect(),
        }
    }
    fn check_relations(&self, snapshot: &Snapshot) -> Result<(), StoreError> {
        let links = snapshot
            .rows
            .values()
            .flat_map(|r| r.relations.values())
            .try_fold(0usize, |n, targets| n.checked_add(targets.len()))
            .ok_or(StoreError::Limit)?;
        if links > MAX_LINKS {
            return Err(StoreError::Limit);
        }
        for ((_, kind, _), row) in &snapshot.rows {
            let resource = self
                .schema
                .resource(kind)
                .ok_or(StoreError::InvalidSchema)?;
            for relation in &resource.relations {
                if relation.cardinality == crate::storage::Cardinality::One
                    && row
                        .relations
                        .get(&relation.name)
                        .is_none_or(|targets| targets.len() != 1)
                {
                    return Err(StoreError::RelationConstraint);
                }
            }
        }
        crate::storage_integrity::validate_snapshot(&self.schema, &self.relation_snapshot(snapshot))
            .map_err(|_| StoreError::RelationConstraint)
    }
    pub fn set_relation(
        &self,
        tx: &mut Transaction,
        kind: &str,
        id: &str,
        name: &str,
        target_ids: Vec<String>,
    ) -> Result<(), StoreError> {
        self.check(tx)?;
        if target_ids.len() > MAX_LINKS {
            return Err(StoreError::Limit);
        }
        let key = self.key(&tx.principal, kind, id)?;
        let row = self.visible(tx, &key).ok_or(StoreError::NotFound)?;
        self.authorize(
            &tx.principal,
            row,
            StoreOperation::Update { fields: vec![] },
        )?;
        let relation = self
            .schema
            .resource(kind)
            .and_then(|r| r.relations.iter().find(|r| r.name == name))
            .ok_or(StoreError::InvalidSchema)?;
        let mut targets = BTreeSet::new();
        for id in target_ids {
            let target = self.key(&tx.principal, &relation.target_resource, &id)?;
            let target_row = self.visible(tx, &target).ok_or(StoreError::NotFound)?;
            self.authorize(
                &tx.principal,
                target_row,
                StoreOperation::Read { fields: vec![] },
            )?;
            if !targets.insert(target) {
                return Err(StoreError::RelationConstraint);
            }
        }
        let mut row = row.clone();
        row.relations.insert(name.into(), targets);
        row.version = tx.prospective_version().ok_or(StoreError::Limit)?;
        if tx.writes.len() >= MAX_ROWS && !tx.writes.contains_key(&key) {
            return Err(StoreError::Limit);
        }
        tx.writes.insert(key, Some(row));
        Ok(())
    }
    pub fn traverse(
        &self,
        tx: &Transaction,
        kind: &str,
        id: &str,
        name: &str,
    ) -> Result<Vec<StoredValue>, StoreError> {
        self.check(tx)?;
        let key = self.key(&tx.principal, kind, id)?;
        let row = self.visible(tx, &key).ok_or(StoreError::NotFound)?;
        self.authorize(&tx.principal, row, StoreOperation::Read { fields: vec![] })?;
        if !self
            .schema
            .resource(kind)
            .is_some_and(|r| r.relations.iter().any(|r| r.name == name))
        {
            return Err(StoreError::InvalidSchema);
        }
        row.relations
            .get(name)
            .into_iter()
            .flatten()
            .map(|target| {
                if self.key(&tx.principal, &target.1, &target.2)? != *target {
                    return Err(StoreError::Denied);
                }
                self.read(tx, &target.1, &target.2)
            })
            .collect()
    }
    pub fn delete(&self, tx: &mut Transaction, kind: &str, id: &str) -> Result<(), StoreError> {
        self.check(tx)?;
        let key = self.key(&tx.principal, kind, id)?;
        let row = self.visible(tx, &key).ok_or(StoreError::NotFound)?;
        self.authorize(&tx.principal, row, StoreOperation::Delete)?;
        let metadata = self.relation_snapshot(&self.materialize(tx));
        let target = entity_key(&key);
        // Bound the recursive contract planner before it can recurse. The store
        // profile deliberately limits each cascade to 128 entities.
        let mut closure = BTreeSet::new();
        let mut pending = vec![target.clone()];
        while let Some(target) = pending.pop() {
            if !closure.insert(target.clone()) {
                continue;
            }
            if closure.len() > 128 {
                return Err(StoreError::Limit);
            }
            for entity in metadata.entities.values() {
                for relation in &entity.relations {
                    if relation.targets.contains(&target)
                        && self
                            .schema
                            .resource(&entity.key.resource)
                            .and_then(|r| r.relations.iter().find(|r| r.name == relation.name))
                            .is_some_and(|r| r.on_delete == crate::storage::DeleteRule::Cascade)
                    {
                        pending.push(entity.key.clone());
                    }
                }
            }
        }
        let plan = crate::storage_delete::plan_delete(&self.schema, &metadata, &target)
            .map_err(|_| StoreError::RelationConstraint)?;
        let mut writes: BTreeMap<Key, Option<Row>> = BTreeMap::new();
        for entity in &plan.delete {
            let key = store_key(entity);
            let row = self.visible(tx, &key).ok_or(StoreError::NotFound)?;
            self.authorize(&tx.principal, row, StoreOperation::Delete)?;
            writes.insert(key, None);
        }
        for detach in &plan.detach {
            if plan.delete.contains(&detach.source) {
                continue;
            }
            let key = store_key(&detach.source);
            let original = self.visible(tx, &key).ok_or(StoreError::NotFound)?;
            self.authorize(
                &tx.principal,
                original,
                StoreOperation::Update { fields: vec![] },
            )?;
            let mut row = writes
                .get(&key)
                .and_then(|r| r.as_ref())
                .unwrap_or(original)
                .clone();
            row.relations
                .get_mut(&detach.relation)
                .ok_or(StoreError::Integrity)?
                .remove(&store_key(&detach.target));
            row.version = tx.prospective_version().ok_or(StoreError::Limit)?;
            writes.insert(key, Some(row));
        }
        if tx
            .writes
            .keys()
            .chain(writes.keys())
            .collect::<BTreeSet<_>>()
            .len()
            > MAX_ROWS
        {
            return Err(StoreError::Limit);
        }
        tx.writes.extend(writes);
        Ok(())
    }
    pub fn enumerate(&self, tx: &Transaction, kind: &str) -> Result<Vec<StoredValue>, StoreError> {
        self.check(tx)?;
        let collection = self.key(&tx.principal, kind, "collection")?;
        let schema = self.schema.resource(kind).unwrap();
        authorize_store_operation(
            schema,
            &tx.principal,
            &ResourceContext::new(kind, "collection", &collection.0),
            &StoreOperation::Enumerate,
        )
        .map_err(|_| StoreError::Denied)?;
        let mut keys: BTreeSet<_> = tx
            .snapshot
            .rows
            .keys()
            .chain(tx.writes.keys())
            .filter(|(scope, resource, _)| scope == &collection.0 && resource == kind)
            .cloned()
            .collect();
        let mut values = Vec::new();
        for key in std::mem::take(&mut keys) {
            match self.read(tx, kind, &key.2) {
                Ok(value) => values.push(value),
                Err(StoreError::NotFound | StoreError::Denied) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(values)
    }
    pub fn migrate(
        &mut self,
        permit: MigrationPermit,
        migration: StoreMigration,
    ) -> Result<(), StoreError> {
        use crate::migration::{MigrationMode, MigrationStep, validate_migration};
        if self.recovery {
            return Err(StoreError::RecoveryRequired);
        }
        let kind = &migration.resource.name;
        if permit.root != self.root
            || permit.resource != *kind
            || permit.from != migration.plan.from_version
            || permit.to != migration.plan.to_version
        {
            return Err(StoreError::Denied);
        }
        if migration.plan.mode != MigrationMode::Offline {
            return Err(StoreError::UnsupportedProfile);
        }
        validate_migration(&migration.plan).map_err(|_| StoreError::InvalidSchema)?;
        let resource_index = self
            .schema
            .resources
            .iter()
            .position(|r| &r.name == kind)
            .ok_or(StoreError::InvalidSchema)?;
        let data_index = self
            .registry
            .iter()
            .position(|s| &s.name == kind)
            .ok_or(StoreError::InvalidSchema)?;
        let old_data = &self.registry[data_index];
        let old_resource = &self.schema.resources[resource_index];
        if old_data.version != migration.plan.from_version
            || migration.data_schema.version != migration.plan.to_version
            || migration.data_schema.name != *kind
        {
            return Err(StoreError::InvalidSchema);
        }
        if old_resource.tenant_isolation != migration.resource.tenant_isolation
            || old_resource.managed_fields != migration.resource.managed_fields
        {
            return Err(StoreError::UnsupportedProfile);
        }
        for field in &old_data.fields {
            match migration.data_schema.fields.iter().find(|f| f.tag == field.tag) {
                None if !migration.plan.steps.iter().any(|s| matches!(s,MigrationStep::RemoveField {tag,allow_data_loss:true} if *tag == field.tag)) => return Err(StoreError::InvalidSchema),
                Some(new) => {
                    if new.ty != field.ty { return Err(StoreError::UnsupportedProfile) }
                    if new.name != field.name && !migration.plan.steps.iter().any(|s| matches!(s,MigrationStep::RenameField {tag} if *tag == field.tag)) { return Err(StoreError::InvalidSchema) }
                    if field.requirement == crate::data_format::FieldRequirement::Optional && new.requirement == crate::data_format::FieldRequirement::Required && !migration.plan.steps.iter().any(|s| matches!(s,MigrationStep::PromoteFieldToRequired {tag} if *tag == field.tag)) { return Err(StoreError::InvalidSchema) }
                    if old_resource.field(&field.name).unwrap().protection != migration.resource.field(&new.name).ok_or(StoreError::InvalidSchema)?.protection
                        && !migration.plan.steps.iter().any(|s| matches!(s,MigrationStep::RotateStorageProtection {profile,to_version:1} if profile == "g0.native.fields")) { return Err(StoreError::InvalidSchema) }
                }
                None => {},
            }
        }
        for field in &migration.data_schema.fields {
            if !old_data.fields.iter().any(|f| f.tag == field.tag) {
                if !migration
                    .plan
                    .steps
                    .iter()
                    .any(|s| matches!(s,MigrationStep::AddOptionalField {tag} if *tag == field.tag))
                {
                    return Err(StoreError::InvalidSchema);
                }
                if field.requirement == crate::data_format::FieldRequirement::Required
                    && (!migration.plan.steps.iter().any(|s| matches!(s,MigrationStep::BackfillField {tag} if *tag == field.tag)) || !migration.plan.steps.iter().any(|s| matches!(s,MigrationStep::PromoteFieldToRequired {tag} if *tag == field.tag))) { return Err(StoreError::InvalidSchema) }
            }
        }
        let mut schema = self.schema.clone();
        schema.resources[resource_index] = migration.resource.clone();
        let mut registry = self.registry.clone();
        registry[data_index] = migration.data_schema.clone();
        Self::validate_definition(&schema, &registry)?;
        if registry.iter().any(|s| s.name == migration.source_schema) {
            return Err(StoreError::InvalidSchema);
        }
        let mut source = old_data.clone();
        source.name = migration.source_schema.clone();
        if !migration.transform.schemas.contains(&source)
            || !registry
                .iter()
                .all(|s| migration.transform.schemas.contains(s))
        {
            return Err(StoreError::InvalidSchema);
        }
        let program = migration
            .transform
            .validated_contract()
            .map_err(|_| StoreError::InvalidSchema)?;
        if program
            .graphs
            .iter()
            .flat_map(|g| &g.nodes)
            .any(|n| !n.effects.is_empty() || !n.required_capabilities.is_empty())
        {
            return Err(StoreError::Denied);
        }
        let graph = program
            .graphs
            .iter()
            .find(|g| g.name == migration.transform.entry_graph)
            .ok_or(StoreError::InvalidSchema)?;
        if graph.inputs.len() != 1
            || graph.outputs.len() != 1
            || graph.inputs[0].ty != SemanticType::Record(migration.source_schema.clone())
            || graph.outputs[0].ty != SemanticType::Record(kind.clone())
        {
            return Err(StoreError::InvalidSchema);
        }
        let mut runtime = crate::execution::Executor::new(&program, migration.limits)
            .map_err(StoreError::Migration)?;
        runtime.set_cancellation(migration.cancellation.clone());
        let epoch = self.epoch.checked_add(1).ok_or(StoreError::Limit)?;
        let generation = self
            .snapshot
            .generation
            .checked_add(1)
            .ok_or(StoreError::Limit)?;
        let mut next = (*self.snapshot).clone();
        next.generation = generation;
        for ((_, resource, _), row) in next.rows.iter_mut() {
            if resource != kind {
                continue;
            }
            let Value::Record { fields, .. } = &row.value else {
                return Err(StoreError::Integrity);
            };
            let input = Value::Record {
                schema: migration.source_schema.clone(),
                fields: fields.clone(),
            };
            let mut outputs = runtime
                .run_graph_cumulative(&graph.name, vec![input])
                .map_err(StoreError::Migration)?;
            let value = outputs.pop().ok_or(StoreError::TypeMismatch)?;
            if !value.fits(&SemanticType::Record(kind.clone()), &registry) {
                return Err(StoreError::TypeMismatch);
            }
            let Value::Record {
                fields: new_fields, ..
            } = &value
            else {
                return Err(StoreError::TypeMismatch);
            };
            for managed in &old_resource.managed_fields {
                if fields.get(&managed.field) != new_fields.get(&managed.field) {
                    return Err(StoreError::Denied);
                }
            }
            for field in &migration.resource.fields {
                if field.protection == FieldProtection::Credential
                    && new_fields
                        .get(&field.name)
                        .is_some_and(|v| !matches!(v, Value::CredentialVerifier(_)))
                {
                    return Err(StoreError::TypeMismatch);
                }
            }
            row.value = value;
            row.version = generation;
        }
        if migration.cancellation.is_cancelled() {
            return Err(StoreError::Migration(
                crate::execution::RuntimeError::Cancelled,
            ));
        }
        let old_schema = std::mem::replace(&mut self.schema, schema);
        let old_registry = std::mem::replace(&mut self.registry, registry);
        match self.persist_snapshot(next) {
            Ok(()) => {
                self.epoch = epoch;
                Ok(())
            }
            Err(error) => {
                self.schema = old_schema;
                self.registry = old_registry;
                Err(error)
            }
        }
    }
    pub fn commit(&mut self, tx: Transaction) -> Result<(), StoreError> {
        self.check(&tx)?;
        if tx.snapshot.generation != self.snapshot.generation {
            return Err(StoreError::Conflict);
        }
        if tx.writes.is_empty() {
            return Ok(());
        }
        let mut next = (*self.snapshot).clone();
        next.generation = next.generation.checked_add(1).ok_or(StoreError::Limit)?;
        for (key, row) in tx.writes {
            if let Some(row) = row {
                next.rows.insert(key, row);
            } else {
                next.rows.remove(&key);
            }
        }
        self.persist_snapshot(next)
    }
    fn persist_snapshot(&mut self, next: Snapshot) -> Result<(), StoreError> {
        if next.rows.len() > MAX_ROWS {
            return Err(StoreError::Limit);
        }
        self.check_indexes(&next)?;
        self.check_relations(&next)?;
        let bytes = self.encode_snapshot(&next)?;
        let checkpoint = StoreCheckpoint {
            generation: next.generation,
            digest: ring::digest::digest(&ring::digest::SHA256, &bytes)
                .as_ref()
                .try_into()
                .unwrap(),
        };
        match atomic_snapshot(&self.root, &bytes) {
            Ok(()) => {
                if let Some(witness) = &self.witness
                    && witness
                        .compare_exchange(Some(&self.checkpoint), &checkpoint)
                        .is_err()
                {
                    self.recovery = true;
                    return Err(StoreError::CommitUncertain);
                }
                self.snapshot = Arc::new(next);
                self.checkpoint = checkpoint;
            }
            Err(StoreError::CommitUncertain) => {
                self.recovery = true;
                return Err(StoreError::CommitUncertain);
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }
    fn check_indexes(&self, snapshot: &Snapshot) -> Result<(), StoreError> {
        for resource in &self.schema.resources {
            for index in resource.indexes.iter().filter(|i| i.unique) {
                let mut seen: BTreeSet<(&str, Vec<&Value>)> = BTreeSet::new();
                for ((scope, kind, _), row) in &snapshot.rows {
                    if kind == &resource.name {
                        let Value::Record { fields, .. } = &row.value else {
                            return Err(StoreError::Integrity);
                        };
                        if let Some(values) = index
                            .fields
                            .iter()
                            .map(|f| fields.get(f))
                            .collect::<Option<Vec<_>>>()
                            && !seen.insert((scope, values))
                        {
                            return Err(StoreError::UniqueConstraint);
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn field_context(&self, key: &Key, name: &str) -> Result<Vec<u8>, StoreError> {
        let schema = self
            .registry
            .iter()
            .find(|s| s.name == key.1)
            .ok_or(StoreError::InvalidSchema)?;
        let field = schema
            .fields
            .iter()
            .find(|f| f.name == name)
            .ok_or(StoreError::InvalidSchema)?;
        let protection = self
            .schema
            .resource(&key.1)
            .and_then(|r| r.field(name))
            .ok_or(StoreError::InvalidSchema)?
            .protection;
        let mut context = Writer(Zeroizing::new(Vec::new()));
        context.string("g0.store.field.v1")?;
        for text in [
            self.root.to_string_lossy().as_ref(),
            &key.0,
            &key.1,
            &key.2,
            name,
        ] {
            context.string(text)?;
        }
        context.put(&schema.version.to_le_bytes())?;
        context.put(&field.tag.to_le_bytes())?;
        let ty = crate::graph_binary::encode_semantic_type(&field.ty)
            .map_err(|_| StoreError::InvalidSchema)?;
        context.blob(&ty)?;
        context.put(&[match protection {
            FieldProtection::Public => 0,
            FieldProtection::Private => 1,
            FieldProtection::Secret => 2,
            FieldProtection::Credential => 3,
        }])?;
        Ok(std::mem::take(&mut *context.0))
    }
    fn field_cipher(&self, context: &[u8]) -> Result<aead::LessSafeKey, StoreError> {
        let info = [context];
        let material = self
            .field_keys
            .expand(&info, &aead::AES_256_GCM)
            .map_err(|_| StoreError::Integrity)?;
        let mut key = Zeroizing::new([0u8; 32]);
        material
            .fill(key.as_mut())
            .map_err(|_| StoreError::Integrity)?;
        Ok(aead::LessSafeKey::new(
            aead::UnboundKey::new(&aead::AES_256_GCM, key.as_ref())
                .map_err(|_| StoreError::Integrity)?,
        ))
    }
    fn encode_fields(
        &self,
        writer: &mut Writer,
        key: &Key,
        value: &Value,
    ) -> Result<(), StoreError> {
        let Value::Record { fields, .. } = value else {
            return Err(StoreError::Integrity);
        };
        let resource = self
            .schema
            .resource(&key.1)
            .ok_or(StoreError::InvalidSchema)?;
        writer.count(fields.len())?;
        for (name, value) in fields.iter() {
            let field = resource.field(name).ok_or(StoreError::InvalidSchema)?;
            writer.string(name)?;
            if field.protection == FieldProtection::Credential {
                let Value::CredentialVerifier(verifier) = value else {
                    return Err(StoreError::Integrity);
                };
                writer.blob(&verifier.encode())?;
                continue;
            }
            let (value, ty) = match (value, &field.ty, field.protection) {
                (Value::Secret(value), SemanticType::Secret(ty), FieldProtection::Secret) => {
                    (value.as_ref(), ty.as_ref())
                }
                (_, _, FieldProtection::Secret | FieldProtection::Credential) => {
                    return Err(StoreError::TypeMismatch);
                }
                (value, ty, _) => (value, ty),
            };
            let mut bytes = Zeroizing::new(encode_value(
                value,
                ty,
                &self.registry,
                CodecLimits {
                    max_bytes: MAX_BYTES.saturating_sub(writer.0.len() + 64),
                    ..Default::default()
                },
            )?);
            if field.protection != FieldProtection::Public {
                let context = self.field_context(key, name)?;
                let cipher = self.field_cipher(&context)?;
                let mut nonce = [0u8; 12];
                SystemRandom::new()
                    .fill(&mut nonce)
                    .map_err(|_| StoreError::Entropy)?;
                cipher
                    .seal_in_place_append_tag(
                        aead::Nonce::assume_unique_for_key(nonce),
                        aead::Aad::from(context),
                        &mut *bytes,
                    )
                    .map_err(|_| StoreError::Integrity)?;
                writer.put(&nonce)?;
            }
            writer.blob(&bytes)?;
        }
        Ok(())
    }
    fn decode_fields(&self, reader: &mut Reader<'_>, key: &Key) -> Result<Value, StoreError> {
        let resource = self
            .schema
            .resource(&key.1)
            .ok_or(StoreError::InvalidSchema)?;
        let count = reader.count()?;
        if count > resource.fields.len() || count > reader.0.len() / 8 {
            return Err(StoreError::Integrity);
        }
        let mut fields = BTreeMap::new();
        let mut previous: Option<String> = None;
        for _ in 0..count {
            let name = reader.string()?;
            if previous.as_ref().is_some_and(|p| p >= &name) {
                return Err(StoreError::Integrity);
            }
            previous = Some(name.clone());
            let field = resource.field(&name).ok_or(StoreError::InvalidSchema)?;
            if field.protection == FieldProtection::Credential {
                let verifier = crate::credential::Verifier::decode(reader.blob()?)?;
                fields.insert(name, Value::CredentialVerifier(verifier));
                continue;
            }
            let ty = match (&field.ty, field.protection) {
                (SemanticType::Secret(ty), FieldProtection::Secret) => ty.as_ref(),
                (_, FieldProtection::Credential) => return Err(StoreError::UnsupportedProfile),
                (ty, _) => ty,
            };
            let value = if field.protection == FieldProtection::Public {
                decode_value(reader.blob()?, ty, &self.registry, CodecLimits::default())?
            } else {
                let nonce = reader
                    .take(12)?
                    .try_into()
                    .map_err(|_| StoreError::Integrity)?;
                let mut bytes = Zeroizing::new(reader.blob()?.to_vec());
                let context = self.field_context(key, &name)?;
                let plaintext = self
                    .field_cipher(&context)?
                    .open_in_place(
                        aead::Nonce::assume_unique_for_key(nonce),
                        aead::Aad::from(context),
                        &mut bytes,
                    )
                    .map_err(|_| StoreError::Integrity)?;
                decode_value(plaintext, ty, &self.registry, CodecLimits::default())?
            };
            fields.insert(
                name,
                if field.protection == FieldProtection::Secret {
                    Value::Secret(Arc::new(value))
                } else {
                    value
                },
            );
        }
        let value = Value::Record {
            schema: key.1.clone(),
            fields: Arc::new(fields),
        };
        if !value.fits(&SemanticType::Record(key.1.clone()), &self.registry) {
            return Err(StoreError::Integrity);
        }
        Ok(value)
    }
    fn schema_digest(&self) -> Result<[u8; 32], StoreError> {
        let mut writer = Writer(Zeroizing::new(Vec::new()));
        writer.string("g0.store.schemas.v1")?;
        let mut schemas: Vec<_> = self.registry.iter().collect();
        schemas.sort_by(|a, b| a.name.cmp(&b.name));
        writer.count(schemas.len())?;
        for schema in schemas {
            writer.string(&schema.name)?;
            writer.put(&schema.version.to_le_bytes())?;
            writer.count(schema.fields.len())?;
            for field in &schema.fields {
                writer.put(&field.tag.to_le_bytes())?;
                writer.string(&field.name)?;
                writer.blob(
                    &crate::graph_binary::encode_semantic_type(&field.ty)
                        .map_err(|_| StoreError::InvalidSchema)?,
                )?;
                writer.put(&[match field.requirement {
                    crate::data_format::FieldRequirement::Required => 0,
                    crate::data_format::FieldRequirement::Optional => 1,
                }])?;
            }
        }
        let mut resources: Vec<_> = self.schema.resources.iter().collect();
        resources.sort_by(|a, b| a.name.cmp(&b.name));
        writer.count(resources.len())?;
        for resource in resources {
            writer.string(&resource.name)?;
            writer.put(&[u8::from(
                resource.tenant_isolation == TenantIsolation::Global,
            )])?;
            let mut fields: Vec<_> = resource.fields.iter().collect();
            fields.sort_by(|a, b| a.name.cmp(&b.name));
            writer.count(fields.len())?;
            for field in fields {
                writer.string(&field.name)?;
                writer.put(&[match field.protection {
                    FieldProtection::Public => 0,
                    FieldProtection::Private => 1,
                    FieldProtection::Secret => 2,
                    FieldProtection::Credential => 3,
                }])?;
            }
            let mut managed: Vec<_> = resource.managed_fields.iter().collect();
            managed.sort_by(|a, b| a.field.cmp(&b.field));
            writer.count(managed.len())?;
            for field in managed {
                writer.string(&field.field)?;
                writer.put(&[match field.source {
                    ManagedFieldSource::CurrentPrincipal => 0,
                    ManagedFieldSource::CurrentScope => 1,
                    ManagedFieldSource::Generated => 2,
                    ManagedFieldSource::StoreClock => 3,
                }])?;
            }
            let mut relations: Vec<_> = resource.relations.iter().collect();
            relations.sort_by(|a, b| a.name.cmp(&b.name));
            writer.count(relations.len())?;
            for relation in relations {
                writer.string(&relation.name)?;
                writer.string(&relation.target_resource)?;
                writer.put(&[
                    match relation.cardinality {
                        crate::storage::Cardinality::One => 0,
                        crate::storage::Cardinality::OptionalOne => 1,
                        crate::storage::Cardinality::Many => 2,
                    },
                    match relation.on_delete {
                        crate::storage::DeleteRule::Restrict => 0,
                        crate::storage::DeleteRule::Cascade => 1,
                        crate::storage::DeleteRule::Detach => 2,
                    },
                ])?;
            }
            writer.count(resource.indexes.len())?;
            for index in &resource.indexes {
                writer.put(&[u8::from(index.unique)])?;
                writer.count(index.fields.len())?;
                for field in &index.fields {
                    writer.string(field)?;
                }
            }
        }
        Ok(ring::digest::digest(&ring::digest::SHA256, &writer.0)
            .as_ref()
            .try_into()
            .unwrap())
    }
    fn encode_snapshot(&self, snapshot: &Snapshot) -> Result<Vec<u8>, StoreError> {
        let mut writer = Writer(Zeroizing::new(Vec::new()));
        writer.put(&snapshot.generation.to_le_bytes())?;
        writer.put(&self.schema_digest()?)?;
        writer.count(snapshot.rows.len())?;
        for ((scope, kind, id), row) in &snapshot.rows {
            writer.string(scope)?;
            writer.string(kind)?;
            writer.string(id)?;
            writer.put(&row.version.to_le_bytes())?;
            writer.string(row.context.owner.as_ref().map_or("", |p| p.0.as_str()))?;
            self.encode_fields(
                &mut writer,
                &(scope.clone(), kind.clone(), id.clone()),
                &row.value,
            )?;
            writer.count(row.relations.len())?;
            for (name, targets) in &row.relations {
                writer.string(name)?;
                writer.count(targets.len())?;
                for (scope, kind, id) in targets {
                    writer.string(scope)?;
                    writer.string(kind)?;
                    writer.string(id)?;
                }
            }
        }
        let mut nonce = [0; 12];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| StoreError::Entropy)?;
        let mut bytes = writer.0;
        self.cipher
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(self.aad(4)),
                &mut *bytes,
            )
            .map_err(|_| StoreError::Integrity)?;
        let mut framed = b"G0S\0\0\0\x04\0".to_vec();
        framed.extend_from_slice(&nonce);
        framed.extend_from_slice(&bytes);
        Ok(framed)
    }
    fn aad(&self, version: u8) -> Vec<u8> {
        let mut aad = if version == 1 {
            b"g0.store.snapshot.v1\0".to_vec()
        } else if version == 2 {
            b"g0.store.snapshot.v2\0".to_vec()
        } else if version == 3 {
            b"g0.store.snapshot.v3\0".to_vec()
        } else {
            b"g0.store.snapshot.v4\0".to_vec()
        };
        aad.extend_from_slice(self.root.to_string_lossy().as_bytes());
        aad
    }
    fn decode_snapshot(&self, bytes: Vec<u8>) -> Result<Snapshot, StoreError> {
        let mut bytes = Zeroizing::new(bytes);
        if bytes.len() < 36
            || &bytes[..6] != b"G0S\0\0\0"
            || !(1..=4).contains(&bytes[6])
            || bytes[7] != 0
        {
            return Err(StoreError::Integrity);
        }
        let nonce: [u8; 12] = bytes[8..20].try_into().unwrap();
        let format_version = bytes[6];
        let plaintext = self
            .cipher
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(self.aad(format_version)),
                &mut bytes[20..],
            )
            .map_err(|_| StoreError::Integrity)?;
        let mut reader = Reader(plaintext);
        let generation = reader.u64()?;
        if format_version >= 4 && reader.take(32)? != self.schema_digest()? {
            return Err(StoreError::InvalidSchema);
        }
        let count = reader.count()?;
        if count > MAX_ROWS || count > reader.0.len() / 28 {
            return Err(StoreError::Integrity);
        }
        let mut rows = BTreeMap::new();
        let mut previous = None;
        let mut link_total = 0usize;
        for _ in 0..count {
            let scope = reader.string()?;
            let kind = reader.string()?;
            let id = reader.string()?;
            let version = reader.u64()?;
            let owner = reader.string()?;
            let key = (scope.clone(), kind.clone(), id.clone());
            if previous.as_ref().is_some_and(|p| p >= &key) || version == 0 || version > generation
            {
                return Err(StoreError::Integrity);
            }
            previous = Some(key.clone());
            let resource = self
                .schema
                .resource(&kind)
                .ok_or(StoreError::InvalidSchema)?;
            if (resource.tenant_isolation == TenantIsolation::Global) != scope.is_empty()
                || id.is_empty()
                || id.len() > 1024
                || scope.len() > 1024
                || owner.is_empty()
                || owner.len() > 1024
            {
                return Err(StoreError::Integrity);
            }
            let value = if format_version >= 3 {
                self.decode_fields(&mut reader, &key)?
            } else {
                decode_value(
                    reader.blob()?,
                    &SemanticType::Record(kind.clone()),
                    &self.registry,
                    CodecLimits::default(),
                )?
            };
            let mut relations = BTreeMap::new();
            if format_version >= 2 {
                let count = reader.count()?;
                if count > resource.relations.len() || count > reader.0.len() / 8 {
                    return Err(StoreError::Integrity);
                }
                let mut previous_name: Option<String> = None;
                for _ in 0..count {
                    let name = reader.string()?;
                    if previous_name.as_ref().is_some_and(|p| p >= &name) {
                        return Err(StoreError::Integrity);
                    }
                    previous_name = Some(name.clone());
                    let count = reader.count()?;
                    link_total = link_total
                        .checked_add(count)
                        .filter(|n| *n <= MAX_LINKS)
                        .ok_or(StoreError::Limit)?;
                    if count > reader.0.len() / 12 {
                        return Err(StoreError::Integrity);
                    }
                    let mut targets = BTreeSet::new();
                    let mut previous_target = None;
                    for _ in 0..count {
                        let key = (reader.string()?, reader.string()?, reader.string()?);
                        if previous_target.as_ref().is_some_and(|p| p >= &key) {
                            return Err(StoreError::Integrity);
                        }
                        previous_target = Some(key.clone());
                        targets.insert(key);
                    }
                    relations.insert(name, targets);
                }
            }
            let mut context = ResourceContext::new(kind, id, scope);
            context.owner = Some(PrincipalId::new(owner));
            rows.insert(
                key,
                Row {
                    context,
                    value,
                    version,
                    relations,
                },
            );
        }
        if !reader.0.is_empty() {
            return Err(StoreError::Integrity);
        }
        let snapshot = Snapshot { generation, rows };
        self.check_indexes(&snapshot)?;
        self.check_relations(&snapshot)?;
        Ok(snapshot)
    }
}

fn entity_key(key: &Key) -> crate::storage_integrity::EntityKey {
    crate::storage_integrity::EntityKey {
        scope: key.0.clone(),
        resource: key.1.clone(),
        id: key.2.clone(),
    }
}
fn store_key(key: &crate::storage_integrity::EntityKey) -> Key {
    (key.scope.clone(), key.resource.clone(), key.id.clone())
}

struct Writer(Zeroizing<Vec<u8>>);
impl Writer {
    fn put(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        if self
            .0
            .len()
            .checked_add(bytes.len())
            .is_none_or(|v| v > MAX_BYTES - 36)
        {
            return Err(StoreError::Limit);
        }
        self.0.extend_from_slice(bytes);
        Ok(())
    }
    fn count(&mut self, n: usize) -> Result<(), StoreError> {
        self.put(
            &u32::try_from(n)
                .map_err(|_| StoreError::Limit)?
                .to_le_bytes(),
        )
    }
    fn blob(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        self.count(bytes.len())?;
        self.put(bytes)
    }
    fn string(&mut self, text: &str) -> Result<(), StoreError> {
        self.blob(text.as_bytes())
    }
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], StoreError> {
        if n > self.0.len() {
            return Err(StoreError::Integrity);
        }
        let (v, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(v)
    }
    fn u64(&mut self) -> Result<u64, StoreError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn count(&mut self) -> Result<usize, StoreError> {
        usize::try_from(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
            .map_err(|_| StoreError::Limit)
    }
    fn blob(&mut self) -> Result<&'a [u8], StoreError> {
        let n = self.count()?;
        self.take(n)
    }
    fn string(&mut self) -> Result<String, StoreError> {
        let bytes = self.blob()?;
        if bytes.len() > 4096 {
            return Err(StoreError::Limit);
        }
        Ok(std::str::from_utf8(bytes)
            .map_err(|_| StoreError::Integrity)?
            .into())
    }
}

fn atomic_snapshot(root: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let mut random = [0; 8];
    SystemRandom::new()
        .fill(&mut random)
        .map_err(|_| StoreError::Entropy)?;
    let temporary = root.join(format!(
        ".g0-snapshot-{}-{}.tmp",
        std::process::id(),
        u64::from_le_bytes(random)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        durable_replace(&temporary, &root.join("snapshot.g0s"))?;
        #[cfg(unix)]
        {
            File::open(root)?
                .sync_all()
                .map_err(|_| StoreError::CommitUncertain)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn durable_replace(source: &Path, destination: &Path) -> Result<(), StoreError> {
    fs::rename(source, destination).map_err(StoreError::Io)
}
#[cfg(windows)]
fn durable_replace(source: &Path, destination: &Path) -> Result<(), StoreError> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(source: *const u16, destination: *const u16, flags: u32) -> i32;
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if source[..source.len() - 1].contains(&0) || destination[..destination.len() - 1].contains(&0)
    {
        return Err(StoreError::Denied);
    }
    // Live nul-terminated buffers, REPLACE_EXISTING | WRITE_THROUGH.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 1 | 8) } == 0 {
        return Err(StoreError::CommitUncertain);
    }
    Ok(())
}
