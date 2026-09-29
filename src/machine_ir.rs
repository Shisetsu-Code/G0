use crate::machine::{AllocationResult, PhysicalLocation};
use crate::memory::IntegerWidth;
use crate::gir::NodeId;
use crate::mir::{
    ArithmeticMode, BoolBinaryKind, CompareKind, MirOp, MirProgram, MirType,
    ValueId,
};

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
    DivChecked {
        dst: PhysicalLocation,
        left: MachineOperand,
        right: MachineOperand,
        left_width: IntegerWidth,
        right_width: IntegerWidth,
        result_width: IntegerWidth,
        compute_width: IntegerWidth,
        signed: bool,
    },
    RemChecked {
        dst: PhysicalLocation,
        left: MachineOperand,
        right: MachineOperand,
        left_width: IntegerWidth,
        right_width: IntegerWidth,
        result_width: IntegerWidth,
        compute_width: IntegerWidth,
        signed: bool,
    },
    Compare {
        dst: PhysicalLocation,
        left: MachineOperand,
        right: MachineOperand,
        kind: CompareKind,
        signed: bool,
    },
    BoolBinary {
        dst: PhysicalLocation,
        left: MachineOperand,
        right: MachineOperand,
        kind: BoolBinaryKind,
    },
    BoolNot {
        dst: PhysicalLocation,
        value: MachineOperand,
    },
    ConvertChecked {
        dst: PhysicalLocation,
        source: MachineOperand,
        output_width: IntegerWidth,
        min: i128,
        max: i128,
    },
    Truncate {
        dst: PhysicalLocation,
        source: MachineOperand,
        output_width: IntegerWidth,
        bits: u16,
        signed: bool,
    },
    Call {
        target: String,
        args: Vec<MachineOperand>,
        output: Option<MachineOutput>,
    },
    SelectCall {
        condition: MachineOperand,
        when_true: String,
        when_false: String,
        args: Vec<MachineOperand>,
        output: Option<MachineOutput>,
    },
    LoopCall {
        condition: String,
        body: String,
        state: MachineOperand,
        output: MachineOutput,
        max_iterations: u64,
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
    CallTooManyArguments {
        target: String,
        count: usize,
        maximum: usize,
    },
    CallTooManyOutputs {
        target: String,
        count: usize,
        maximum: usize,
    },
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
            MirOp::Div {
                mode,
                signed,
                compute_width,
            }
            | MirOp::Rem {
                mode,
                signed,
                compute_width,
            } => {
                if *mode != ArithmeticMode::Checked {
                    issues.push(MachineLoweringIssue::NonCheckedArithmetic);
                    continue;
                }
                if instruction.inputs.len() != 2
                    || instruction.outputs.len() != 1
                {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }

                let Some(left_location) =
                    location(allocation, instruction.inputs[0])
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.inputs[0],
                    ));
                    continue;
                };
                let Some(right_location) =
                    location(allocation, instruction.inputs[1])
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.inputs[1],
                    ));
                    continue;
                };
                let Some(dst) =
                    location(allocation, instruction.outputs[0])
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.outputs[0],
                    ));
                    continue;
                };

                let Some(left_width) =
                    integer_width(mir, instruction.inputs[0])
                else {
                    issues.push(
                        MachineLoweringIssue::UnsupportedArithmeticType(
                            instruction.inputs[0],
                        ),
                    );
                    continue;
                };
                let Some(right_width) =
                    integer_width(mir, instruction.inputs[1])
                else {
                    issues.push(
                        MachineLoweringIssue::UnsupportedArithmeticType(
                            instruction.inputs[1],
                        ),
                    );
                    continue;
                };
                let Some(result_width) =
                    integer_width(mir, instruction.outputs[0])
                else {
                    issues.push(
                        MachineLoweringIssue::UnsupportedArithmeticType(
                            instruction.outputs[0],
                        ),
                    );
                    continue;
                };

                let left = MachineOperand::Location {
                    location: left_location,
                    ty: MachineValueType::Integer(left_width),
                };
                let right = MachineOperand::Location {
                    location: right_location,
                    ty: MachineValueType::Integer(right_width),
                };

                let op = match instruction.op {
                    MirOp::Div { .. } => MachineOp::DivChecked {
                        dst,
                        left,
                        right,
                        left_width,
                        right_width,
                        result_width,
                        compute_width: *compute_width,
                        signed: *signed,
                    },
                    MirOp::Rem { .. } => MachineOp::RemChecked {
                        dst,
                        left,
                        right,
                        left_width,
                        right_width,
                        result_width,
                        compute_width: *compute_width,
                        signed: *signed,
                    },
                    _ => unreachable!(),
                };

                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op,
                });
            }
            MirOp::Compare {
                kind,
                signed,
                ..
            } => {
                if instruction.inputs.len() != 2
                    || instruction.outputs.len() != 1
                {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }

                let Some(left_location) =
                    location(allocation, instruction.inputs[0])
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.inputs[0],
                    ));
                    continue;
                };
                let Some(right_location) =
                    location(allocation, instruction.inputs[1])
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.inputs[1],
                    ));
                    continue;
                };
                let Some(dst) =
                    location(allocation, instruction.outputs[0])
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.outputs[0],
                    ));
                    continue;
                };
                let Some(left_ty) =
                    machine_value_type(mir, instruction.inputs[0])
                else {
                    issues.push(
                        MachineLoweringIssue::UnsupportedArithmeticType(
                            instruction.inputs[0],
                        ),
                    );
                    continue;
                };
                let Some(right_ty) =
                    machine_value_type(mir, instruction.inputs[1])
                else {
                    issues.push(
                        MachineLoweringIssue::UnsupportedArithmeticType(
                            instruction.inputs[1],
                        ),
                    );
                    continue;
                };

                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op: MachineOp::Compare {
                        dst,
                        left: MachineOperand::Location {
                            location: left_location,
                            ty: left_ty,
                        },
                        right: MachineOperand::Location {
                            location: right_location,
                            ty: right_ty,
                        },
                        kind: *kind,
                        signed: *signed,
                    },
                });
            }
            MirOp::BoolBinary { kind } => {
                if instruction.inputs.len() != 2
                    || instruction.outputs.len() != 1
                {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }
                let Some(left) = location(allocation, instruction.inputs[0])
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.inputs[0],
                    ));
                    continue;
                };
                let Some(right) = location(allocation, instruction.inputs[1])
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.inputs[1],
                    ));
                    continue;
                };
                let Some(dst) = location(allocation, instruction.outputs[0])
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.outputs[0],
                    ));
                    continue;
                };

                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op: MachineOp::BoolBinary {
                        dst,
                        left: MachineOperand::Location {
                            location: left,
                            ty: MachineValueType::Bool,
                        },
                        right: MachineOperand::Location {
                            location: right,
                            ty: MachineValueType::Bool,
                        },
                        kind: *kind,
                    },
                });
            }
            MirOp::BoolNot => {
                if instruction.inputs.len() != 1
                    || instruction.outputs.len() != 1
                {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }
                let Some(value_location) =
                    location(allocation, instruction.inputs[0])
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.inputs[0],
                    ));
                    continue;
                };
                let Some(dst) = location(allocation, instruction.outputs[0])
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        instruction.outputs[0],
                    ));
                    continue;
                };

                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op: MachineOp::BoolNot {
                        dst,
                        value: MachineOperand::Location {
                            location: value_location,
                            ty: MachineValueType::Bool,
                        },
                    },
                });
            }
            MirOp::ConvertChecked { min, max } => {
                if instruction.inputs.len() != 1
                    || instruction.outputs.len() != 1
                {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }

                let input = instruction.inputs[0];
                let output = instruction.outputs[0];
                let Some(source_location) = location(allocation, input)
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(input));
                    continue;
                };
                let Some(dst) = location(allocation, output) else {
                    issues.push(MachineLoweringIssue::MissingLocation(output));
                    continue;
                };
                let Some(source_width) = integer_width(mir, input) else {
                    issues.push(
                        MachineLoweringIssue::UnsupportedArithmeticType(input),
                    );
                    continue;
                };
                let Some(output_width) = integer_width(mir, output) else {
                    issues.push(
                        MachineLoweringIssue::UnsupportedArithmeticType(output),
                    );
                    continue;
                };

                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op: MachineOp::ConvertChecked {
                        dst,
                        source: MachineOperand::Location {
                            location: source_location,
                            ty: MachineValueType::Integer(source_width),
                        },
                        output_width,
                        min: *min,
                        max: *max,
                    },
                });
            }
            MirOp::Truncate { bits, signed } => {
                if instruction.inputs.len() != 1
                    || instruction.outputs.len() != 1
                {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }

                let input = instruction.inputs[0];
                let output = instruction.outputs[0];
                let Some(source_location) = location(allocation, input)
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(input));
                    continue;
                };
                let Some(dst) = location(allocation, output) else {
                    issues.push(MachineLoweringIssue::MissingLocation(output));
                    continue;
                };
                let Some(source_width) = integer_width(mir, input) else {
                    issues.push(
                        MachineLoweringIssue::UnsupportedArithmeticType(input),
                    );
                    continue;
                };
                let Some(output_width) = integer_width(mir, output) else {
                    issues.push(
                        MachineLoweringIssue::UnsupportedArithmeticType(output),
                    );
                    continue;
                };

                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op: MachineOp::Truncate {
                        dst,
                        source: MachineOperand::Location {
                            location: source_location,
                            ty: MachineValueType::Integer(source_width),
                        },
                        output_width,
                        bits: *bits,
                        signed: *signed,
                    },
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
            MirOp::SelectCall {
                when_true,
                when_false,
            } => {
                if instruction.inputs.is_empty() {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }
                let payload_count = instruction.inputs.len() - 1;
                if payload_count > crate::abi::G0_SCALAR_ARG_LIMIT {
                    issues.push(MachineLoweringIssue::CallTooManyArguments {
                        target: format!("{when_true}|{when_false}"),
                        count: payload_count,
                        maximum: crate::abi::G0_SCALAR_ARG_LIMIT,
                    });
                    continue;
                }
                if instruction.outputs.len() > 1 {
                    issues.push(MachineLoweringIssue::CallTooManyOutputs {
                        target: format!("{when_true}|{when_false}"),
                        count: instruction.outputs.len(),
                        maximum: 1,
                    });
                    continue;
                }

                let condition_value = instruction.inputs[0];
                let Some(condition_location) =
                    location(allocation, condition_value)
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        condition_value,
                    ));
                    continue;
                };
                if mir.values.get(&condition_value).map(|value| value.ty)
                    != Some(MirType::Bool)
                {
                    issues.push(
                        MachineLoweringIssue::UnsupportedArithmeticType(
                            condition_value,
                        ),
                    );
                    continue;
                }
                let condition = MachineOperand::Location {
                    location: condition_location,
                    ty: MachineValueType::Bool,
                };

                let mut args = Vec::with_capacity(payload_count);
                let mut invalid = false;
                for value in instruction.inputs.iter().skip(1) {
                    let Some(value_location) = location(allocation, *value)
                    else {
                        issues.push(MachineLoweringIssue::MissingLocation(
                            *value,
                        ));
                        invalid = true;
                        continue;
                    };
                    let Some(ty) = machine_value_type(mir, *value) else {
                        issues.push(
                            MachineLoweringIssue::UnsupportedArithmeticType(
                                *value,
                            ),
                        );
                        invalid = true;
                        continue;
                    };
                    args.push(MachineOperand::Location {
                        location: value_location,
                        ty,
                    });
                }

                let output =
                    if let Some(value) = instruction.outputs.first() {
                        let Some(value_location) =
                            location(allocation, *value)
                        else {
                            issues.push(
                                MachineLoweringIssue::MissingLocation(*value),
                            );
                            continue;
                        };
                        let Some(ty) = machine_value_type(mir, *value) else {
                            issues.push(
                                MachineLoweringIssue::UnsupportedArithmeticType(
                                    *value,
                                ),
                            );
                            continue;
                        };
                        Some(MachineOutput {
                            location: value_location,
                            ty,
                        })
                    } else {
                        None
                    };

                if !invalid {
                    operations.push(MachineInstruction {
                        source_node: instruction.source_node,
                        op: MachineOp::SelectCall {
                            condition,
                            when_true: crate::abi::graph_symbol(when_true),
                            when_false: crate::abi::graph_symbol(when_false),
                            args,
                            output,
                        },
                    });
                }
            }
            MirOp::LoopCall {
                condition,
                body,
                max_iterations,
            } => {
                if instruction.inputs.len() != 1
                    || instruction.outputs.len() != 1
                    || *max_iterations == 0
                {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }

                let input = instruction.inputs[0];
                let output_value = instruction.outputs[0];

                let Some(input_location) = location(allocation, input) else {
                    issues.push(MachineLoweringIssue::MissingLocation(input));
                    continue;
                };
                let Some(output_location) =
                    location(allocation, output_value)
                else {
                    issues.push(MachineLoweringIssue::MissingLocation(
                        output_value,
                    ));
                    continue;
                };

                let Some(input_ty) = machine_value_type(mir, input) else {
                    issues.push(
                        MachineLoweringIssue::UnsupportedArithmeticType(input),
                    );
                    continue;
                };
                let Some(output_ty) =
                    machine_value_type(mir, output_value)
                else {
                    issues.push(
                        MachineLoweringIssue::UnsupportedArithmeticType(
                            output_value,
                        ),
                    );
                    continue;
                };

                if input_ty != output_ty {
                    issues.push(MachineLoweringIssue::WrongShape);
                    continue;
                }

                operations.push(MachineInstruction {
                    source_node: instruction.source_node,
                    op: MachineOp::LoopCall {
                        condition: crate::abi::graph_symbol(condition),
                        body: crate::abi::graph_symbol(body),
                        state: MachineOperand::Location {
                            location: input_location,
                            ty: input_ty,
                        },
                        output: MachineOutput {
                            location: output_location,
                            ty: output_ty,
                        },
                        max_iterations: *max_iterations,
                    },
                });
            }
            MirOp::Call { target } => {
                if instruction.inputs.len() > crate::abi::G0_SCALAR_ARG_LIMIT {
                    issues.push(MachineLoweringIssue::CallTooManyArguments {
                        target: target.clone(),
                        count: instruction.inputs.len(),
                        maximum: crate::abi::G0_SCALAR_ARG_LIMIT,
                    });
                    continue;
                }
                if instruction.outputs.len() > 1 {
                    issues.push(MachineLoweringIssue::CallTooManyOutputs {
                        target: target.clone(),
                        count: instruction.outputs.len(),
                        maximum: 1,
                    });
                    continue;
                }

                let mut args = Vec::with_capacity(instruction.inputs.len());
                let mut call_invalid = false;
                for value in &instruction.inputs {
                    let Some(location) = location(allocation, *value) else {
                        issues.push(MachineLoweringIssue::MissingLocation(*value));
                        call_invalid = true;
                        continue;
                    };
                    let Some(ty) = machine_value_type(mir, *value) else {
                        issues.push(
                            MachineLoweringIssue::UnsupportedArithmeticType(*value),
                        );
                        call_invalid = true;
                        continue;
                    };
                    args.push(MachineOperand::Location { location, ty });
                }

                let output = if let Some(value) = instruction.outputs.first() {
                    let Some(location) = location(allocation, *value) else {
                        issues.push(MachineLoweringIssue::MissingLocation(*value));
                        continue;
                    };
                    let Some(ty) = machine_value_type(mir, *value) else {
                        issues.push(
                            MachineLoweringIssue::UnsupportedArithmeticType(*value),
                        );
                        continue;
                    };
                    Some(MachineOutput { location, ty })
                } else {
                    None
                };

                if !call_invalid {
                    operations.push(MachineInstruction {
                        source_node: instruction.source_node,
                        op: MachineOp::Call {
                            target: crate::abi::graph_symbol(target),
                            args,
                            output,
                        },
                    });
                }
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

        let has_call = operations
            .iter()
            .any(|instruction| {
                matches!(
                    instruction.op,
                    MachineOp::Call { .. }
                        | MachineOp::SelectCall { .. }
                        | MachineOp::LoopCall { .. }
                )
            });
        let stack_bytes = if has_call {
            allocation
                .stack_bytes
                .saturating_add(128)
                .div_ceil(16)
                .saturating_mul(16)
        } else {
            allocation.stack_bytes
        };

        Ok(MachineProgram {
            operations,
            stack_bytes,
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
