use crate::machine::{AllocationResult, PhysicalLocation};
use crate::memory::IntegerWidth;
use crate::gir::NodeId;
use crate::mir::{ArithmeticMode, MirOp, MirProgram, MirType, ValueId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineOperand {
    Location(PhysicalLocation),
    Immediate(i128),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineOp {
    Move {
        dst: PhysicalLocation,
        src: MachineOperand,
    },
    AddChecked {
        dst: PhysicalLocation,
        left: MachineOperand,
        right: MachineOperand,
        width: IntegerWidth,
    },
    SubChecked {
        dst: PhysicalLocation,
        left: MachineOperand,
        right: MachineOperand,
        width: IntegerWidth,
    },
    MulChecked {
        dst: PhysicalLocation,
        left: MachineOperand,
        right: MachineOperand,
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MachineProgram {
    pub operations: Vec<MachineInstruction>,
    pub stack_bytes: u32,
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

                let Some(width) = integer_width(mir, instruction.outputs[0]) else {
                    issues.push(MachineLoweringIssue::UnsupportedArithmeticType(
                        instruction.outputs[0],
                    ));
                    continue;
                };

                let left = MachineOperand::Location(left);
                let right = MachineOperand::Location(right);
                let op = match instruction.op {
                    MirOp::Add { .. } => MachineOp::AddChecked {
                        dst,
                        left,
                        right,
                        width,
                    },
                    MirOp::Sub { .. } => MachineOp::SubChecked {
                        dst,
                        left,
                        right,
                        width,
                    },
                    MirOp::Mul { .. } => MachineOp::MulChecked {
                        dst,
                        left,
                        right,
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
                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op: MachineOp::Move {
                        dst,
                        src: MachineOperand::Location(src),
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
        Ok(MachineProgram {
            operations,
            stack_bytes: allocation.stack_bytes,
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
