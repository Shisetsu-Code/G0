//! Direct System V x86-64 scheduling for immutable aggregate values.
//! Values cross graph boundaries as bounded runtime handles; calls, selection,
//! loops and map iteration are machine-code control flow, not an interpreter.
use crate::{
    abi::graph_symbol,
    call_graph::{build_call_graph, reachable_program_graphs},
    gir::{Graph, Operation, SemanticType, SourceEndpoint, TargetEndpoint},
    program::{PlatformContract, ProgramContract, validate_program},
    program_binary::{ProgramDocument, decode_program, encode_program},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregateCompileIssue {
    InvalidProgram,
    EntryInterface,
    Unsupported { graph: String, node: u32 },
    InvalidTopology,
    SizeOverflow,
}
#[derive(Debug, Clone)]
pub struct CompiledAggregate {
    pub assembly: String,
    pub document: Vec<u8>,
}

pub fn compile_program(
    program: &ProgramContract,
) -> Result<CompiledAggregate, AggregateCompileIssue> {
    validate_program(program, &PlatformContract::bootstrap_x86_64_v3())
        .map_err(|_| AggregateCompileIssue::InvalidProgram)?;
    if !program.external_subgraphs.is_empty() {
        return Err(AggregateCompileIssue::InvalidProgram);
    }
    // The embedded document currently serializes these executable sections.
    // Never silently discard additional ownership, effect or resource contracts.
    let closed = ProgramContract {
        entry_graph: program.entry_graph.clone(),
        graphs: program.graphs.clone(),
        schemas: program.schemas.clone(),
        ..ProgramContract::default()
    };
    if &closed != program {
        return Err(AggregateCompileIssue::InvalidProgram);
    }
    let document = encode_program(&ProgramDocument {
        entry_graph: program
            .entry_graph
            .clone()
            .ok_or(AggregateCompileIssue::InvalidProgram)?,
        graphs: program.graphs.clone(),
        schemas: program.schemas.clone(),
    })
    .map_err(|_| AggregateCompileIssue::InvalidProgram)?;
    // Match the metadata indices to the canonical graph/node order used by the runtime.
    let canonical = decode_program(&document)
        .map_err(|_| AggregateCompileIssue::InvalidProgram)?
        .validated_contract()
        .map_err(|_| AggregateCompileIssue::InvalidProgram)?;
    let entry = canonical
        .graphs
        .iter()
        .find(|g| Some(&g.name) == canonical.entry_graph.as_ref())
        .ok_or(AggregateCompileIssue::InvalidProgram)?;
    if entry.outputs.len() != 1 {
        return Err(AggregateCompileIssue::EntryInterface);
    }
    let mut assembly = String::from(".text\n");
    let calls = build_call_graph(&canonical.graphs, &canonical.external_subgraphs)
        .map_err(|_| AggregateCompileIssue::InvalidProgram)?;
    let reachable = reachable_program_graphs(&calls, &canonical.graphs, &entry.name);
    for (index, graph) in canonical.graphs.iter().enumerate() {
        if reachable.contains(&graph.name) {
            for port in graph.inputs.iter().chain(&graph.outputs).chain(
                graph
                    .nodes
                    .iter()
                    .flat_map(|n| n.inputs.iter().chain(&n.outputs)),
            ) {
                if !type_supported(&port.ty, &canonical.schemas, &mut BTreeSet::new(), 0) {
                    return Err(AggregateCompileIssue::Unsupported {
                        graph: graph.name.clone(),
                        node: 0,
                    });
                }
            }
            emit_graph(&mut assembly, graph, index)?;
        }
    }
    let symbol = graph_symbol(&entry.name);
    writeln!(assembly,".globl g0_compiled_entry_with_inputs\n.type g0_compiled_entry_with_inputs,@function\ng0_compiled_entry_with_inputs:\n jmp {symbol}\n.size g0_compiled_entry_with_inputs,.-g0_compiled_entry_with_inputs").unwrap();
    writeln!(assembly,".globl g0_compiled_entry\n.type g0_compiled_entry,@function\ng0_compiled_entry:\n xor %esi,%esi\n xor %edx,%edx\n jmp {symbol}\n.size g0_compiled_entry,.-g0_compiled_entry").unwrap();
    assembly.push_str(
        ".globl g0_compiled_entry_handle\n.set g0_compiled_entry_handle,g0_compiled_entry\n",
    );
    writeln!(assembly,".globl g0_compiled_context\n.type g0_compiled_context,@function\ng0_compiled_context:\n mov %rdx,%r8\n mov %rsi,%rcx\n mov %rdi,%rdx\n lea .Lg0_document(%rip),%rdi\n mov ${},%rsi\n jmp g0_native_context_new\n.size g0_compiled_context,.-g0_compiled_context",document.len()).unwrap();
    assembly.push_str(".globl g0_machine_main\n.type g0_machine_main,@function\ng0_machine_main:\n push %r12\n push %r13\n sub $8,%rsp\n mov $1000000,%rdi\n mov $67108864,%rsi\n mov $128,%rdx\n call g0_compiled_context\n test %rax,%rax\n jz .Lagg_main_trap\n mov %rax,%r12\n mov %r12,%rdi\n call g0_compiled_entry\n mov %r12,%rdi\n mov %rax,%rsi\n call g0_native_result_integer\n mov %rax,%r13\n mov %r12,%rdi\n call g0_native_failed\n test %rax,%rax\n jnz .Lagg_main_failed\n mov %r12,%rdi\n call g0_native_context_free\n mov %r13,%rax\n add $8,%rsp\n pop %r13\n pop %r12\n ret\n.Lagg_main_failed:\n mov %r12,%rdi\n call g0_native_context_free\n.Lagg_main_trap:\n ud2\n.size g0_machine_main,.-g0_machine_main\n");
    assembly.push_str(".section .rodata\n.Lg0_document:\n");
    for bytes in document.chunks(24) {
        assembly.push_str(".byte ");
        for (i, b) in bytes.iter().enumerate() {
            if i > 0 {
                assembly.push(',')
            }
            write!(assembly, "{b}").unwrap()
        }
        assembly.push('\n')
    }
    assembly.push_str(".section .note.GNU-stack,\"\",@progbits\n");
    Ok(CompiledAggregate { assembly, document })
}

fn type_supported(
    ty: &SemanticType,
    schemas: &[crate::data_format::DataSchema],
    path: &mut BTreeSet<String>,
    depth: usize,
) -> bool {
    if depth >= 128 {
        return false;
    }
    match ty {
        SemanticType::Bool
        | SemanticType::Integer(_)
        | SemanticType::Text
        | SemanticType::Bytes => true,
        SemanticType::Array(t, _)
        | SemanticType::Vector(t, _)
        | SemanticType::Slice(t)
        | SemanticType::Option(t) => type_supported(t, schemas, path, depth + 1),
        SemanticType::Result(a, b) => {
            type_supported(a, schemas, path, depth + 1)
                && type_supported(b, schemas, path, depth + 1)
        }
        SemanticType::Record(name) | SemanticType::Variant(name) => {
            if !path.insert(name.clone()) {
                return true;
            }
            let supported = schemas.iter().find(|s| &s.name == name).is_some_and(|s| {
                s.fields
                    .iter()
                    .all(|f| type_supported(&f.ty, schemas, path, depth + 1))
            });
            path.remove(name);
            supported
        }
        _ => false,
    }
}

fn emit_graph(
    assembly: &mut String,
    graph: &Graph,
    index: usize,
) -> Result<(), AggregateCompileIssue> {
    let unsupported = |node: u32| AggregateCompileIssue::Unsupported {
        graph: graph.name.clone(),
        node,
    };
    let mut slots = BTreeMap::new();
    let mut next = 0usize;
    let mut inputs: Vec<_> = graph.inputs.iter().collect();
    inputs.sort_by_key(|p| p.id);
    for p in &inputs {
        slots.insert(SourceEndpoint::GraphInput(p.id), next);
        next += 1;
    }
    for n in &graph.nodes {
        if !n.effects.is_empty() || !n.required_capabilities.is_empty() {
            return Err(unsupported(n.id));
        }
        for output in &n.outputs {
            slots.insert(
                SourceEndpoint::NodeOutput {
                    node: n.id,
                    port: output.id,
                },
                next,
            );
            next += 1;
        }
    }
    let sources: BTreeMap<_, _> = graph
        .edges
        .iter()
        .map(|e| (e.to.clone(), e.from.clone()))
        .collect();
    let scratch = next;
    let max_args = graph
        .nodes
        .iter()
        .map(|n| n.inputs.len().max(n.outputs.len()))
        .max()
        .unwrap_or(0)
        .max(graph.outputs.len())
        .max(1);
    next += max_args;
    let local = next;
    next += 6;
    // Argument arrays grow toward higher addresses, unlike ordinary local slots.
    let offset = |slot: usize| {
        16 + if (scratch..scratch + max_args).contains(&slot) {
            (scratch + max_args - 1 - (slot - scratch)) * 8
        } else {
            slot * 8
        }
    };
    let bytes = next
        .checked_mul(8)
        .ok_or(AggregateCompileIssue::SizeOverflow)?;
    let frame = (bytes + 8).div_ceil(16) * 16 - 8;
    if frame > 1024 * 1024 {
        return Err(AggregateCompileIssue::SizeOverflow);
    }
    let symbol = graph_symbol(&graph.name);
    let prefix = format!(".Lagg_{index}");
    writeln!(assembly,".type {symbol},@function\n{symbol}:\n push %rbp\n mov %rsp,%rbp\n push %r12\n sub ${frame},%rsp\n mov %rdi,%r12").unwrap();
    writeln!(assembly," mov %rsi,-{}(%rbp)\n mov %rdx,%rcx\n mov %rsi,%rdx\n mov %r12,%rdi\n mov ${index},%rsi\n call g0_native_graph_enter\n test %rax,%rax\n jz {prefix}_fail_unentered\n mov -{}(%rbp),%rsi",offset(local+4),offset(local+4)).unwrap();
    for (i, p) in inputs.iter().enumerate() {
        writeln!(
            assembly,
            " mov {}(%rsi),%rax\n mov %rax,-{}(%rbp)",
            i * 8,
            offset(slots[&SourceEndpoint::GraphInput(p.id)])
        )
        .unwrap()
    }
    let mut done = BTreeSet::new();
    let mut pending: BTreeSet<_> = graph.nodes.iter().map(|n| n.id).collect();
    while !pending.is_empty() {
        let id = *pending
            .iter()
            .find(|id| {
                graph
                    .edges
                    .iter()
                    .filter(|e| matches!(e.to,TargetEndpoint::NodeInput{node,..} if node==**id))
                    .all(|e| match e.from {
                        SourceEndpoint::NodeOutput { node, .. } => done.contains(&node),
                        _ => true,
                    })
            })
            .ok_or(AggregateCompileIssue::InvalidTopology)?;
        let node_index = graph.nodes.iter().position(|n| n.id == id).unwrap();
        let node = &graph.nodes[node_index];
        let mut ports: Vec<_> = node.inputs.iter().collect();
        ports.sort_by_key(|p| p.id);
        for (i, p) in ports.iter().enumerate() {
            let source = sources
                .get(&TargetEndpoint::NodeInput {
                    node: id,
                    port: p.id,
                })
                .ok_or(AggregateCompileIssue::InvalidTopology)?;
            writeln!(
                assembly,
                " mov -{}(%rbp),%rax\n mov %rax,-{}(%rbp)",
                offset(slots[source]),
                offset(scratch + i)
            )
            .unwrap()
        }
        let args = |a: &mut String, start: usize, count: usize| {
            writeln!(
                a,
                " mov %r12,%rdi\n lea -{}(%rbp),%rsi\n mov ${count},%rdx",
                offset(scratch + start)
            )
            .unwrap()
        };
        let label = format!("{prefix}_n{id}");
        if matches!(
            node.operation,
            Operation::Subgraph(_)
                | Operation::Select { .. }
                | Operation::Loop { .. }
                | Operation::Map { .. }
                | Operation::Match { .. }
        ) {
            writeln!(assembly," mov %r12,%rdi\n mov ${index},%rsi\n mov ${node_index},%rdx\n lea -{}(%rbp),%rcx\n mov ${},%r8\n call g0_native_control_begin\n test %rax,%rax\n jz {prefix}_fail",offset(scratch),ports.len()).unwrap();
        }
        match &node.operation {
            Operation::Subgraph(target) => {
                args(assembly, 0, ports.len());
                writeln!(assembly, " call {}", graph_symbol(target)).unwrap()
            }
            Operation::Select {
                when_true,
                when_false,
            } => {
                writeln!(assembly," mov %r12,%rdi\n mov -{}(%rbp),%rsi\n call g0_native_truth\n test %rax,%rax\n jz {label}_false",offset(scratch)).unwrap();
                args(assembly, 1, ports.len() - 1);
                writeln!(
                    assembly,
                    " call {}\n jmp {label}_selected\n{label}_false:",
                    graph_symbol(when_true)
                )
                .unwrap();
                args(assembly, 1, ports.len() - 1);
                writeln!(
                    assembly,
                    " call {}\n{label}_selected:",
                    graph_symbol(when_false)
                )
                .unwrap();
            }
            Operation::Loop {
                condition,
                body,
                max_iterations,
            } => {
                writeln!(assembly, " movq $0,-{}(%rbp)\n{label}_head:\n mov %r12,%rdi\n call g0_native_tick\n test %rax,%rax\n jz {prefix}_fail", offset(local)).unwrap();
                args(assembly, 0, ports.len());
                writeln!(assembly," call {}\n test %rax,%rax\n jz {prefix}_fail\n mov %r12,%rdi\n mov %rax,%rsi\n call g0_native_truth\n test %rax,%rax\n jz {label}_done\n movabs ${max_iterations},%rax\n cmp %rax,-{}(%rbp)\n jae {label}_bound",graph_symbol(condition),offset(local)).unwrap();
                args(assembly, 0, ports.len());
                writeln!(
                    assembly,
                    " call {}\n test %rax,%rax\n jz {prefix}_fail",
                    graph_symbol(body)
                )
                .unwrap();
                if ports.len() == 1 {
                    writeln!(assembly, " mov %rax,-{}(%rbp)", offset(scratch)).unwrap()
                } else {
                    writeln!(assembly, " mov %rax,-{}(%rbp)", offset(local + 4)).unwrap();
                    for i in 0..ports.len() {
                        writeln!(assembly," mov %r12,%rdi\n mov -{}(%rbp),%rsi\n mov ${i},%rdx\n call g0_native_pack_item\n test %rax,%rax\n jz {prefix}_fail\n mov %rax,-{}(%rbp)",offset(local+4),offset(scratch+i)).unwrap()
                    }
                }
                writeln!(assembly," incq -{}(%rbp)\n jmp {label}_head\n{label}_bound:\n mov %r12,%rdi\n mov ${index},%rsi\n mov ${node_index},%rdx\n call g0_native_loop_fail\n jmp {prefix}_fail\n{label}_done:",offset(local)).unwrap();
                if ports.len() == 1 {
                    writeln!(assembly, " mov -{}(%rbp),%rax", offset(scratch)).unwrap()
                } else {
                    args(assembly, 0, ports.len());
                    assembly.push_str(" call g0_native_pack\n")
                }
            }
            Operation::Match { arms, default } => {
                writeln!(assembly," mov %r12,%rdi\n mov ${index},%rsi\n mov ${node_index},%rdx\n mov -{}(%rbp),%rcx\n call g0_native_match_arm",offset(scratch)).unwrap();
                for i in 0..arms.len() {
                    writeln!(assembly, " cmp ${i},%rax\n je {label}_arm{i}").unwrap()
                }
                args(assembly, 1, ports.len() - 1);
                writeln!(
                    assembly,
                    " call {}\n jmp {label}_matched",
                    graph_symbol(default)
                )
                .unwrap();
                for (i, arm) in arms.iter().enumerate() {
                    writeln!(assembly, "{label}_arm{i}:").unwrap();
                    args(assembly, 1, ports.len() - 1);
                    writeln!(
                        assembly,
                        " call {}\n jmp {label}_matched",
                        graph_symbol(&arm.graph)
                    )
                    .unwrap()
                }
                writeln!(assembly, "{label}_matched:").unwrap();
            }
            Operation::Map { body } => {
                writeln!(assembly," mov %r12,%rdi\n mov -{}(%rbp),%rsi\n call g0_native_sequence_len\n mov %rax,-{}(%rbp)\n mov %r12,%rdi\n mov %rax,%rsi\n call g0_native_map_begin\n test %rax,%rax\n jz {prefix}_fail\n mov %rax,-{}(%rbp)\n movq $0,-{}(%rbp)\n{label}_head:\n mov -{}(%rbp),%rax\n cmp -{}(%rbp),%rax\n jae {label}_done\n mov %r12,%rdi\n mov -{}(%rbp),%rsi\n mov %rax,%rdx\n call g0_native_sequence_item\n test %rax,%rax\n jz {prefix}_fail\n mov %rax,-{}(%rbp)",offset(scratch),offset(local),offset(local+1),offset(local+2),offset(local+2),offset(local),offset(scratch),offset(local+3)).unwrap();
                writeln!(assembly," mov %r12,%rdi\n lea -{}(%rbp),%rsi\n mov $1,%rdx\n call {}\n test %rax,%rax\n jz {prefix}_fail\n mov %r12,%rdi\n mov -{}(%rbp),%rsi\n mov %rax,%rdx\n call g0_native_map_push\n test %rax,%rax\n jz {prefix}_fail\n incq -{}(%rbp)\n jmp {label}_head\n{label}_done:\n mov %r12,%rdi\n mov ${index},%rsi\n mov ${node_index},%rdx\n mov -{}(%rbp),%rcx\n call g0_native_map_finish",offset(local+3),graph_symbol(body),offset(local+1),offset(local+2),offset(local+1)).unwrap();
            }
            Operation::Const(_)
            | Operation::Add
            | Operation::Sub
            | Operation::Mul
            | Operation::Div
            | Operation::Rem
            | Operation::Eq
            | Operation::Lt
            | Operation::Le
            | Operation::Gt
            | Operation::Ge
            | Operation::And
            | Operation::Or
            | Operation::Xor
            | Operation::Not
            | Operation::ConvertChecked
            | Operation::Truncate { .. }
            | Operation::MakeArray
            | Operation::Index
            | Operation::Length
            | Operation::TextConcat
            | Operation::BytesConcat
            | Operation::BytesSlice
            | Operation::ArrayConcat
            | Operation::Range
            | Operation::BytesFromArray
            | Operation::EncodeUtf8
            | Operation::DecodeUtf8
            | Operation::FormatInteger
            | Operation::TextJoin
            | Operation::MakeRecord { .. }
            | Operation::Field { .. }
            | Operation::MakeVariant { .. }
            | Operation::VariantPayload { .. }
            | Operation::Some
            | Operation::None
            | Operation::Ok
            | Operation::Err
            | Operation::UnwrapOr => {
                if node.outputs.len() != 1 {
                    return Err(unsupported(id));
                }
                writeln!(assembly," mov %r12,%rdi\n mov ${index},%rsi\n mov ${node_index},%rdx\n lea -{}(%rbp),%rcx\n mov ${},%r8\n call g0_native_primitive",offset(scratch),ports.len()).unwrap();
            }
            _ => return Err(unsupported(id)),
        }
        writeln!(assembly, " test %rax,%rax\n jz {prefix}_fail").unwrap();
        if node.outputs.len() == 1 {
            writeln!(
                assembly,
                " mov %rax,-{}(%rbp)",
                offset(
                    slots[&SourceEndpoint::NodeOutput {
                        node: id,
                        port: node.outputs[0].id
                    }]
                )
            )
            .unwrap()
        } else {
            writeln!(assembly," mov %r12,%rdi\n mov ${index},%rsi\n mov ${node_index},%rdx\n mov %rax,%rcx\n call g0_native_pack_check\n test %rax,%rax\n jz {prefix}_fail\n mov %rax,-{}(%rbp)",offset(local+4)).unwrap();
            let mut outputs: Vec<_> = node.outputs.iter().collect();
            outputs.sort_by_key(|p| p.id);
            for (i, p) in outputs.iter().enumerate() {
                writeln!(assembly," mov %r12,%rdi\n mov -{}(%rbp),%rsi\n mov ${i},%rdx\n call g0_native_pack_item\n test %rax,%rax\n jz {prefix}_fail\n mov %rax,-{}(%rbp)",offset(local+4),offset(slots[&SourceEndpoint::NodeOutput{node:id,port:p.id}])).unwrap()
            }
        }
        writeln!(
            assembly,
            " mov %r12,%rdi\n call g0_native_failed\n test %rax,%rax\n jnz {prefix}_fail"
        )
        .unwrap();
        done.insert(id);
        pending.remove(&id);
    }
    let mut outputs: Vec<_> = graph.outputs.iter().collect();
    outputs.sort_by_key(|p| p.id);
    for (i, p) in outputs.iter().enumerate() {
        let source = sources
            .get(&TargetEndpoint::GraphOutput(p.id))
            .ok_or(AggregateCompileIssue::InvalidTopology)?;
        writeln!(
            assembly,
            " mov -{}(%rbp),%rax\n mov %rax,-{}(%rbp)",
            offset(slots[source]),
            offset(scratch + i)
        )
        .unwrap()
    }
    if outputs.len() == 1 {
        writeln!(
            assembly,
            " mov -{}(%rbp),%rdx\n mov %r12,%rdi\n mov ${index},%rsi\n call g0_native_graph_result",
            offset(scratch)
        )
        .unwrap()
    } else {
        writeln!(assembly," mov %r12,%rdi\n lea -{}(%rbp),%rsi\n mov ${},%rdx\n call g0_native_pack\n mov %r12,%rdi\n mov ${index},%rsi\n movabs $18446744073709551615,%rdx\n mov %rax,%rcx\n call g0_native_pack_check",offset(scratch),outputs.len()).unwrap();
    }
    writeln!(assembly," test %rax,%rax\n jz {prefix}_fail\n mov %rax,-{}(%rbp)\n mov %r12,%rdi\n call g0_native_leave\n mov -{}(%rbp),%rax\n jmp {prefix}_return\n{prefix}_fail:\n mov %r12,%rdi\n call g0_native_leave\n{prefix}_fail_unentered:\n xor %eax,%eax\n{prefix}_return:\n add ${frame},%rsp\n pop %r12\n pop %rbp\n ret\n.size {symbol},.-{symbol}",offset(local+5),offset(local+5)).unwrap();
    Ok(())
}
