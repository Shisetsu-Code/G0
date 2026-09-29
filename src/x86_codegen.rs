use crate::machine::{Gpr, PhysicalLocation};
use crate::machine_ir::{
    MachineOp, MachineOperand, MachineProgram, MachineValueType,
};
use crate::memory::IntegerWidth;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X86CodegenIssue {
    Unsupported128BitInteger,
    UnsupportedStackWidth(u16),
    ImmediateOutOfRange(i128),
    InvalidCallTarget(String),
    UnsupportedReturnArity(usize),
}

pub fn emit_x86_64(
    program: &MachineProgram,
) -> Result<String, Vec<X86CodegenIssue>> {
    let mut issues = Vec::new();
    let mut out = String::new();

    out.push_str(".intel_syntax noprefix\n");
    out.push_str(".text\n");
    out.push_str(".global g0_machine_main\n");
    out.push_str(".type g0_machine_main, @function\n");
    out.push_str("g0_machine_main:\n");
    out.push_str("    push rbp\n");
    out.push_str("    mov rbp, rsp\n");
    if program.stack_bytes != 0 {
        out.push_str(&format!("    sub rsp, {}\n", program.stack_bytes));
    }

    let mut trap_index = 0_u32;
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
                let trap = format!(".Ltrap_{}", trap_index);
                trap_index += 1;
                if let Err(issue) = emit_checked_binary(
                    &mut out,
                    "add",
                    *dst,
                    *left,
                    *right,
                    *left_width,
                    *right_width,
                    *width,
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
                let trap = format!(".Ltrap_{}", trap_index);
                trap_index += 1;
                if let Err(issue) = emit_checked_binary(
                    &mut out,
                    "sub",
                    *dst,
                    *left,
                    *right,
                    *left_width,
                    *right_width,
                    *width,
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
                let trap = format!(".Ltrap_{}", trap_index);
                trap_index += 1;
                if let Err(issue) = emit_checked_binary(
                    &mut out,
                    "imul",
                    *dst,
                    *left,
                    *right,
                    *left_width,
                    *right_width,
                    *width,
                    &trap,
                ) {
                    issues.push(issue);
                } else {
                    trap_labels.push(trap);
                }
            }
            MachineOp::Call { target } => {
                if !valid_symbol(target) {
                    issues.push(X86CodegenIssue::InvalidCallTarget(
                        target.clone(),
                    ));
                } else {
                    out.push_str(&format!("    call {}\n", target));
                }
            }
        }
    }

    match program.outputs.as_slice() {
        [] => {}
        [location] => {
            if let Err(issue) = load_to_register(&mut out, "rax", *location) {
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

    out.push_str(".size g0_machine_main, .-g0_machine_main\n");
    out.push_str(".section .note.GNU-stack,\"\" ,@progbits\n");

    if issues.is_empty() {
        Ok(out)
    } else {
        Err(issues)
    }
}

fn emit_move(
    out: &mut String,
    dst: PhysicalLocation,
    src: MachineOperand,
) -> Result<(), X86CodegenIssue> {
    match src {
        MachineOperand::Immediate(value) => {
            emit_immediate_to_rax(out, value)?;
        }
        MachineOperand::Location(location) => {
            load_to_register(out, "rax", location)?;
        }
    }

    store_from_register(out, dst, "rax")
}

fn emit_checked_binary(
    out: &mut String,
    mnemonic: &str,
    dst: PhysicalLocation,
    left: MachineOperand,
    right: MachineOperand,
    width: IntegerWidth,
    trap: &str,
) -> Result<(), X86CodegenIssue> {
    if matches!(width, IntegerWidth::U128 | IntegerWidth::I128) {
        return Err(X86CodegenIssue::Unsupported128BitInteger);
    }

    if matches!(left, MachineOperand::Location(PhysicalLocation::Stack { .. }))
        || matches!(right, MachineOperand::Location(PhysicalLocation::Stack { .. }))
    {
        return Err(X86CodegenIssue::ArithmeticSpillUnsupported);
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
                return Err(X86CodegenIssue::Unsupported128BitInteger);
            }
            out.push_str("    imul rax, r11\n");
            emit_overflow_guard(out, width, trap, true);
        }
        _ => unreachable!(),
    }

    store_from_register(out, dst, "rax")
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
        MachineOperand::Location(location) => {
            load_to_register(out, register, location)
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
            let address = stack_address(offset, bytes);
            match bytes {
                1 => out.push_str(&format!(
                    "    movzx {}, BYTE PTR {}\n",
                    register, address
                )),
                2 => out.push_str(&format!(
                    "    movzx {}, WORD PTR {}\n",
                    register, address
                )),
                4 => out.push_str(&format!(
                    "    mov {}, DWORD PTR {}\n",
                    register32(register),
                    address
                )),
                8 => out.push_str(&format!(
                    "    mov {}, QWORD PTR {}\n",
                    register, address
                )),
                other => {
                    return Err(X86CodegenIssue::UnsupportedStackWidth(other));
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
                other => {
                    return Err(X86CodegenIssue::UnsupportedStackWidth(other));
                }
            }
            Ok(())
        }
    }
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
    use crate::machine_ir::MachineInstruction;

    #[test]
    fn emits_basic_checked_machine_program() {
        let program = MachineProgram {
            operations: vec![
                MachineInstruction {
                    source_node: 1,
                    op: MachineOp::Move {
                        dst: PhysicalLocation::Register(Gpr::Rcx),
                        src: MachineOperand::Immediate(20),
                    },
                },
                MachineInstruction {
                    source_node: 2,
                    op: MachineOp::Move {
                        dst: PhysicalLocation::Register(Gpr::Rdx),
                        src: MachineOperand::Immediate(22),
                    },
                },
                MachineInstruction {
                    source_node: 3,
                    op: MachineOp::AddChecked {
                        dst: PhysicalLocation::Register(Gpr::R8),
                        left: MachineOperand::Location(
                            PhysicalLocation::Register(Gpr::Rcx),
                        ),
                        right: MachineOperand::Location(
                            PhysicalLocation::Register(Gpr::Rdx),
                        ),
                        width: IntegerWidth::U8,
                    },
                },
            ],
            stack_bytes: 0,
            outputs: vec![PhysicalLocation::Register(Gpr::R8)],
        };

        let asm = emit_x86_64(&program).unwrap();
        assert!(asm.contains("add rax, r11"));
        assert!(asm.contains("cmp rax, 255"));
        assert!(asm.contains("mov rax, r8"));
    }

    #[test]
    fn unsafe_call_symbol_is_rejected() {
        let program = MachineProgram {
            operations: vec![MachineInstruction {
                source_node: 1,
                op: MachineOp::Call {
                    target: "foo; rm".into(),
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
                },
            }],
            stack_bytes: 16,
            outputs: vec![],
        };

        assert_eq!(
            emit_x86_64(&program),
            Err(vec![X86CodegenIssue::UnsupportedStackWidth(16)])
        );
    }
}
