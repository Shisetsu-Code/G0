use g0::authority::{
    Action, PolicyExpr, PolicyRule, Principal, ResourceContext,
};
use g0::gir::SemanticType;
use g0::storage::{
    authorize_store_operation, FieldProtection, FieldSchema, ResourceSchema,
    StoreAccessIssue, StoreOperation,
};

fn message_schema() -> ResourceSchema {
    let mut schema = ResourceSchema::new("Message");
    schema.fields.push(FieldSchema {
        name: "body".into(),
        ty: SemanticType::Text,
        protection: FieldProtection::Private,
        mutable: true,
    });
    schema.policies.rules.push(PolicyRule {
        action: Action::read(),
        allow_if: PolicyExpr::Any(vec![
            PolicyExpr::PrincipalOwnsResource,
            PolicyExpr::PrincipalInRelation {
                relation: "conversation.member".into(),
            },
        ]),
    });
    schema
}

#[test]
fn changing_resource_id_with_valid_session_does_not_bypass_store_policy() {
    let schema = message_schema();

    let alice = Principal::new("alice", "tenant-a");
    let mallory = Principal::new("mallory", "tenant-a");

    let mut alices_message =
        ResourceContext::new("Message", "message-alice", "tenant-a");
    alices_message.owner = Some(alice.id.clone());
    alices_message.add_principal_relation(
        "conversation.member",
        alice.id.clone(),
    );

    let read = StoreOperation::Read {
        fields: vec!["body".into()],
    };

    assert!(
        authorize_store_operation(
            &schema,
            &alice,
            &alices_message,
            &read,
        )
        .is_ok()
    );

    let denied = authorize_store_operation(
        &schema,
        &mallory,
        &alices_message,
        &read,
    )
    .unwrap_err();

    assert!(denied.iter().any(|issue| {
        matches!(
            issue,
            StoreAccessIssue::AuthorizationDenied(_)
        )
    }));
}

#[test]
fn cross_tenant_resource_is_denied_before_resource_policy_can_help() {
    let schema = message_schema();
    let alice = Principal::new("alice", "tenant-a");

    let mut resource =
        ResourceContext::new("Message", "message-b", "tenant-b");
    resource.owner = Some(alice.id.clone());

    let denied = authorize_store_operation(
        &schema,
        &alice,
        &resource,
        &StoreOperation::Read {
            fields: vec!["body".into()],
        },
    )
    .unwrap_err();

    assert!(denied.iter().any(|issue| {
        matches!(
            issue,
            StoreAccessIssue::AuthorizationDenied(
                g0::authority::DenyReason::ScopeMismatch
            )
        )
    }));
}

#[test]
fn read_permission_never_implies_enumeration_permission() {
    let schema = message_schema();
    let alice = Principal::new("alice", "tenant-a");
    let mut resource =
        ResourceContext::new("Message", "message-a", "tenant-a");
    resource.owner = Some(alice.id.clone());

    let denied = authorize_store_operation(
        &schema,
        &alice,
        &resource,
        &StoreOperation::Enumerate,
    )
    .unwrap_err();

    assert!(denied.iter().any(|issue| {
        matches!(
            issue,
            StoreAccessIssue::AuthorizationDenied(
                g0::authority::DenyReason::NoPolicy
            )
        )
    }));
}
