use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CryptoPrimitive {
    Seal,
    Open,
    Sign,
    Verify,
    DeriveKey,
    KeyExchange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CryptoGuarantee {
    AuthenticatedEncryption,
    Integrity,
    Unforgeability,
    ForwardSecrecy,
    PostCompromiseSecurity,
    QuantumResistance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CryptoRequirement {
    /// Semantic security floor; concrete algorithm/key sizes remain profile details.
    pub minimum_security_bits: u16,
    pub primitives: BTreeSet<CryptoPrimitive>,
    pub guarantees: BTreeSet<CryptoGuarantee>,
}

impl CryptoRequirement {
    pub fn new(minimum_security_bits: u16) -> Self {
        Self {
            minimum_security_bits,
            primitives: BTreeSet::new(),
            guarantees: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CryptoProfile {
    pub id: String,
    pub security_bits: u16,
    pub primitives: BTreeSet<CryptoPrimitive>,
    pub guarantees: BTreeSet<CryptoGuarantee>,
}

impl CryptoProfile {
    pub fn satisfies(&self, requirement: &CryptoRequirement) -> bool {
        self.security_bits >= requirement.minimum_security_bits
            && requirement
                .primitives
                .iter()
                .all(|primitive| self.primitives.contains(primitive))
            && requirement
                .guarantees
                .iter()
                .all(|guarantee| self.guarantees.contains(guarantee))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptoProfileIssue {
    SecurityFloorNotMet {
        required_bits: u16,
        profile_bits: u16,
    },
    MissingPrimitive(CryptoPrimitive),
    MissingGuarantee(CryptoGuarantee),
}

pub fn validate_crypto_profile(
    profile: &CryptoProfile,
    requirement: &CryptoRequirement,
) -> Result<(), Vec<CryptoProfileIssue>> {
    let mut issues = Vec::new();

    if profile.security_bits < requirement.minimum_security_bits {
        issues.push(CryptoProfileIssue::SecurityFloorNotMet {
            required_bits: requirement.minimum_security_bits,
            profile_bits: profile.security_bits,
        });
    }

    for primitive in &requirement.primitives {
        if !profile.primitives.contains(primitive) {
            issues.push(CryptoProfileIssue::MissingPrimitive(*primitive));
        }
    }

    for guarantee in &requirement.guarantees {
        if !profile.guarantees.contains(guarantee) {
            issues.push(CryptoProfileIssue::MissingGuarantee(*guarantee));
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyPurpose {
    DataProtection,
    Signing,
    Session,
    Storage,
    Identity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPolicy {
    pub purpose: KeyPurpose,
    pub exportable: bool,
    pub hardware_backed_required: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> CryptoProfile {
        CryptoProfile {
            id: "crypto.profile.current".into(),
            security_bits: 192,
            primitives: [
                CryptoPrimitive::Seal,
                CryptoPrimitive::Open,
                CryptoPrimitive::Sign,
                CryptoPrimitive::Verify,
                CryptoPrimitive::DeriveKey,
                CryptoPrimitive::KeyExchange,
            ]
            .into_iter()
            .collect(),
            guarantees: [
                CryptoGuarantee::AuthenticatedEncryption,
                CryptoGuarantee::Integrity,
                CryptoGuarantee::Unforgeability,
                CryptoGuarantee::ForwardSecrecy,
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn application_requests_guarantees_not_algorithms() {
        let mut requirement = CryptoRequirement::new(128);
        requirement.primitives.insert(CryptoPrimitive::Seal);
        requirement
            .guarantees
            .insert(CryptoGuarantee::AuthenticatedEncryption);

        assert!(profile().satisfies(&requirement));
    }

    #[test]
    fn profile_cannot_silently_drop_required_security_property() {
        let mut requirement = CryptoRequirement::new(128);
        requirement
            .guarantees
            .insert(CryptoGuarantee::QuantumResistance);

        assert_eq!(
            validate_crypto_profile(&profile(), &requirement),
            Err(vec![CryptoProfileIssue::MissingGuarantee(
                CryptoGuarantee::QuantumResistance
            )])
        );
    }

    #[test]
    fn lower_security_profile_never_satisfies_higher_floor() {
        let requirement = CryptoRequirement::new(256);
        assert_eq!(
            validate_crypto_profile(&profile(), &requirement),
            Err(vec![CryptoProfileIssue::SecurityFloorNotMet {
                required_bits: 256,
                profile_bits: 192,
            }])
        );
    }
}
