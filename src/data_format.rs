use std::collections::BTreeSet;

use crate::gir::SemanticType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireEndianness {
    Little,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalVarInt {
    Minimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFormatProfile {
    pub id: String,
    pub endianness: WireEndianness,
    pub canonical_varint: CanonicalVarInt,
    pub reject_duplicate_fields: bool,
    pub reject_noncanonical_order: bool,
    pub reject_unknown_required_fields: bool,
}

impl DataFormatProfile {
    pub fn canonical() -> Self {
        Self {
            id: "g0.data.canonical.v1".into(),
            endianness: WireEndianness::Little,
            canonical_varint: CanonicalVarInt::Minimal,
            reject_duplicate_fields: true,
            reject_noncanonical_order: true,
            reject_unknown_required_fields: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldRequirement {
    Required,
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaField {
    pub tag: u32,
    pub name: String,
    pub ty: SemanticType,
    pub requirement: FieldRequirement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSchema {
    pub name: String,
    pub version: u32,
    pub fields: Vec<SchemaField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaIssue {
    ZeroVersion,
    ZeroTag(String),
    DuplicateTag(u32),
    DuplicateName(String),
    NonCanonicalFieldOrder,
}

pub fn validate_schema(schema: &DataSchema) -> Result<(), Vec<SchemaIssue>> {
    let mut issues = Vec::new();
    let mut tags = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut previous_tag = 0_u32;

    if schema.version == 0 {
        issues.push(SchemaIssue::ZeroVersion);
    }

    for field in &schema.fields {
        if field.tag == 0 {
            issues.push(SchemaIssue::ZeroTag(field.name.clone()));
        }
        if !tags.insert(field.tag) {
            issues.push(SchemaIssue::DuplicateTag(field.tag));
        }
        if !names.insert(field.name.as_str()) {
            issues.push(SchemaIssue::DuplicateName(field.name.clone()));
        }
        if previous_tag != 0 && field.tag <= previous_tag {
            issues.push(SchemaIssue::NonCanonicalFieldOrder);
        }
        previous_tag = field.tag;
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaCompatibility {
    Compatible,
    Breaking(Vec<CompatibilityIssue>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatibilityIssue {
    FieldRemoved {
        tag: u32,
        name: String,
    },
    FieldTypeChanged {
        tag: u32,
        name: String,
    },
    RequiredFieldAdded {
        tag: u32,
        name: String,
    },
    TagReused {
        tag: u32,
        old_name: String,
        new_name: String,
    },
}

pub fn check_schema_evolution(
    old: &DataSchema,
    new: &DataSchema,
) -> SchemaCompatibility {
    let mut issues = Vec::new();

    for old_field in &old.fields {
        match new.fields.iter().find(|field| field.tag == old_field.tag) {
            None => issues.push(CompatibilityIssue::FieldRemoved {
                tag: old_field.tag,
                name: old_field.name.clone(),
            }),
            Some(new_field) => {
                if new_field.name != old_field.name {
                    issues.push(CompatibilityIssue::TagReused {
                        tag: old_field.tag,
                        old_name: old_field.name.clone(),
                        new_name: new_field.name.clone(),
                    });
                }
                if new_field.ty != old_field.ty {
                    issues.push(CompatibilityIssue::FieldTypeChanged {
                        tag: old_field.tag,
                        name: old_field.name.clone(),
                    });
                }
            }
        }
    }

    for new_field in &new.fields {
        if old.fields.iter().all(|field| field.tag != new_field.tag)
            && new_field.requirement == FieldRequirement::Required
        {
            issues.push(CompatibilityIssue::RequiredFieldAdded {
                tag: new_field.tag,
                name: new_field.name.clone(),
            });
        }
    }

    if issues.is_empty() {
        SchemaCompatibility::Compatible
    } else {
        SchemaCompatibility::Breaking(issues)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataBoundaryFormat {
    G0Canonical,
    JsonAdapter,
    ExternalAdapter(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(version: u32) -> DataSchema {
        DataSchema {
            name: "User".into(),
            version,
            fields: vec![
                SchemaField {
                    tag: 1,
                    name: "id".into(),
                    ty: SemanticType::Text,
                    requirement: FieldRequirement::Required,
                },
                SchemaField {
                    tag: 2,
                    name: "name".into(),
                    ty: SemanticType::Text,
                    requirement: FieldRequirement::Optional,
                },
            ],
        }
    }

    #[test]
    fn canonical_schema_requires_stable_unique_tags() {
        assert!(validate_schema(&schema(1)).is_ok());
    }

    #[test]
    fn optional_field_can_be_added_compatibly() {
        let old = schema(1);
        let mut new = schema(2);
        new.fields.push(SchemaField {
            tag: 3,
            name: "avatar".into(),
            ty: SemanticType::Bytes,
            requirement: FieldRequirement::Optional,
        });

        assert_eq!(
            check_schema_evolution(&old, &new),
            SchemaCompatibility::Compatible
        );
    }

    #[test]
    fn required_field_addition_is_breaking() {
        let old = schema(1);
        let mut new = schema(2);
        new.fields.push(SchemaField {
            tag: 3,
            name: "tenant".into(),
            ty: SemanticType::Text,
            requirement: FieldRequirement::Required,
        });

        assert!(matches!(
            check_schema_evolution(&old, &new),
            SchemaCompatibility::Breaking(_)
        ));
    }

    #[test]
    fn json_is_an_adapter_not_internal_semantics() {
        let format = DataBoundaryFormat::JsonAdapter;
        assert_ne!(format, DataBoundaryFormat::G0Canonical);
    }
}
