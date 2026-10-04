//! Operation shapes are checked without a schema at the graph boundary;
//! named record/variant fields are checked at the closed-program boundary.
use crate::{
    data_format::{DataSchema, FieldRequirement},
    gir::*,
};
use std::collections::{BTreeMap, BTreeSet};

pub fn validate_node(node: &Node, schemas: Option<&[DataSchema]>) -> Result<(), String> {
    let mut inputs: Vec<_> = node.inputs.iter().collect();
    inputs.sort_by_key(|p| p.id);
    let mut outputs: Vec<_> = node.outputs.iter().collect();
    outputs.sort_by_key(|p| p.id);
    let op = &node.operation;
    let arity = match op {
        Operation::MakeArray => inputs.len(),
        Operation::MakeRecord { fields, .. } => fields.len(),
        Operation::Index
        | Operation::TextConcat
        | Operation::TextJoin
        | Operation::BytesConcat
        | Operation::ArrayConcat
        | Operation::CheckedAdd
        | Operation::CheckedSub
        | Operation::CheckedMul
        | Operation::UnwrapOr => 2,
        Operation::BytesSlice => 3,
        Operation::None => 0,
        Operation::Length
        | Operation::Range
        | Operation::BytesFromArray
        | Operation::ResultIsOk
        | Operation::EncodeUtf8
        | Operation::DecodeUtf8
        | Operation::DecodeInteger128Le
        | Operation::FormatInteger
        | Operation::Field { .. }
        | Operation::MakeVariant { .. }
        | Operation::VariantPayload { .. }
        | Operation::Some
        | Operation::Ok
        | Operation::Err => 1,
        _ => return Ok(()),
    };
    if inputs.len() != arity || outputs.len() != 1 {
        return Err("invalid aggregate operation arity".into());
    }
    let output = &outputs[0].ty;
    let input = |i: usize| &inputs[i].ty;
    let element = |ty: &SemanticType| match ty {
        SemanticType::Array(t, _) | SemanticType::Slice(t) | SemanticType::Vector(t, _) => {
            Some(t.as_ref().clone())
        }
        _ => None,
    };
    let schema = |name: &str| schemas.and_then(|schemas| schemas.iter().find(|s| s.name == name));
    let valid = match op {
        Operation::CheckedAdd | Operation::CheckedSub | Operation::CheckedMul => {
            inputs
                .iter()
                .all(|p| matches!(p.ty, SemanticType::Integer(_)))
                && matches!(output, SemanticType::Result(ok, err)
                    if matches!(ok.as_ref(), SemanticType::Integer(t) if t.min == i128::MIN && t.max == i128::MAX)
                    && err.as_ref() == &SemanticType::Bool)
        }
        Operation::ResultIsOk => {
            matches!(input(0), SemanticType::Result(_, _)) && output == &SemanticType::Bool
        }
        Operation::MakeArray => match output {
            SemanticType::Array(t, len) | SemanticType::Vector(t, len) => {
                *len == inputs.len() && inputs.iter().all(|p| type_assignable(&p.ty, t))
            }
            SemanticType::Slice(t) => inputs.iter().all(|p| type_assignable(&p.ty, t)),
            _ => false,
        },
        Operation::Index => {
            matches!(input(1), SemanticType::Integer(_))
                && match (input(0), output) {
                    (
                        SemanticType::Array(t, _)
                        | SemanticType::Vector(t, _)
                        | SemanticType::Slice(t),
                        SemanticType::Option(out),
                    ) => type_assignable(t, out),
                    (SemanticType::Bytes, SemanticType::Option(out)) => type_assignable(
                        &SemanticType::Integer(IntegerType { min: 0, max: 255 }),
                        out,
                    ),
                    _ => false,
                }
        }
        Operation::Length => {
            matches!(
                input(0),
                SemanticType::Array(..)
                    | SemanticType::Vector(..)
                    | SemanticType::Slice(_)
                    | SemanticType::Bytes
            ) && matches!(output,SemanticType::Integer(t) if t.min == 0 && t.max >= u64::MAX as i128)
        }
        Operation::TextConcat => {
            input(0) == &SemanticType::Text && input(1) == input(0) && output == input(0)
        }
        Operation::TextJoin => {
            matches!(input(0), SemanticType::Slice(t) | SemanticType::Array(t, _) | SemanticType::Vector(t, _) if t.as_ref() == &SemanticType::Text)
                && input(1) == &SemanticType::Text
                && output == &SemanticType::Text
        }
        Operation::BytesConcat => {
            input(0) == &SemanticType::Bytes && input(1) == input(0) && output == input(0)
        }
        Operation::BytesSlice => {
            input(0) == &SemanticType::Bytes
                && matches!(input(1), SemanticType::Integer(_))
                && matches!(input(2), SemanticType::Integer(_))
                && output == &SemanticType::Bytes
        }
        Operation::ArrayConcat => match (element(input(0)), element(input(1)), output) {
            (Some(a), Some(b), SemanticType::Slice(out)) => {
                type_assignable(&a, out) && type_assignable(&b, out)
            }
            _ => false,
        },
        Operation::Range => {
            matches!(input(0),SemanticType::Integer(t) if t.min >= 0)
                && matches!((input(0),output),(SemanticType::Integer(count),SemanticType::Slice(t)) if matches!(t.as_ref(),SemanticType::Integer(out) if out.min == 0 && out.max >= count.max.saturating_sub(1)))
        }
        Operation::BytesFromArray => {
            element(input(0))
                .is_some_and(|t| matches!(t,SemanticType::Integer(t) if t.min >= 0 && t.max <= 255))
                && output == &SemanticType::Bytes
        }
        Operation::EncodeUtf8 => input(0) == &SemanticType::Text && output == &SemanticType::Bytes,
        Operation::DecodeInteger128Le => {
            input(0) == &SemanticType::Bytes
                && matches!(output, SemanticType::Integer(t) if t.min == i128::MIN && t.max == i128::MAX)
        }
        Operation::DecodeUtf8 => {
            input(0) == &SemanticType::Bytes
                && output
                    == &SemanticType::Result(
                        Box::new(SemanticType::Text),
                        Box::new(SemanticType::Bytes),
                    )
        }
        Operation::FormatInteger => {
            matches!(input(0), SemanticType::Integer(_)) && output == &SemanticType::Text
        }
        Operation::MakeRecord {
            schema: name,
            fields,
        } => {
            let distinct = fields.iter().collect::<BTreeSet<_>>().len() == fields.len();
            !name.is_empty()
                && distinct
                && output == &SemanticType::Record(name.clone())
                && if schemas.is_none() {
                    true
                } else {
                    schema(name).is_some_and(|s| {
                        fields.iter().zip(&inputs).all(|(name, p)| {
                            s.fields
                                .iter()
                                .find(|f| &f.name == name)
                                .is_some_and(|f| type_assignable(&p.ty, &f.ty))
                        }) && s
                            .fields
                            .iter()
                            .filter(|f| f.requirement == FieldRequirement::Required)
                            .all(|f| fields.contains(&f.name))
                    })
                }
        }
        Operation::Field { name } => match input(0) {
            SemanticType::Record(record) if !name.is_empty() => {
                schemas.is_none()
                    || schema(record)
                        .and_then(|s| s.fields.iter().find(|f| &f.name == name))
                        .is_some_and(|f| {
                            let ty = if f.requirement == FieldRequirement::Optional {
                                SemanticType::Option(Box::new(f.ty.clone()))
                            } else {
                                f.ty.clone()
                            };
                            type_assignable(&ty, output)
                        })
            }
            _ => false,
        },
        Operation::MakeVariant { schema: name, tag } => {
            !name.is_empty()
                && !tag.is_empty()
                && output == &SemanticType::Variant(name.clone())
                && (schemas.is_none()
                    || schema(name)
                        .and_then(|s| s.fields.iter().find(|f| &f.name == tag))
                        .is_some_and(|f| type_assignable(input(0), &f.ty)))
        }
        Operation::VariantPayload { tag } => match (input(0), output) {
            (SemanticType::Variant(name), SemanticType::Option(out)) if !tag.is_empty() => {
                schemas.is_none()
                    || schema(name)
                        .and_then(|s| s.fields.iter().find(|f| &f.name == tag))
                        .is_some_and(|f| type_assignable(&f.ty, out))
            }
            _ => false,
        },
        Operation::Some => matches!(output,SemanticType::Option(t) if type_assignable(input(0),t)),
        Operation::None => matches!(output, SemanticType::Option(_)),
        Operation::Ok => matches!(output,SemanticType::Result(t,_) if type_assignable(input(0),t)),
        Operation::Err => matches!(output,SemanticType::Result(_,t) if type_assignable(input(0),t)),
        Operation::UnwrapOr => match input(0) {
            SemanticType::Option(t) | SemanticType::Result(t, _) => {
                type_assignable(t, output) && type_assignable(input(1), output)
            }
            _ => false,
        },
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err("incompatible aggregate operation types or schema".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositeIssue {
    pub graph: String,
    pub node: Option<NodeId>,
    pub message: String,
}

pub fn validate_program(graphs: &[Graph], schemas: &[DataSchema]) -> Vec<CompositeIssue> {
    let mut issues = Vec::new();
    let mut names = BTreeSet::new();
    for schema in schemas {
        if schema.name.is_empty() || !names.insert(&schema.name) {
            issues.push(CompositeIssue {
                graph: String::new(),
                node: None,
                message: "empty or duplicate schema name".into(),
            });
        }
    }
    for graph in graphs {
        let mut linear_sources = BTreeMap::new();
        for port in &graph.inputs {
            linear_sources.insert(
                SourceEndpoint::GraphInput(port.id),
                contains_linear(&port.ty, schemas, &mut BTreeSet::new(), 0),
            );
        }
        for node in &graph.nodes {
            for port in &node.outputs {
                linear_sources.insert(
                    SourceEndpoint::NodeOutput {
                        node: node.id,
                        port: port.id,
                    },
                    contains_linear(&port.ty, schemas, &mut BTreeSet::new(), 0),
                );
            }
        }
        let mut used = BTreeSet::new();
        for edge in &graph.edges {
            if linear_sources.get(&edge.from) == Some(&true) && !used.insert(edge.from.clone()) {
                issues.push(CompositeIssue{graph:graph.name.clone(),node:None,message:"linear value, including handles nested in a named type, has more than one consumer".into()});
            }
        }
        for port in graph.inputs.iter().chain(&graph.outputs).chain(
            graph
                .nodes
                .iter()
                .flat_map(|n| n.inputs.iter().chain(&n.outputs)),
        ) {
            if !references_exist(&port.ty, &names, 0) {
                issues.push(CompositeIssue {
                    graph: graph.name.clone(),
                    node: None,
                    message: "unknown named type or excessive type depth".into(),
                });
            }
        }
        for node in &graph.nodes {
            if let Err(message) = validate_node(node, Some(schemas)) {
                issues.push(CompositeIssue {
                    graph: graph.name.clone(),
                    node: Some(node.id),
                    message,
                });
            }
        }
    }
    for schema in schemas {
        for field in &schema.fields {
            if !references_exist(&field.ty, &names, 0) {
                issues.push(CompositeIssue {
                    graph: String::new(),
                    node: None,
                    message: "unknown schema field type or excessive type depth".into(),
                });
            }
        }
    }
    issues
}

fn contains_linear(
    ty: &SemanticType,
    schemas: &[DataSchema],
    path: &mut BTreeSet<String>,
    depth: usize,
) -> bool {
    if depth >= 128 {
        return true;
    }
    match ty {
        SemanticType::Unique(_) => true,
        SemanticType::Record(name) | SemanticType::Variant(name) => {
            if !path.insert(name.clone()) {
                return false;
            }
            let linear = schemas.iter().find(|s| &s.name == name).is_some_and(|s| {
                s.fields
                    .iter()
                    .any(|f| contains_linear(&f.ty, schemas, path, depth + 1))
            });
            path.remove(name);
            linear
        }
        SemanticType::Array(t, _)
        | SemanticType::Vector(t, _)
        | SemanticType::Slice(t)
        | SemanticType::Option(t)
        | SemanticType::Secret(t)
        | SemanticType::Credential(t)
        | SemanticType::State(t)
        | SemanticType::Atomic(t)
        | SemanticType::Versioned(t) => contains_linear(t, schemas, path, depth + 1),
        SemanticType::Result(a, b) => {
            contains_linear(a, schemas, path, depth + 1)
                || contains_linear(b, schemas, path, depth + 1)
        }
        _ => false,
    }
}

fn references_exist(ty: &SemanticType, names: &BTreeSet<&String>, depth: usize) -> bool {
    if depth >= 128 {
        return false;
    }
    match ty {
        SemanticType::Record(name) | SemanticType::Variant(name) => names.contains(name),
        SemanticType::Array(t, _)
        | SemanticType::Vector(t, _)
        | SemanticType::Slice(t)
        | SemanticType::Option(t)
        | SemanticType::Secret(t)
        | SemanticType::Credential(t)
        | SemanticType::Unique(t)
        | SemanticType::Borrow(t)
        | SemanticType::Shared(t)
        | SemanticType::State(t)
        | SemanticType::Atomic(t)
        | SemanticType::Versioned(t) => references_exist(t, names, depth + 1),
        SemanticType::Result(a, b) => {
            references_exist(a, names, depth + 1) && references_exist(b, names, depth + 1)
        }
        _ => true,
    }
}
