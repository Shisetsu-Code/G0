use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::gir::{
    CapabilityClass, Effect, Graph, IntegerType, Literal, Node, NodeId, Operation, Port,
    SemanticType, SourceEndpoint, TargetEndpoint, type_assignable,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationCode {
    DuplicateNodeId,
    DuplicatePortId,
    DuplicatePortName,
    UnknownSource,
    UnknownTarget,
    TypeMismatch,
    MissingInputDriver,
    MultipleInputDrivers,
    LinearFanout,
    MissingGraphOutputDriver,
    RawCycle,
    OperationShape,
    ArithmeticRange,
    ArithmeticRangeUnprovable,
    EffectWithoutCapability,
    ImportHasEffects,
    ImportHasCapabilities,
    ExecutionMissingEffect,
    ExecutionMissingCapability,
    StorageMissingEffect,
    StorageMissingCapability,
    PureNodeHasEffects,
    PureNodeHasCapabilities,
    UnorderedConflictingEffects {
        first: NodeId,
        second: NodeId,
        effect: Effect,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub code: ValidationCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }

    fn push(&mut self, code: ValidationCode, message: impl Into<String>) {
        self.issues.push(ValidationIssue {
            code,
            message: message.into(),
        });
    }
}

impl fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} GIR validation error(s):", self.issues.len())?;
        for issue in &self.issues {
            writeln!(f, "- {:?}: {}", issue.code, issue.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationReport {}

pub fn validate(graph: &Graph) -> Result<(), ValidationReport> {
    let mut report = ValidationReport::default();

    validate_port_set("graph input", &graph.inputs, &mut report);
    validate_port_set("graph output", &graph.outputs, &mut report);

    let mut node_ids = BTreeSet::new();
    for node in &graph.nodes {
        if !node_ids.insert(node.id) {
            report.push(
                ValidationCode::DuplicateNodeId,
                format!("node id {} is declared more than once", node.id),
            );
        }
        validate_port_set(
            &format!("node {} input", node.id),
            &node.inputs,
            &mut report,
        );
        validate_port_set(
            &format!("node {} output", node.id),
            &node.outputs,
            &mut report,
        );
        validate_operation(node, &mut report);
        validate_effect_capabilities(node, &mut report);
    }

    let mut driver_count: BTreeMap<TargetEndpoint, usize> = BTreeMap::new();
    let mut adjacency: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    let mut linear_sources = BTreeSet::new();

    for edge in &graph.edges {
        let source_ty = resolve_source_type(graph, &edge.from);
        let target_ty = resolve_target_type(graph, &edge.to);
        if source_ty.is_some_and(|ty| contains_unique(ty, 0))
            && !linear_sources.insert(edge.from.clone())
        {
            report.push(
                ValidationCode::LinearFanout,
                format!("linear source {:?} has more than one consumer", edge.from),
            );
        }

        if source_ty.is_none() {
            report.push(
                ValidationCode::UnknownSource,
                format!("edge source {:?} does not exist", edge.from),
            );
        }

        if target_ty.is_none() {
            report.push(
                ValidationCode::UnknownTarget,
                format!("edge target {:?} does not exist", edge.to),
            );
        }

        if let (Some(source_ty), Some(target_ty)) = (source_ty, target_ty)
            && !type_assignable(source_ty, target_ty)
        {
            report.push(
                ValidationCode::TypeMismatch,
                format!(
                    "edge {:?} -> {:?} connects incompatible types {:?} and {:?}; G0 has no implicit casts",
                    edge.from, edge.to, source_ty, target_ty
                ),
            );
        }

        *driver_count.entry(edge.to.clone()).or_insert(0) += 1;

        if let (
            SourceEndpoint::NodeOutput { node: from, .. },
            TargetEndpoint::NodeInput { node: to, .. },
        ) = (&edge.from, &edge.to)
            && node_ids.contains(from)
            && node_ids.contains(to)
        {
            adjacency.entry(*from).or_default().push(*to);
        }
    }

    for node in &graph.nodes {
        for port in &node.inputs {
            let target = TargetEndpoint::NodeInput {
                node: node.id,
                port: port.id,
            };
            match driver_count.get(&target).copied().unwrap_or(0) {
                0 => report.push(
                    ValidationCode::MissingInputDriver,
                    format!("node {} input '{}' has no source", node.id, port.name),
                ),
                1 => {}
                n => report.push(
                    ValidationCode::MultipleInputDrivers,
                    format!(
                        "node {} input '{}' has {} sources; an input port must have exactly one driver",
                        node.id, port.name, n
                    ),
                ),
            }
        }
    }

    for port in &graph.outputs {
        let target = TargetEndpoint::GraphOutput(port.id);
        match driver_count.get(&target).copied().unwrap_or(0) {
            0 => report.push(
                ValidationCode::MissingGraphOutputDriver,
                format!("graph output '{}' has no source", port.name),
            ),
            1 => {}
            n => report.push(
                ValidationCode::MultipleInputDrivers,
                format!(
                    "graph output '{}' has {} sources; a graph output must have exactly one driver",
                    port.name, n
                ),
            ),
        }
    }

    if let Some(node) = detect_cycle(&node_ids, &adjacency) {
        report.push(
            ValidationCode::RawCycle,
            format!(
                "raw graph cycle detected through node {}; iteration must be owned by a structured Loop primitive",
                node
            ),
        );
    }

    let reachability = transitive_reachability(&node_ids, &adjacency);
    for (index, first) in graph.nodes.iter().enumerate() {
        for second in graph.nodes.iter().skip(index + 1) {
            for effect in first.effects.intersection(&second.effects) {
                let ordered = reaches(&reachability, first.id, second.id)
                    || reaches(&reachability, second.id, first.id);
                if !ordered {
                    report.push(
                        ValidationCode::UnorderedConflictingEffects {
                            first: first.id,
                            second: second.id,
                            effect: *effect,
                        },
                        format!(
                            "nodes {} and {} both perform {:?} without a dependency ordering them",
                            first.id, second.id, effect
                        ),
                    );
                }
            }
        }
    }

    if report.is_empty() {
        Ok(())
    } else {
        Err(report)
    }
}

fn validate_port_set(label: &str, ports: &[Port], report: &mut ValidationReport) {
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();

    for port in ports {
        if !ids.insert(port.id) {
            report.push(
                ValidationCode::DuplicatePortId,
                format!("{label} port id {} is duplicated", port.id),
            );
        }
        if !names.insert(port.name.as_str()) {
            report.push(
                ValidationCode::DuplicatePortName,
                format!("{label} port name '{}' is duplicated", port.name),
            );
        }
    }
}

fn contains_unique(ty: &SemanticType, depth: usize) -> bool {
    if depth >= 128 {
        return true;
    }
    match ty {
        SemanticType::Unique(_) => true,
        SemanticType::Array(t, _)
        | SemanticType::Vector(t, _)
        | SemanticType::Slice(t)
        | SemanticType::Option(t)
        | SemanticType::Secret(t)
        | SemanticType::Credential(t)
        | SemanticType::State(t)
        | SemanticType::Atomic(t)
        | SemanticType::Versioned(t) => contains_unique(t, depth + 1),
        SemanticType::Result(a, b) => {
            contains_unique(a, depth + 1) || contains_unique(b, depth + 1)
        }
        _ => false,
    }
}

fn resolve_source_type<'a>(graph: &'a Graph, source: &SourceEndpoint) -> Option<&'a SemanticType> {
    match source {
        SourceEndpoint::GraphInput(port) => {
            graph.inputs.iter().find(|p| p.id == *port).map(|p| &p.ty)
        }
        SourceEndpoint::NodeOutput { node, port } => graph
            .nodes
            .iter()
            .find(|n| n.id == *node)
            .and_then(|n| n.outputs.iter().find(|p| p.id == *port))
            .map(|p| &p.ty),
    }
}

