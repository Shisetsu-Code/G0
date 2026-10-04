pub mod abi;
pub mod accelerator;
pub mod artifact;
pub mod auth;
pub mod audit;
pub mod authority;
pub mod backend;
pub mod benchmark;
pub mod build_lock;
pub mod call_graph;
pub mod concurrency;
pub mod compiler;
pub mod control;
pub mod crypto;
pub mod data_format;
pub mod diagnostics;
pub mod effects;
pub mod entropy;
pub mod evolution;
pub mod freshness;
pub mod filesystem;
pub mod gir;
pub mod gir_validate;
pub mod gir_optimize;
pub mod graph;
pub mod graph_binary;
pub mod graph_binary_decode;
pub mod graph_format;
pub mod graph_module;
pub mod hardware;
pub mod information_flow;
pub mod ingress;
pub mod inline;
pub mod invariants;
pub mod instrumentation;
pub mod io;
pub mod math;
pub mod math_lowering;
pub mod machine;
pub mod machine_ir;
pub mod memory;
pub mod memory_select;
pub mod mir;
pub mod mir_validate;
pub mod migration;
pub mod network;
pub mod native_program;
pub mod network_runtime;
pub mod optimize;
pub mod optimization_gate;
pub mod optimizer_feedback;
pub mod ownership;
pub mod outbox;
pub mod package;
pub mod parser;
pub mod performance_gate;
pub mod policy_plan;
pub mod program;
pub mod program_binary;
pub mod profiling;
pub mod query;
pub mod runtime;
pub mod roles;
pub mod scheduler;
pub mod secure_index;
pub mod security;
pub mod side_channel;
pub mod source_map;
pub mod storage;
pub mod storage_crypto;
pub mod storage_delete;
pub mod storage_integrity;
pub mod symbol;
pub mod task_runtime;
pub mod text;
pub mod time;
pub mod tuning;
pub mod transaction;
pub mod validate;
pub mod vectorize;
pub mod x86_codegen;

use graph::Graph;

#[derive(Debug)]
pub enum CompileError {
    Parse(parser::ParseError),
    Validate(validate::ValidationError),
    Io(std::io::Error),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "{e}"),
            Self::Validate(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CompileError {}

impl From<parser::ParseError> for CompileError {
    fn from(value: parser::ParseError) -> Self {
        Self::Parse(value)
    }
}

impl From<validate::ValidationError> for CompileError {
    fn from(value: validate::ValidationError) -> Self {
        Self::Validate(value)
    }
}

impl From<std::io::Error> for CompileError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn compile_source(source: &str) -> Result<String, CompileError> {
    let mut graph: Graph = parser::parse(source)?;
    validate::validate(&graph)?;
    optimize::constant_fold(&mut graph);
    validate::validate(&graph)?;
    Ok(backend::x86_64::emit(&graph))
}
