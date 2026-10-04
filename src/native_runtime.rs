//! Explicit bounded ABI for generated native wrappers. No ambient capabilities.
use crate::{
    execution::RuntimeError,
    gir::SemanticType,
    program_binary::{ProgramBinaryIssue, decode_program},
    value::Value,
    value_codec::{CodecLimits, decode_value},
};

#[derive(Debug)]
pub enum NativeRuntimeError {
    TooLarge,
    Document(ProgramBinaryIssue),
    EntryInterface,
    Input,
    Runtime(RuntimeError),
}

pub fn execute_embedded(
    program_bytes: &[u8],
    input: &[u8],
) -> Result<Vec<Value>, NativeRuntimeError> {
    if program_bytes.len() > crate::bootstrap_compiler::MAX_SOURCE_BYTES
        || input.len() > 8 * 1024 * 1024
    {
        return Err(NativeRuntimeError::TooLarge);
    }
    let document = decode_program(program_bytes).map_err(NativeRuntimeError::Document)?;
    let program = document
        .validated_contract()
        .map_err(NativeRuntimeError::Document)?;
    let entry = program
        .graphs
        .iter()
        .find(|g| g.name == document.entry_graph)
        .expect("validated entry");
    let args = match entry.inputs.as_slice() {
        [] if input.is_empty() => vec![],
        [p] if p.ty == SemanticType::Bytes => vec![Value::Bytes(input.into())],
        [p] => vec![
            decode_value(input, &p.ty, &program.schemas, CodecLimits::default())
                .map_err(|_| NativeRuntimeError::Input)?,
        ],
        _ => return Err(NativeRuntimeError::EntryInterface),
    };
    let mut host = crate::resource_host::ResourceHost::new(
        std::sync::Arc::new(program),
        crate::bootstrap_compiler::compiler_limits(),
        Default::default(),
    )
    .map_err(NativeRuntimeError::Runtime)?;
    host.run_graph(&document.entry_graph, args)
        .map_err(NativeRuntimeError::Runtime)
}

pub struct NativeResult {
    output: Result<Vec<Value>, NativeRuntimeError>,
}

/// # Safety
/// Each nonempty byte range must refer to readable memory of the supplied length
/// until this call returns. Returned handles must be freed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_runtime_entry(
    program: *const u8,
    program_len: usize,
    input: *const u8,
    input_len: usize,
) -> *mut NativeResult {
    let output = if program_len > crate::bootstrap_compiler::MAX_SOURCE_BYTES
        || input_len > 8 * 1024 * 1024
    {
        Err(NativeRuntimeError::TooLarge)
    } else if (program.is_null() && program_len != 0) || (input.is_null() && input_len != 0) {
        Err(NativeRuntimeError::Input)
    } else {
        let program = if program_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(program, program_len) }
        };
        let input = if input_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(input, input_len) }
        };
        execute_embedded(program, input)
    };
    Box::into_raw(Box::new(NativeResult { output }))
}
/// # Safety
/// `result` must be null or a live handle returned by `g0_runtime_entry`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_runtime_status(result: *const NativeResult) -> i32 {
    if result.is_null() {
        return 1;
    }
    i32::from(unsafe { &*result }.output.is_err())
}
/// # Safety
/// `result` must be a live handle and `length` writable. The returned view
/// remains valid only until the handle is freed. It grants no ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_runtime_bytes(
    result: *const NativeResult,
    length: *mut usize,
) -> *const u8 {
    if result.is_null() || length.is_null() {
        return std::ptr::null();
    }
    unsafe { *length = 0 };
    let Ok(values) = &unsafe { &*result }.output else {
        return std::ptr::null();
    };
    let bytes = match values.as_slice() {
        [Value::Text(text)] => text.as_bytes(),
        [Value::Bytes(bytes)] => bytes.as_ref(),
        _ => return std::ptr::null(),
    };
    unsafe { *length = bytes.len() };
    bytes.as_ptr()
}
/// # Safety
/// `result` must be a live handle and `output` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_runtime_integer(result: *const NativeResult, output: *mut i64) -> i32 {
    if result.is_null() || output.is_null() {
        return 1;
    }
    if let Ok(values) = &unsafe { &*result }.output
        && let [Value::Integer(value)] = values.as_slice()
        && let Ok(value) = i64::try_from(*value)
    {
        unsafe { *output = value };
        return 0;
    }
    1
}
/// # Safety
/// `result` must be null or a live handle, and must never be reused after freeing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_runtime_free(result: *mut NativeResult) {
    if !result.is_null() {
        drop(unsafe { Box::from_raw(result) });
    }
}