fn resolve_target_type<'a>(graph: &'a Graph, target: &TargetEndpoint) -> Option<&'a SemanticType> {
    match target {
        TargetEndpoint::GraphOutput(port) => {
            graph.outputs.iter().find(|p| p.id == *port).map(|p| &p.ty)
        }
        TargetEndpoint::NodeInput { node, port } => graph
            .nodes
            .iter()
            .find(|n| n.id == *node)
            .and_then(|n| n.inputs.iter().find(|p| p.id == *port))
            .map(|p| &p.ty),
    }
}

fn validate_operation(node: &Node, report: &mut ValidationReport) {
    match &node.operation {
        Operation::RegionOpen
        | Operation::RegionOpenSecret
        | Operation::RegionOpenChild { .. }
        | Operation::RegionAllocate
        | Operation::RegionWrite
        | Operation::RegionRead
        | Operation::RegionClose => {
            if let Err(message) = crate::resource_host::validate_node(node) {
                report.push(
                    ValidationCode::OperationShape,
                    format!("node {}: {message}", node.id),
                );
            }
        }
        Operation::Const(literal) => {
            require_shape(node, 0, 1, report);
            if let Some(output) = node.outputs.first()
                && !literal_fits(literal, &output.ty)
            {
                report.push(
                    ValidationCode::OperationShape,
                    format!(
                        "node {} constant is incompatible with output type {:?}",
                        node.id, output.ty
                    ),
                );
            }
            require_pure(node, report);
        }
        Operation::Add | Operation::Sub | Operation::Mul | Operation::Div | Operation::Rem => {
            require_shape(node, 2, 1, report);
            require_pure(node, report);
            validate_integer_arithmetic(node, report);
        }
        Operation::Eq | Operation::Lt | Operation::Le | Operation::Gt | Operation::Ge => {
            require_shape(node, 2, 1, report);
            require_pure(node, report);
            validate_integer_comparison(node, report);
        }
        Operation::And | Operation::Or | Operation::Xor => {
            require_shape(node, 2, 1, report);
            require_pure(node, report);
            validate_boolean_operation(node, 2, report);
        }
        Operation::Not => {
            require_shape(node, 1, 1, report);
            require_pure(node, report);
            validate_boolean_operation(node, 1, report);
        }
        Operation::ConvertChecked => {
            require_shape(node, 1, 1, report);
            require_pure(node, report);
            validate_checked_conversion(node, report);
        }
        Operation::MakeArray
        | Operation::Index
        | Operation::Length
        | Operation::TextConcat
        | Operation::TextJoin
        | Operation::BytesConcat
        | Operation::EncodeUtf8
        | Operation::DecodeUtf8
        | Operation::FormatInteger
        | Operation::MakeRecord { .. }
        | Operation::Field { .. }
        | Operation::MakeVariant { .. }
        | Operation::VariantPayload { .. }
        | Operation::Some
        | Operation::None
        | Operation::Ok
        | Operation::Err
        | Operation::UnwrapOr => {
            require_pure(node, report);
            if let Err(message) = crate::composite::validate_node(node, None) {
                report.push(
                    ValidationCode::OperationShape,
                    format!("node {}: {message}", node.id),
                );
            }
        }
        Operation::Truncate { bits, signed } => {
            require_shape(node, 1, 1, report);
            require_pure(node, report);
            validate_truncate(node, *bits, *signed, report);
        }
        Operation::Import(_) => {
            if !node.effects.is_empty() {
                report.push(
                    ValidationCode::ImportHasEffects,
                    format!("import node {} declares effects; import is definition-only and may never execute", node.id),
                );
            }
            if !node.required_capabilities.is_empty() {
                report.push(
                    ValidationCode::ImportHasCapabilities,
                    format!("import node {} requests capabilities; importing must never grant or require execution authority", node.id),
                );
            }
        }
        Operation::StoreRead { .. }
        | Operation::StoreCreate { .. }
        | Operation::StoreUpdate { .. }
        | Operation::StoreDelete { .. }
        | Operation::StoreEnumerate { .. }
        | Operation::StoreSetRelation { .. }
        | Operation::StoreTraverse { .. } => {
            require_storage(node, report);
        }
        Operation::StoreSetCredential { .. } | Operation::StoreVerifyCredential { .. } => {
            require_storage(node, report);
            let credential = node.inputs.get(1).is_some_and(|p| matches!(&p.ty, SemanticType::Credential(t) if matches!(t.as_ref(), SemanticType::Text | SemanticType::Bytes)));
            let shape = (2..=3).contains(&node.inputs.len())
                && node.outputs.len() == 1
                && node
                    .inputs
                    .first()
                    .is_some_and(|p| p.ty == SemanticType::Text)
                && credential
                && (node.inputs.len() == 2
                    || matches!(node.inputs[2].ty, SemanticType::Integer(_)))
                && node.outputs.first().is_some_and(|p| {
                    if matches!(node.operation, Operation::StoreVerifyCredential { .. }) {
                        p.ty == SemanticType::Bool
                    } else {
                        matches!(p.ty, SemanticType::Integer(_))
                    }
                });
            if !shape {
                report.push(ValidationCode::OperationShape, format!("credential node {} requires ID, typed credential, optional mutation version and one result", node.id));
            }
        }
        Operation::LocalExecute(_) => {
            require_execution(
                node,
                Effect::LocalExecution,
                CapabilityClass::LocalExecution,
                "local execution",
                report,
            );
        }
        Operation::RemoteExecute { .. } => {
            require_execution(
                node,
                Effect::RemoteExecution,
                CapabilityClass::RemoteExecution,
                "remote execution",
                report,
            );
        }
        Operation::Select { .. }
        | Operation::TaskSpawn { .. }
        | Operation::TaskJoin { .. }
        | Operation::TaskSpawnScoped { .. }
        | Operation::TaskJoinScoped { .. }
        | Operation::TaskSpawnHosted { .. }
        | Operation::TaskJoinHosted { .. }
        | Operation::Map { .. }
        | Operation::Match { .. }
        | Operation::Loop { .. }
        | Operation::Subgraph(_)
        | Operation::Instantiate(_) => {}
    }
}

