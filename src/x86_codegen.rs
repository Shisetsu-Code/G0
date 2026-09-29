use crate::machine::{Gpr, PhysicalLocation};
use crate::machine_ir::{
    MachineOp, MachineOperand, MachineProgram, MachineValueType,
};
use crate::memory::IntegerWidth;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X86CodegenIssue {
    Unsupported128BitInteger,
    UnsupportedStackWidth(u16),
    StackTypeWidthMismatch {
        slot_bytes: u16,
        type_bytes: u16,
    },
    ImmediateOutOfRange(i128),
    InvalidCallTarget(String),
    UnsupportedReturnArity(usize),
    OperandWidthMismatch,
}

pub fn emit_x86_64(
    program: &MachineProgram,
) -> Result<String, Vec<X86CodegenIssue>> {
    emit_x86_64_named(program, "g0_machine_main", true)
}

pub fn emit_x86_64_named(
    program: &MachineProgram,
    symbol: &str,
    exported: bool,
) -> Result<String, Vec<X86CodegenIssue>> {
    if !valid_symbol(symbol) {
        return Err(vec![X86CodegenIssue::InvalidCallTarget(
            symbol.to_owned(),
        )]);
    }

    let mut issues = Vec::new();
    let mut out = String::new();

    out.push_str(".intel_syntax noprefix\n");
    out.push_str(".text\n");
    if exported {
        out.push_str(&format!(".global {}\n", symbol));
    }
    out.push_str(&format!(".type {}, @function\n", symbol));
    out.push_str(&format!("{}:\n", symbol));
    out.push_str("    push rbp\n");
    out.push_str("    mov rbp, rsp\n");
    if program.stack_bytes != 0 {
        out.push_str(&format!("    sub rsp, {}\n", program.stack_bytes));
    }

    let mut trap_index = 0_u32;
    let mut control_index = 0_u32;
    let mut trap_labels = Vec::new();

    for instruction in &program.operations {
        out.push_str(&format!(
            "    # g0.node {}\n",
            instruction.source_node
        ));
        match &instruction.op {
            MachineOp::Move { dst, src, ty } => {
                if let Err(issue) = emit_move(&mut out, *dst, *src, *ty) {
                    issues.push(issue);
                }
            }
            MachineOp::AddChecked {
                dst,
                left,
                right,
                left_width,
                right_width,
                width,
            } => {
                let trap =
                    format!(".L{}_trap_{}", symbol, trap_index);
                trap_index += 1;
                if let Err(issue) = emit_checked_binary(
                    &mut out,
                    "add",
                    *dst,
                    *left,
                    *right,
                    ArithmeticWidths {
                        left: *left_width,
                        right: *right_width,
                        result: *width,
                    },
                    &trap,
                ) {
                    issues.push(issue);
                } else {
                    trap_labels.push(trap);
                }
            }
            MachineOp::SubChecked {
                dst,
                left,
                right,
                left_width,
                right_width,
                width,
            } => {
                let trap =
                    format!(".L{}_trap_{}", symbol, trap_index);
                trap_index += 1;
                if let Err(issue) = emit_checked_binary(
                    &mut out,
                    "sub",
                    *dst,
                    *left,
                    *right,
                    ArithmeticWidths {
                        left: *left_width,
                        right: *right_width,
                        result: *width,
                    },
                    &trap,
                ) {
                    issues.push(issue);
                } else {
                    trap_labels.push(trap);
                }
            }
            MachineOp::MulChecked {
                dst,
                left,
                right,
                left_width,
                right_width,
                width,
            } => {
                let trap =
                    format!(".L{}_trap_{}", symbol, trap_index);
                trap_index += 1;
                if let Err(issue) = emit_checked_binary(
                    &mut out,
                    "imul",
                    *dst,
                    *left,
                    *right,
                    ArithmeticWidths {
                        left: *left_width,
                        right: *right_width,
                        result: *width,
                    },
                    &trap,
                ) {
                    issues.push(issue);
                } else {
                    trap_labels.push(trap);
                }
            }
            MachineOp::Call {
                target,
                args,
                output,
            } => {
                if let Err(issue) = emit_call(
                    &mut out,
                    target,
                    args,
                    *output,
                    program.stack_bytes,
                ) {
                    issues.push(issue);
                }
            }
            MachineOp::SelectCall {
                condition,
                when_true,
                when_false,
                args,
                output,
            } => {
                let false_label = format!(
                    ".L{}_select_{}_false",
                    symbol, control_index
                );
                let join_label = format!(
                    ".L{}_select_{}_join",
                    symbol, control_index
                );
                control_index += 1;

                if let Err(issue) =
                    load_operand_to_register(&mut out, "rax", *condition)
                {
                    issues.push(issue);
                    continue;
                }
                out.push_str("    test rax, rax\n");
                out.push_str(&format!("    jz {}\n", false_label));

                if let Err(issue) = emit_call(
                    &mut out,
                    when_true,
                    args,
                    *output,
                    program.stack_bytes,
                ) {
                    issues.push(issue);
                }
                out.push_str(&format!("    jmp {}\n", join_label));
                out.push_str(&format!("{}:\n", false_label));

                if let Err(issue) = emit_call(
                    &mut out,
                    when_false,
                    args,
                    *output,
                    program.stack_bytes,
                ) {
                    issues.push(issue);
                }
                out.push_str(&format!("{}:\n", join_label));
            }
        }
    }

    match program.outputs.as_slice() {
        [] => {}
        [output] => {
            if let Err(issue) = load_to_register(
                &mut out,
                "rax",
                output.location,
                output.ty,
            ) {
                issues.push(issue);
            }
        }
        outputs => {
            issues.push(X86CodegenIssue::UnsupportedReturnArity(
                outputs.len(),
            ));
        }
    }

    out.push_str("    leave\n");
    out.push_str("    ret\n");

    for trap in trap_labels {
        out.push_str(&format!("{}:\n", trap));
        out.push_str("    ud2\n");
    }

    out.push_str(&format!(
        ".size {}, .-{}\n",
        symbol, symbol
    ));
    out.push_str(".section .note.GNU-stack,\"\" ,@progbits\n");

    if issues.is_empty() {
        Ok(out)
    } else {
        Err(issues)
    }
}

