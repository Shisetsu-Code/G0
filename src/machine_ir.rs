use crate::machine::{AllocationResult, PhysicalLocation};
use crate::memory::IntegerWidth;
use crate::gir::NodeId;
use crate::mir::{ArithmeticMode, MirOp, MirProgram, MirType, ValueId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineValueType {
    Bool,
    Integer(IntegerWidth),
    Pointer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineOperand {
    Location {
        location: PhysicalLocation,
        ty: MachineValueType,
    },
    Immediate(i128),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineOp {
    Move {
        dst: PhysicalLocation,
        src: MachineOperand,
        ty: MachineValueType,
    },
    AddChecked {
        dst: PhysicalLocation,
        left: MachineOperand,
        right: MachineOperand,
        left_width: IntegerWidth,
        right_width: IntegerWidth,
        width: IntegerWidth,
    },
    SubChecked {
        dst: PhysicalLocation,
        left: MachineOperand,
        right: MachineOperand,
        left_width: IntegerWidth,
        right_width: IntegerWidth,
        width: IntegerWidth,
    },
    MulChecked {
        dst: PhysicalLocation,
        left: MachineOperand,
        right: MachineOperand,
        left_width: IntegerWidth,
        right_width: IntegerWidth,
        width: IntegerWidth,
    },
    Call {
        target: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineInstruction {
    pub source_node: NodeId,
    pub op: MachineOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineOutput {
    pub location: PhysicalLocation,
    pub ty: MachineValueType,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MachineProgram {
    pub operations: Vec<MachineInstruction>,
    pub stack_bytes: u32,
    pub outputs: Vec<MachineOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineLoweringIssue {
    MissingLocation(ValueId),
    UnsupportedMirOperation(String),
    UnsupportedArithmeticType(ValueId),
    NonCheckedArithmetic,
    WrongShape,
}

pub fn lower_mir(
    mir: &MirProgram,
    allocation: &AllocationResult,
) -> Result<MachineProgram, Vec<MachineLoweringIssue>> {
    let mut operations = Vec::new();
    let mut issues = Vec::new();

    for instruction in &mir.instructions {
        match &instruction.op {
            MirOp::ConstInteger(value) => {
                if instruction.outputs.len() != 1 {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }
                let Some(dst) = location(allocation, instruction.outputs[0]) else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.outputs[0],
                    ));
                    continue;
                };
                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op: MachineOp::Move {
                        dst,
                        src: MachineOperand::Immediate(*value),
                        ty: MachineValueType::Integer(
                            integer_width(mir, instruction.outputs[0])
                                .expect("integer const output"),
                        ),
                    },
                });
            }
            MirOp::ConstBool(value) => {
                if instruction.outputs.len() != 1 {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }
                let Some(dst) = location(allocation, instruction.outputs[0]) else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.outputs[0],
                    ));
                    continue;
                };
                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op: MachineOp::Move {
                        dst,
                        src: MachineOperand::Immediate(i128::from(*value)),
                        ty: MachineValueType::Bool,
                    },
                });
            }
            MirOp::Add { mode } | MirOp::Sub { mode } | MirOp::Mul { mode } => {
                if *mode != ArithmeticMode::Checked {
                    issues.push(MachineLoweringIssue::NonCheckedArithmetic);
                    continue;
                }
                if instruction.inputs.len() != 2 || instruction.outputs.len() != 1 {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }

                let Some(left) = location(allocation, instruction.inputs[0]) else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.inputs[0],
                    ));
                    continue;
                };
                let Some(right) = location(allocation, instruction.inputs[1]) else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.inputs[1],
                    ));
                    continue;
                };
                let Some(dst) = location(allocation, instruction.outputs[0]) else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.outputs[0],
                    ));
                    continue;
                };

                let Some(left_width) = integer_width(mir, instruction.inputs[0]) else {
                    issues.push(MachineLoweringIssue::UnsupportedArithmeticType(
                        instruction.inputs[0],
                    ));
                    continue;
                };
                let Some(right_width) = integer_width(mir, instruction.inputs[1]) else {
                    issues.push(MachineLoweringIssue::UnsupportedArithmeticType(
                        instruction.inputs[1],
                    ));
                    continue;
                };
                let Some(width) = integer_width(mir, instruction.outputs[0]) else {
                    issues.push(MachineLoweringIssue::UnsupportedArithmeticType(
                        instruction.outputs[0],
                    ));
                    continue;
                };

                let left = MachineOperand::Location {
                    location: left,
                    ty: MachineValueType::Integer(left_width),
                };
                let right = MachineOperand::Location {
                    location: right,
                    ty: MachineValueType::Integer(right_width),
                };
                let op = match instruction.op {
                    MirOp::Add { .. } => MachineOp::AddChecked {
                        dst,
                        left,
                        right,
                        left_width,
                        right_width,
                        width,
                    },
                    MirOp::Sub { .. } => MachineOp::SubChecked {
                        dst,
                        left,
                        right,
                        left_width,
                        right_width,
                        width,
                    },
                    MirOp::Mul { .. } => MachineOp::MulChecked {
                        dst,
                        left,
                        right,
                        left_width,
                        right_width,
                        width,
                    },
                    _ => unreachable!(),
                };
                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op,
                });
            }
            MirOp::Copy | MirOp::Move => {
                if instruction.inputs.len() != 1 || instruction.outputs.len() != 1 {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }
                let Some(src) = location(allocation, instruction.inputs[0]) else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.inputs[0],
                    ));
                    continue;
                };
                let Some(dst) = location(allocation, instruction.outputs[0]) else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.outputs[0],
                    ));
                    continue;
                };
                let Some(ty) = machine_value_type(mir, instruction.inputs[0]) else {
                    issues.push(MachineLoweringIssue::UnsupportedArithmeticType(
                        instruction.inputs[0],
                    ));
                    continue;
                };
                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op: MachineOp::Move {
                        dst,
                        src: MachineOperand::Location {
                            location: src,
                            ty,
                        },
                        ty,
                    },
                });
            }
            MirOp::Call { target } => {
                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op: MachineOp::Call {
                        target: target.clone(),
                    },
                });
            }
            MirOp::Load | MirOp::Store => {
                issues.push(MachineLoweringIssue::UnsupportedMirOperation(
                    format!("{:?}", instruction.op),
                ));
            }
        }
    }

    if issues.is_empty() {
        let mut outputs = Vec::with_capacity(mir.outputs.len());
        for value in &mir.outputs {
            let Some(output) = location(allocation, *value) else {
                return Err(vec![MachineLoweringIssue::MissingLocation(*value)]);
            };
            let Some(ty) = machine_value_type(mir, *value) else {
                return Err(vec![
                    MachineLoweringIssue::UnsupportedArithmeticType(*value),
                ]);
            };
            outputs.push(MachineOutput {
                location: output,
                ty,
            });
        }

        Ok(MachineProgram {
            operations,
            stack_bytes: allocation.stack_bytes,
            outputs,
        })
    } else {
        Err(issues)
    }
}