fn require_shape(node: &Node, inputs: usize, outputs: usize, report: &mut ValidationReport) {
    if node.inputs.len() != inputs || node.outputs.len() != outputs {
        report.push(
            ValidationCode::OperationShape,
            format!(
                "node {} requires {} input(s) and {} output(s), got {} and {}",
                node.id,
                inputs,
                outputs,
                node.inputs.len(),
                node.outputs.len()
            ),
        );
    }
}

fn require_pure(node: &Node, report: &mut ValidationReport) {
    if !node.effects.is_empty() {
        report.push(
            ValidationCode::PureNodeHasEffects,
            format!("pure node {} declares effects", node.id),
        );
    }
    if !node.required_capabilities.is_empty() {
        report.push(
            ValidationCode::PureNodeHasCapabilities,
            format!("pure node {} requests capabilities", node.id),
        );
    }
}

fn literal_fits(literal: &Literal, ty: &SemanticType) -> bool {
    match (literal, ty) {
        (Literal::Bool(_), SemanticType::Bool) => true,
        (Literal::Integer(value), SemanticType::Integer(range)) => {
            *value >= range.min && *value <= range.max
        }
        (Literal::Text(_), SemanticType::Text) => true,
        (Literal::Bytes(_), SemanticType::Bytes) => true,
        _ => false,
    }
}

