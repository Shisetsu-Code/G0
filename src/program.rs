use crate::concurrency::{
    validate_concurrent_region, ConcurrencyIssue, ConcurrentRegion,
};
use crate::crypto::{
    validate_crypto_profile, CryptoProfile, CryptoProfileIssue,
    CryptoRequirement,
};
use crate::gir::Graph;
use crate::gir_validate::{self, ValidationIssue};
use crate::network::{
    validate_network_profile, ConnectionRequirement, NetworkProfile,
    NetworkProfileIssue,
};
use crate::security::{
    validate_password_contract, PasswordTuning, SecurityContractIssue,
    SecurityProfile,
};
use crate::storage::{
    validate_store_schema, StorageSchemaIssue, StoreSchema,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgramContract {
    pub graphs: Vec<Graph>,
    pub store: StoreSchema,
    pub concurrent_regions: Vec<ConcurrentRegion>,
    pub connections: Vec<ConnectionRequirement>,
    pub crypto: Vec<CryptoRequirement>,
    pub password_tuning: Option<PasswordTuning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformContract {
    pub network: NetworkProfile,
    pub crypto: CryptoProfile,
    pub security: SecurityProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramIssue {
    Graph {
        graph: String,
        issues: Vec<ValidationIssue>,
    },
    Store(Vec<StorageSchemaIssue>),
    Concurrency {
        region: usize,
        issues: Vec<ConcurrencyIssue>,
    },
    Network {
        requirement: usize,
        issues: Vec<NetworkProfileIssue>,
    },
    Crypto {
        requirement: usize,
        issues: Vec<CryptoProfileIssue>,
    },
    Security(Vec<SecurityContractIssue>),
}

pub fn validate_program(
    program: &ProgramContract,
    platform: &PlatformContract,
) -> Result<(), Vec<ProgramIssue>> {
    let mut issues = Vec::new();

    for graph in &program.graphs {
        if let Err(report) = gir_validate::validate(graph) {
            issues.push(ProgramIssue::Graph {
                graph: graph.name.clone(),
                issues: report.issues,
            });
        }
    }

    if let Err(store_issues) = validate_store_schema(&program.store) {
        issues.push(ProgramIssue::Store(store_issues));
    }

    for (index, region) in program.concurrent_regions.iter().enumerate() {
        if let Err(region_issues) = validate_concurrent_region(region) {
            issues.push(ProgramIssue::Concurrency {
                region: index,
                issues: region_issues,
            });
        }
    }

    for (index, requirement) in program.connections.iter().enumerate() {
        if let Err(network_issues) =
            validate_network_profile(&platform.network, requirement)
        {
            issues.push(ProgramIssue::Network {
                requirement: index,
                issues: network_issues,
            });
        }
    }

    for (index, requirement) in program.crypto.iter().enumerate() {
        if let Err(crypto_issues) =
            validate_crypto_profile(&platform.crypto, requirement)
        {
            issues.push(ProgramIssue::Crypto {
                requirement: index,
                issues: crypto_issues,
            });
        }
    }

    if let Some(password_tuning) = program.password_tuning
        && let Err(security_issues) =
            validate_password_contract(&platform.security, password_tuning)
    {
        issues.push(ProgramIssue::Security(security_issues));
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::concurrency::{
        AccessKind, StateAccess, StateDiscipline, TaskAccess,
    };
    use crate::crypto::{CryptoGuarantee, CryptoPrimitive};
    use crate::gir::ParameterPolicy;
    use crate::network::ConnectionFeature;
    use crate::storage::{Cardinality, DeleteRule, RelationSchema, ResourceSchema};

    fn platform() -> PlatformContract {
        PlatformContract {
            network: NetworkProfile::new(
                "network.current",
                [
                    ConnectionFeature::Reliable,
                    ConnectionFeature::Ordered,
                    ConnectionFeature::Multiplexed,
                ],
            ),
            crypto: CryptoProfile {
                id: "crypto.current".into(),
                security_bits: 192,
                primitives: [
                    CryptoPrimitive::Seal,
                    CryptoPrimitive::Open,
                    CryptoPrimitive::Sign,
                    CryptoPrimitive::Verify,
                ]
                .into_iter()
                .collect(),
                guarantees: [
                    CryptoGuarantee::AuthenticatedEncryption,
                    CryptoGuarantee::Integrity,
                    CryptoGuarantee::Unforgeability,
                ]
                .into_iter()
                .collect(),
            },
            security: SecurityProfile {
                id: "security.high".into(),
                password_memory_floor_mib: 128,
                password_work_floor: 3,
                side_channel_resistant_verification: true,
                uniform_public_auth_failure: true,
                credential_storage_is_verifier_only: true,
            },
        }
    }

    #[test]
    fn empty_program_contract_is_valid() {
        assert!(
            validate_program(&ProgramContract::default(), &platform()).is_ok()
        );
    }

    #[test]
    fn compiler_gate_reports_multiple_independent_failures() {
        let mut message = ResourceSchema::new("Message");
        message.relations.push(RelationSchema {
            name: "author".into(),
            target_resource: "MissingUser".into(),
            cardinality: Cardinality::One,
            on_delete: DeleteRule::Restrict,
        });

        let region = ConcurrentRegion {
            tasks: vec![
                TaskAccess {
                    task: 1,
                    accesses: vec![StateAccess {
                        state: "balance".into(),
                        discipline: StateDiscipline::Unique,
                        kind: AccessKind::Write,
                    }],
                },
                TaskAccess {
                    task: 2,
                    accesses: vec![StateAccess {
                        state: "balance".into(),
                        discipline: StateDiscipline::Unique,
                        kind: AccessKind::Read,
                    }],
                },
            ],
        };

        let program = ProgramContract {
            store: StoreSchema {
                resources: vec![message],
            },
            concurrent_regions: vec![region],
            connections: vec![ConnectionRequirement::with([
                ConnectionFeature::Multipath,
            ])],
            password_tuning: Some(PasswordTuning {
                memory_mib: ParameterPolicy::Bounded { min: 32, max: 1024 },
                work_factor: ParameterPolicy::Bounded { min: 1, max: 12 },
                parallelism: ParameterPolicy::Bounded { min: 1, max: 8 },
            }),
            ..ProgramContract::default()
        };

        let issues = validate_program(&program, &platform()).unwrap_err();
        assert!(issues.iter().any(|issue| matches!(issue, ProgramIssue::Store(_))));
        assert!(
            issues
                .iter()
                .any(|issue| matches!(issue, ProgramIssue::Concurrency { .. }))
        );
        assert!(
            issues
                .iter()
                .any(|issue| matches!(issue, ProgramIssue::Network { .. }))
        );
        assert!(
            issues
                .iter()
                .any(|issue| matches!(issue, ProgramIssue::Security(_)))
        );
    }

    #[test]
    fn crypto_requirement_is_checked_against_selected_platform_profile() {
        let mut requirement = CryptoRequirement::new(256);
        requirement.primitives = BTreeSet::from([CryptoPrimitive::KeyExchange]);
        requirement.guarantees =
            BTreeSet::from([CryptoGuarantee::QuantumResistance]);

        let program = ProgramContract {
            crypto: vec![requirement],
            ..ProgramContract::default()
        };

        let issues = validate_program(&program, &platform()).unwrap_err();
        assert!(
            issues
                .iter()
                .any(|issue| matches!(issue, ProgramIssue::Crypto { .. }))
        );
    }
}
