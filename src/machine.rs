use std::collections::{BTreeMap, BTreeSet};

use crate::memory::IntegerWidth;
use crate::mir::{MirProgram, MirType, ValueId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Gpr {
    Rax,
    Rcx,
    Rdx,
    Rsi,
    Rdi,
    R8,
    R9,
    R10,
    R11,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalLocation {
    Register(Gpr),
    Stack { offset: u32, bytes: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineProfile {
    pub gpr_count: usize,
    pub stack_alignment: u32,
    pub cache_line_bytes: u32,
    pub vector_bits: u16,
}

impl MachineProfile {
    pub fn x86_64_v3() -> Self {
        Self {
            gpr_count: 6,
            stack_alignment: 16,
            cache_line_bytes: 64,
            vector_bits: 256,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveInterval {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AllocationResult {
    pub locations: BTreeMap<ValueId, PhysicalLocation>,
    pub intervals: BTreeMap<ValueId, LiveInterval>,
    pub stack_bytes: u32,
    pub spills: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocationIssue {
    UnknownValue(ValueId),
    InvalidProfile,
    TooManyInputs {
        inputs: usize,
        capacity: usize,
    },
    UnsupportedInputType(ValueId),
}

pub fn compute_live_intervals(
    program: &MirProgram,
) -> Result<BTreeMap<ValueId, LiveInterval>, AllocationIssue> {
    let mut intervals = BTreeMap::<ValueId, LiveInterval>::new();

    for (index, instruction) in program.instructions.iter().enumerate() {
        for output in &instruction.outputs {
            if !program.values.contains_key(output) {
                return Err(AllocationIssue::UnknownValue(*output));
            }
            intervals.entry(*output).or_insert(LiveInterval {
                start: index,
                end: index,
            });
        }

        for input in &instruction.inputs {
            if !program.values.contains_key(input) {
                return Err(AllocationIssue::UnknownValue(*input));
            }
            intervals
                .entry(*input)
                .and_modify(|interval| interval.end = interval.end.max(index))
                .or_insert(LiveInterval {
                    start: 0,
                    end: index,
                });
        }
    }

    Ok(intervals)
}

pub fn linear_scan_allocate(
    program: &MirProgram,
    profile: MachineProfile,
) -> Result<AllocationResult, AllocationIssue> {
    if profile.gpr_count == 0 || profile.stack_alignment == 0 {
        return Err(AllocationIssue::InvalidProfile);
    }

    let intervals = compute_live_intervals(program)?;
    // G0 integer/pointer ABI input order. RAX/RDX/R11 remain backend scratch.
    let register_pool = [
        Gpr::Rdi,
        Gpr::Rsi,
        Gpr::Rcx,
        Gpr::R8,
        Gpr::R9,
        Gpr::R10,
    ];
    let available_registers: Vec<Gpr> = register_pool
        .into_iter()
        .take(profile.gpr_count.min(register_pool.len()))
        .collect();

    if program.inputs.len() > available_registers.len() {
        return Err(AllocationIssue::TooManyInputs {
            inputs: program.inputs.len(),
            capacity: available_registers.len(),
        });
    }

    let mut precolored = BTreeMap::<ValueId, Gpr>::new();
    for (value, register) in program
        .inputs
        .iter()
        .copied()
        .zip(available_registers.iter().copied())
    {
        let Some(mir_value) = program.values.get(&value) else {
            return Err(AllocationIssue::UnknownValue(value));
        };
        if !scalar_register_eligible(mir_value.ty) {
            return Err(AllocationIssue::UnsupportedInputType(value));
        }
        precolored.insert(value, register);
    }

    let mut ordered: Vec<(ValueId, LiveInterval)> =
        intervals.iter().map(|(id, interval)| (*id, *interval)).collect();
    ordered.sort_by_key(|(_, interval)| (interval.start, interval.end));

    let mut active: Vec<(ValueId, LiveInterval, Gpr)> = Vec::new();
    let mut free: BTreeSet<Gpr> = available_registers.iter().copied().collect();
    for (value, register) in &precolored {
        if intervals.contains_key(value) {
            free.remove(register);
        }
    }
    let mut result = AllocationResult {
        intervals: intervals.clone(),
        ..AllocationResult::default()
    };
    let mut stack_offset = 0_u32;

    for (value, interval) in ordered {
        active.retain(|(_, active_interval, register)| {
            if active_interval.end < interval.start {
                free.insert(*register);
                false
            } else {
                true
            }
        });

        if let Some(register) = precolored.get(&value).copied() {
            free.remove(&register);
            result
                .locations
                .insert(value, PhysicalLocation::Register(register));
            active.push((value, interval, register));
            active.sort_by_key(|(_, active_interval, _)| active_interval.end);
            continue;
        }

        if scalar_register_eligible(program.values[&value].ty)
            && let Some(register) = free.pop_first()
        {
            result
                .locations
                .insert(value, PhysicalLocation::Register(register));
            active.push((value, interval, register));
            active.sort_by_key(|(_, active_interval, _)| active_interval.end);
            continue;
        }

        let bytes = type_size_bytes(program.values[&value].ty);
        let aligned = align_up(stack_offset, bytes.max(1) as u32);
        result.locations.insert(
            value,
            PhysicalLocation::Stack {
                offset: aligned,
                bytes,
            },
        );
        stack_offset = aligned.saturating_add(bytes as u32);
        result.spills = result.spills.saturating_add(1);
    }

    result.stack_bytes = align_up(stack_offset, profile.stack_alignment);
    Ok(result)
}

fn scalar_register_eligible(ty: MirType) -> bool {
    matches!(
        ty,
        MirType::Bool
            | MirType::Integer(_)
            | MirType::Pointer
            | MirType::TextHandle
    )
}

fn type_size_bytes(ty: MirType) -> u16 {
    match ty {
        MirType::Bool => 1,
        MirType::Integer(width) => match width {
            IntegerWidth::U8 | IntegerWidth::I8 => 1,
            IntegerWidth::U16 | IntegerWidth::I16 => 2,
            IntegerWidth::U32 | IntegerWidth::I32 => 4,
            IntegerWidth::U64 | IntegerWidth::I64 => 8,
            IntegerWidth::U128 | IntegerWidth::I128 => 16,
        },
        MirType::Float32 => 4,
        MirType::Float64 | MirType::Pointer | MirType::TextHandle => 8,
        MirType::Bytes => 16,
    }
}

fn align_up(value: u32, alignment: u32) -> u32 {
    if alignment <= 1 {
        return value;
    }
    let remainder = value % alignment;
    if remainder == 0 {
        value
    } else {
        value.saturating_add(alignment - remainder)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RegisterPressure {
    pub peak_live_values: usize,
    pub allocated_register_values: usize,
    pub spills: u32,
    pub stack_bytes: u32,
}

pub fn register_pressure(
    allocation: &AllocationResult,
) -> RegisterPressure {
    let peak_live_values = allocation
        .intervals
        .values()
        .flat_map(|interval| interval.start..=interval.end)
        .fold(BTreeMap::<usize, usize>::new(), |mut counts, point| {
            *counts.entry(point).or_default() += 1;
            counts
        })
        .into_values()
        .max()
        .unwrap_or(0);

    let allocated_register_values = allocation
        .locations
        .values()
        .filter(|location| matches!(location, PhysicalLocation::Register(_)))
        .count();

    RegisterPressure {
        peak_live_values,
        allocated_register_values,
        spills: allocation.spills,
        stack_bytes: allocation.stack_bytes,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::mir::{MirInstruction, MirOp, MirValue};

    fn value(id: ValueId) -> (ValueId, MirValue) {
        (
            id,
            MirValue {
                id,
                ty: MirType::Integer(IntegerWidth::U64),
                location: None,
            },
        )
    }

    #[test]
    fn linear_scan_reuses_register_after_interval_ends() {
        let program = MirProgram {
            inputs: vec![],
            values: BTreeMap::from([value(0), value(1), value(2), value(3)]),
            instructions: vec![
                MirInstruction {
                    source_node: 1,
                    op: MirOp::ConstInteger(1),
                    inputs: vec![],
                    outputs: vec![0],
                },
                MirInstruction {
                    source_node: 2,
                    op: MirOp::Copy,
                    inputs: vec![0],
                    outputs: vec![1],
                },
                MirInstruction {
                    source_node: 3,
                    op: MirOp::ConstInteger(2),
                    inputs: vec![],
                    outputs: vec![2],
                },
                MirInstruction {
                    source_node: 4,
                    op: MirOp::Copy,
                    inputs: vec![2],
                    outputs: vec![3],
                },
            ],
            outputs: vec![],
        };

        let allocation = linear_scan_allocate(
            &program,
            MachineProfile {
                gpr_count: 1,
                ..MachineProfile::x86_64_v3()
            },
        )
        .unwrap();

        assert!(allocation.spills <= 2);
    }

    #[test]
    fn register_pressure_reports_spills() {
        let program = MirProgram {
            inputs: vec![],
            values: BTreeMap::from([value(0), value(1), value(2)]),
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
                    outputs: vec![1],
                },
                MirInstruction {
                    source_node: 3,
                    op: MirOp::Add {
                        mode: crate::mir::ArithmeticMode::Checked,
                    },
                    inputs: vec![0, 1],
                    outputs: vec![2],
                },
            ],
            outputs: vec![],
        };

        let allocation = linear_scan_allocate(
            &program,
            MachineProfile {
                gpr_count: 1,
                ..MachineProfile::x86_64_v3()
            },
        )
        .unwrap();
        let pressure = register_pressure(&allocation);

        assert!(pressure.peak_live_values >= 2);
        assert!(pressure.spills >= 1);
    }
}