fn validate_truncate(node: &Node, bits: u16, signed: bool, report: &mut ValidationReport) {
    if node.inputs.len() != 1 || node.outputs.len() != 1 {
        return;
    }

    if !matches!(node.inputs[0].ty, SemanticType::Integer(_)) {
        report.push(
            ValidationCode::OperationShape,
            format!(
                "node {} Truncate accepts semantic Integer input only",
                node.id
            ),
        );
        return;
    }

    let Some(expected) = truncate_integer_range(bits, signed) else {
        report.push(
            ValidationCode::OperationShape,
            format!(
                "node {} Truncate supports bit widths 1..=64 in the bootstrap backend",
                node.id
            ),
        );
        return;
    };

    if node.outputs[0].ty != SemanticType::Integer(expected.clone()) {
        report.push(
            ValidationCode::OperationShape,
            format!(
                "node {} Truncate({}, signed={}) must declare output range {}..={}",
                node.id, bits, signed, expected.min, expected.max
            ),
        );
    }
}

fn truncate_integer_range(bits: u16, signed: bool) -> Option<IntegerType> {
    if bits == 0 || bits > 64 {
        return None;
    }

    if signed {
        let magnitude = 1_i128.checked_shl(u32::from(bits - 1))?;
        Some(IntegerType {
            min: -magnitude,
            max: magnitude - 1,
        })
    } else {
        let span = 1_i128.checked_shl(u32::from(bits))?;
        Some(IntegerType {
            min: 0,
            max: span - 1,
        })
    }
}

fn validate_checked_conversion(node: &Node, report: &mut ValidationReport) {
    if node.inputs.len() != 1 || node.outputs.len() != 1 {
        return;
    }

    if !matches!(node.inputs[0].ty, SemanticType::Integer(_))
        || !matches!(node.outputs[0].ty, SemanticType::Integer(_))
    {
        report.push(
            ValidationCode::OperationShape,
            format!(
                "node {} ConvertChecked accepts and returns semantic Integer values only",
                node.id
            ),
        );
    }
}

