//! Runtime values. Shared immutable payloads make graph fan-out explicit and
//! cheap; mutable cells belong to a region, not to these values.
use crate::{
    data_format::{DataSchema, FieldRequirement},
    gir::{Literal, SemanticType},
};
use std::{collections::BTreeMap, fmt, sync::Arc};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    Bool(bool),
    Integer(i128),
    Text(Arc<str>),
    Bytes(Arc<[u8]>),
    Array(Arc<[Value]>),
    Record {
        schema: String,
        fields: Arc<BTreeMap<String, Value>>,
    },
    Variant {
        schema: String,
        tag: String,
        payload: Arc<Value>,
    },
    Option(Option<Arc<Value>>),
    Result(Result<Arc<Value>, Arc<Value>>),
    Secret(Arc<Value>),
    Credential(Arc<Value>),
    CredentialVerifier(crate::credential::Verifier),
    NativeHandle(crate::resource_host::NativeHandle),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Secret(_) => f.write_str("Secret(<redacted>)"),
            Self::Credential(_) => f.write_str("Credential(<redacted>)"),
            Self::CredentialVerifier(v) => v.fmt(f),
            Self::NativeHandle(handle) => handle.fmt(f),
            Self::Bool(v) => v.fmt(f),
            Self::Integer(v) => v.fmt(f),
            Self::Text(v) => v.fmt(f),
            Self::Bytes(v) => v.fmt(f),
            Self::Array(v) => v.fmt(f),
            Self::Record { schema, fields } => {
                f.debug_struct(schema).field("fields", fields).finish()
            }
            Self::Variant {
                schema,
                tag,
                payload,
            } => f
                .debug_struct(schema)
                .field("tag", tag)
                .field("payload", payload)
                .finish(),
            Self::Option(v) => v.fmt(f),
            Self::Result(v) => v.fmt(f),
        }
    }
}

impl From<&Literal> for Value {
    fn from(literal: &Literal) -> Self {
        match literal {
            Literal::Bool(v) => Self::Bool(*v),
            Literal::Integer(v) => Self::Integer(*v),
            Literal::Text(v) => Self::Text(v.as_str().into()),
            Literal::Bytes(v) => Self::Bytes(v.as_slice().into()),
        }
    }
}

impl Value {
    pub(crate) fn contains_native_handles(&self) -> bool {
        self.contains_handles_at(0, &mut 0)
    }
    fn contains_handles_at(&self, depth: usize, visited: &mut usize) -> bool {
        if depth >= 128 || *visited >= 1_000_000 {
            return true;
        }
        *visited += 1;
        match self {
            Self::NativeHandle(_) => true,
            Self::Array(v) => v.iter().any(|v| v.contains_handles_at(depth + 1, visited)),
            Self::Record { fields, .. } => fields
                .values()
                .any(|v| v.contains_handles_at(depth + 1, visited)),
            Self::Variant { payload, .. }
            | Self::Option(Some(payload))
            | Self::Result(Ok(payload))
            | Self::Result(Err(payload))
            | Self::Secret(payload)
            | Self::Credential(payload) => payload.contains_handles_at(depth + 1, visited),
            _ => false,
        }
    }
    pub fn fits(&self, ty: &SemanticType, schemas: &[DataSchema]) -> bool {
        self.fits_at(ty, schemas, 0, &mut 0)
    }

    fn fits_at(
        &self,
        ty: &SemanticType,
        schemas: &[DataSchema],
        depth: usize,
        visited: &mut usize,
    ) -> bool {
        if depth >= 128 || *visited >= 1_000_000 {
            return false;
        }
        *visited += 1;
        let mut fits = |v: &Value, t: &SemanticType| v.fits_at(t, schemas, depth + 1, visited);
        match (self, ty) {
            (Self::CredentialVerifier(v), SemanticType::Credential(t)) => v.fits(t),
            (Self::NativeHandle(handle), SemanticType::Unique(ty)) => {
                matches!(ty.as_ref(),SemanticType::Reference(name) if handle.kind() == name)
            }
            (Self::Bool(_), SemanticType::Bool)
            | (Self::Text(_), SemanticType::Text)
            | (Self::Bytes(_), SemanticType::Bytes) => true,
            (Self::Integer(v), SemanticType::Integer(t)) => t.min <= *v && *v <= t.max,
            (Self::Array(v), SemanticType::Array(t, len) | SemanticType::Vector(t, len)) => {
                v.len() == *len && v.iter().all(|v| fits(v, t))
            }
            (Self::Array(v), SemanticType::Slice(t)) => v.iter().all(|v| fits(v, t)),
            (Self::Record { schema, fields }, SemanticType::Record(name)) if schema == name => {
                schemas.iter().find(|s| &s.name == name).is_some_and(|s| {
                    fields
                        .keys()
                        .all(|name| s.fields.iter().any(|f| &f.name == name))
                        && s.fields.iter().all(|field| match fields.get(&field.name) {
                            Some(value) => fits(value, &field.ty),
                            None => field.requirement == FieldRequirement::Optional,
                        })
                })
            }
            (
                Self::Variant {
                    schema,
                    tag,
                    payload,
                },
                SemanticType::Variant(name),
            ) if schema == name => schemas
                .iter()
                .find(|s| &s.name == name)
                .and_then(|s| s.fields.iter().find(|f| &f.name == tag))
                .is_some_and(|f| fits(payload, &f.ty)),
            (Self::Option(None), SemanticType::Option(_)) => true,
            (Self::Option(Some(v)), SemanticType::Option(t)) => fits(v, t),
            (Self::Result(Ok(v)), SemanticType::Result(t, _))
            | (Self::Result(Err(v)), SemanticType::Result(_, t)) => fits(v, t),
            (Self::Secret(v), SemanticType::Secret(t))
            | (Self::Credential(v), SemanticType::Credential(t)) => fits(v, t),
            _ => false,
        }
    }

    /// Conservative resident size including nested payloads and container overhead.
    pub fn resident_bytes(&self) -> Option<u64> {
        self.resident_at(0, &mut 0)
    }

    fn resident_at(&self, depth: usize, visited: &mut usize) -> Option<u64> {
        if depth >= 128 || *visited >= 1_000_000 {
            return None;
        }
        *visited += 1;
        let base = std::mem::size_of::<Self>() as u64;
        let payload = match self {
            Self::Text(v) => Some(v.len() as u64),
            Self::Bytes(v) => Some(v.len() as u64),
            Self::Array(v) => v.iter().try_fold(0u64, |sum, v| {
                sum.checked_add(v.resident_at(depth + 1, visited)?)
            }),
            Self::Record { schema, fields } => {
                fields.iter().try_fold(schema.len() as u64, |sum, (k, v)| {
                    sum.checked_add(k.len() as u64)?
                        .checked_add(v.resident_at(depth + 1, visited)?)?
                        .checked_add(128)
                })
            }
            Self::Variant {
                schema,
                tag,
                payload,
            } => payload
                .resident_at(depth + 1, visited)?
                .checked_add(schema.len() as u64)?
                .checked_add(tag.len() as u64),
            Self::Option(Some(v))
            | Self::Result(Ok(v))
            | Self::Result(Err(v))
            | Self::Secret(v)
            | Self::Credential(v) => v.resident_at(depth + 1, visited),
            _ => Some(0),
        }?;
        base.checked_add(payload)
    }
}
