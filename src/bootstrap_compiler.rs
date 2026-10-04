//! Bootstrap native-wrapper backend. The transformation is a G0 program;
//! this module describes its graph and supplies validation and bounded hosting.
use crate::{
    execution::{ExecutionLimits, Executor, RuntimeError},
    gir::*,
    program_binary::*,
    value::Value,
};
use std::collections::BTreeSet;
#[path = "bootstrap_compiler_parser.rs"]
mod parser;

pub const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;
/// Compiler callbacks run on a reserved, bounded stack instead of inheriting
/// the host's main-thread stack (two MiB on Windows).
pub const HOST_STACK_BYTES: usize = 16 * 1024 * 1024;
pub const COMPILER_SOURCE: &[u8] = include_bytes!("../compiler/native-wrapper.g0p");


pub fn compile_native(source: &[u8]) -> Result<String, BootstrapError> {
    compile_with(COMPILER_SOURCE, source)
}

/// Executes the GIR native emitter for closed, pure programs.
pub fn compile_direct_native(source: &[u8]) -> Result<String, BootstrapError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(BootstrapError::TooLarge);
    }
    let source = source.to_vec();
    run_host(move || compile_direct_inner(&source))
}

fn compile_direct_inner(source: &[u8]) -> Result<String, BootstrapError> {
    decode_program(source).map_err(BootstrapError::Document)?;
    let document = compiler_document();
    let contract = document
        .validated_contract()
        .map_err(BootstrapError::Document)?;
    let mut executor =
        Executor::new(&contract, compiler_limits()).map_err(BootstrapError::Runtime)?;
    let input = Value::Bytes(source.into());
    let output = executor
        .run_graph("compile-direct", vec![input])
        .map_err(BootstrapError::Runtime)?;
    match output.as_slice() {
        [Value::Text(text)] if !text.starts_with("G0 compiler:") => {
            Ok(text.to_string())
        }
        _ => Err(BootstrapError::Output),
    }
}

#[derive(Debug)]
pub enum BootstrapError {
    TooLarge,
    Document(ProgramBinaryIssue),
    Runtime(RuntimeError),
    Output,
    HostSpawn(std::io::Error),
    HostPanic,
}
impl std::fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for BootstrapError {}

pub fn compile_with(compiler: &[u8], source: &[u8]) -> Result<String, BootstrapError> {
    if source.len() > MAX_SOURCE_BYTES || compiler.len() > MAX_SOURCE_BYTES {
        return Err(BootstrapError::TooLarge);
    }
    let compiler = compiler.to_vec();
    let source = source.to_vec();
    run_host(move || compile_inner(&compiler, &source))
}

fn run_host(
    task: impl FnOnce() -> Result<String, BootstrapError> + Send + 'static,
) -> Result<String, BootstrapError> {
    std::thread::Builder::new()
        .name("g0-compiler-host".into())
        .stack_size(HOST_STACK_BYTES)
        .spawn(task)
        .map_err(BootstrapError::HostSpawn)?
        .join()
        .map_err(|_| BootstrapError::HostPanic)?
}

fn compile_inner(compiler: &[u8], source: &[u8]) -> Result<String, BootstrapError> {
    decode_program(source).map_err(BootstrapError::Document)?;
    let document = decode_program(compiler).map_err(BootstrapError::Document)?;
    let program = document
        .validated_contract()
        .map_err(BootstrapError::Document)?;
    let mut executor =
        Executor::new(&program, compiler_limits()).map_err(BootstrapError::Runtime)?;
    let output = executor
        .run_graph(&document.entry_graph, vec![Value::Bytes(source.into())])
        .map_err(BootstrapError::Runtime)?;
    match output.as_slice() {
        [Value::Text(text)] if !text.starts_with("G0 compiler:") => {
            Ok(text.to_string())
        }
        _ => Err(BootstrapError::Output),
    }
}

pub fn compiler_limits() -> ExecutionLimits {
    ExecutionLimits {
        max_steps: 32_000_000,
        // This is cumulative logical allocation, including graph metadata on
        // every call and shared byte values forwarded through reader loops.
        max_value_bytes: 32 * 1024 * 1024 * 1024,
        max_call_depth: 128,
    }
}

