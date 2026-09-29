use std::collections::BTreeMap;

use crate::gir::NodeId;
use crate::machine_ir::MachineProgram;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineRange {
    pub first_instruction: usize,
    pub last_instruction: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceMap {
    pub by_node: BTreeMap<NodeId, Vec<MachineRange>>,
}

pub fn build_source_map(program: &MachineProgram) -> SourceMap {
    let mut indices = BTreeMap::<NodeId, Vec<usize>>::new();

    for (index, instruction) in program.operations.iter().enumerate() {
        indices
            .entry(instruction.source_node)
            .or_default()
            .push(index);
    }

    let mut by_node = BTreeMap::new();
    for (node, points) in indices {
        by_node.insert(node, compress_ranges(&points));
    }

    SourceMap { by_node }
}

fn compress_ranges(points: &[usize]) -> Vec<MachineRange> {
    let Some((&first, rest)) = points.split_first() else {
        return Vec::new();
    };

    let mut ranges = Vec::new();
    let mut start = first;
    let mut previous = first;

    for point in rest {
        if *point == previous + 1 {
            previous = *point;
            continue;
        }
        ranges.push(MachineRange {
            first_instruction: start,
            last_instruction: previous,
        });
        start = *point;
        previous = *point;
    }

    ranges.push(MachineRange {
        first_instruction: start,
        last_instruction: previous,
    });
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::{Gpr, PhysicalLocation};
    use crate::machine_ir::{
        MachineInstruction, MachineOp, MachineOperand, MachineValueType,
    };

    #[test]
    fn source_map_groups_machine_operations_by_gir_node() {
        let program = MachineProgram {
            operations: vec![
                MachineInstruction {
                    source_node: 1,
                    op: MachineOp::Move {
                        dst: PhysicalLocation::Register(Gpr::Rcx),
                        src: MachineOperand::Immediate(1),
                        ty: MachineValueType::Integer(
                            crate::memory::IntegerWidth::U8,
                        ),
                    },
                },
                MachineInstruction {
                    source_node: 1,
                    op: MachineOp::Move {
                        dst: PhysicalLocation::Register(Gpr::Rdx),
                        src: MachineOperand::Immediate(2),
                        ty: MachineValueType::Integer(
                            crate::memory::IntegerWidth::U8,
                        ),
                    },
                },
                MachineInstruction {
                    source_node: 2,
                    op: MachineOp::Move {
                        dst: PhysicalLocation::Register(Gpr::R8),
                        src: MachineOperand::Immediate(3),
                        ty: MachineValueType::Integer(
                            crate::memory::IntegerWidth::U8,
                        ),
                    },
                },
            ],
            stack_bytes: 0,
            outputs: vec![],
        };

        let map = build_source_map(&program);
        assert_eq!(
            map.by_node[&1],
            vec![MachineRange {
                first_instruction: 0,
                last_instruction: 1,
            }]
        );
        assert_eq!(
            map.by_node[&2],
            vec![MachineRange {
                first_instruction: 2,
                last_instruction: 2,
            }]
        );
    }
}