fn location(
    allocation: &AllocationResult,
    value: ValueId,
) -> Option<PhysicalLocation> {
    allocation.locations.get(&value).copied()
}

fn machine_value_type(
    mir: &MirProgram,
    value: ValueId,
) -> Option<MachineValueType> {
    match mir.values.get(&value)?.ty {
        MirType::Bool => Some(MachineValueType::Bool),
        MirType::Integer(width) => Some(MachineValueType::Integer(width)),
        MirType::Pointer | MirType::TextHandle => Some(MachineValueType::Pointer),
        MirType::Float32 | MirType::Float64 | MirType::Bytes => None,
    }
}

fn integer_width(mir: &MirProgram, value: ValueId) -> Option<IntegerWidth> {
    match mir.values.get(&value)?.ty {
        MirType::Integer(width) => Some(width),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::machine::Gpr;
    use crate::mir::{MirInstruction, MirValue};

    #[test]
    fn checked_add_lowers_to_three_operand_machine_op() {
        let mir = MirProgram {
            inputs: vec![],
            values: BTreeMap::from([
                (
                    0,
                    MirValue {
                        id: 0,
                        ty: MirType::Integer(IntegerWidth::U8),
                        location: None,
                    },
                ),
                (
                    1,
                    MirValue {
                        id: 1,
                        ty: MirType::Integer(IntegerWidth::U8),
                        location: None,
                    },
                ),
                (
                    2,
                    MirValue {
                        id: 2,
                        ty: MirType::Integer(IntegerWidth::U16),
                        location: None,
                    },
                ),
            ]),
            instructions: vec![MirInstruction {
                source_node: 1,
                op: MirOp::Add {
                    mode: ArithmeticMode::Checked,
                },
                inputs: vec![0, 1],
                outputs: vec![2],
            }],
            outputs: vec![],
        };

        let allocation = AllocationResult {
            locations: BTreeMap::from([
                (0, PhysicalLocation::Register(Gpr::Rcx)),
                (1, PhysicalLocation::Register(Gpr::Rdx)),
                (2, PhysicalLocation::Register(Gpr::R8)),
            ]),
            stack_bytes: 0,
            ..AllocationResult::default()
        };

        let machine = lower_mir(&mir, &allocation).unwrap();
        assert!(matches!(
            machine.operations[0].op,
            MachineOp::AddChecked {
                dst: PhysicalLocation::Register(Gpr::R8),
                ..
            }
        ));
    }
}
