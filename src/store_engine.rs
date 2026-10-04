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
    aead,
    rand::{SecureRandom, SystemRandom},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ROWS: usize = 100_000;
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
    UnsupportedProfile,
    RecoveryRequired,
    CommitUncertain,
    Io(std::io::Error),
    Codec(CodecError),
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

pub struct NativeStore {
    root: PathBuf,
    _lock: File,
    schema: StoreSchema,
    registry: Vec<DataSchema>,
    cipher: aead::LessSafeKey,
    snapshot: Arc<Snapshot>,
    instance: u64,
    epoch: u64,
    recovery: bool,
}
impl NativeStore {
    pub fn open(
        permit: StorePermit,
        schema: StoreSchema,
        registry: Vec<DataSchema>,
        mut key: [u8; 32],
    ) -> Result<Self, StoreError> {
        validate_store_schema(&schema).map_err(|_| StoreError::InvalidSchema)?;
        let mut names = BTreeSet::new();
        for s in &registry {
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
            if !resource.relations.is_empty()
                || resource
                    .fields
                    .iter()
                    .any(|f| f.protection != FieldProtection::Public)
                || resource.managed_fields.iter().any(|m| {
                    matches!(
                        m.source,
                        ManagedFieldSource::Generated | ManagedFieldSource::StoreClock
                    )
                })
            {
                return Err(StoreError::UnsupportedProfile);
            }
            crate::value_codec::validate_public_type(
                &SemanticType::Record(resource.name.clone()),
                &registry,
            )?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(permit.root.join("store.lock"))?;
        lock.try_lock().map_err(|_| StoreError::Busy)?;
        let cipher =
            aead::UnboundKey::new(&aead::AES_256_GCM, &key).map_err(|_| StoreError::Integrity);
        for byte in &mut key {
            unsafe {
                std::ptr::write_volatile(byte, 0);
            }
        }
        std::sync::atomic::compiler_fence(Ordering::SeqCst);
        let cipher = aead::LessSafeKey::new(cipher?);
        let instance = INSTANCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .map_err(|_| StoreError::Limit)?;
        let mut store = Self {
            root: permit.root,
            _lock: lock,
            schema,
            registry,
            cipher,
            snapshot: Arc::new(Snapshot::default()),
            instance,
            epoch: 0,
            recovery: false,
        };
        let path = store.root.join("snapshot.g0s");
        match File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
                if bytes.len() > MAX_BYTES {
                    return Err(StoreError::Limit);
                }
                store.snapshot = Arc::new(store.decode_snapshot(bytes)?);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        Ok(store)
    }
    pub fn begin(&self, principal: Principal) -> Transaction {
        Transaction {
            store: self.instance,
            principal,
            epoch: self.epoch,
            snapshot: self.snapshot.clone(),
            writes: BTreeMap::new(),
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
        let row = Row {
            context,
            value,
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
    pub fn delete(&self, tx: &mut Transaction, kind: &str, id: &str) -> Result<(), StoreError> {
        self.check(tx)?;
        let key = self.key(&tx.principal, kind, id)?;
        let row = self.visible(tx, &key).ok_or(StoreError::NotFound)?;
        self.authorize(&tx.principal, row, StoreOperation::Delete)?;
        if tx.writes.len() >= MAX_ROWS {
            return Err(StoreError::Limit);
        }
        tx.writes.insert(key, None);
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
        if next.rows.len() > MAX_ROWS {
            return Err(StoreError::Limit);
        }
        self.check_indexes(&next)?;
        let bytes = self.encode_snapshot(&next)?;
        match atomic_snapshot(&self.root, &bytes) {
            Ok(()) => self.snapshot = Arc::new(next),
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
    fn encode_snapshot(&self, snapshot: &Snapshot) -> Result<Vec<u8>, StoreError> {
        let mut writer = Writer(Vec::new());
        writer.put(&snapshot.generation.to_le_bytes())?;
        writer.count(snapshot.rows.len())?;
        for ((scope, kind, id), row) in &snapshot.rows {
            writer.string(scope)?;
            writer.string(kind)?;
            writer.string(id)?;
            writer.put(&row.version.to_le_bytes())?;
            writer.string(row.context.owner.as_ref().map_or("", |p| p.0.as_str()))?;
            let bytes = encode_value(
                &row.value,
                &SemanticType::Record(kind.clone()),
                &self.registry,
                CodecLimits {
                    max_bytes: MAX_BYTES.saturating_sub(writer.0.len() + 64),
                    ..Default::default()
                },
            )?;
            writer.blob(&bytes)?;
        }
        let mut nonce = [0; 12];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| StoreError::Entropy)?;
        let mut bytes = writer.0;
        self.cipher
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(self.aad()),
                &mut bytes,
            )
            .map_err(|_| StoreError::Integrity)?;
        let mut framed = b"G0S\0\0\0\x01\0".to_vec();
        framed.extend_from_slice(&nonce);
        framed.extend_from_slice(&bytes);
        Ok(framed)
    }
    fn aad(&self) -> Vec<u8> {
        let mut aad = b"g0.store.snapshot.v1\0".to_vec();
        aad.extend_from_slice(self.root.to_string_lossy().as_bytes());
        aad
    }
    fn decode_snapshot(&self, mut bytes: Vec<u8>) -> Result<Snapshot, StoreError> {
        if bytes.len() < 36 || &bytes[..8] != b"G0S\0\0\0\x01\0" {
            return Err(StoreError::Integrity);
        }
        let nonce: [u8; 12] = bytes[8..20].try_into().unwrap();
        let plaintext = self
            .cipher
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(self.aad()),
                &mut bytes[20..],
            )
            .map_err(|_| StoreError::Integrity)?;
        let mut reader = Reader(plaintext);
        let generation = reader.u64()?;
        let count = reader.count()?;
        if count > MAX_ROWS || count > reader.0.len() / 28 {
            return Err(StoreError::Integrity);
        }
        let mut rows = BTreeMap::new();
        let mut previous = None;
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
            let value = decode_value(
                reader.blob()?,
                &SemanticType::Record(kind.clone()),
                &self.registry,
                CodecLimits::default(),
            )?;
            let mut context = ResourceContext::new(kind, id, scope);
            context.owner = Some(PrincipalId::new(owner));
            rows.insert(
                key,
                Row {
                    context,
                    value,
                    version,
                },
            );
        }
        if !reader.0.is_empty() {
            return Err(StoreError::Integrity);
        }
        let snapshot = Snapshot { generation, rows };
        self.check_indexes(&snapshot)?;
        Ok(snapshot)
    }
}

struct Writer(Vec<u8>);
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