fn validate_integer_comparison(node: &Node, report: &mut ValidationReport) {
    if node.inputs.len() != 2 || node.outputs.len() != 1 {
        return;
    }

    let valid_inputs = matches!(
        (&node.inputs[0].ty, &node.inputs[1].ty),
        (SemanticType::Integer(left), SemanticType::Integer(right))
            if left == right
    );
    let valid_output = node.outputs[0].ty == SemanticType::Bool;

    if !valid_inputs || !valid_output {
        report.push(
            ValidationCode::OperationShape,
            format!(
                "node {} {:?} requires two identical semantic Integer inputs and one Bool output",
                node.id, node.operation
            ),
        );
    }
}

fn validate_boolean_operation(node: &Node, expected_inputs: usize, report: &mut ValidationReport) {
    if node.inputs.len() != expected_inputs || node.outputs.len() != 1 {
        return;
    }

    let inputs_are_bool = node.inputs.iter().all(|port| port.ty == SemanticType::Bool);
    let output_is_bool = node.outputs[0].ty == SemanticType::Bool;

    if !inputs_are_bool || !output_is_bool {
        report.push(
            ValidationCode::OperationShape,
            format!(
                "node {} {:?} accepts Bool inputs and returns Bool only",
                node.id, node.operation
            ),
        );
    }
}

fn validate_integer_arithmetic(node: &Node, report: &mut ValidationReport) {
    if node.inputs.len() != 2 || node.outputs.len() != 1 {
        return;
    }

    let (Some(a), Some(b), Some(out)) = (
        integer_type(&node.inputs[0].ty),
        integer_type(&node.inputs[1].ty),
        integer_type(&node.outputs[0].ty),
    ) else {
        report.push(
            ValidationCode::OperationShape,
            format!(
                "node {} {:?} accepts and returns semantic Integer values only",
                node.id, node.operation
            ),
        );
        return;
    };

    let needed = match &node.operation {
        Operation::Add => range_add(a, b),
        Operation::Sub => range_sub(a, b),
        Operation::Mul => range_mul(a, b),
        Operation::Div => range_div(a, b),
        Operation::Rem => range_rem(a, b),
        _ => unreachable!(),
    };

    match needed {
        Some(needed) => {
            if !out.contains(&needed) {
                report.push(
                    ValidationCode::ArithmeticRange,
                    format!(
                        "node {} {:?} may produce range {}..={}, but output declares {}..={}",
                        node.id, node.operation, needed.min, needed.max, out.min, out.max
                    ),
                );
            }
        }
        None => report.push(
            ValidationCode::ArithmeticRangeUnprovable,
            format!(
                "node {} {:?} range exceeds current GIR i128 proof domain; use a wider future semantic integer domain or explicit checked semantics",
                node.id, node.operation
            ),
        ),
    }
}

fn integer_type(ty: &SemanticType) -> Option<&IntegerType> {
    match ty {
        SemanticType::Integer(value) => Some(value),
        _ => None,
    }
}

fn range_add(a: &IntegerType, b: &IntegerType) -> Option<IntegerType> {
    Some(IntegerType {
        min: a.min.checked_add(b.min)?,
        max: a.max.checked_add(b.max)?,
    })
}

fn range_sub(a: &IntegerType, b: &IntegerType) -> Option<IntegerType> {
    Some(IntegerType {
        min: a.min.checked_sub(b.max)?,
        max: a.max.checked_sub(b.min)?,
    })
}

fn range_mul(a: &IntegerType, b: &IntegerType) -> Option<IntegerType> {
    let values = [
        a.min.checked_mul(b.min)?,
        a.min.checked_mul(b.max)?,
        a.max.checked_mul(b.min)?,
        a.max.checked_mul(b.max)?,
    ];
    Some(IntegerType {
        min: *values.iter().min()?,
        max: *values.iter().max()?,
    })
}

fn range_div(a: &IntegerType, b: &IntegerType) -> Option<IntegerType> {
    let mut divisors = Vec::new();
    for candidate in [b.min, b.max, -1, 1] {
        if candidate >= b.min
            && candidate <= b.max
            && candidate != 0
            && !divisors.contains(&candidate)
        {
            divisors.push(candidate);
        }
    }

    if divisors.is_empty() {
        return None;
    }

    let mut values = Vec::new();
    for numerator in [a.min, a.max] {
        for divisor in &divisors {
            if let Some(value) = numerator.checked_div(*divisor) {
                values.push(value);
            }
        }
    }

    Some(IntegerType {
        min: *values.iter().min()?,
        max: *values.iter().max()?,
    })
}

