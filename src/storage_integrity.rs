use std::collections::{BTreeMap, BTreeSet};

use crate::storage::{Cardinality, StoreSchema, TenantIsolation};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntityKey {
    pub resource: String,
    pub id: String,
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationInstance {
    pub name: String,
    pub targets: Vec<EntityKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityInstance {
    pub key: EntityKey,
    pub relations: Vec<RelationInstance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreSnapshot {
    pub entities: BTreeMap<EntityKey, EntityInstance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityIssue {
    UnknownResource(String),
    DuplicateRelation {
        entity: EntityKey,
        relation: String,
    },
    UnknownRelation {
        entity: EntityKey,
        relation: String,
    },
    CardinalityViolation {
        entity: EntityKey,
        relation: String,
        count: usize,
    },
    MissingTarget {
        entity: EntityKey,
        relation: String,
        target: EntityKey,
    },
    WrongTargetResource {
        entity: EntityKey,
        relation: String,
        expected: String,
        actual: String,
    },
    CrossScopeReference {
        entity: EntityKey,
        relation: String,
        target: EntityKey,
    },
}

pub fn validate_snapshot(
    schema: &StoreSchema,
    snapshot: &StoreSnapshot,
) -> Result<(), Vec<IntegrityIssue>> {
    let mut issues = Vec::new();

    for entity in snapshot.entities.values() {
        let Some(resource) = schema.resource(&entity.key.resource) else {
            issues.push(IntegrityIssue::UnknownResource(
                entity.key.resource.clone(),
            ));
            continue;
        };

        let mut seen_relations = BTreeSet::new();
        for relation_value in &entity.relations {
            if !seen_relations.insert(relation_value.name.as_str()) {
                issues.push(IntegrityIssue::DuplicateRelation {
                    entity: entity.key.clone(),
                    relation: relation_value.name.clone(),
                });
            }

            let Some(relation_schema) = resource
                .relations
                .iter()
                .find(|relation| relation.name == relation_value.name)
            else {
                issues.push(IntegrityIssue::UnknownRelation {
                    entity: entity.key.clone(),
                    relation: relation_value.name.clone(),
                });
                continue;
            };

            if !cardinality_valid(
                relation_schema.cardinality,
                relation_value.targets.len(),
            ) {
                issues.push(IntegrityIssue::CardinalityViolation {
                    entity: entity.key.clone(),
                    relation: relation_value.name.clone(),
                    count: relation_value.targets.len(),
                });
            }

            for target in &relation_value.targets {
                if target.resource != relation_schema.target_resource {
                    issues.push(IntegrityIssue::WrongTargetResource {
                        entity: entity.key.clone(),
                        relation: relation_value.name.clone(),
                        expected: relation_schema.target_resource.clone(),
                        actual: target.resource.clone(),
                    });
                    continue;
                }

                if !snapshot.entities.contains_key(target) {
                    issues.push(IntegrityIssue::MissingTarget {
                        entity: entity.key.clone(),
                        relation: relation_value.name.clone(),
                        target: target.clone(),
                    });
                    continue;
                }

                let target_schema = schema
                    .resource(&target.resource)
                    .expect("target schema validated by resource equality");

                if resource.tenant_isolation == TenantIsolation::CurrentScope
                    && target_schema.tenant_isolation
                        == TenantIsolation::CurrentScope
                    && entity.key.scope != target.scope
                {
                    issues.push(IntegrityIssue::CrossScopeReference {
                        entity: entity.key.clone(),
                        relation: relation_value.name.clone(),
                        target: target.clone(),
                    });
                }
            }
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn cardinality_valid(cardinality: Cardinality, count: usize) -> bool {
    match cardinality {
        Cardinality::One => count == 1,
        Cardinality::OptionalOne => count <= 1,
        Cardinality::Many => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{
        DeleteRule, RelationSchema, ResourceSchema, StoreSchema,
    };

    fn schema() -> StoreSchema {
        let user = ResourceSchema::new("User");
        let mut message = ResourceSchema::new("Message");
        message.relations.push(RelationSchema {
            name: "author".into(),
            target_resource: "User".into(),
            cardinality: Cardinality::One,
            on_delete: DeleteRule::Restrict,
        });
        StoreSchema {
            resources: vec![user, message],
        }
    }

    #[test]
    fn verified_relation_points_to_existing_typed_entity() {
        let user = EntityInstance {
            key: EntityKey {
                resource: "User".into(),
                id: "u1".into(),
                scope: "tenant-a".into(),
            },
            relations: vec![],
        };
        let message = EntityInstance {
            key: EntityKey {
                resource: "Message".into(),
                id: "m1".into(),
                scope: "tenant-a".into(),
            },
            relations: vec![RelationInstance {
                name: "author".into(),
                targets: vec![user.key.clone()],
            }],
        };
        let snapshot = StoreSnapshot {
            entities: BTreeMap::from([
                (user.key.clone(), user),
                (message.key.clone(), message),
            ]),
        };

        assert!(validate_snapshot(&schema(), &snapshot).is_ok());
    }

    #[test]
    fn dangling_reference_is_rejected() {
        let message = EntityInstance {
            key: EntityKey {
                resource: "Message".into(),
                id: "m1".into(),
                scope: "tenant-a".into(),
            },
            relations: vec![RelationInstance {
                name: "author".into(),
                targets: vec![EntityKey {
                    resource: "User".into(),
                    id: "missing".into(),
                    scope: "tenant-a".into(),
                }],
            }],
        };
        let snapshot = StoreSnapshot {
            entities: BTreeMap::from([(message.key.clone(), message)]),
        };

        assert!(validate_snapshot(&schema(), &snapshot)
            .unwrap_err()
            .iter()
            .any(|issue| matches!(issue, IntegrityIssue::MissingTarget { .. })));
    }

    #[test]
    fn scoped_relation_cannot_cross_tenant() {
        let user = EntityInstance {
            key: EntityKey {
                resource: "User".into(),
                id: "u1".into(),
                scope: "tenant-b".into(),
            },
            relations: vec![],
        };
        let message = EntityInstance {
            key: EntityKey {
                resource: "Message".into(),
                id: "m1".into(),
                scope: "tenant-a".into(),
            },
            relations: vec![RelationInstance {
                name: "author".into(),
                targets: vec![user.key.clone()],
            }],
        };
        let snapshot = StoreSnapshot {
            entities: BTreeMap::from([
                (user.key.clone(), user),
                (message.key.clone(), message),
            ]),
        };

        assert!(validate_snapshot(&schema(), &snapshot)
            .unwrap_err()
            .iter()
            .any(|issue| {
                matches!(issue, IntegrityIssue::CrossScopeReference { .. })
            }));
    }
}
