use std::collections::BTreeSet;

use crate::mir::{
    ArithmeticMode, MirInstruction, MirOp, MirProgram, MirType, ValueId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirIssue {
    UnknownInput(ValueId),
    UnknownOutput(ValueId),
    ValueDefinedTwice(ValueId),
    WrongInputCount {
        instruction: usize,
        expected: usize,
        actual: usize,
    },
    WrongOutputCount {
        instruction: usize,
        expected: usize,
        actual: usize,
    },
    TypeMismatch {
        instruction: usize,
    },
    ArithmeticMustBeChecked {
        instruction: usize,
    },
    UnknownProgramOutput(ValueId),
    ProgramOutputNotDefined(ValueId),
    UnknownProgramInput(ValueId),
    DuplicateProgramInput(ValueId),
    UseBeforeDefinition {
        instruction: usize,
        value: ValueId,
    },
}

pub fn validate_mir(program: &MirProgram) -> Result<(), Vec<MirIssue>> {
    let mut issues = Vec::new();
    let mut defined = BTreeSet::<ValueId>::new();

    for input in &program.inputs {
        if !program.values.contains_key(input) {
            issues.push(MirIssue::UnknownProgramInput(*input));
        }
        if !defined.insert(*input) {
            issues.push(MirIssue::DuplicateProgramInput(*input));
        }
    }

    for (index, instruction) in program.instructions.iter().enumerate() {
        for input in &instruction.inputs {
            if !program.values.contains_key(input) {
                issues.push(MirIssue::UnknownInput(*input));
            } else if !defined.contains(input) {
                issues.push(MirIssue::UseBeforeDefinition {
                    instruction: index,
                    value: *input,
                });
            }
        }

        for output in &instruction.outputs {
            if !program.values.contains_key(output) {
                issues.push(MirIssue::UnknownOutput(*output));
            }
            if !defined.insert(*output) {
                issues.push(MirIssue::ValueDefinedTwice(*output));
            }
        }

        validate_instruction(program, index, instruction, &mut issues);
    }

    for output in &program.outputs {
        if !program.values.contains_key(output) {
            issues.push(MirIssue::UnknownProgramOutput(*output));
        } else if !defined.contains(output) {
            issues.push(MirIssue::ProgramOutputNotDefined(*output));
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn validate_instruction(
    program: &MirProgram,
    index: usize,
    instruction: &MirInstruction,
    issues: &mut Vec<MirIssue>,
) {
    match &instruction.op {
        MirOp::ConstInteger(_) => {
            require_shape(index, instruction, 0, 1, issues);
            if let Some(output) = instruction.outputs.first()
                && !matches!(
                    program.values.get(output).map(|value| value.ty),
                    Some(MirType::Integer(_))
                )
            {
                issues.push(MirIssue::TypeMismatch { instruction: index });
            }
        }
        MirOp::ConstBool(_) => {
            require_shape(index, instruction, 0, 1, issues);
            if let Some(output) = instruction.outputs.first()
                && program.values.get(output).map(|value| value.ty)
                    != Some(MirType::Bool)
            {
                issues.push(MirIssue::TypeMismatch { instruction: index });
            }
        }
        MirOp::Add { mode } | MirOp::Sub { mode } | MirOp::Mul { mode } => {
            require_shape(index, instruction, 2, 1, issues);
            if *mode != ArithmeticMode::Checked {
                issues.push(MirIssue::ArithmeticMustBeChecked {
                    instruction: index,
                });
            }
            validate_integer_arithmetic_types(
                program,
                index,
                instruction,
                issues,
            );
        }
        MirOp::Copy | MirOp::Move => {
            require_shape(index, instruction, 1, 1, issues);
            if let (Some(input), Some(output)) = (
                instruction.inputs.first(),
                instruction.outputs.first(),
            ) && program.values.get(input).map(|value| value.ty)
                != program.values.get(output).map(|value| value.ty)
            {
                issues.push(MirIssue::TypeMismatch { instruction: index });
            }
        }
        MirOp::SelectCall { .. } => {
            if instruction.inputs.is_empty() {
                issues.push(MirIssue::WrongInputCount {
                    instruction: index,
                    expected: 1,
                    actual: 0,
                });
            } else if program
                .values
                .get(&instruction.inputs[0])
                .map(|value| value.ty)
                != Some(MirType::Bool)
            {
                issues.push(MirIssue::TypeMismatch {
                    instruction: index,
                });
            }
            if instruction.outputs.len() > 1 {
                issues.push(MirIssue::WrongOutputCount {
                    instruction: index,
                    expected: 1,
                    actual: instruction.outputs.len(),
                });
            }
        }
        MirOp::Load | MirOp::Store | MirOp::Call { .. } => {}
    }
}

fn require_shape(
    index: usize,
    instruction: &MirInstruction,
    expected_inputs: usize,
    expected_outputs: usize,
    issues: &mut Vec<MirIssue>,
) {
    if instruction.inputs.len() != expected_inputs {
        issues.push(MirIssue::WrongInputCount {
            instruction: index,
            expected: expected_inputs,
            actual: instruction.inputs.len(),
        });
    }
    if instruction.outputs.len() != expected_outputs {
        issues.push(MirIssue::WrongOutputCount {
            instruction: index,
            expected: expected_outputs,
            actual: instruction.outputs.len(),
        });
    }
}

fn validate_integer_arithmetic_types(
    program: &MirProgram,
    index: usize,
    instruction: &MirInstruction,
    issues: &mut Vec<MirIssue>,
) {
    if instruction.inputs.len() != 2 || instruction.outputs.len() != 1 {
        return;
    }

    let inputs_are_integer = instruction.inputs.iter().all(|value| {
        matches!(
            program.values.get(value).map(|value| value.ty),
            Some(MirType::Integer(_))
        )
    });
    let output_is_integer = matches!(
        program
            .values
            .get(&instruction.outputs[0])
            .map(|value| value.ty),
        Some(MirType::Integer(_))
    );

    if !inputs_are_integer || !output_is_integer {
        issues.push(MirIssue::TypeMismatch { instruction: index });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::memory::IntegerWidth;
    use crate::mir::{MirInstruction, MirValue};

    fn value(id: ValueId, ty: MirType) -> (ValueId, MirValue) {
        (
            id,
            MirValue {
                id,
                ty,
                location: None,
            },
        )
    }

    #[test]
    fn checked_integer_mir_validates() {
        let program = MirProgram {
            inputs: vec![],
            values: BTreeMap::from([
                value(0, MirType::Integer(IntegerWidth::U8)),
                value(1, MirType::Integer(IntegerWidth::U8)),
                value(2, MirType::Integer(IntegerWidth::U16)),
            ]),
            instructions: vec![
                MirInstruction {
                    source_node: 1,
                    op: MirOp::ConstInteger(200),
                    inputs: vec![],
                    outputs: vec![0],
                },
                MirInstruction {
                    source_node: 2,
                    op: MirOp::ConstInteger(100),
                    inputs: vec![],
                    outputs: vec![1],
                },
                MirInstruction {
                    source_node: 3,
                    op: MirOp::Add {
                        mode: ArithmeticMode::Checked,
                    },
                    inputs: vec![0, 1],
                    outputs: vec![2],
                },
            ],
            outputs: vec![],
        };

        assert!(validate_mir(&program).is_ok());
    }

    #[test]
    fn program_output_must_be_defined() {
        let program = MirProgram {
            inputs: vec![],
            values: BTreeMap::from([value(
                0,
                MirType::Integer(IntegerWidth::U8),
            )]),
            instructions: vec![],
            outputs: vec![0],
        };

        assert_eq!(
            validate_mir(&program),
            Err(vec![MirIssue::ProgramOutputNotDefined(0)])
        );
    }

    #[test]
    fn wrapping_arithmetic_is_not_an_implicit_lowering_choice() {
        let program = MirProgram {
            inputs: vec![],
            values: BTreeMap::from([
                value(0, MirType::Integer(IntegerWidth::U8)),
                value(1, MirType::Integer(IntegerWidth::U8)),
                value(2, MirType::Integer(IntegerWidth::U8)),
            ]),
            instructions: vec![MirInstruction {
                source_node: 1,
                op: MirOp::Add {
                    mode: ArithmeticMode::Wrapping,
                },
                inputs: vec![0, 1],
                outputs: vec![2],
            }],
            outputs: vec![],
        };

        assert!(validate_mir(&program)
            .unwrap_err()
            .iter()
            .any(|issue| {
                matches!(issue, MirIssue::ArithmeticMustBeChecked { .. })
            }));
    }

    #[test]
    fn ssa_value_cannot_be_defined_twice() {
        let program = MirProgram {
            inputs: vec![],
            values: BTreeMap::from([value(
                0,
                MirType::Integer(IntegerWidth::U8),
            )]),
            instructions: vec![
                MirInstruction {
                    source_node: 1,
                    op: MirOp::ConstInteger(1),
                    inputs: vec![],
                    outputs: vec![0],
                },
                MirInstruction {
                    source_node: 2,
                    op: MirOp::ConstInteger(2),
                    inputs: vec![],
                    outputs: vec![0],
                },
            ],
            outputs: vec![],
        };

        assert!(validate_mir(&program)
            .unwrap_err()
            .contains(&MirIssue::ValueDefinedTwice(0)));
    }
}
