use crate::gir::SemanticType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DataClass {
    Public,
    Private,
    Secret,
    Credential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExternalSink {
    Log,
    Debug,
    Serialize,
    Network,
    Audit,
    ProtectedStore,
    CredentialVerifier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisclosureGrant {
    pub class: DataClass,
    pub sink: ExternalSink,
    pub purpose: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InformationFlowIssue {
    DisclosureRequired {
        class: DataClass,
        sink: ExternalSink,
    },
    SinkForbidden {
        class: DataClass,
        sink: ExternalSink,
    },
    InvalidDisclosureGrant {
        expected_class: DataClass,
        expected_sink: ExternalSink,
    },
}

pub fn classify_semantic_type(ty: &SemanticType) -> DataClass {
    match ty {
        SemanticType::Credential(_) => DataClass::Credential,
        SemanticType::Secret(_) => DataClass::Secret,
        SemanticType::Reference(_)
        | SemanticType::Record(_)
        | SemanticType::Variant(_) => DataClass::Private,
        SemanticType::Array(inner, _)
        | SemanticType::Slice(inner)
        | SemanticType::Vector(inner, _)
        | SemanticType::Option(inner)
        | SemanticType::Unique(inner)
        | SemanticType::Borrow(inner)
        | SemanticType::Shared(inner)
        | SemanticType::State(inner)
        | SemanticType::Atomic(inner)
        | SemanticType::Versioned(inner) => classify_semantic_type(inner),
        SemanticType::Result(ok, error) => {
            classify_semantic_type(ok).max(classify_semantic_type(error))
        }
        SemanticType::Bool
        | SemanticType::Integer(_)
        | SemanticType::BigInteger
        | SemanticType::Rational
        | SemanticType::Decimal(_)
        | SemanticType::Float(_)
        | SemanticType::BigFloat(_)
        | SemanticType::Text
        | SemanticType::Bytes => DataClass::Public,
    }
}

pub fn validate_external_flow(
    class: DataClass,
    sink: ExternalSink,
    disclosure: Option<&DisclosureGrant>,
) -> Result<(), InformationFlowIssue> {
    match class {
        DataClass::Public => Ok(()),
        DataClass::Private => match sink {
            ExternalSink::ProtectedStore => Ok(()),
            ExternalSink::Serialize | ExternalSink::Network => {
                require_disclosure(class, sink, disclosure)
            }
            ExternalSink::Log
            | ExternalSink::Debug
            | ExternalSink::Audit
            | ExternalSink::CredentialVerifier => {
                Err(InformationFlowIssue::SinkForbidden { class, sink })
            }
        },
        DataClass::Secret => match sink {
            ExternalSink::ProtectedStore => Ok(()),
            ExternalSink::Serialize | ExternalSink::Network => {
                require_disclosure(class, sink, disclosure)
            }
            ExternalSink::Log
            | ExternalSink::Debug
            | ExternalSink::Audit
            | ExternalSink::CredentialVerifier => {
                Err(InformationFlowIssue::SinkForbidden { class, sink })
            }
        },
        DataClass::Credential => match sink {
            ExternalSink::CredentialVerifier => Ok(()),
            _ => Err(InformationFlowIssue::SinkForbidden { class, sink }),
        },
    }
}

fn require_disclosure(
    class: DataClass,
    sink: ExternalSink,
    disclosure: Option<&DisclosureGrant>,
) -> Result<(), InformationFlowIssue> {
    let Some(grant) = disclosure else {
        return Err(InformationFlowIssue::DisclosureRequired { class, sink });
    };

    if grant.class != class || grant.sink != sink {
        return Err(InformationFlowIssue::InvalidDisclosureGrant {
            expected_class: class,
            expected_sink: sink,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gir::SemanticType;

    #[test]
    fn nested_secret_taints_container() {
        let ty = SemanticType::Option(Box::new(SemanticType::Secret(
            Box::new(SemanticType::Bytes),
        )));
        assert_eq!(classify_semantic_type(&ty), DataClass::Secret);
    }

    #[test]
    fn credentials_can_only_flow_to_verifier() {
        assert!(
            validate_external_flow(
                DataClass::Credential,
                ExternalSink::CredentialVerifier,
                None,
            )
            .is_ok()
        );
        assert_eq!(
            validate_external_flow(
                DataClass::Credential,
                ExternalSink::Log,
                None,
            ),
            Err(InformationFlowIssue::SinkForbidden {
                class: DataClass::Credential,
                sink: ExternalSink::Log,
            })
        );
    }

    #[test]
    fn raw_secret_can_never_be_logged_even_with_disclosure() {
        let grant = DisclosureGrant {
            class: DataClass::Secret,
            sink: ExternalSink::Log,
            purpose: "debug".into(),
        };

        assert!(matches!(
            validate_external_flow(
                DataClass::Secret,
                ExternalSink::Log,
                Some(&grant),
            ),
            Err(InformationFlowIssue::SinkForbidden { .. })
        ));
    }

    #[test]
    fn secret_network_export_is_explicit_and_scoped() {
        assert_eq!(
            validate_external_flow(
                DataClass::Secret,
                ExternalSink::Network,
                None,
            ),
            Err(InformationFlowIssue::DisclosureRequired {
                class: DataClass::Secret,
                sink: ExternalSink::Network,
            })
        );

        let grant = DisclosureGrant {
            class: DataClass::Secret,
            sink: ExternalSink::Network,
            purpose: "send to authorized peer".into(),
        };

        assert!(
            validate_external_flow(
                DataClass::Secret,
                ExternalSink::Network,
                Some(&grant),
            )
            .is_ok()
        );
    }

    #[test]
    fn secret_storage_is_allowed_only_through_protected_store_sink() {
        assert!(
            validate_external_flow(
                DataClass::Secret,
                ExternalSink::ProtectedStore,
                None,
            )
            .is_ok()
        );
        assert!(
            validate_external_flow(
                DataClass::Secret,
                ExternalSink::Serialize,
                None,
            )
            .is_err()
        );
    }
}
