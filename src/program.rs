use crate::authority::Action;
use crate::auth::{
    validate_authentication_profile, validate_one_time_code,
    AuthenticationProfile, AuthenticationProfileIssue, OneTimeCodeContract,
    OneTimeCodeIssue,
};
use crate::concurrency::{
    validate_concurrent_region, ConcurrencyIssue, ConcurrentRegion,
};
use crate::crypto::{
    validate_crypto_profile, CryptoProfile, CryptoProfileIssue,
    CryptoRequirement,
};
use crate::data_format::{validate_schema, DataSchema, SchemaIssue};
use crate::entropy::{
    validate_randomness_use, EntropyIssue, RandomUse, RandomnessClass,
};
use crate::gir::Graph;
use crate::gir_validate::{self, ValidationIssue};
use crate::hardware::{
    validate_hardware_profile, HardwareIssue, HardwareProfile,
    HardwareRequirement,
};
use crate::io::{
    validate_io_pipeline, IoPipeline, IoPipelineIssue, IoValueKind,
};
use crate::math::{
    validate_math_requirement, MathContractIssue, MathOperation, MathProfile,
    MathRequirement,
};
use crate::memory::{
    validate_memory_plan, MemoryIssue, MemoryPlan,
};
use crate::migration::{
    validate_migration, MigrationIssue, MigrationPlan,
};
use crate::network::{
    validate_network_profile, ConnectionRequirement, NetworkProfile,
    NetworkProfileIssue,
};
use crate::ownership::{
    validate_ownership, OwnershipIssue, OwnershipPlan,
};
use crate::policy_plan::{compile_policy, PolicyCompileIssue};
use crate::query::{
    validate_query_with_protected_indexes, QueryIssue, QuerySpec,
};
use crate::runtime::{validate_region_plan, RegionIssue, RegionPlan};
use crate::scheduler::{
    validate_schedule, ScheduleGraph, ScheduleIssue, SchedulerProfile,
};
use crate::secure_index::{
    validate_protected_index, ProtectedIndexIssue, ProtectedIndexSpec,
};
use crate::security::{
    validate_password_contract, PasswordTuning, SecurityContractIssue,
    SecurityProfile,
};
use crate::side_channel::{
    validate_side_channels, SideChannelIssue,
};
use crate::storage::{
    validate_store_schema, StorageSchemaIssue, StoreSchema,
};
use crate::storage_crypto::{
    validate_binding_coverage, validate_field_binding,
    validate_storage_crypto_profile, ProtectedFieldBinding,
    StorageCryptoIssue, StorageCryptoProfile,
};
use crate::task_runtime::{
    validate_task_plan, TaskPlan, TaskPlanIssue,
};
use crate::text::{
    validate_text_boundary, validate_text_contract, TextBoundaryIssue,
    TextBoundaryRequirement, TextContract, TextContractIssue,
};
use crate::time::{validate_clock_use, ClockKind, TimeIssue, TimeUse};
use crate::transaction::{
    validate_transaction, TransactionContract, TransactionIssue,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathUse {
    pub operation: MathOperation,
    pub requirement: MathRequirement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphOwnership {
    pub graph: String,
    pub plan: OwnershipPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgramContract {
    pub graphs: Vec<Graph>,
    /// None means a library. Executables name exactly one explicit entry graph.
    pub entry_graph: Option<String>,
    pub store: StoreSchema,
    pub concurrent_regions: Vec<ConcurrentRegion>,
    pub connections: Vec<ConnectionRequirement>,
    pub crypto: Vec<CryptoRequirement>,
    pub math: Vec<MathUse>,
    pub ownership: Vec<GraphOwnership>,
    pub memory: Vec<MemoryPlan>,
    pub password_tuning: Option<PasswordTuning>,
    pub text_boundaries: Vec<TextBoundaryRequirement>,
    pub io_pipelines: Vec<(IoPipeline, IoValueKind)>,
    pub hardware: HardwareRequirement,
    pub regions: Vec<RegionPlan>,
    pub schemas: Vec<DataSchema>,
    pub time_uses: Vec<(ClockKind, TimeUse)>,
    pub randomness_uses: Vec<(RandomnessClass, RandomUse)>,
    pub schedules: Vec<ScheduleGraph>,
    pub queries: Vec<QuerySpec>,
    pub protected_indexes: Vec<ProtectedIndexSpec>,
    pub task_plans: Vec<TaskPlan>,
    pub storage_crypto_bindings: Vec<ProtectedFieldBinding>,
    pub transactions: Vec<TransactionContract>,
    pub migrations: Vec<MigrationPlan>,
    pub authentication: Vec<AuthenticationProfile>,
    pub one_time_codes: Vec<OneTimeCodeContract>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformContract {
    pub network: NetworkProfile,
    pub crypto: CryptoProfile,
    pub security: SecurityProfile,
    pub math: MathProfile,
    pub text: TextContract,
    pub hardware: HardwareProfile,
    pub scheduler: SchedulerProfile,
    pub storage_crypto: StorageCryptoProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramIssue {
    DuplicateGraphName(String),
    UnknownEntryGraph(String),
    Graph {
        graph: String,
        issues: Vec<ValidationIssue>,
    },
    SideChannel {
        graph: String,
        issues: Vec<SideChannelIssue>,
    },
    Store(Vec<StorageSchemaIssue>),
    StorePolicy {
        resource: String,
        action: Action,
        issues: Vec<PolicyCompileIssue>,
    },
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
    Math {
        requirement: usize,
        issues: Vec<MathContractIssue>,
    },
    Ownership {
        graph: String,
        issues: Vec<OwnershipIssue>,
    },
    OwnershipUnknownGraph(String),
    Memory {
        plan: usize,
        issues: Vec<MemoryIssue>,
    },
    Text(Vec<TextContractIssue>),
    TextBoundary {
        boundary: usize,
        issue: TextBoundaryIssue,
    },
    Io {
        pipeline: usize,
        issues: Vec<IoPipelineIssue>,
    },
    Hardware(Vec<HardwareIssue>),
    Region {
        plan: usize,
        issues: Vec<RegionIssue>,
    },
    Schema {
        schema: usize,
        issues: Vec<SchemaIssue>,
    },
    Time {
        usage: usize,
        issue: TimeIssue,
    },
    Entropy {
        usage: usize,
        issue: EntropyIssue,
    },
    Schedule {
        schedule: usize,
        issues: Vec<ScheduleIssue>,
    },
    Query {
        query: usize,
        issues: Vec<QueryIssue>,
    },
    ProtectedIndex {
        index: usize,
        issues: Vec<ProtectedIndexIssue>,
    },
    TaskPlan {
        plan: usize,
        issues: Vec<TaskPlanIssue>,
    },
    StorageCrypto(Vec<StorageCryptoIssue>),
    Transaction {
        transaction: usize,
        issues: Vec<TransactionIssue>,
    },
    Migration {
        migration: usize,
        issues: Vec<MigrationIssue>,
    },
    Authentication {
        profile: usize,
        issues: Vec<AuthenticationProfileIssue>,
    },
    OneTimeCode {
        profile: usize,
        issues: Vec<OneTimeCodeIssue>,
    },
    Security(Vec<SecurityContractIssue>),
}

pub fn validate_program(
    program: &ProgramContract,
    platform: &PlatformContract,
) -> Result<(), Vec<ProgramIssue>> {
    let mut issues = Vec::new();

    let mut graph_names = std::collections::BTreeSet::new();
    for graph in &program.graphs {
        if !graph_names.insert(graph.name.as_str()) {
            issues.push(ProgramIssue::DuplicateGraphName(
                graph.name.clone(),
            ));
        }
    }

    if let Some(entry) = &program.entry_graph
        && !graph_names.contains(entry.as_str())
    {
        issues.push(ProgramIssue::UnknownEntryGraph(entry.clone()));
    }

    for graph in &program.graphs {
        if let Err(report) = gir_validate::validate(graph) {
            issues.push(ProgramIssue::Graph {
                graph: graph.name.clone(),
                issues: report.issues,
            });
        }
        if let Err(side_channel_issues) = validate_side_channels(graph) {
            issues.push(ProgramIssue::SideChannel {
                graph: graph.name.clone(),
                issues: side_channel_issues,
            });
        }
    }

    if let Err(store_issues) = validate_store_schema(&program.store) {
        issues.push(ProgramIssue::Store(store_issues));
    }

    for resource in &program.store.resources {
        for rule in &resource.policies.rules {
            if let Err(policy_issues) =
                compile_policy(&resource.policies, &rule.action)
            {
                issues.push(ProgramIssue::StorePolicy {
                    resource: resource.name.clone(),
                    action: rule.action.clone(),
                    issues: policy_issues,
                });
            }
        }
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

    for (index, math_use) in program.math.iter().enumerate() {
        if let Err(math_issues) = validate_math_requirement(
            &platform.math,
            math_use.operation,
            math_use.requirement,
        ) {
            issues.push(ProgramIssue::Math {
                requirement: index,
                issues: math_issues,
            });
        }
    }

    for ownership in &program.ownership {
        match program.graphs.iter().find(|graph| graph.name == ownership.graph) {
            Some(graph) => {
                if let Err(ownership_issues) =
                    validate_ownership(graph, &ownership.plan)
                {
                    issues.push(ProgramIssue::Ownership {
                        graph: ownership.graph.clone(),
                        issues: ownership_issues,
                    });
                }
            }
            None => issues.push(ProgramIssue::OwnershipUnknownGraph(
                ownership.graph.clone(),
            )),
        }
    }

    for (index, memory) in program.memory.iter().enumerate() {
        if let Err(memory_issues) = validate_memory_plan(memory) {
            issues.push(ProgramIssue::Memory {
                plan: index,
                issues: memory_issues,
            });
        }
    }

    if let Err(text_issues) = validate_text_contract(&platform.text) {
        issues.push(ProgramIssue::Text(text_issues));
    }

    for (index, boundary) in program.text_boundaries.iter().enumerate() {
        if let Err(issue) = validate_text_boundary(&platform.text, boundary) {
            issues.push(ProgramIssue::TextBoundary {
                boundary: index,
                issue,
            });
        }
    }

    for (index, (pipeline, expected)) in
        program.io_pipelines.iter().enumerate()
    {
        if let Err(io_issues) = validate_io_pipeline(pipeline, *expected) {
            issues.push(ProgramIssue::Io {
                pipeline: index,
                issues: io_issues,
            });
        }
    }

    if let Err(hardware_issues) =
        validate_hardware_profile(&platform.hardware, &program.hardware)
    {
        issues.push(ProgramIssue::Hardware(hardware_issues));
    }

    for (index, region) in program.regions.iter().enumerate() {
        if let Err(region_issues) = validate_region_plan(region) {
            issues.push(ProgramIssue::Region {
                plan: index,
                issues: region_issues,
            });
        }
    }

    for (index, schema) in program.schemas.iter().enumerate() {
        if let Err(schema_issues) = validate_schema(schema) {
            issues.push(ProgramIssue::Schema {
                schema: index,
                issues: schema_issues,
            });
        }
    }

    for (index, (clock, usage)) in program.time_uses.iter().enumerate() {
        if let Err(issue) = validate_clock_use(*clock, *usage) {
            issues.push(ProgramIssue::Time {
                usage: index,
                issue,
            });
        }
    }

    for (index, (class, usage)) in
        program.randomness_uses.iter().enumerate()
    {
        if let Err(issue) = validate_randomness_use(*class, *usage) {
            issues.push(ProgramIssue::Entropy {
                usage: index,
                issue,
            });
        }
    }

    for (index, schedule) in program.schedules.iter().enumerate() {
        if let Err(schedule_issues) =
            validate_schedule(schedule, &platform.scheduler)
        {
            issues.push(ProgramIssue::Schedule {
                schedule: index,
                issues: schedule_issues,
            });
        }
    }

    for (index, query) in program.queries.iter().enumerate() {
        if let Err(query_issues) = validate_query_with_protected_indexes(
            &program.store,
            query,
            &program.protected_indexes,
        ) {
            issues.push(ProgramIssue::Query {
                query: index,
                issues: query_issues,
            });
        }
    }

    for (index, protected_index) in
        program.protected_indexes.iter().enumerate()
    {
        if let Err(index_issues) = validate_protected_index(
            &program.store,
            protected_index,
            platform.crypto.security_bits,
        ) {
            issues.push(ProgramIssue::ProtectedIndex {
                index,
                issues: index_issues,
            });
        }
    }

    for (index, task_plan) in program.task_plans.iter().enumerate() {
        if let Err(task_issues) = validate_task_plan(task_plan) {
            issues.push(ProgramIssue::TaskPlan {
                plan: index,
                issues: task_issues,
            });
        }
    }

    let mut storage_crypto_issues = Vec::new();
    if let Err(profile_issues) = validate_storage_crypto_profile(
        &platform.storage_crypto,
        platform.crypto.security_bits,
    ) {
        storage_crypto_issues.extend(profile_issues);
    }
    if let Err(coverage_issues) = validate_binding_coverage(
        &program.store,
        &program.storage_crypto_bindings,
    ) {
        storage_crypto_issues.extend(coverage_issues);
    }
    for binding in &program.storage_crypto_bindings {
        if let Err(binding_issues) = validate_field_binding(
            &program.store,
            &platform.storage_crypto,
            binding,
        ) {
            storage_crypto_issues.extend(binding_issues);
        }
    }
    if !storage_crypto_issues.is_empty() {
        issues.push(ProgramIssue::StorageCrypto(storage_crypto_issues));
    }

    for (index, transaction) in program.transactions.iter().enumerate() {
        if let Err(transaction_issues) = validate_transaction(transaction) {
            issues.push(ProgramIssue::Transaction {
                transaction: index,
                issues: transaction_issues,
            });
        }
    }

    for (index, migration) in program.migrations.iter().enumerate() {
        if let Err(migration_issues) = validate_migration(migration) {
            issues.push(ProgramIssue::Migration {
                migration: index,
                issues: migration_issues,
            });
        }
    }

    for (index, profile) in program.authentication.iter().enumerate() {
        if let Err(auth_issues) = validate_authentication_profile(profile) {
            issues.push(ProgramIssue::Authentication {
                profile: index,
                issues: auth_issues,
            });
        }
    }

    for (index, otp) in program.one_time_codes.iter().enumerate() {
        if let Err(otp_issues) = validate_one_time_code(*otp) {
            issues.push(ProgramIssue::OneTimeCode {
                profile: index,
                issues: otp_issues,
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
    use crate::hardware::{CacheProfile, HardwareFeature};
    use crate::io::{IoSourceKind, IoTransform};
    use crate::math::{MathMode, RoundingMode};
    use crate::network::ConnectionFeature;
    use crate::text::{CoreTextCodec, TextCodec, TextContract};
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
            math: MathProfile {
                id: "math.current".into(),
                max_precision_bits: 4096,
                deterministic: true,
                correctly_rounded_transcendentals: true,
                supported_rounding: vec![
                    RoundingMode::NearestEven,
                    RoundingMode::TowardZero,
                    RoundingMode::TowardPositive,
                    RoundingMode::TowardNegative,
                ],
            },
            text: TextContract::strict("platform-selected"),
            storage_crypto: StorageCryptoProfile {
                id: "store.secure".into(),
                minimum_security_bits: 192,
                current_key_version: 1,
                bind_tenant_as_aad: true,
                bind_resource_as_aad: true,
                bind_field_as_aad: true,
            },
            scheduler: SchedulerProfile {
                parallel_capacity: 12,
                work_stealing: true,
                numa_aware: true,
            },
            hardware: HardwareProfile {
                id: "x86_64-v3-dev".into(),
                logical_cores: 12,
                physical_cores: 6,
                memory_bytes: 32 * 1024 * 1024 * 1024,
                numa_nodes: 1,
                cache: CacheProfile {
                    line_bytes: 64,
                    l1_data_bytes: 32 * 1024,
                    l2_bytes_per_core: 512 * 1024,
                    l3_bytes_total: 32 * 1024 * 1024,
                },
                features: [
                    HardwareFeature::Vector128,
                    HardwareFeature::Vector256,
                    HardwareFeature::FusedMultiplyAdd,
                    HardwareFeature::BitManipulation,
                    HardwareFeature::PopulationCount,
                    HardwareFeature::Iommu,
                ]
                .into_iter()
                .collect(),
            },
        }
    }

    #[test]
    fn explicit_entry_must_name_existing_graph() {
        let program = ProgramContract {
            entry_graph: Some("missing".into()),
            ..ProgramContract::default()
        };

        assert_eq!(
            validate_program(&program, &platform()),
            Err(vec![ProgramIssue::UnknownEntryGraph("missing".into())])
        );
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
    fn math_precision_is_part_of_program_validation() {
        let program = ProgramContract {
            math: vec![MathUse {
                operation: MathOperation::Sin,
                requirement: MathRequirement {
                    mode: MathMode::Exact,
                    precision_bits: ParameterPolicy::Fixed(256),
                    rounding: RoundingMode::NearestEven,
                    max_relative_error_ppb: Some(0),
                    allow_reassociation: false,
                },
            }],
            ..ProgramContract::default()
        };

        let issues = validate_program(&program, &platform()).unwrap_err();
        assert!(
            issues
                .iter()
                .any(|issue| matches!(issue, ProgramIssue::Math { .. }))
        );
    }

    #[test]
    fn invalid_text_platform_contract_blocks_program() {
        let mut platform = platform();
        platform.text.allow_raw_byte_indexing = true;

        let issues =
            validate_program(&ProgramContract::default(), &platform).unwrap_err();
        assert!(
            issues
                .iter()
                .any(|issue| matches!(issue, ProgramIssue::Text(_)))
        );
    }

    #[test]
    fn text_codec_is_selected_per_boundary_without_changing_text_type() {
        let mut platform = platform();
        platform.text.enable_core_codec(CoreTextCodec::Utf16Le);

        let program = ProgramContract {
            text_boundaries: vec![
                TextBoundaryRequirement {
                    name: "api".into(),
                    codec: TextCodec::Core(CoreTextCodec::Utf8),
                },
                TextBoundaryRequirement {
                    name: "legacy-file".into(),
                    codec: TextCodec::Core(CoreTextCodec::Utf16Le),
                },
            ],
            ..ProgramContract::default()
        };

        assert!(validate_program(&program, &platform).is_ok());
    }

    #[test]
    fn text_io_without_decode_is_rejected_by_program_gate() {
        let program = ProgramContract {
            io_pipelines: vec![(
                IoPipeline {
                    source: IoSourceKind::File,
                    transforms: vec![IoTransform::ReadBytes],
                },
                IoValueKind::Text,
            )],
            ..ProgramContract::default()
        };

        let issues = validate_program(&program, &platform()).unwrap_err();
        assert!(
            issues
                .iter()
                .any(|issue| matches!(issue, ProgramIssue::Io { .. }))
        );
    }

    #[test]
    fn invalid_zero_copy_memory_plan_blocks_program() {
        use crate::memory::{
            AllocationIntent, LayoutKind, LifetimeClass, MemoryDomain,
        };

        let program = ProgramContract {
            memory: vec![MemoryPlan {
                allocations: vec![AllocationIntent {
                    id: "buffer".into(),
                    size_bytes: ParameterPolicy::Fixed(4096),
                    alignment_bytes: ParameterPolicy::Fixed(64),
                    lifetime: LifetimeClass::Graph,
                    interval: None,
                    allowed_domains: [MemoryDomain::Stack]
                        .into_iter()
                        .collect(),
                    layout: vec![LayoutKind::Native],
                    zero_copy: true,
                    movable: true,
                    pinned: false,
                }],
            }],
            ..ProgramContract::default()
        };

        let issues = validate_program(&program, &platform()).unwrap_err();
        assert!(
            issues
                .iter()
                .any(|issue| matches!(issue, ProgramIssue::Memory { .. }))
        );
    }

    #[test]
    fn hardware_requirement_is_a_compile_gate() {
        let program = ProgramContract {
            hardware: HardwareRequirement {
                min_logical_cores: 8,
                min_memory_bytes: 8 * 1024 * 1024 * 1024,
                required_features: [HardwareFeature::Vector512]
                    .into_iter()
                    .collect(),
            },
            ..ProgramContract::default()
        };

        let issues = validate_program(&program, &platform()).unwrap_err();
        assert!(
            issues
                .iter()
                .any(|issue| matches!(issue, ProgramIssue::Hardware(_)))
        );
    }

    #[test]
    fn ambient_semantics_are_compile_gates() {
        use crate::runtime::{RegionKind, RegionSpec};

        let program = ProgramContract {
            regions: vec![RegionPlan {
                regions: vec![RegionSpec {
                    id: "secret".into(),
                    parent: None,
                    kind: RegionKind::Secret,
                    max_bytes: Some(4096),
                    zero_on_release: false,
                }],
                ..RegionPlan::default()
            }],
            time_uses: vec![(ClockKind::Wall, TimeUse::Timeout)],
            randomness_uses: vec![(
                RandomnessClass::Deterministic,
                RandomUse::SessionKey,
            )],
            ..ProgramContract::default()
        };

        let issues = validate_program(&program, &platform()).unwrap_err();
        assert!(issues.iter().any(|issue| {
            matches!(issue, ProgramIssue::Region { .. })
        }));
        assert!(issues.iter().any(|issue| {
            matches!(issue, ProgramIssue::Time { .. })
        }));
        assert!(issues.iter().any(|issue| {
            matches!(issue, ProgramIssue::Entropy { .. })
        }));
    }

    #[test]
    fn invalid_schedule_is_rejected_by_program_gate() {
        use crate::scheduler::{
            Affinity, ParallelismPolicy, SchedulingClass, TaskSpec,
        };

        let program = ProgramContract {
            schedules: vec![ScheduleGraph {
                tasks: vec![
                    TaskSpec {
                        id: 1,
                        dependencies: [2].into_iter().collect(),
                        class: SchedulingClass::Throughput,
                        parallelism: ParallelismPolicy::Auto,
                        affinity: Affinity::Any,
                        estimated_cost_ns: 1,
                    },
                    TaskSpec {
                        id: 2,
                        dependencies: [1].into_iter().collect(),
                        class: SchedulingClass::Throughput,
                        parallelism: ParallelismPolicy::Auto,
                        affinity: Affinity::Any,
                        estimated_cost_ns: 1,
                    },
                ],
            }],
            ..ProgramContract::default()
        };

        let issues = validate_program(&program, &platform()).unwrap_err();
        assert!(issues.iter().any(|issue| {
            matches!(issue, ProgramIssue::Schedule { .. })
        }));
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