pub fn emit_entry_wrapper(
    entry_symbol: &str,
) -> Result<String, X86CodegenIssue> {
    if !valid_symbol(entry_symbol) {
        return Err(X86CodegenIssue::InvalidCallTarget(
            entry_symbol.to_owned(),
        ));
    }

    Ok(format!(
        ".intel_syntax noprefix\n.text\n.global g0_machine_main\n.type g0_machine_main, @function\ng0_machine_main:\n    push rbp\n    mov rbp, rsp\n    call {entry_symbol}\n    leave\n    ret\n.size g0_machine_main, .-g0_machine_main\n"
    ))
}

fn emit_call(
    out: &mut String,
    target: &str,
    args: &[MachineOperand],
    output: Option<crate::machine_ir::MachineOutput>,
    stack_bytes: u32,
) -> Result<(), X86CodegenIssue> {
    if !valid_symbol(target) {
        return Err(X86CodegenIssue::InvalidCallTarget(target.to_owned()));
    }
    if args.len() > crate::abi::G0_SCALAR_ARG_LIMIT {
        return Err(X86CodegenIssue::UnsupportedReturnArity(args.len()));
    }
    if stack_bytes < 128 {
        return Err(X86CodegenIssue::UnsupportedStackWidth(0));
    }

    let base = stack_bytes - 128;
    let registers = ["rdi", "rsi", "rcx", "r8", "r9", "r10"];

    for (index, register) in registers.iter().enumerate() {
        let address = scratch_address(base, (index as u32) * 8);
        out.push_str(&format!(
            "    mov QWORD PTR {}, {}\n",
            address, register
        ));
    }

    for (index, arg) in args.iter().copied().enumerate() {
        load_operand_to_register(out, "rax", arg)?;
        let address = scratch_address(base, 48 + (index as u32) * 8);
        out.push_str(&format!(
            "    mov QWORD PTR {}, rax\n",
            address
        ));
    }

    for (index, _) in args.iter().enumerate() {
        let address = scratch_address(base, 48 + (index as u32) * 8);
        out.push_str(&format!(
            "    mov {}, QWORD PTR {}\n",
            registers[index], address
        ));
    }

    out.push_str(&format!("    call {}\n", target));

    if output.is_some() {
        let address = scratch_address(base, 96);
        out.push_str(&format!(
            "    mov QWORD PTR {}, rax\n",
            address
        ));
    }

    for (index, register) in registers.iter().enumerate() {
        let address = scratch_address(base, (index as u32) * 8);
        out.push_str(&format!(
            "    mov {}, QWORD PTR {}\n",
            register, address
        ));
    }

    if let Some(output) = output {
        let address = scratch_address(base, 96);
        out.push_str(&format!(
            "    mov rax, QWORD PTR {}\n",
            address
        ));
        store_from_register(
            out,
            output.location,
            "rax",
            output.ty,
        )?;
    }

    Ok(())
}

