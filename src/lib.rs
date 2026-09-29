pub mod artifact;
pub mod authority;
pub mod backend;
pub mod concurrency;
pub mod crypto;
pub mod effects;
pub mod freshness;
pub mod gir;
pub mod gir_validate;
pub mod graph;
pub mod information_flow;
pub mod io;
pub mod math;
pub mod memory;
pub mod network;
pub mod optimize;
pub mod ownership;
pub mod parser;
pub mod program;
pub mod profiling;
pub mod security;
pub mod storage;
pub mod symbol;
pub mod text;
pub mod tuning;
pub mod validate;

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
