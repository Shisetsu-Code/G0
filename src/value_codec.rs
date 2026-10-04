//! G0V 0.1: schema-directed canonical values, shared by storage and transport.
//! No secret/credential serialization. Lengths, depth and value counts are bounded.
use crate::{
    data_format::{DataSchema, FieldRequirement, validate_schema},
    gir::SemanticType,
    value::Value,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Debug, Clone, Copy)]
pub struct CodecLimits {
    pub max_bytes: usize,
    pub max_values: usize,
    pub max_depth: usize,
}
impl Default for CodecLimits {
    fn default() -> Self {
        Self {
            max_bytes: 64 * 1024 * 1024,
            max_values: 1_000_000,
            max_depth: 128,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    Truncated,
    InvalidHeader,
    TypeMismatch,
    InvalidTag,
    InvalidUtf8,
    SchemaVersion,
    InvalidSchema,
    NonCanonical,
    TrailingBytes,
    Sensitive,
    Unsupported,
    Limit,
}

fn validate_registry(schemas: &[DataSchema]) -> Result<(), CodecError> {
    let mut names = BTreeSet::new();
    for s in schemas {
        if s.name.is_empty() || !names.insert(&s.name) || validate_schema(s).is_err() {
            return Err(CodecError::InvalidSchema);
        }
    }
    Ok(())
}

pub fn validate_public_type(ty: &SemanticType, schemas: &[DataSchema]) -> Result<(), CodecError> {
    validate_registry(schemas)?;
    public_type(ty, schemas, &mut BTreeSet::new(), 0)
}

/// Checks the type, including absent optional fields, so a wrapper cannot hide
/// a secret type from a public serialization boundary.
fn public_type(
    ty: &SemanticType,
    schemas: &[DataSchema],
    visited: &mut BTreeSet<String>,
    depth: usize,
) -> Result<(), CodecError> {
    if depth >= 128 {
        return Err(CodecError::Limit);
    }
    match ty {
        SemanticType::Secret(_) | SemanticType::Credential(_) => return Err(CodecError::Sensitive),
        SemanticType::Array(t, _)
        | SemanticType::Vector(t, _)
        | SemanticType::Slice(t)
        | SemanticType::Option(t) => public_type(t, schemas, visited, depth + 1)?,
        SemanticType::Result(a, b) => {
            public_type(a, schemas, visited, depth + 1)?;
            public_type(b, schemas, visited, depth + 1)?;
        }
        SemanticType::Record(name) | SemanticType::Variant(name) => {
            let s = schemas
                .iter()
                .find(|s| &s.name == name)
                .ok_or(CodecError::InvalidSchema)?;
            if visited.insert(name.clone()) {
                for field in &s.fields {
                    public_type(&field.ty, schemas, visited, depth + 1)?;
                }
            }
        }
        SemanticType::Bool
        | SemanticType::Integer(_)
        | SemanticType::Text
        | SemanticType::Bytes => {}
        _ => return Err(CodecError::Unsupported),
    }
    Ok(())
}

pub fn encode_value(
    value: &Value,
    ty: &SemanticType,
    schemas: &[DataSchema],
    limits: CodecLimits,
) -> Result<Vec<u8>, CodecError> {
    validate_registry(schemas)?;
    public_type(ty, schemas, &mut BTreeSet::new(), 0)?;
    if !value.fits(ty, schemas) {
        return Err(CodecError::TypeMismatch);
    }
    let type_bytes =
        crate::graph_binary::encode_semantic_type(ty).map_err(|_| CodecError::TypeMismatch)?;
    let mut writer = Writer {
        bytes: Vec::new(),
        limits,
        count: 0,
    };
    writer.put(b"G0V\0\0\0\x01\0")?;
    writer.blob(&type_bytes)?;
    writer.value(value, ty, schemas, 0)?;
    Ok(writer.bytes)
}

pub fn decode_value(
    bytes: &[u8],
    ty: &SemanticType,
    schemas: &[DataSchema],
    limits: CodecLimits,
) -> Result<Value, CodecError> {
    if bytes.len() > limits.max_bytes {
        return Err(CodecError::Limit);
    }
    validate_registry(schemas)?;
    public_type(ty, schemas, &mut BTreeSet::new(), 0)?;
    let mut reader = Reader {
        remaining: bytes,
        limits,
        count: 0,
        allocated: 0,
    };
    if reader.take(8)? != b"G0V\0\0\0\x01\0" {
        return Err(CodecError::InvalidHeader);
    }
    let encoded = reader.blob()?;
    let decoded = crate::graph_binary_decode::decode_semantic_type(encoded)
        .map_err(|_| CodecError::TypeMismatch)?;
    if &decoded != ty {
        return Err(CodecError::TypeMismatch);
    }
    let value = reader.value(ty, schemas, 0)?;
    if !reader.remaining.is_empty() {
        return Err(CodecError::TrailingBytes);
    }
    if !value.fits(ty, schemas) {
        return Err(CodecError::TypeMismatch);
    }
    Ok(value)
}

struct Writer {
    bytes: Vec<u8>,
    limits: CodecLimits,
    count: usize,
}
impl Writer {
    fn put(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|v| v > self.limits.max_bytes)
        {
            return Err(CodecError::Limit);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    fn length(&mut self, len: usize) -> Result<(), CodecError> {
        self.put(
            &u32::try_from(len)
                .map_err(|_| CodecError::Limit)?
                .to_le_bytes(),
        )
    }
    fn blob(&mut self, blob: &[u8]) -> Result<(), CodecError> {
        self.length(blob.len())?;
        self.put(blob)
    }
    fn value(
        &mut self,
        value: &Value,
        ty: &SemanticType,
        schemas: &[DataSchema],
        depth: usize,
    ) -> Result<(), CodecError> {
        if depth >= self.limits.max_depth || self.count >= self.limits.max_values {
            return Err(CodecError::Limit);
        }
        self.count += 1;
        match (value, ty) {
            (Value::Bool(v), SemanticType::Bool) => self.put(&[u8::from(*v)])?,
            (Value::Integer(v), SemanticType::Integer(_)) => self.put(&v.to_le_bytes())?,
            (Value::Text(v), SemanticType::Text) => self.blob(v.as_bytes())?,
            (Value::Bytes(v), SemanticType::Bytes) => self.blob(v)?,
            (
                Value::Array(v),
                SemanticType::Array(t, _) | SemanticType::Vector(t, _) | SemanticType::Slice(t),
            ) => {
                self.length(v.len())?;
                for v in v.iter() {
                    self.value(v, t, schemas, depth + 1)?;
                }
            }
            (Value::Record { schema, fields }, SemanticType::Record(_)) => {
                let s = schemas
                    .iter()
                    .find(|s| &s.name == schema)
                    .ok_or(CodecError::InvalidSchema)?;
                self.put(&s.version.to_le_bytes())?;
                self.length(fields.len())?;
                for field in &s.fields {
                    if let Some(v) = fields.get(&field.name) {
                        self.put(&field.tag.to_le_bytes())?;
                        self.value(v, &field.ty, schemas, depth + 1)?;
                    }
                }
            }
            (
                Value::Variant {
                    schema,
                    tag,
                    payload,
                },
                SemanticType::Variant(_),
            ) => {
                let s = schemas
                    .iter()
                    .find(|s| &s.name == schema)
                    .ok_or(CodecError::InvalidSchema)?;
                let field = s
                    .fields
                    .iter()
                    .find(|f| &f.name == tag)
                    .ok_or(CodecError::InvalidSchema)?;
                self.put(&s.version.to_le_bytes())?;
                self.put(&field.tag.to_le_bytes())?;
                self.value(payload, &field.ty, schemas, depth + 1)?;
            }
            (Value::Option(v), SemanticType::Option(t)) => {
                self.put(&[u8::from(v.is_some())])?;
                if let Some(v) = v {
                    self.value(v, t, schemas, depth + 1)?;
                }
            }
            (Value::Result(v), SemanticType::Result(ok, err)) => match v {
                Ok(v) => {
                    self.put(&[0])?;
                    self.value(v, ok, schemas, depth + 1)?;
                }
                Err(v) => {
                    self.put(&[1])?;
                    self.value(v, err, schemas, depth + 1)?;
                }
            },
            _ => return Err(CodecError::TypeMismatch),
        }
        Ok(())
    }
}

struct Reader<'a> {
    remaining: &'a [u8],
    limits: CodecLimits,
    count: usize,
    allocated: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], CodecError> {
        if len > self.remaining.len() {
            return Err(CodecError::Truncated);
        }
        let (value, rest) = self.remaining.split_at(len);
        self.remaining = rest;
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn length(&mut self) -> Result<usize, CodecError> {
        usize::try_from(self.u32()?).map_err(|_| CodecError::Limit)
    }
    fn blob(&mut self) -> Result<&'a [u8], CodecError> {
        let length = self.length()?;
        self.take(length)
    }
    fn charge(&mut self, bytes: usize) -> Result<(), CodecError> {
        self.allocated = self
            .allocated
            .checked_add(bytes)
            .filter(|v| *v <= self.limits.max_bytes)
            .ok_or(CodecError::Limit)?;
        Ok(())
    }
    fn value(
        &mut self,
        ty: &SemanticType,
        schemas: &[DataSchema],
        depth: usize,
    ) -> Result<Value, CodecError> {
        if depth >= self.limits.max_depth || self.count >= self.limits.max_values {
            return Err(CodecError::Limit);
        }
        self.count += 1;
        self.charge(std::mem::size_of::<Value>())?;
        Ok(match ty {
            SemanticType::Bool => Value::Bool(match self.take(1)?[0] {
                0 => false,
                1 => true,
                _ => return Err(CodecError::InvalidTag),
            }),
            SemanticType::Integer(range) => {
                let v = i128::from_le_bytes(self.take(16)?.try_into().unwrap());
                if v < range.min || v > range.max {
                    return Err(CodecError::TypeMismatch);
                }
                Value::Integer(v)
            }
            SemanticType::Text => {
                let blob = self.blob()?;
                self.charge(blob.len())?;
                Value::Text(
                    std::str::from_utf8(blob)
                        .map_err(|_| CodecError::InvalidUtf8)?
                        .into(),
                )
            }
            SemanticType::Bytes => {
                let blob = self.blob()?;
                self.charge(blob.len())?;
                Value::Bytes(blob.into())
            }
            SemanticType::Array(t, _) | SemanticType::Vector(t, _) | SemanticType::Slice(t) => {
                let len = self.length()?;
                if let SemanticType::Array(_, expected) | SemanticType::Vector(_, expected) = ty
                    && len != *expected
                {
                    return Err(CodecError::TypeMismatch);
                }
                if len > self.remaining.len()
                    || len > self.limits.max_values.saturating_sub(self.count)
                {
                    return Err(CodecError::Limit);
                }
                self.charge(
                    len.checked_mul(std::mem::size_of::<Value>())
                        .ok_or(CodecError::Limit)?,
                )?;
                let mut values = Vec::with_capacity(len);
                for _ in 0..len {
                    values.push(self.value(t, schemas, depth + 1)?);
                }
                Value::Array(values.into())
            }
            SemanticType::Record(name) => {
                let s = schemas
                    .iter()
                    .find(|s| &s.name == name)
                    .ok_or(CodecError::InvalidSchema)?;
                if self.u32()? != s.version {
                    return Err(CodecError::SchemaVersion);
                }
                let len = self.length()?;
                if len > s.fields.len() {
                    return Err(CodecError::InvalidSchema);
                }
                let mut fields = BTreeMap::new();
                let mut previous = 0;
                for _ in 0..len {
                    let tag = self.u32()?;
                    if tag <= previous {
                        return Err(CodecError::NonCanonical);
                    }
                    previous = tag;
                    let field = s
                        .fields
                        .iter()
                        .find(|f| f.tag == tag)
                        .ok_or(CodecError::InvalidSchema)?;
                    self.charge(field.name.len() + 128)?;
                    fields.insert(
                        field.name.clone(),
                        self.value(&field.ty, schemas, depth + 1)?,
                    );
                }
                if s.fields.iter().any(|f| {
                    f.requirement == FieldRequirement::Required && !fields.contains_key(&f.name)
                }) {
                    return Err(CodecError::InvalidSchema);
                }
                self.charge(name.len())?;
                Value::Record {
                    schema: name.clone(),
                    fields: Arc::new(fields),
                }
            }
            SemanticType::Variant(name) => {
                let s = schemas
                    .iter()
                    .find(|s| &s.name == name)
                    .ok_or(CodecError::InvalidSchema)?;
                if self.u32()? != s.version {
                    return Err(CodecError::SchemaVersion);
                }
                let tag = self.u32()?;
                let field = s
                    .fields
                    .iter()
                    .find(|f| f.tag == tag)
                    .ok_or(CodecError::InvalidSchema)?;
                self.charge(name.len() + field.name.len())?;
                Value::Variant {
                    schema: name.clone(),
                    tag: field.name.clone(),
                    payload: Arc::new(self.value(&field.ty, schemas, depth + 1)?),
                }
            }
            SemanticType::Option(t) => Value::Option(match self.take(1)?[0] {
                0 => None,
                1 => Some(Arc::new(self.value(t, schemas, depth + 1)?)),
                _ => return Err(CodecError::InvalidTag),
            }),
            SemanticType::Result(ok, err) => Value::Result(match self.take(1)?[0] {
                0 => Ok(Arc::new(self.value(ok, schemas, depth + 1)?)),
                1 => Err(Arc::new(self.value(err, schemas, depth + 1)?)),
                _ => return Err(CodecError::InvalidTag),
            }),
            _ => return Err(CodecError::Unsupported),
        })
    }
}