fn scratch_address(base: u32, offset: u32) -> String {
    format!("[rbp-{}]", base.saturating_add(offset).saturating_add(8))
}

fn emit_move(
    out: &mut String,
    dst: PhysicalLocation,
    src: MachineOperand,
    ty: MachineValueType,
) -> Result<(), X86CodegenIssue> {
    match src {
        MachineOperand::Immediate(value) => {
            emit_immediate_to_rax(out, value)?;
        }
        MachineOperand::Location {
            location,
            ty: source_ty,
        } => {
            load_to_register(out, "rax", location, source_ty)?;
        }
    }

    store_from_register(out, dst, "rax", ty)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArithmeticWidths {
    left: IntegerWidth,
    right: IntegerWidth,
    result: IntegerWidth,
}

fn emit_checked_binary(
    out: &mut String,
    mnemonic: &str,
    dst: PhysicalLocation,
    left: MachineOperand,
    right: MachineOperand,
    widths: ArithmeticWidths,
    trap: &str,
) -> Result<(), X86CodegenIssue> {
    if operand_integer_width(left) != Some(widths.left)
        || operand_integer_width(right) != Some(widths.right)
    {
        return Err(X86CodegenIssue::OperandWidthMismatch);
    }
    let width = widths.result;
    if matches!(width, IntegerWidth::U128 | IntegerWidth::I128) {
        return Err(X86CodegenIssue::Unsupported128BitInteger);
    }

    load_operand_to_register(out, "rax", left)?;
    load_operand_to_register(out, "r11", right)?;

    match mnemonic {
        "add" => {
            out.push_str("    add rax, r11\n");
            emit_overflow_guard(out, width, trap, false);
        }
        "sub" => {
            out.push_str("    sub rax, r11\n");
            emit_overflow_guard(out, width, trap, false);
        }
        "imul" => {
            if width == IntegerWidth::U64 {
                out.push_str("    mul r11\n");
                out.push_str("    test rdx, rdx\n");
                out.push_str(&format!("    jne {}\n", trap));
            } else {
                out.push_str("    imul rax, r11\n");
                emit_overflow_guard(out, width, trap, true);
            }
        }
        _ => unreachable!(),
    }

    store_from_register(
        out,
        dst,
        "rax",
        MachineValueType::Integer(width),
    )
}

fn operand_integer_width(
    operand: MachineOperand,
) -> Option<IntegerWidth> {
    match operand {
        MachineOperand::Location {
            ty: MachineValueType::Integer(width),
            ..
        } => Some(width),
        _ => None,
    }
}

fn emit_overflow_guard(
    out: &mut String,
    width: IntegerWidth,
    trap: &str,
    multiplication: bool,
) {
    match width {
        IntegerWidth::U8 => {
            out.push_str("    cmp rax, 255\n");
            out.push_str(&format!("    ja {}\n", trap));
        }
        IntegerWidth::U16 => {
            out.push_str("    cmp rax, 65535\n");
            out.push_str(&format!("    ja {}\n", trap));
        }
        IntegerWidth::U32 => {
            out.push_str("    mov r11, 4294967295\n");
            out.push_str("    cmp rax, r11\n");
            out.push_str(&format!("    ja {}\n", trap));
        }
        IntegerWidth::U64 => {
            if !multiplication {
                out.push_str(&format!("    jc {}\n", trap));
            }
        }
        IntegerWidth::I8 => {
            emit_signed_bounds(out, -128, 127, trap);
        }
        IntegerWidth::I16 => {
            emit_signed_bounds(out, -32768, 32767, trap);
        }
        IntegerWidth::I32 => {
            emit_signed_bounds(out, i32::MIN as i64, i32::MAX as i64, trap);
        }
        IntegerWidth::I64 => {
            out.push_str(&format!("    jo {}\n", trap));
        }
        IntegerWidth::U128 | IntegerWidth::I128 => {}
    }
}

fn emit_signed_bounds(out: &mut String, min: i64, max: i64, trap: &str) {
    out.push_str(&format!("    cmp rax, {}\n", max));
    out.push_str(&format!("    jg {}\n", trap));
    out.push_str(&format!("    cmp rax, {}\n", min));
    out.push_str(&format!("    jl {}\n", trap));
}

fn load_operand_to_register(
    out: &mut String,
    register: &str,
    operand: MachineOperand,
) -> Result<(), X86CodegenIssue> {
    match operand {
        MachineOperand::Immediate(value) => {
            let rendered = render_immediate(value)?;
            out.push_str(&format!("    mov {}, {}\n", register, rendered));
            Ok(())
        }
        MachineOperand::Location { location, ty } => {
            load_to_register(out, register, location, ty)
        }
    }
}

fn emit_immediate_to_rax(
    out: &mut String,
    value: i128,
) -> Result<(), X86CodegenIssue> {
    let rendered = render_immediate(value)?;
    out.push_str(&format!("    mov rax, {}\n", rendered));
    Ok(())
}

fn render_immediate(value: i128) -> Result<String, X86CodegenIssue> {
    if value < i64::MIN as i128 || value > u64::MAX as i128 {
        return Err(X86CodegenIssue::ImmediateOutOfRange(value));
    }
    Ok(value.to_string())
}

fn load_to_register(
    out: &mut String,
    register: &str,
    location: PhysicalLocation,
    ty: MachineValueType,
) -> Result<(), X86CodegenIssue> {
    match location {
        PhysicalLocation::Register(source) => {
            out.push_str(&format!(
                "    mov {}, {}\n",
                register,
                register_name(source)
            ));
            Ok(())
        }
        PhysicalLocation::Stack { offset, bytes } => {
            let type_bytes = machine_type_bytes(ty)?;
            if bytes != type_bytes {
                return Err(X86CodegenIssue::StackTypeWidthMismatch {
                    slot_bytes: bytes,
                    type_bytes,
                });
            }

            let address = stack_address(offset, bytes);
            match ty {
                MachineValueType::Bool
                | MachineValueType::Integer(IntegerWidth::U8) => {
                    out.push_str(&format!(
                        "    movzx {}, BYTE PTR {}\n",
                        register, address
                    ));
                }
                MachineValueType::Integer(IntegerWidth::I8) => {
                    out.push_str(&format!(
                        "    movsx {}, BYTE PTR {}\n",
                        register, address
                    ));
                }
                MachineValueType::Integer(IntegerWidth::U16) => {
                    out.push_str(&format!(
                        "    movzx {}, WORD PTR {}\n",
                        register, address
                    ));
                }
                MachineValueType::Integer(IntegerWidth::I16) => {
                    out.push_str(&format!(
                        "    movsx {}, WORD PTR {}\n",
                        register, address
                    ));
                }
                MachineValueType::Integer(IntegerWidth::U32) => {
                    out.push_str(&format!(
                        "    mov {}, DWORD PTR {}\n",
                        register32(register), address
                    ));
                }
                MachineValueType::Integer(IntegerWidth::I32) => {
                    out.push_str(&format!(
                        "    movsxd {}, DWORD PTR {}\n",
                        register, address
                    ));
                }
                MachineValueType::Integer(IntegerWidth::U64)
                | MachineValueType::Integer(IntegerWidth::I64)
                | MachineValueType::Pointer => {
                    out.push_str(&format!(
                        "    mov {}, QWORD PTR {}\n",
                        register, address
                    ));
                }
                MachineValueType::Integer(IntegerWidth::U128)
                | MachineValueType::Integer(IntegerWidth::I128) => {
                    return Err(X86CodegenIssue::Unsupported128BitInteger);
                }
            }
            Ok(())
        }
    }
}

fn store_from_register(
    out: &mut String,
    location: PhysicalLocation,
    register: &str,
    ty: MachineValueType,
) -> Result<(), X86CodegenIssue> {
    match location {
        PhysicalLocation::Register(dst) => {
            out.push_str(&format!(
                "    mov {}, {}\n",
                register_name(dst),
                register
            ));
            Ok(())
        }
        PhysicalLocation::Stack { offset, bytes } => {
            let type_bytes = machine_type_bytes(ty)?;
            if bytes != type_bytes {
                return Err(X86CodegenIssue::StackTypeWidthMismatch {
                    slot_bytes: bytes,
                    type_bytes,
                });
            }

            let address = stack_address(offset, bytes);
            match bytes {
                1 => out.push_str(&format!(
                    "    mov BYTE PTR {}, {}\n",
                    address,
                    register8(register)
                )),
                2 => out.push_str(&format!(
                    "    mov WORD PTR {}, {}\n",
                    address,
                    register16(register)
                )),
                4 => out.push_str(&format!(
                    "    mov DWORD PTR {}, {}\n",
                    address,
                    register32(register)
                )),
                8 => out.push_str(&format!(
                    "    mov QWORD PTR {}, {}\n",
                    address, register
                )),
                16 => {
                    return Err(X86CodegenIssue::Unsupported128BitInteger);
                }
                other => {
                    return Err(X86CodegenIssue::UnsupportedStackWidth(other));
                }
            }
            Ok(())
        }
    }
}

fn machine_type_bytes(
    ty: MachineValueType,
) -> Result<u16, X86CodegenIssue> {
    Ok(match ty {
        MachineValueType::Bool => 1,
        MachineValueType::Pointer => 8,
        MachineValueType::Integer(width) => match width {
            IntegerWidth::U8 | IntegerWidth::I8 => 1,
            IntegerWidth::U16 | IntegerWidth::I16 => 2,
            IntegerWidth::U32 | IntegerWidth::I32 => 4,
            IntegerWidth::U64 | IntegerWidth::I64 => 8,
            IntegerWidth::U128 | IntegerWidth::I128 => {
                return Err(X86CodegenIssue::Unsupported128BitInteger);
            }
        },
    })
}

fn stack_address(offset: u32, bytes: u16) -> String {
    format!("[rbp-{}]", offset.saturating_add(bytes as u32))
}

fn register_name(register: Gpr) -> &'static str {
    match register {
        Gpr::Rax => "rax",
        Gpr::Rcx => "rcx",
        Gpr::Rdx => "rdx",
        Gpr::Rsi => "rsi",
        Gpr::Rdi => "rdi",
        Gpr::R8 => "r8",
        Gpr::R9 => "r9",
        Gpr::R10 => "r10",
        Gpr::R11 => "r11",
    }
}