fn range_rem(a: &IntegerType, b: &IntegerType) -> Option<IntegerType> {
    if b.min == 0 && b.max == 0 {
        return None;
    }
    if a.min == i128::MIN && a.max == i128::MIN && b.min == -1 && b.max == -1 {
        return None;
    }

    let max_abs = b.min.unsigned_abs().max(b.max.unsigned_abs());
    if max_abs == 0 {
        return None;
    }

    let bound_u = max_abs.saturating_sub(1).min(i128::MAX as u128);
    let bound = bound_u as i128;

    Some(IntegerType {
        min: if a.min < 0 { -bound } else { 0 },
        max: if a.max > 0 { bound } else { 0 },
    })
}

fn capability_class_for_effect(effect: Effect) -> Option<CapabilityClass> {
    match effect {
        Effect::MemoryWrite => None,
        Effect::Storage => Some(CapabilityClass::Storage),
        Effect::Network => Some(CapabilityClass::Network),
        Effect::Clock => Some(CapabilityClass::Clock),
        Effect::Entropy => Some(CapabilityClass::Entropy),
        Effect::Device => Some(CapabilityClass::Device),
        Effect::Process => Some(CapabilityClass::Process),
        Effect::LocalExecution => Some(CapabilityClass::LocalExecution),
        Effect::RemoteExecution => Some(CapabilityClass::RemoteExecution),
        Effect::Accelerator => Some(CapabilityClass::Accelerator),
        Effect::Audit => Some(CapabilityClass::Audit),
    }
}

fn validate_effect_capabilities(node: &Node, report: &mut ValidationReport) {
    for effect in &node.effects {
        let Some(class) = capability_class_for_effect(*effect) else {
            continue;
        };
        if !node
            .required_capabilities
            .iter()
            .any(|cap| cap.class == class)
        {
            report.push(
                ValidationCode::EffectWithoutCapability,
                format!(
                    "node {} declares {:?} effect without a matching {:?} capability",
                    node.id, effect, class
                ),
            );
        }
    }
}

fn require_storage(node: &Node, report: &mut ValidationReport) {
    if !node.effects.contains(&Effect::Storage) {
        report.push(
            ValidationCode::StorageMissingEffect,
            format!(
                "node {} {:?} is a Store operation but does not declare Storage effect",
                node.id, node.operation
            ),
        );
    }

    if !node
        .required_capabilities
        .iter()
        .any(|cap| cap.class == CapabilityClass::Storage)
    {
        report.push(
            ValidationCode::StorageMissingCapability,
            format!(
                "node {} {:?} is a Store operation without explicit Storage capability",
                node.id, node.operation
            ),
        );
    }
}

fn require_execution(
    node: &Node,
    effect: Effect,
    class: CapabilityClass,
    label: &str,
    report: &mut ValidationReport,
) {
    if !node.effects.contains(&effect) {
        report.push(
            ValidationCode::ExecutionMissingEffect,
            format!(
                "node {} performs {label} but does not declare {:?} effect",
                node.id, effect
            ),
        );
    }
    if !node
        .required_capabilities
        .iter()
        .any(|cap| cap.class == class)
    {
        report.push(
            ValidationCode::ExecutionMissingCapability,
            format!(
                "node {} performs {label} without explicit {:?} capability",
                node.id, class
            ),
        );
    }
}

fn transitive_reachability(
    node_ids: &BTreeSet<NodeId>,
    adjacency: &BTreeMap<NodeId, Vec<NodeId>>,
) -> BTreeMap<NodeId, BTreeSet<NodeId>> {
    let mut result = BTreeMap::new();

    for node in node_ids {
        let mut seen = BTreeSet::new();
        let mut stack = vec![*node];

        while let Some(current) = stack.pop() {
            if let Some(children) = adjacency.get(&current) {
                for child in children {
                    if seen.insert(*child) {
                        stack.push(*child);
                    }
                }
            }
        }

        result.insert(*node, seen);
    }

    result
}