struct Builder {
    graph: Graph,
}
impl Builder {
    fn new(name: &str, ty: SemanticType) -> Self {
        let mut graph = Graph::new(name);
        graph.inputs = vec![port(0, ty)];
        graph.outputs = vec![port(0, SemanticType::Text)];
        Self { graph }
    }
    fn op(
        &mut self,
        operation: Operation,
        args: Vec<(SourceEndpoint, SemanticType)>,
        output: SemanticType,
    ) -> SourceEndpoint {
        let id = self.graph.nodes.len() as u32 + 1;
        let inputs = args
            .iter()
            .enumerate()
            .map(|(id, (_, ty))| port(id as u16, ty.clone()))
            .collect();
        self.graph.nodes.push(Node {
            id,
            operation,
            inputs,
            outputs: vec![port(0, output)],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });
        for (index, (from, _)) in args.into_iter().enumerate() {
            self.graph.edges.push(Edge {
                from,
                to: TargetEndpoint::NodeInput {
                    node: id,
                    port: index as u16,
                },
            });
        }
        SourceEndpoint::NodeOutput { node: id, port: 0 }
    }
    fn text(&mut self, value: &str) -> SourceEndpoint {
        self.op(
            Operation::Const(Literal::Text(value.into())),
            vec![],
            SemanticType::Text,
        )
    }
    fn concat(&mut self, a: SourceEndpoint, b: SourceEndpoint) -> SourceEndpoint {
        self.op(
            Operation::TextConcat,
            vec![(a, SemanticType::Text), (b, SemanticType::Text)],
            SemanticType::Text,
        )
    }
    fn finish(mut self, source: SourceEndpoint) -> Graph {
        self.graph.edges.push(Edge {
            from: source,
            to: TargetEndpoint::GraphOutput(0),
        });
        self.graph
    }
}
fn port(id: u16, ty: SemanticType) -> Port {
    Port {
        id,
        name: format!("p{id}"),
        ty,
    }
}