fn register32(register: &str) -> &str {
    match register {
        "rax" => "eax",
        "r11" => "r11d",
        _ => register,
    }
}

fn register16(register: &str) -> &str {
    match register {
        "rax" => "ax",
        "r11" => "r11w",
        _ => register,
    }
}

fn register8(register: &str) -> &str {
    match register {
        "rax" => "al",
        "r11" => "r11b",
        _ => register,
    }
}

fn valid_symbol(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'.' | b'$')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::Gpr;
    use crate::machine_ir::{MachineInstruction, MachineOutput};

    #[test]
    fn named_functions_get_unique_trap_labels() {
        let program = MachineProgram {
            operations: vec![MachineInstruction {
                source_node: 1,
                op: MachineOp::AddChecked {
                    dst: PhysicalLocation::Register(Gpr::R8),
                    left: MachineOperand::Location {
                        location: PhysicalLocation::Register(Gpr::Rcx),
                        ty: MachineValueType::Integer(IntegerWidth::U8),
                    },
                    right: MachineOperand::Location {
                        location: PhysicalLocation::Register(Gpr::Rsi),
                        ty: MachineValueType::Integer(IntegerWidth::U8),
                    },
                    left_width: IntegerWidth::U8,
                    right_width: IntegerWidth::U8,
                    width: IntegerWidth::U8,
                },
            }],
            stack_bytes: 0,
            outputs: vec![MachineOutput {
                location: PhysicalLocation::Register(Gpr::R8),
                ty: MachineValueType::Integer(IntegerWidth::U8),
            }],
        };

        let asm =
            emit_x86_64_named(&program, "g0_g_61", false).unwrap();
        assert!(asm.contains(".Lg0_g_61_trap_0"));
        assert!(asm.contains(".type g0_g_61, @function"));
    }

    #[test]
    fn emits_basic_checked_machine_program() {
        let program = MachineProgram {
            operations: vec![
                MachineInstruction {
                    source_node: 1,
                    op: MachineOp::Move {
                        dst: PhysicalLocation::Register(Gpr::Rcx),
                        src: MachineOperand::Immediate(20),
                        ty: MachineValueType::Integer(IntegerWidth::U8),
                    },
                },
                MachineInstruction {
                    source_node: 2,
                    op: MachineOp::Move {
                        dst: PhysicalLocation::Register(Gpr::Rdx),
                        src: MachineOperand::Immediate(22),
                        ty: MachineValueType::Integer(IntegerWidth::U8),
                    },
                },
                MachineInstruction {
                    source_node: 3,
                    op: MachineOp::AddChecked {
                        dst: PhysicalLocation::Register(Gpr::R8),
                        left: MachineOperand::Location {
                            location: PhysicalLocation::Register(Gpr::Rcx),
                            ty: MachineValueType::Integer(IntegerWidth::U8),
                        },
                        right: MachineOperand::Location {
                            location: PhysicalLocation::Register(Gpr::Rdx),
                            ty: MachineValueType::Integer(IntegerWidth::U8),
                        },
                        left_width: IntegerWidth::U8,
                        right_width: IntegerWidth::U8,
                        width: IntegerWidth::U8,
                    },
                },
            ],
            stack_bytes: 0,
            outputs: vec![MachineOutput {
                location: PhysicalLocation::Register(Gpr::R8),
                ty: MachineValueType::Integer(IntegerWidth::U8),
            }],
        };

        let asm = emit_x86_64(&program).unwrap();
        assert!(asm.contains("add rax, r11"));
        assert!(asm.contains("cmp rax, 255"));
        assert!(asm.contains("mov rax, r8"));
    }

    #[test]
    fn signed_spill_reload_uses_sign_extension() {
        let stack = PhysicalLocation::Stack {
            offset: 0,
            bytes: 1,
        };
        let program = MachineProgram {
            operations: vec![
                MachineInstruction {
                    source_node: 1,
                    op: MachineOp::Move {
                        dst: stack,
                        src: MachineOperand::Immediate(-1),
                        ty: MachineValueType::Integer(IntegerWidth::I8),
                    },
                },
                MachineInstruction {
                    source_node: 2,
                    op: MachineOp::Move {
                        dst: PhysicalLocation::Register(Gpr::Rcx),
                        src: MachineOperand::Location {
                            location: stack,
                            ty: MachineValueType::Integer(IntegerWidth::I8),
                        },
                        ty: MachineValueType::Integer(IntegerWidth::I8),
                    },
                },
            ],
            stack_bytes: 16,
            outputs: vec![MachineOutput {
                location: PhysicalLocation::Register(Gpr::Rcx),
                ty: MachineValueType::Integer(IntegerWidth::I8),
            }],
        };

        let asm = emit_x86_64(&program).unwrap();
        assert!(asm.contains("movsx rax, BYTE PTR"));
    }

    #[test]
    fn unsigned_spill_reload_uses_zero_extension() {
        let stack = PhysicalLocation::Stack {
            offset: 0,
            bytes: 1,
        };
        let program = MachineProgram {
            operations: vec![
                MachineInstruction {
                    source_node: 1,
                    op: MachineOp::Move {
                        dst: stack,
                        src: MachineOperand::Immediate(255),
                        ty: MachineValueType::Integer(IntegerWidth::U8),
                    },
                },
                MachineInstruction {
                    source_node: 2,
                    op: MachineOp::Move {
                        dst: PhysicalLocation::Register(Gpr::Rcx),
                        src: MachineOperand::Location {
                            location: stack,
                            ty: MachineValueType::Integer(IntegerWidth::U8),
                        },
                        ty: MachineValueType::Integer(IntegerWidth::U8),
                    },
                },
            ],
            stack_bytes: 16,
            outputs: vec![MachineOutput {
                location: PhysicalLocation::Register(Gpr::Rcx),
                ty: MachineValueType::Integer(IntegerWidth::U8),
            }],
        };

        let asm = emit_x86_64(&program).unwrap();
        assert!(asm.contains("movzx rax, BYTE PTR"));
    }

    #[test]
    fn call_stages_arguments_and_restores_work_registers() {
        let program = MachineProgram {
            operations: vec![MachineInstruction {
                source_node: 7,
                op: MachineOp::Call {
                    target: "g0_g_776f726b6572".into(),
                    args: vec![
                        MachineOperand::Location {
                            location: PhysicalLocation::Register(Gpr::Rdi),
                            ty: MachineValueType::Integer(IntegerWidth::U64),
                        },
                        MachineOperand::Location {
                            location: PhysicalLocation::Register(Gpr::Rsi),
                            ty: MachineValueType::Integer(IntegerWidth::U64),
                        },
                    ],
                    output: Some(MachineOutput {
                        location: PhysicalLocation::Register(Gpr::Rcx),
                        ty: MachineValueType::Integer(IntegerWidth::U64),
                    }),
                },
            }],
            stack_bytes: 128,
            outputs: vec![MachineOutput {
                location: PhysicalLocation::Register(Gpr::Rcx),
                ty: MachineValueType::Integer(IntegerWidth::U64),
            }],
        };

        let asm = emit_x86_64(&program).unwrap();
        assert!(asm.contains("call g0_g_776f726b6572"));
        assert!(asm.contains("mov QWORD PTR [rbp-8], rdi"));
        assert!(asm.contains("mov rdi, QWORD PTR"));
        assert!(asm.contains("mov rcx, QWORD PTR"));
    }

    #[test]
    fn unsafe_call_symbol_is_rejected() {
        let program = MachineProgram {
            operations: vec![MachineInstruction {
                source_node: 1,
                op: MachineOp::Call {
                    target: "foo; rm".into(),
                    args: vec![],
                    output: None,
                },
            }],
            stack_bytes: 0,
            outputs: vec![],
        };

        assert!(matches!(
            emit_x86_64(&program).unwrap_err().first(),
            Some(X86CodegenIssue::InvalidCallTarget(_))
        ));
    }

    #[test]
    fn unsupported_wide_stack_slot_is_rejected() {
        let program = MachineProgram {
            operations: vec![MachineInstruction {
                source_node: 1,
                op: MachineOp::Move {
                    dst: PhysicalLocation::Stack {
                        offset: 0,
                        bytes: 16,
                    },
                    src: MachineOperand::Immediate(1),
                    ty: MachineValueType::Integer(IntegerWidth::U128),
                },
            }],
            stack_bytes: 16,
            outputs: vec![],
        };

        assert_eq!(
            emit_x86_64(&program),
            Err(vec![X86CodegenIssue::Unsupported128BitInteger])
        );
    }
}
