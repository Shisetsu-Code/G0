use crate::authority::Action;
use crate::secure_index::{ProtectedIndexMode, ProtectedIndexSpec};
use crate::storage::{IndexIntent, ResourceSchema, StoreSchema};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    Equal { field: String },
    Range { field: String },
    Relation { relation: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuerySpec {
    pub resource: String,
    pub predicates: Vec<Predicate>,
    pub projection: Vec<String>,
    pub limit: Option<u32>,
    pub allow_enumeration: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessPath {
    PrimaryLookup,
    Index {
        fields: Vec<String>,
        unique: bool,
    },
    RelationIndex {
        relation: String,
    },
    ProtectedEqualityIndex {
        field: String,
    },
    FullScan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryIssue {
    UnknownResource(String),
    UnknownField(String),
    UnknownRelation(String),
    ReadPolicyMissing,
    EnumerationPolicyMissing,
    FullScanNotExplicit,
    ZeroLimit,
}

pub fn validate_query(
    store: &StoreSchema,
    query: &QuerySpec,
) -> Result<(), Vec<QueryIssue>> {
    validate_query_with_protected_indexes(store, query, &[])
}

pub fn validate_query_with_protected_indexes(
    store: &StoreSchema,
    query: &QuerySpec,
    protected_indexes: &[ProtectedIndexSpec],
) -> Result<(), Vec<QueryIssue>> {
    let Some(resource) = store.resource(&query.resource) else {
        return Err(vec![QueryIssue::UnknownResource(query.resource.clone())]);
    };

    let mut issues = Vec::new();

    if resource.policies.rule_for(&Action::read()).is_none() {
        issues.push(QueryIssue::ReadPolicyMissing);
    }

    for field in &query.projection {
        if resource.field(field).is_none() {
            issues.push(QueryIssue::UnknownField(field.clone()));
        }
    }

    for predicate in &query.predicates {
        match predicate {
            Predicate::Equal { field } | Predicate::Range { field } => {
                if resource.field(field).is_none() {
                    issues.push(QueryIssue::UnknownField(field.clone()));
                }
            }
            Predicate::Relation { relation } => {
                if resource
                    .relations
                    .iter()
                    .all(|candidate| candidate.name != *relation)
                {
                    issues.push(QueryIssue::UnknownRelation(relation.clone()));
                }
            }
        }
    }

    if query.limit == Some(0) {
        issues.push(QueryIssue::ZeroLimit);
    }

    let path =
        choose_access_path_with_protected(resource, query, protected_indexes);
    if path == AccessPath::FullScan {
        if !query.allow_enumeration {
            issues.push(QueryIssue::FullScanNotExplicit);
        }
        if resource
            .policies
            .rule_for(&Action::enumerate())
            .is_none()
        {
            issues.push(QueryIssue::EnumerationPolicyMissing);
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn choose_access_path(
    resource: &ResourceSchema,
    query: &QuerySpec,
) -> AccessPath {
    choose_access_path_with_protected(resource, query, &[])
}

pub fn choose_access_path_with_protected(
    resource: &ResourceSchema,
    query: &QuerySpec,
    protected_indexes: &[ProtectedIndexSpec],
) -> AccessPath {
    for predicate in &query.predicates {
        if let Predicate::Relation { relation } = predicate
            && resource
                .relations
                .iter()
                .any(|candidate| candidate.name == *relation)
        {
            return AccessPath::RelationIndex {
                relation: relation.clone(),
            };
        }
    }

    for predicate in &query.predicates {
        if let Predicate::Equal { field } = predicate
            && protected_indexes.iter().any(|index| {
                index.resource == resource.name
                    && index.field == *field
                    && index.mode
                        == ProtectedIndexMode::EqualityBlindIndex
            })
        {
            return AccessPath::ProtectedEqualityIndex {
                field: field.clone(),
            };
        }
    }

    let predicate_fields: Vec<&str> = query
        .predicates
        .iter()
        .filter_map(|predicate| match predicate {
            Predicate::Equal { field } | Predicate::Range { field } => {
                Some(field.as_str())
            }
            Predicate::Relation { .. } => None,
        })
        .collect();

    if let Some(index) = best_index(&resource.indexes, &predicate_fields) {
        return AccessPath::Index {
            fields: index.fields.clone(),
            unique: index.unique,
        };
    }

    AccessPath::FullScan
}

fn best_index<'a>(
    indexes: &'a [IndexIntent],
    predicates: &[&str],
) -> Option<&'a IndexIntent> {
    indexes
        .iter()
        .filter(|index| {
            index
                .fields
                .iter()
                .all(|field| predicates.contains(&field.as_str()))
        })
        .max_by_key(|index| (index.unique, index.fields.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::{PolicyExpr, PolicyRule};
    use crate::gir::SemanticType;
    use crate::storage::{
        FieldProtection, FieldSchema, ResourceSchema, StoreSchema,
    };

    fn schema() -> StoreSchema {
        let mut message = ResourceSchema::new("Message");
        message.fields = vec![
            FieldSchema {
                name: "id".into(),
                ty: SemanticType::Text,
                protection: FieldProtection::Public,
                mutable: false,
            },
            FieldSchema {
                name: "author".into(),
                ty: SemanticType::Text,
                protection: FieldProtection::Public,
                mutable: false,
            },
        ];
        message.indexes.push(IndexIntent {
            fields: vec!["author".into()],
            unique: false,
        });
        message.policies.rules.push(PolicyRule {
            action: Action::read(),
            allow_if: PolicyExpr::All(vec![]),
        });

        StoreSchema {
            resources: vec![message],
        }
    }

    #[test]
    fn indexed_filter_avoids_enumeration_requirement() {
        let store = schema();
        let query = QuerySpec {
            resource: "Message".into(),
            predicates: vec![Predicate::Equal {
                field: "author".into(),
            }],
            projection: vec!["id".into()],
            limit: Some(100),
            allow_enumeration: false,
        };

        assert!(validate_query(&store, &query).is_ok());
        assert!(matches!(
            choose_access_path(store.resource("Message").unwrap(), &query),
            AccessPath::Index { .. }
        ));
    }

    #[test]
    fn full_scan_requires_explicit_enumeration_policy() {
        let store = schema();
        let query = QuerySpec {
            resource: "Message".into(),
            predicates: vec![],
            projection: vec!["id".into()],
            limit: None,
            allow_enumeration: true,
        };

        assert!(validate_query(&store, &query)
            .unwrap_err()
            .contains(&QueryIssue::EnumerationPolicyMissing));
    }
}
