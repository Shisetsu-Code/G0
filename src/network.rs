use std::collections::BTreeSet;

/// Security is not a selectable feature of a G0 connection.
/// Every conforming implementation must provide all of these guarantees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MandatorySecurityGuarantee {
    Confidentiality,
    Integrity,
    PeerAuthentication,
    ReplayResistance,
}

pub fn mandatory_security_guarantees() -> BTreeSet<MandatorySecurityGuarantee> {
    [
        MandatorySecurityGuarantee::Confidentiality,
        MandatorySecurityGuarantee::Integrity,
        MandatorySecurityGuarantee::PeerAuthentication,
        MandatorySecurityGuarantee::ReplayResistance,
    ]
    .into_iter()
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConnectionFeature {
    Reliable,
    Ordered,
    Multiplexed,
    Migratable,
    Datagram,
    Multipath,
    HardwareOffload,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConnectionRequirement {
    pub features: BTreeSet<ConnectionFeature>,
}

impl ConnectionRequirement {
    pub fn with(features: impl IntoIterator<Item = ConnectionFeature>) -> Self {
        Self {
            features: features.into_iter().collect(),
        }
    }
}

/// A platform profile maps semantic connection requirements to concrete
/// protocols. Protocol names intentionally do not appear in G0 Core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkProfile {
    pub id: String,
    pub features: BTreeSet<ConnectionFeature>,
}

impl NetworkProfile {
    pub fn new(
        id: impl Into<String>,
        features: impl IntoIterator<Item = ConnectionFeature>,
    ) -> Self {
        Self {
            id: id.into(),
            features: features.into_iter().collect(),
        }
    }

    pub fn satisfies(&self, requirement: &ConnectionRequirement) -> bool {
        requirement
            .features
            .iter()
            .all(|feature| self.features.contains(feature))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkProfileIssue {
    MissingFeature(ConnectionFeature),
}

pub fn validate_network_profile(
    profile: &NetworkProfile,
    requirement: &ConnectionRequirement,
) -> Result<(), Vec<NetworkProfileIssue>> {
    let issues: Vec<NetworkProfileIssue> = requirement
        .features
        .iter()
        .filter(|feature| !profile.features.contains(feature))
        .copied()
        .map(NetworkProfileIssue::MissingFeature)
        .collect();

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_security_is_mandatory_not_configurable() {
        let guarantees = mandatory_security_guarantees();
        assert!(guarantees.contains(&MandatorySecurityGuarantee::Confidentiality));
        assert!(guarantees.contains(&MandatorySecurityGuarantee::Integrity));
        assert!(guarantees.contains(&MandatorySecurityGuarantee::PeerAuthentication));
        assert!(guarantees.contains(&MandatorySecurityGuarantee::ReplayResistance));
    }

    #[test]
    fn profile_is_selected_by_capabilities_not_protocol_name() {
        let requirement = ConnectionRequirement::with([
            ConnectionFeature::Reliable,
            ConnectionFeature::Ordered,
            ConnectionFeature::Multiplexed,
        ]);
        let profile = NetworkProfile::new(
            "network.profile.current",
            [
                ConnectionFeature::Reliable,
                ConnectionFeature::Ordered,
                ConnectionFeature::Multiplexed,
                ConnectionFeature::Migratable,
            ],
        );

        assert!(profile.satisfies(&requirement));
        assert!(validate_network_profile(&profile, &requirement).is_ok());
    }

    #[test]
    fn missing_semantic_feature_is_rejected_without_fallback() {
        let requirement =
            ConnectionRequirement::with([ConnectionFeature::Multipath]);
        let profile = NetworkProfile::new(
            "network.profile.current",
            [ConnectionFeature::Reliable],
        );

        assert_eq!(
            validate_network_profile(&profile, &requirement),
            Err(vec![NetworkProfileIssue::MissingFeature(
                ConnectionFeature::Multipath
            )])
        );
    }
}
