//! Bounded text forms used by native controls. These describe GIR, never grants.
use super::EditorError;
use crate::{
    data_format::{DataSchema, FieldRequirement, SchemaField},
    gir::*,
};
use std::collections::BTreeMap;

pub const OPERATION_NAMES: &[&str] = &[
    "ConstInteger",
    "ConstBool",
    "ConstText",
    "ConstBytes",
    "Add",
    "Sub",
    "Mul",
    "Div",
    "Rem",
    "Eq",
    "Lt",
    "Le",
    "Gt",
    "Ge",
    "And",
    "Or",
    "Xor",
    "Not",
    "ConvertChecked",
    "MakeArray",
    "Index",
    "Length",
    "TextConcat",
    "BytesConcat",
    "BytesSlice",
    "ArrayConcat",
    "Range",
    "BytesFromArray",
    "CheckedAdd",
    "CheckedSub",
    "CheckedMul",
    "ResultIsOk",
    "EncodeUtf8",
    "DecodeUtf8",
    "FormatInteger",
    "MakeRecord",
    "Field",
    "MakeVariant",
    "VariantPayload",
    "Some",
    "None",
    "Ok",
    "Err",
    "UnwrapOr",
    "Map",
    "TextJoin",
    "RegionOpen",
    "RegionOpenSecret",
    "RegionOpenChild",
    "RegionAllocate",
    "RegionWrite",
    "RegionRead",
    "RegionClose",
    "TaskSpawn",
    "TaskJoin",
    "TaskSpawnScoped",
    "TaskJoinScoped",
    "TaskSpawnHosted",
    "TaskJoinHosted",
    "Truncate",
    "Select",
    "Match",
    "Loop",
    "Subgraph",
    "Import",
    "Instantiate",
    "StoreRead",
    "StoreCreate",
    "StoreUpdate",
    "StoreDelete",
    "StoreEnumerate",
    "StoreSetRelation",
    "StoreSetCredential",
    "StoreVerifyCredential",
    "StoreTraverse",
    "LocalExecute",
    "RemoteExecute",
];
fn invalid(message: &str) -> EditorError {
    EditorError::Validation(message.into())
}
fn number<T: std::str::FromStr>(text: &str) -> Result<T, EditorError> {
    text.parse()
        .map_err(|_| invalid("invalid numeric form parameter"))
}
fn form<'a>(text: &'a str, keys: &[&str]) -> Result<BTreeMap<&'a str, &'a str>, EditorError> {
    if text.len() > 8192 {
        return Err(EditorError::Limit);
    }
    let mut result = BTreeMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid("form requires key=value lines"))?;
        let key = key.trim();
        if !keys.contains(&key) || result.insert(key, value.trim()).is_some() {
            return Err(invalid("unknown or duplicate form key"));
        }
    }
    if keys.iter().any(|key| !result.contains_key(key)) {
        return Err(invalid("missing form key"));
    }
    Ok(result)
}
fn split_types(text: &str, separator: char) -> Result<Vec<&str>, EditorError> {
    let mut depth = 0usize;
    let mut start = 0;
    let mut parts = vec![];
    for (i, c) in text.char_indices() {
        match c {
            '(' => {
                depth += 1;
                if depth > 32 {
                    return Err(EditorError::Limit);
                }
            }
            ')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("unbalanced type parentheses"))?
            }
            c if c == separator && depth == 0 => {
                parts.push(text[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(invalid("unbalanced type parentheses"));
    }
    parts.push(text[start..].trim());
    Ok(parts)
}
pub fn parse_type(text: &str) -> Result<SemanticType, EditorError> {
    type_at(text.trim(), 0)
}
fn type_at(text: &str, depth: usize) -> Result<SemanticType, EditorError> {
    if text.len() > 4096 || depth > 32 {
        return Err(EditorError::Limit);
    }
    let atom = match text {
        "bool" => Some(SemanticType::Bool),
        "text" => Some(SemanticType::Text),
        "bytes" => Some(SemanticType::Bytes),
        "bigint" => Some(SemanticType::BigInteger),
        "rational" => Some(SemanticType::Rational),
        "float" => Some(SemanticType::Float(FloatType {
            max_relative_error_ppb: None,
        })),
        _ => None,
    };
    if let Some(atom) = atom {
        return Ok(atom);
    }
    let (name, inner) = text
        .split_once('(')
        .ok_or_else(|| invalid("unknown semantic type"))?;
    let inner = inner
        .strip_suffix(')')
        .ok_or_else(|| invalid("unterminated semantic type"))?;
    let items = split_types(inner, ',')?;
    let one = || {
        if items.len() == 1 {
            Ok(items[0])
        } else {
            Err(invalid("type expects one parameter"))
        }
    };
    let nested = || type_at(one()?, depth + 1).map(Box::new);
    let named = || {
        let name = one()?;
        if name.is_empty() || name.len() > 1024 {
            Err(EditorError::Limit)
        } else {
            Ok(name.into())
        }
    };
    Ok(match name {
        "int" if items.len() == 2 => SemanticType::Integer(
            IntegerType::new(number(items[0])?, number(items[1])?).map_err(invalid)?,
        ),
        "decimal" if items.len() == 2 => SemanticType::Decimal(
            DecimalType::new(number(items[0])?, number(items[1])?).map_err(invalid)?,
        ),
        "float" => SemanticType::Float(FloatType {
            max_relative_error_ppb: Some(number(one()?)?),
        }),
        "bigfloat" => SemanticType::BigFloat(BigFloatType::new(number(one()?)?).map_err(invalid)?),
        "record" => SemanticType::Record(named()?),
        "variant" => SemanticType::Variant(named()?),
        "reference" => SemanticType::Reference(named()?),
        "array" | "vector" if items.len() == 2 => {
            let len: usize = number(items[0])?;
            if len > 1_000_000 {
                return Err(EditorError::Limit);
            }
            let ty = Box::new(type_at(items[1], depth + 1)?);
            if name == "array" {
                SemanticType::Array(ty, len)
            } else {
                SemanticType::Vector(ty, len)
            }
        }
        "result" if items.len() == 2 => SemanticType::Result(
            Box::new(type_at(items[0], depth + 1)?),
            Box::new(type_at(items[1], depth + 1)?),
        ),
        "slice" => SemanticType::Slice(nested()?),
        "option" => SemanticType::Option(nested()?),
        "secret" => SemanticType::Secret(nested()?),
        "credential" => SemanticType::Credential(nested()?),
        "unique" => SemanticType::Unique(nested()?),
        "borrow" => SemanticType::Borrow(nested()?),
        "shared" => SemanticType::Shared(nested()?),
        "state" => SemanticType::State(nested()?),
        "atomic" => SemanticType::Atomic(nested()?),
        "versioned" => SemanticType::Versioned(nested()?),
        _ => return Err(invalid("unknown type or incorrect type parameters")),
    })
}
fn ports(text: &str) -> Result<Vec<Port>, EditorError> {
    if text.is_empty() {
        return Ok(vec![]);
    }
    let parts = split_types(text, ';')?;
    if parts.len() > 64 {
        return Err(EditorError::Limit);
    }
    parts
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let (name, ty) = text
                .split_once(':')
                .ok_or_else(|| invalid("port requires name:type"))?;
            let (id, name) = match name.split_once(',') {
                Some((id, name)) => (number::<PortId>(id)?, name),
                None => (i as PortId, name),
            };
            if name.is_empty() || name.len() > 1024 {
                return Err(EditorError::Limit);
            }
            Ok(Port {
                id,
                name: name.into(),
                ty: parse_type(ty)?,
            })
        })
        .collect()
}
fn effect(text: &str) -> Result<Effect, EditorError> {
    Ok(match text {
        "MemoryWrite" => Effect::MemoryWrite,
        "Storage" => Effect::Storage,
        "Network" => Effect::Network,
        "Clock" => Effect::Clock,
        "Entropy" => Effect::Entropy,
        "Device" => Effect::Device,
        "Process" => Effect::Process,
        "LocalExecution" => Effect::LocalExecution,
        "RemoteExecution" => Effect::RemoteExecution,
        "Accelerator" => Effect::Accelerator,
        "Audit" => Effect::Audit,
        _ => return Err(invalid("unknown effect")),
    })
}
fn capability_class(text: &str) -> Result<CapabilityClass, EditorError> {
    Ok(match text {
        "Resource" => CapabilityClass::Resource,
        "Storage" => CapabilityClass::Storage,
        "Network" => CapabilityClass::Network,
        "Clock" => CapabilityClass::Clock,
        "Entropy" => CapabilityClass::Entropy,
        "Device" => CapabilityClass::Device,
        "Process" => CapabilityClass::Process,
        "LocalExecution" => CapabilityClass::LocalExecution,
        "RemoteExecution" => CapabilityClass::RemoteExecution,
        "Accelerator" => CapabilityClass::Accelerator,
        "Audit" => CapabilityClass::Audit,
        _ => return Err(invalid("unknown capability class")),
    })
}
pub fn parse_node_form(text: &str) -> Result<Node, EditorError> {
    let fields = form(
        text,
        &["operation", "inputs", "outputs", "effects", "capabilities"],
    )?;
    let effects = fields["effects"]
        .split(';')
        .filter(|s| !s.is_empty())
        .map(|s| effect(s.trim()))
        .collect::<Result<_, _>>()?;
    let required_capabilities = fields["capabilities"]
        .split(';')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let parts: Vec<_> = s.split(',').map(str::trim).collect();
            if parts.len() != 4 || parts.iter().any(|s| s.is_empty() || s.len() > 1024) {
                return Err(invalid("capability requires class,action,resource,scope"));
            }
            Ok(Capability::new(
                capability_class(parts[0])?,
                parts[1],
                parts[2],
                parts[3],
            ))
        })
        .collect::<Result<_, _>>()?;
    Ok(Node {
        id: 0,
        operation: parse_operation(fields["operation"])?,
        inputs: ports(fields["inputs"])?,
        outputs: ports(fields["outputs"])?,
        effects,
        required_capabilities,
    })
}
pub fn parse_graph_form(text: &str) -> Result<(Vec<Port>, Vec<Port>), EditorError> {
    let fields = form(text, &["inputs", "outputs"])?;
    Ok((ports(fields["inputs"])?, ports(fields["outputs"])?))
}
pub fn type_text(ty: &SemanticType) -> String {
    match ty {
        SemanticType::Bool => "bool".into(),
        SemanticType::Text => "text".into(),
        SemanticType::Bytes => "bytes".into(),
        SemanticType::BigInteger => "bigint".into(),
        SemanticType::Rational => "rational".into(),
        SemanticType::Integer(t) => format!("int({},{})", t.min, t.max),
        SemanticType::Decimal(t) => format!("decimal({},{})", t.precision_digits, t.scale),
        SemanticType::Float(t) => t
            .max_relative_error_ppb
            .map_or("float".into(), |n| format!("float({n})")),
        SemanticType::BigFloat(t) => format!("bigfloat({})", t.precision_bits),
        SemanticType::Record(n) => format!("record({n})"),
        SemanticType::Variant(n) => format!("variant({n})"),
        SemanticType::Reference(n) => format!("reference({n})"),
        SemanticType::Array(t, n) => format!("array({n},{})", type_text(t)),
        SemanticType::Vector(t, n) => format!("vector({n},{})", type_text(t)),
        SemanticType::Result(a, b) => format!("result({},{})", type_text(a), type_text(b)),
        SemanticType::Slice(t) => format!("slice({})", type_text(t)),
        SemanticType::Option(t) => format!("option({})", type_text(t)),
        SemanticType::Secret(t) => format!("secret({})", type_text(t)),
        SemanticType::Credential(t) => format!("credential({})", type_text(t)),
        SemanticType::Unique(t) => format!("unique({})", type_text(t)),
        SemanticType::Borrow(t) => format!("borrow({})", type_text(t)),
        SemanticType::Shared(t) => format!("shared({})", type_text(t)),
        SemanticType::State(t) => format!("state({})", type_text(t)),
        SemanticType::Atomic(t) => format!("atomic({})", type_text(t)),
        SemanticType::Versioned(t) => format!("versioned({})", type_text(t)),
    }
}
pub fn ports_text(ports: &[Port]) -> Result<String, EditorError> {
    // Explicit IDs preserve imported interfaces without renumbering endpoints.
    if ports
        .iter()
        .any(|p| p.name.contains([':', ';', '\r', '\n', '=']))
    {
        return Err(invalid("port name delimiters require native API editing"));
    }
    Ok(ports
        .iter()
        .map(|p| format!("{},{}:{}", p.id, p.name, type_text(&p.ty)))
        .collect::<Vec<_>>()
        .join(";"))
}
pub fn node_form_text(node: &Node) -> Result<String, EditorError> {
    let list = |names: &[String]| {
        if names.is_empty() {
            "-".into()
        } else {
            names.join(",")
        }
    };
    let operation = match &node.operation {
        Operation::Const(Literal::Integer(n)) => format!("ConstInteger {n}"),
        Operation::Const(Literal::Bool(n)) => format!("ConstBool {n}"),
        Operation::Const(Literal::Text(n)) => {
            if n.contains(['\r', '\n']) {
                return Err(invalid("multiline constant uses literal properties"));
            }
            format!("ConstText {n}")
        }
        Operation::Const(Literal::Bytes(n)) => format!(
            "ConstBytes {}",
            if n.is_empty() {
                "-".into()
            } else {
                n.iter().map(|b| format!("{b:02x}")).collect::<String>()
            }
        ),
        Operation::MakeRecord { schema, fields } => format!("MakeRecord {schema} {}", list(fields)),
        Operation::Field { name } => format!("Field {name}"),
        Operation::MakeVariant { schema, tag } => format!("MakeVariant {schema} {tag}"),
        Operation::VariantPayload { tag } => format!("VariantPayload {tag}"),
        Operation::Map { body } => format!("Map {body}"),
        Operation::RegionOpenChild { secret } => format!("RegionOpenChild {secret}"),
        Operation::TaskSpawn {
            body,
            max_steps,
            max_value_bytes,
        } => format!("TaskSpawn {body} {max_steps} {max_value_bytes}"),
        Operation::TaskSpawnScoped {
            body,
            max_steps,
            max_value_bytes,
        } => format!("TaskSpawnScoped {body} {max_steps} {max_value_bytes}"),
        Operation::TaskSpawnHosted {
            body,
            max_steps,
            max_value_bytes,
        } => format!("TaskSpawnHosted {body} {max_steps} {max_value_bytes}"),
        Operation::TaskJoin { body } => format!("TaskJoin {body}"),
        Operation::TaskJoinScoped { body } => format!("TaskJoinScoped {body}"),
        Operation::TaskJoinHosted { body } => format!("TaskJoinHosted {body}"),
        Operation::Truncate { bits, signed } => format!("Truncate {bits} {signed}"),
        Operation::Select {
            when_true,
            when_false,
        } => format!("Select {when_true} {when_false}"),
        Operation::Match { arms, default } => format!(
            "Match {default} {}",
            arms.iter()
                .map(|a| format!("{}:{}", a.tag, a.graph))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        Operation::Loop {
            condition,
            body,
            max_iterations,
        } => format!("Loop {condition} {body} {max_iterations}"),
        Operation::Subgraph(n) => format!("Subgraph {n}"),
        Operation::Import(n) => format!("Import {n}"),
        Operation::Instantiate(n) => format!("Instantiate {n}"),
        Operation::StoreRead { resource, fields } => {
            format!("StoreRead {resource} {}", list(fields))
        }
        Operation::StoreCreate { resource, fields } => {
            format!("StoreCreate {resource} {}", list(fields))
        }
        Operation::StoreUpdate { resource, fields } => {
            format!("StoreUpdate {resource} {}", list(fields))
        }
        Operation::StoreDelete { resource } => format!("StoreDelete {resource}"),
        Operation::StoreEnumerate { resource } => format!("StoreEnumerate {resource}"),
        Operation::StoreSetRelation { resource, relation } => {
            format!("StoreSetRelation {resource} {relation}")
        }
        Operation::StoreSetCredential { resource, field } => {
            format!("StoreSetCredential {resource} {field}")
        }
        Operation::StoreVerifyCredential { resource, field } => {
            format!("StoreVerifyCredential {resource} {field}")
        }
        Operation::StoreTraverse { resource, relation } => {
            format!("StoreTraverse {resource} {relation}")
        }
        Operation::LocalExecute(n) => format!("LocalExecute {n}"),
        Operation::RemoteExecute { target, artifact } => {
            format!("RemoteExecute {target} {artifact}")
        }
        operation => format!("{operation:?}"),
    };
    let effects = node
        .effects
        .iter()
        .map(|e| format!("{e:?}"))
        .collect::<Vec<_>>()
        .join(";");
    let capabilities = node
        .required_capabilities
        .iter()
        .map(|c| format!("{:?},{},{},{}", c.class, c.action, c.resource, c.scope))
        .collect::<Vec<_>>()
        .join(";");
    let text = format!(
        "operation={operation}\r\ninputs={}\r\noutputs={}\r\neffects={effects}\r\ncapabilities={capabilities}",
        ports_text(&node.inputs)?,
        ports_text(&node.outputs)?
    );
    if text.len() > 8192 {
        return Err(EditorError::Limit);
    }
    // Refuse lossy delimiter representations when editing loaded definitions.
    let parsed = parse_node_form(&text)?;
    if parsed.operation != node.operation
        || parsed.inputs != node.inputs
        || parsed.outputs != node.outputs
        || parsed.effects != node.effects
        || parsed.required_capabilities != node.required_capabilities
    {
        return Err(invalid(
            "definition cannot be losslessly represented by the bounded text form",
        ));
    }
    Ok(text)
}
pub fn parse_schema_form(text: &str) -> Result<DataSchema, EditorError> {
    let fields = form(text, &["name", "version", "fields"])?;
    let name = fields["name"];
    if name.is_empty() || name.len() > 1024 {
        return Err(EditorError::Limit);
    }
    let mut result = vec![];
    for field in fields["fields"].split(';').filter(|s| !s.is_empty()) {
        let parts: Vec<_> = field.splitn(4, ',').map(str::trim).collect();
        if parts.len() != 4 || parts[1].is_empty() || parts[1].len() > 1024 {
            return Err(invalid("field requires tag,name,required|optional,type"));
        }
        let requirement = match parts[2] {
            "required" => FieldRequirement::Required,
            "optional" => FieldRequirement::Optional,
            _ => return Err(invalid("invalid field requirement")),
        };
        result.push(SchemaField {
            tag: number(parts[0])?,
            name: parts[1].into(),
            requirement,
            ty: parse_type(parts[3])?,
        });
        if result.len() > 256 {
            return Err(EditorError::Limit);
        }
    }
    let schema = DataSchema {
        name: name.into(),
        version: number(fields["version"])?,
        fields: result,
    };
    crate::data_format::validate_schema(&schema).map_err(|e| invalid(&format!("{e:?}")))?;
    Ok(schema)
}
fn parse_operation(text: &str) -> Result<Operation, EditorError> {
    let words: Vec<_> = text.split_whitespace().collect();
    let Some(name) = words.first().copied() else {
        return Err(invalid("missing operation"));
    };
    if words.iter().any(|word| word.len() > 1024) {
        return Err(EditorError::Limit);
    }
    let expected = match name {
        "ConstText" => {
            return Ok(Operation::Const(Literal::Text(
                text.strip_prefix("ConstText").unwrap().trim_start().into(),
            )));
        }
        "ConstInteger" | "ConstBool" | "ConstBytes" | "Field" | "VariantPayload" | "Map"
        | "RegionOpenChild" | "TaskJoin" | "TaskJoinScoped" | "TaskJoinHosted" | "Subgraph"
        | "Import" | "Instantiate" | "StoreDelete" | "StoreEnumerate" | "LocalExecute" => 1,
        "MakeRecord"
        | "MakeVariant"
        | "Truncate"
        | "Select"
        | "StoreRead"
        | "StoreCreate"
        | "StoreUpdate"
        | "StoreSetRelation"
        | "StoreSetCredential"
        | "StoreVerifyCredential"
        | "StoreTraverse"
        | "RemoteExecute" => 2,
        "TaskSpawn" | "TaskSpawnScoped" | "TaskSpawnHosted" | "Loop" => 3,
        "Match" => {
            if words.len() < 3 {
                return Err(invalid("Match requires default graph and tag:graph arms"));
            }
            words.len() - 1
        }
        _ => 0,
    };
    if words.len() != expected + 1 {
        return Err(invalid("incorrect operation parameter count"));
    }
    let p = &words[1..];
    let list = |s: &str| {
        if s == "-" {
            vec![]
        } else {
            s.split(',').map(str::to_owned).collect()
        }
    };
    Ok(match name {
        "ConstInteger" => Operation::Const(Literal::Integer(number(p[0])?)),
        "ConstBool" => Operation::Const(Literal::Bool(number(p[0])?)),
        "ConstBytes" => Operation::Const(Literal::Bytes(if p[0] == "-" {
            vec![]
        } else {
            super::parse_bytes(p[0])?
        })),
        "Add" => Operation::Add,
        "Sub" => Operation::Sub,
        "Mul" => Operation::Mul,
        "Div" => Operation::Div,
        "Rem" => Operation::Rem,
        "Eq" => Operation::Eq,
        "Lt" => Operation::Lt,
        "Le" => Operation::Le,
        "Gt" => Operation::Gt,
        "Ge" => Operation::Ge,
        "And" => Operation::And,
        "Or" => Operation::Or,
        "Xor" => Operation::Xor,
        "Not" => Operation::Not,
        "ConvertChecked" => Operation::ConvertChecked,
        "MakeArray" => Operation::MakeArray,
        "Index" => Operation::Index,
        "Length" => Operation::Length,
        "TextConcat" => Operation::TextConcat,
        "BytesConcat" => Operation::BytesConcat,
        "BytesSlice" => Operation::BytesSlice,
        "ArrayConcat" => Operation::ArrayConcat,
        "Range" => Operation::Range,
        "BytesFromArray" => Operation::BytesFromArray,
        "CheckedAdd" => Operation::CheckedAdd,
        "CheckedSub" => Operation::CheckedSub,
        "CheckedMul" => Operation::CheckedMul,
        "ResultIsOk" => Operation::ResultIsOk,
        "EncodeUtf8" => Operation::EncodeUtf8,
        "DecodeUtf8" => Operation::DecodeUtf8,
        "FormatInteger" => Operation::FormatInteger,
        "MakeRecord" => Operation::MakeRecord {
            schema: p[0].into(),
            fields: list(p[1]),
        },
        "Field" => Operation::Field { name: p[0].into() },
        "MakeVariant" => Operation::MakeVariant {
            schema: p[0].into(),
            tag: p[1].into(),
        },
        "VariantPayload" => Operation::VariantPayload { tag: p[0].into() },
        "Some" => Operation::Some,
        "None" => Operation::None,
        "Ok" => Operation::Ok,
        "Err" => Operation::Err,
        "UnwrapOr" => Operation::UnwrapOr,
        "Map" => Operation::Map { body: p[0].into() },
        "TextJoin" => Operation::TextJoin,
        "RegionOpen" => Operation::RegionOpen,
        "RegionOpenSecret" => Operation::RegionOpenSecret,
        "RegionOpenChild" => Operation::RegionOpenChild {
            secret: number(p[0])?,
        },
        "RegionAllocate" => Operation::RegionAllocate,
        "RegionWrite" => Operation::RegionWrite,
        "RegionRead" => Operation::RegionRead,
        "RegionClose" => Operation::RegionClose,
        "TaskSpawn" => Operation::TaskSpawn {
            body: p[0].into(),
            max_steps: number(p[1])?,
            max_value_bytes: number(p[2])?,
        },
        "TaskJoin" => Operation::TaskJoin { body: p[0].into() },
        "TaskSpawnScoped" => Operation::TaskSpawnScoped {
            body: p[0].into(),
            max_steps: number(p[1])?,
            max_value_bytes: number(p[2])?,
        },
        "TaskJoinScoped" => Operation::TaskJoinScoped { body: p[0].into() },
        "TaskSpawnHosted" => Operation::TaskSpawnHosted {
            body: p[0].into(),
            max_steps: number(p[1])?,
            max_value_bytes: number(p[2])?,
        },
        "TaskJoinHosted" => Operation::TaskJoinHosted { body: p[0].into() },
        "Truncate" => Operation::Truncate {
            bits: number(p[0])?,
            signed: number(p[1])?,
        },
        "Select" => Operation::Select {
            when_true: p[0].into(),
            when_false: p[1].into(),
        },
        "Match" => Operation::Match {
            default: p[0].into(),
            arms: p[1..]
                .iter()
                .map(|s| {
                    let (tag, graph) = s
                        .split_once(':')
                        .ok_or_else(|| invalid("Match arm requires tag:graph"))?;
                    Ok(MatchArm {
                        tag: tag.into(),
                        graph: graph.into(),
                    })
                })
                .collect::<Result<_, EditorError>>()?,
        },
        "Loop" => Operation::Loop {
            condition: p[0].into(),
            body: p[1].into(),
            max_iterations: number(p[2])?,
        },
        "Subgraph" => Operation::Subgraph(p[0].into()),
        "Import" => Operation::Import(p[0].into()),
        "Instantiate" => Operation::Instantiate(p[0].into()),
        "StoreRead" => Operation::StoreRead {
            resource: p[0].into(),
            fields: list(p[1]),
        },
        "StoreCreate" => Operation::StoreCreate {
            resource: p[0].into(),
            fields: list(p[1]),
        },
        "StoreUpdate" => Operation::StoreUpdate {
            resource: p[0].into(),
            fields: list(p[1]),
        },
        "StoreDelete" => Operation::StoreDelete {
            resource: p[0].into(),
        },
        "StoreEnumerate" => Operation::StoreEnumerate {
            resource: p[0].into(),
        },
        "StoreSetRelation" => Operation::StoreSetRelation {
            resource: p[0].into(),
            relation: p[1].into(),
        },
        "StoreSetCredential" => Operation::StoreSetCredential {
            resource: p[0].into(),
            field: p[1].into(),
        },
        "StoreVerifyCredential" => Operation::StoreVerifyCredential {
            resource: p[0].into(),
            field: p[1].into(),
        },
        "StoreTraverse" => Operation::StoreTraverse {
            resource: p[0].into(),
            relation: p[1].into(),
        },
        "LocalExecute" => Operation::LocalExecute(p[0].into()),
        "RemoteExecute" => Operation::RemoteExecute {
            target: p[0].into(),
            artifact: p[1].into(),
        },
        _ => return Err(invalid("unknown operation")),
    })
}