fn reaches(reachability: &BTreeMap<NodeId, BTreeSet<NodeId>>, from: NodeId, to: NodeId) -> bool {
    reachability
        .get(&from)
        .is_some_and(|nodes| nodes.contains(&to))
}

fn detect_cycle(
    node_ids: &BTreeSet<NodeId>,
    adjacency: &BTreeMap<NodeId, Vec<NodeId>>,
) -> Option<NodeId> {
    fn visit(
        node: NodeId,
        adjacency: &BTreeMap<NodeId, Vec<NodeId>>,
        state: &mut BTreeMap<NodeId, u8>,
    ) -> Option<NodeId> {
        match state.get(&node).copied().unwrap_or(0) {
            1 => return Some(node),
            2 => return None,
            _ => {}
        }

        state.insert(node, 1);
        if let Some(next) = adjacency.get(&node) {
            for child in next {
                if let Some(cycle) = visit(*child, adjacency, state) {
                    return Some(cycle);
                }
            }
        }
        state.insert(node, 2);
        None
    }

    let mut state = BTreeMap::new();
    for node in node_ids {
        if let Some(cycle) = visit(*node, adjacency, &mut state) {
            return Some(cycle);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{
        Capability, Edge, Graph, Operation, Port, SemanticType, SourceEndpoint, TargetEndpoint,
    };

    fn integer(min: i128, max: i128) -> SemanticType {
        SemanticType::Integer(IntegerType::new(min, max).unwrap())
    }

    fn port(id: u16, name: &str, ty: SemanticType) -> Port {
        Port {
            id,
            name: name.into(),
            ty,
        }
    }

    fn pure_node(id: u32, operation: Operation, inputs: Vec<Port>, outputs: Vec<Port>) -> Node {
        Node {
            id,
            operation,
            inputs,
            outputs,
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        }
    }

    #[test]
    fn validates_typed_graph_with_graph_boundaries() {
        let mut graph = Graph::new("add");
        graph.inputs = vec![port(0, "a", integer(0, 10)), port(1, "b", integer(0, 20))];
        graph.outputs = vec![port(0, "sum", integer(0, 30))];
        graph.nodes = vec![pure_node(
            10,
            Operation::Add,
            vec![port(0, "a", integer(0, 10)), port(1, "b", integer(0, 20))],
            vec![port(0, "sum", integer(0, 30))],
        )];
        graph.edges = vec![
            Edge {
                from: SourceEndpoint::GraphInput(0),
                to: TargetEndpoint::NodeInput { node: 10, port: 0 },
            },
            Edge {
                from: SourceEndpoint::GraphInput(1),
                to: TargetEndpoint::NodeInput { node: 10, port: 1 },
            },
            Edge {
                from: SourceEndpoint::NodeOutput { node: 10, port: 0 },
                to: TargetEndpoint::GraphOutput(0),
            },
        ];

        assert!(validate(&graph).is_ok());
    }

    #[test]
    fn rejects_implicit_casts_on_edges() {
        let mut graph = Graph::new("bad");
        graph.inputs = vec![port(0, "a", SemanticType::Bool)];
        graph.outputs = vec![port(0, "out", integer(0, 1))];
        graph.edges = vec![Edge {
            from: SourceEndpoint::GraphInput(0),
            to: TargetEndpoint::GraphOutput(0),
        }];

        let report = validate(&graph).unwrap_err();
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.code == ValidationCode::TypeMismatch)
        );
    }

    #[test]
    fn rejects_arithmetic_range_that_cannot_hold_result() {
        let mut graph = Graph::new("overflow");
        graph.outputs = vec![port(0, "sum", integer(0, 10))];
        graph.nodes = vec![
            pure_node(
                1,
                Operation::Const(Literal::Integer(10)),
                vec![],
                vec![port(0, "v", integer(10, 10))],
            ),
            pure_node(
                2,
                Operation::Const(Literal::Integer(10)),
                vec![],
                vec![port(0, "v", integer(10, 10))],
            ),
            pure_node(
                3,
                Operation::Add,
                vec![port(0, "a", integer(10, 10)), port(1, "b", integer(10, 10))],
                vec![port(0, "sum", integer(0, 10))],
            ),
        ];
        graph.edges = vec![
            Edge {
                from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
                to: TargetEndpoint::NodeInput { node: 3, port: 0 },
            },
            Edge {
                from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
                to: TargetEndpoint::NodeInput { node: 3, port: 1 },
            },
            Edge {
                from: SourceEndpoint::NodeOutput { node: 3, port: 0 },
                to: TargetEndpoint::GraphOutput(0),
            },
        ];

        let report = validate(&graph).unwrap_err();
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.code == ValidationCode::ArithmeticRange)
        );
    }

    #[test]
    fn import_can_never_have_effects_or_authority() {
        let mut effects = BTreeSet::new();
        effects.insert(Effect::Network);

        let mut caps = BTreeSet::new();
        caps.insert(Capability::new(
            CapabilityClass::Network,
            "connect",
            "any",
            "global",
        ));

        let mut graph = Graph::new("import");
        graph.nodes = vec![Node {
            id: 1,
            operation: Operation::Import("plugin".into()),
            inputs: vec![],
            outputs: vec![],
            effects,
            required_capabilities: caps,
        }];

        let report = validate(&graph).unwrap_err();
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.code == ValidationCode::ImportHasEffects)
        );
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.code == ValidationCode::ImportHasCapabilities)
        );
    }

    #[test]
    fn remote_execution_requires_effect_and_capability() {
        let mut graph = Graph::new("remote");
        graph.nodes = vec![pure_node(
            1,
            Operation::RemoteExecute {
                target: "worker".into(),
                artifact: "job".into(),
            },
            vec![],
            vec![],
        )];

        let report = validate(&graph).unwrap_err();
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.code == ValidationCode::ExecutionMissingEffect)
        );
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.code == ValidationCode::ExecutionMissingCapability)
        );
    }

    #[test]
    fn native_store_operation_requires_storage_effect_and_capability() {
        let mut graph = Graph::new("store");
        graph.nodes = vec![pure_node(
            1,
            Operation::StoreRead {
                resource: "Message".into(),
                fields: vec!["body".into()],
            },
            vec![],
            vec![],
        )];

        let report = validate(&graph).unwrap_err();
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.code == ValidationCode::StorageMissingEffect)
        );
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.code == ValidationCode::StorageMissingCapability)
        );
    }

    #[test]
    fn native_store_operation_accepts_explicit_storage_authority() {
        let mut effects = BTreeSet::new();
        effects.insert(Effect::Storage);

        let mut capabilities = BTreeSet::new();
        capabilities.insert(Capability::new(
            CapabilityClass::Storage,
            "access",
            "Store",
            "current",
        ));

        let mut graph = Graph::new("store");
        graph.nodes = vec![Node {
            id: 1,
            operation: Operation::StoreRead {
                resource: "Message".into(),
                fields: vec!["body".into()],
            },
            inputs: vec![],
            outputs: vec![],
            effects,
            required_capabilities: capabilities,
        }];

        assert!(validate(&graph).is_ok());
    }

    #[test]
    fn same_effect_cannot_be_unordered() {
        let mut effects = BTreeSet::new();
        effects.insert(Effect::Storage);

        let mut capabilities = BTreeSet::new();
        capabilities.insert(Capability::new(
            CapabilityClass::Storage,
            "write",
            "Store",
            "current",
        ));

        let mut graph = Graph::new("effects");
        graph.nodes = vec![
            Node {
                id: 1,
                operation: Operation::Subgraph("store.a".into()),
                inputs: vec![],
                outputs: vec![],
                effects: effects.clone(),
                required_capabilities: capabilities.clone(),
            },
            Node {
                id: 2,
                operation: Operation::Subgraph("store.b".into()),
                inputs: vec![],
                outputs: vec![],
                effects,
                required_capabilities: capabilities,
            },
        ];

        let report = validate(&graph).unwrap_err();
        assert!(report.issues.iter().any(|issue| {
            matches!(
                issue.code,
                ValidationCode::UnorderedConflictingEffects {
                    effect: Effect::Storage,
                    ..
                }
            )
        }));
    }

    #[test]
    fn effect_requires_matching_ambient_capability() {
        let mut effects = BTreeSet::new();
        effects.insert(Effect::Storage);

        let mut graph = Graph::new("store");
        graph.nodes = vec![Node {
            id: 1,
            operation: Operation::Subgraph("store.read".into()),
            inputs: vec![],
            outputs: vec![],
            effects,
            required_capabilities: BTreeSet::new(),
        }];

        let report = validate(&graph).unwrap_err();
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.code == ValidationCode::EffectWithoutCapability)
        );
    }
}
