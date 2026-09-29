use std::collections::BTreeSet;

use crate::storage::{DeleteRule, StoreSchema};
use crate::storage_integrity::{EntityKey, StoreSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetachOperation {
    pub source: EntityKey,
    pub relation: String,
    pub target: EntityKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeletePlan {
    pub delete: BTreeSet<EntityKey>,
    pub detach: Vec<DetachOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeletePlanIssue {
    TargetMissing(EntityKey),
    UnknownSourceSchema(String),
    UnknownRelationSchema {
        resource: String,
        relation: String,
    },
    RestrictedByReference {
        source: EntityKey,
        relation: String,
    },
    CascadeCycle(EntityKey),
}

pub fn plan_delete(
    schema: &StoreSchema,
    snapshot: &StoreSnapshot,
    target: &EntityKey,
) -> Result<DeletePlan, Vec<DeletePlanIssue>> {
    if !snapshot.entities.contains_key(target) {
        return Err(vec![DeletePlanIssue::TargetMissing(target.clone())]);
    }

    let mut plan = DeletePlan::default();
    let mut visiting = BTreeSet::new();
    let mut issues = Vec::new();

    plan_delete_recursive(
        schema,
        snapshot,
        target,
        &mut plan,
        &mut visiting,
        &mut issues,
    );

    if issues.is_empty() {
        Ok(plan)
    } else {
        Err(issues)
    }
}

fn plan_delete_recursive(
    schema: &StoreSchema,
    snapshot: &StoreSnapshot,
    target: &EntityKey,
    plan: &mut DeletePlan,
    visiting: &mut BTreeSet<EntityKey>,
    issues: &mut Vec<DeletePlanIssue>,
) {
    if plan.delete.contains(target) {
        return;
    }
    if !visiting.insert(target.clone()) {
        issues.push(DeletePlanIssue::CascadeCycle(target.clone()));
        return;
    }

    for source in snapshot.entities.values() {
        let Some(source_schema) = schema.resource(&source.key.resource) else {
            issues.push(DeletePlanIssue::UnknownSourceSchema(
                source.key.resource.clone(),
            ));
            continue;
        };

        for relation_value in &source.relations {
            if !relation_value.targets.contains(target) {
                continue;
            }

            let Some(relation_schema) = source_schema
                .relations
                .iter()
                .find(|relation| relation.name == relation_value.name)
            else {
                issues.push(DeletePlanIssue::UnknownRelationSchema {
                    resource: source.key.resource.clone(),
                    relation: relation_value.name.clone(),
                });
                continue;
            };

            match relation_schema.on_delete {
                DeleteRule::Restrict => {
                    issues.push(DeletePlanIssue::RestrictedByReference {
                        source: source.key.clone(),
                        relation: relation_value.name.clone(),
                    });
                }
                DeleteRule::Detach => {
                    plan.detach.push(DetachOperation {
                        source: source.key.clone(),
                        relation: relation_value.name.clone(),
                        target: target.clone(),
                    });
                }
                DeleteRule::Cascade => {
                    plan_delete_recursive(
                        schema,
                        snapshot,
                        &source.key,
                        plan,
                        visiting,
                        issues,
                    );
                }
            }
        }
    }

    visiting.remove(target);
    plan.delete.insert(target.clone());
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::storage::{
        Cardinality, RelationSchema, ResourceSchema, StoreSchema,
    };
    use crate::storage_integrity::{EntityInstance, RelationInstance};

    fn key(resource: &str, id: &str) -> EntityKey {
        EntityKey {
            resource: resource.into(),
            id: id.into(),
            scope: "tenant-a".into(),
        }
    }

    #[test]
    fn restrict_relation_blocks_delete() {
        let user = ResourceSchema::new("User");
        let mut message = ResourceSchema::new("Message");
        message.relations.push(RelationSchema {
            name: "author".into(),
            target_resource: "User".into(),
            cardinality: Cardinality::One,
            on_delete: DeleteRule::Restrict,
        });
        let schema = StoreSchema {
            resources: vec![user, message],
        };

        let user_key = key("User", "u1");
        let message_key = key("Message", "m1");
        let snapshot = StoreSnapshot {
            entities: BTreeMap::from([
                (
                    user_key.clone(),
                    EntityInstance {
                        key: user_key.clone(),
                        relations: vec![],
                    },
                ),
                (
                    message_key.clone(),
                    EntityInstance {
                        key: message_key.clone(),
                        relations: vec![RelationInstance {
                            name: "author".into(),
                            targets: vec![user_key.clone()],
                        }],
                    },
                ),
            ]),
        };

        assert!(plan_delete(&schema, &snapshot, &user_key)
            .unwrap_err()
            .iter()
            .any(|issue| matches!(
                issue,
                DeletePlanIssue::RestrictedByReference { .. }
            )));
    }

    #[test]
    fn cascade_deletes_referring_entity_first() {
        let parent = ResourceSchema::new("Parent");
        let mut child = ResourceSchema::new("Child");
        child.relations.push(RelationSchema {
            name: "parent".into(),
            target_resource: "Parent".into(),
            cardinality: Cardinality::One,
            on_delete: DeleteRule::Cascade,
        });
        let schema = StoreSchema {
            resources: vec![parent, child],
        };

        let parent_key = key("Parent", "p1");
        let child_key = key("Child", "c1");
        let snapshot = StoreSnapshot {
            entities: BTreeMap::from([
                (
                    parent_key.clone(),
                    EntityInstance {
                        key: parent_key.clone(),
                        relations: vec![],
                    },
                ),
                (
                    child_key.clone(),
                    EntityInstance {
                        key: child_key.clone(),
                        relations: vec![RelationInstance {
                            name: "parent".into(),
                            targets: vec![parent_key.clone()],
                        }],
                    },
                ),
            ]),
        };

        let plan = plan_delete(&schema, &snapshot, &parent_key).unwrap();
        assert!(plan.delete.contains(&parent_key));
        assert!(plan.delete.contains(&child_key));
    }
}