/// Regenerates the canonical source graph. Compilation algorithms are GIR
/// operations, not Rust transformations of the input document.
pub fn compiler_document() -> ProgramDocument {
    let mut byte = Builder::new(
        "emit-byte",
        SemanticType::Integer(IntegerType { min: 0, max: 255 }),
    );
    let number = byte.op(
        Operation::FormatInteger,
        vec![(
            SourceEndpoint::GraphInput(0),
            SemanticType::Integer(IntegerType { min: 0, max: 255 }),
        )],
        SemanticType::Text,
    );
    let byte = byte.finish(number);
    let mut main = Builder::new("emit-wrapper", SemanticType::Bytes);
    let length_type = SemanticType::Integer(IntegerType {
        min: 0,
        max: u64::MAX as i128,
    });
    let length = main.op(
        Operation::Length,
        vec![(SourceEndpoint::GraphInput(0), SemanticType::Bytes)],
        length_type.clone(),
    );
    let number = main.op(
        Operation::FormatInteger,
        vec![(length, length_type)],
        SemanticType::Text,
    );
    let prefix=main.text(".text\n.globl g0_compiled_entry\n.type g0_compiled_entry, @function\ng0_compiled_entry:\n    movq %rsi, %rcx\n    movq %rdi, %rdx\n    leaq .Lg0_program(%rip), %rdi\n    movq $");
    let suffix=main.text(", %rsi\n    jmp g0_runtime_entry\n.size g0_compiled_entry, .-g0_compiled_entry\n.globl g0_compiled_entry_with_limits\n.type g0_compiled_entry_with_limits, @function\ng0_compiled_entry_with_limits:\n    movq %rdx, %r8\n    movq %rsi, %rcx\n    movq %rdi, %rdx\n    leaq .Lg0_program(%rip), %rdi\n    movq $");
    let header = main.concat(prefix, number.clone());
    let header = main.concat(header, suffix);
    let header = main.concat(header, number);
    let suffix=main.text(", %rsi\n    jmp g0_runtime_entry_with_limits\n.size g0_compiled_entry_with_limits, .-g0_compiled_entry_with_limits\n.section .rodata\n.Lg0_program:\n.byte ");
    let header = main.concat(header, suffix);
    let lines = main.op(
        Operation::Subgraph("emit-program-byte-values".into()),
        vec![(SourceEndpoint::GraphInput(0), SemanticType::Bytes)],
        SemanticType::Text,
    );
    let assembly = main.concat(header, lines);
    let footer = main.text("\n.section .note.GNU-stack,\"\",@progbits\n");
    let assembly = main.concat(assembly, footer);
    let mut entry = Builder::new("compile-wrapper", SemanticType::Bytes);
    let valid = entry.op(
        Operation::Subgraph("reader-container-syntax".into()),
        vec![(SourceEndpoint::GraphInput(0), SemanticType::Bytes)],
        SemanticType::Bool,
    );
    let result = entry.op(
        Operation::Select {
            when_true: "emit-wrapper".into(),
            when_false: "invalid-container".into(),
        },
        vec![
            (valid, SemanticType::Bool),
            (SourceEndpoint::GraphInput(0), SemanticType::Bytes),
        ],
        SemanticType::Text,
    );
    entry.graph.nodes.last_mut().unwrap().inputs[1].name = "p0".into();
    entry.graph.nodes.last_mut().unwrap().inputs[0].name = "selector".into();
    let mut invalid = Builder::new("invalid-container", SemanticType::Bytes);
    let message = invalid.text("G0 compiler: invalid container");
    let mut direct=Builder::new("compile-direct",SemanticType::Bytes);
    let valid=direct.op(Operation::Subgraph("reader-container".into()),vec![(SourceEndpoint::GraphInput(0),SemanticType::Bytes)],SemanticType::Bool);
    let native=direct.op(Operation::Select{when_true:"compile-direct-validated".into(),when_false:"invalid-container".into()},vec![(valid,SemanticType::Bool),(SourceEndpoint::GraphInput(0),SemanticType::Bytes)],SemanticType::Text);
    let node=direct.graph.nodes.last_mut().unwrap();node.inputs[0].name="selector".into();node.inputs[1].name="p0".into();
    let mut checked=Builder::new("compile-direct-validated",SemanticType::Bytes);
    let domain=checked.op(Operation::Subgraph("control-domain".into()),vec![(SourceEndpoint::GraphInput(0),SemanticType::Bytes)],SemanticType::Bool);
    let names=checked.op(Operation::Subgraph("validator-program-names".into()),vec![(SourceEndpoint::GraphInput(0),SemanticType::Bytes)],SemanticType::Bool);
    let domain=checked.op(Operation::And,vec![(domain,SemanticType::Bool),(names,SemanticType::Bool)],SemanticType::Bool);
    let references=checked.op(Operation::Subgraph("validator-program-references".into()),vec![(SourceEndpoint::GraphInput(0),SemanticType::Bytes)],SemanticType::Bool);
    let domain=checked.op(Operation::And,vec![(domain,SemanticType::Bool),(references,SemanticType::Bool)],SemanticType::Bool);
    let acyclic=checked.op(Operation::Subgraph("validator-program-callcycles-fast".into()),vec![(SourceEndpoint::GraphInput(0),SemanticType::Bytes)],SemanticType::Bool);
    let domain=checked.op(Operation::And,vec![(domain,SemanticType::Bool),(acyclic,SemanticType::Bool)],SemanticType::Bool);
    let schemas=checked.op(Operation::Subgraph("validator-program-schemas".into()),vec![(SourceEndpoint::GraphInput(0),SemanticType::Bytes)],SemanticType::Bool);
    let domain=checked.op(Operation::And,vec![(domain,SemanticType::Bool),(schemas,SemanticType::Bool)],SemanticType::Bool);
    let schema_types=checked.op(Operation::Subgraph("validator-program-schema-types".into()),vec![(SourceEndpoint::GraphInput(0),SemanticType::Bytes)],SemanticType::Bool);
    let domain=checked.op(Operation::And,vec![(domain,SemanticType::Bool),(schema_types,SemanticType::Bool)],SemanticType::Bool);
    let port_types=checked.op(Operation::Subgraph("validator-program-port-types".into()),vec![(SourceEndpoint::GraphInput(0),SemanticType::Bytes)],SemanticType::Bool);
    let domain=checked.op(Operation::And,vec![(domain,SemanticType::Bool),(port_types,SemanticType::Bool)],SemanticType::Bool);
    let emitted=checked.op(Operation::Select{when_true:"control-compile".into(),when_false:"invalid-native-domain".into()},vec![(domain,SemanticType::Bool),(SourceEndpoint::GraphInput(0),SemanticType::Bytes)],SemanticType::Text);
    let node=checked.graph.nodes.last_mut().unwrap();node.inputs[0].name="selector".into();node.inputs[1].name="p0".into();
    let mut unsupported=Builder::new("invalid-native-domain",SemanticType::Bytes);let error=unsupported.text("G0 compiler: unsupported native profile");
    let mut legacy=Builder::new("compile",SemanticType::Bytes);let wrapper=legacy.op(Operation::Subgraph("compile-wrapper".into()),vec![(SourceEndpoint::GraphInput(0),SemanticType::Bytes)],SemanticType::Text);
    let mut graphs = vec![
        entry.finish(result),
        main.finish(assembly),
        byte,
        invalid.finish(message),
        direct.finish(native),
        checked.finish(emitted),
        unsupported.finish(error),
        legacy.finish(wrapper),
    ];
    graphs.extend(parser::graphs());
    ProgramDocument {
        entry_graph: "compile".into(),
        graphs,
        schemas: vec![],
    }
}
