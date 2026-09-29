use crate::gir::SemanticType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeValue {
    Bool(bool),
    Integer(i128),
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputTrust {
    InternalVerified,
    ExternalUntrusted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputContract {
    pub name: String,
    pub ty: SemanticType,
    pub trust: InputTrust,
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngressIssue {
    TypeMismatch,
    IntegerOutOfRange {
        value: i128,
        min: i128,
        max: i128,
    },
    TooLarge {
        actual: u64,
        maximum: u64,
    },
    CredentialRequiresCredentialVerifier,
    SecretRequiresProtectedIngress,
    ReferenceRequiresStoreVerification,
}

pub fn validate_ingress(
    contract: &InputContract,
    value: &RuntimeValue,
) -> Result<(), Vec<IngressIssue>> {
    let mut issues = Vec::new();

    match (&contract.ty, value) {
        (SemanticType::Bool, RuntimeValue::Bool(_)) => {}
        (SemanticType::Integer(range), RuntimeValue::Integer(value)) => {
            if *value < range.min || *value > range.max {
                issues.push(IngressIssue::IntegerOutOfRange {
                    value: *value,
                    min: range.min,
                    max: range.max,
                });
            }
        }
        (SemanticType::Text, RuntimeValue::Text(text)) => {
            validate_length(contract, text.len() as u64, &mut issues);
        }
        (SemanticType::Bytes, RuntimeValue::Bytes(bytes)) => {
            validate_length(contract, bytes.len() as u64, &mut issues);
        }
        (SemanticType::Credential(_), _) => {
            issues.push(IngressIssue::CredentialRequiresCredentialVerifier);
        }
        (SemanticType::Secret(_), _) if contract.trust == InputTrust::ExternalUntrusted => {
            issues.push(IngressIssue::SecretRequiresProtectedIngress);
        }
        (SemanticType::Reference(_), _)
            if contract.trust == InputTrust::ExternalUntrusted =>
        {
            issues.push(IngressIssue::ReferenceRequiresStoreVerification);
        }
        _ => issues.push(IngressIssue::TypeMismatch),
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn validate_length(
    contract: &InputContract,
    actual: u64,
    issues: &mut Vec<IngressIssue>,
) {
    if let Some(maximum) = contract.max_bytes
        && actual > maximum
    {
        issues.push(IngressIssue::TooLarge { actual, maximum });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gir::IntegerType;

    #[test]
    fn runtime_integer_must_respect_semantic_range() {
        let contract = InputContract {
            name: "packet_length".into(),
            ty: SemanticType::Integer(IntegerType::new(0, 255).unwrap()),
            trust: InputTrust::ExternalUntrusted,
            max_bytes: None,
        };

        assert_eq!(
            validate_ingress(&contract, &RuntimeValue::Integer(300)),
            Err(vec![IngressIssue::IntegerOutOfRange {
                value: 300,
                min: 0,
                max: 255,
            }])
        );
    }

    #[test]
    fn external_raw_value_cannot_enter_as_credential() {
        let contract = InputContract {
            name: "password".into(),
            ty: SemanticType::Credential(Box::new(SemanticType::Text)),
            trust: InputTrust::ExternalUntrusted,
            max_bytes: Some(4096),
        };

        assert_eq!(
            validate_ingress(
                &contract,
                &RuntimeValue::Text("password".into()),
            ),
            Err(vec![IngressIssue::CredentialRequiresCredentialVerifier])
        );
    }

    #[test]
    fn input_size_limit_is_structural() {
        let contract = InputContract {
            name: "body".into(),
            ty: SemanticType::Bytes,
            trust: InputTrust::ExternalUntrusted,
            max_bytes: Some(4),
        };

        assert_eq!(
            validate_ingress(
                &contract,
                &RuntimeValue::Bytes(vec![0; 5]),
            ),
            Err(vec![IngressIssue::TooLarge {
                actual: 5,
                maximum: 4,
            }])
        );
    }
}
