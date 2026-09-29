use crate::gir::SemanticType;

pub const G0_SCALAR_ARG_LIMIT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiScalarClass {
    IntegerLike,
    PointerLike,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbiSignature {
    pub inputs: Vec<AbiScalarClass>,
    pub outputs: Vec<AbiScalarClass>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbiIssue {
    TooManyInputs { count: usize, maximum: usize },
    TooManyOutputs { count: usize, maximum: usize },
    UnsupportedType,
}

pub fn lower_signature(
    inputs: &[SemanticType],
    outputs: &[SemanticType],
) -> Result<AbiSignature, Vec<AbiIssue>> {
    let mut issues = Vec::new();

    if inputs.len() > G0_SCALAR_ARG_LIMIT {
        issues.push(AbiIssue::TooManyInputs {
            count: inputs.len(),
            maximum: G0_SCALAR_ARG_LIMIT,
        });
    }
    if outputs.len() > 1 {
        issues.push(AbiIssue::TooManyOutputs {
            count: outputs.len(),
            maximum: 1,
        });
    }

    let lowered_inputs: Vec<_> = inputs
        .iter()
        .filter_map(|ty| match abi_class(ty) {
            Some(class) => Some(class),
            None => {
                issues.push(AbiIssue::UnsupportedType);
                None
            }
        })
        .collect();

    let lowered_outputs: Vec<_> = outputs
        .iter()
        .filter_map(|ty| match abi_class(ty) {
            Some(class) => Some(class),
            None => {
                issues.push(AbiIssue::UnsupportedType);
                None
            }
        })
        .collect();

    if issues.is_empty() {
        Ok(AbiSignature {
            inputs: lowered_inputs,
            outputs: lowered_outputs,
        })
    } else {
        Err(issues)
    }
}

fn abi_class(ty: &SemanticType) -> Option<AbiScalarClass> {
    match ty {
        SemanticType::Bool | SemanticType::Integer(_) => {
            Some(AbiScalarClass::IntegerLike)
        }
        SemanticType::Reference(_)
        | SemanticType::Unique(_)
        | SemanticType::Borrow(_)
        | SemanticType::Shared(_)
        | SemanticType::State(_)
        | SemanticType::Atomic(_)
        | SemanticType::Versioned(_) => Some(AbiScalarClass::PointerLike),
        _ => None,
    }
}

pub fn graph_symbol(name: &str) -> String {
    let mut symbol = String::from("g0_g_");
    for byte in name.as_bytes() {
        use std::fmt::Write;
        write!(&mut symbol, "{byte:02x}").expect("String write");
    }
    symbol
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gir::IntegerType;

    #[test]
    fn unicode_graph_names_map_to_safe_unique_symbols() {
        let a = graph_symbol("módulo🙂");
        let b = graph_symbol("modulo🙂");
        assert_ne!(a, b);
        assert!(a.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'_'
        }));
    }

    #[test]
    fn scalar_signature_is_supported() {
        let sig = lower_signature(
            &[
                SemanticType::Integer(IntegerType::new(0, 10).unwrap()),
                SemanticType::Bool,
            ],
            &[SemanticType::Integer(IntegerType::new(0, 20).unwrap())],
        )
        .unwrap();

        assert_eq!(sig.inputs.len(), 2);
        assert_eq!(sig.outputs.len(), 1);
    }
}
