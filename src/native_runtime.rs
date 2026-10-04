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
    execute_embedded_with_limits(
        program_bytes,
        input,
        crate::execution::ExecutionLimits::default(),
    )
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NativeLimits {
    pub max_steps: u64,
    pub max_value_bytes: u64,
    pub max_call_depth: u64,
}
impl NativeLimits {
    pub fn execution_limits(self) -> Result<crate::execution::ExecutionLimits, NativeRuntimeError> {
        if self.max_steps == 0
            || self.max_steps > 64_000_000
            || self.max_value_bytes == 0
            || self.max_value_bytes > 32 * 1024 * 1024 * 1024
            || !(1..=128).contains(&self.max_call_depth)
        {
            return Err(NativeRuntimeError::TooLarge);
        }
        Ok(crate::execution::ExecutionLimits {
            max_steps: self.max_steps,
            max_value_bytes: self.max_value_bytes,
            max_call_depth: self.max_call_depth as usize,
        })
    }
}
pub fn execute_embedded_with_limits(
    program_bytes: &[u8],
    input: &[u8],
    limits: crate::execution::ExecutionLimits,
) -> Result<Vec<Value>, NativeRuntimeError> {
    if program_bytes.len() > crate::bootstrap_compiler::MAX_SOURCE_BYTES
        || input.len() > 8 * 1024 * 1024
    {
        return Err(NativeRuntimeError::TooLarge);
    }
    if !input.is_empty()
        && (input.len() as u64)
            .checked_add(std::mem::size_of::<Value>() as u64)
            .is_none_or(|bytes| bytes > limits.max_value_bytes)
    {
        return Err(NativeRuntimeError::Runtime(RuntimeError::MemoryLimit));
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
            decode_value(
                input,
                &p.ty,
                &program.schemas,
                CodecLimits {
                    max_bytes: usize::try_from(limits.max_value_bytes).unwrap_or(usize::MAX),
                    ..Default::default()
                },
            )
            .map_err(|_| NativeRuntimeError::Input)?,
        ],
        _ => return Err(NativeRuntimeError::EntryInterface),
    };
    let mut host = crate::resource_host::ResourceHost::new(
        std::sync::Arc::new(program),
        limits,
        Default::default(),
    )
    .map_err(NativeRuntimeError::Runtime)?;
    host.run_graph(&document.entry_graph, args)
        .map_err(NativeRuntimeError::Runtime)
}

pub struct NativeResult {
    pub(crate) output: Result<Vec<Value>, NativeRuntimeError>,
}

/// Bounded diagnostic codes: 0 success, 1 steps, 2 memory, 3 call depth,
/// 4 cancellation, 5 input/interface/host limits, 6 program/runtime failure,
/// 7 null handle. This exposes no graph names, values or protected data.
///
/// # Safety
/// A nonnull handle must be live and returned by this runtime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_runtime_failure_kind(result: *const NativeResult) -> i32 {
    let Some(result) = (unsafe { result.as_ref() }) else {
        return 7;
    };
    match &result.output {
        Ok(_) => 0,
        Err(NativeRuntimeError::Runtime(RuntimeError::StepLimit)) => 1,
        Err(NativeRuntimeError::Runtime(RuntimeError::MemoryLimit)) => 2,
        Err(NativeRuntimeError::Runtime(RuntimeError::CallDepth)) => 3,
        Err(NativeRuntimeError::Runtime(RuntimeError::Cancelled)) => 4,
        Err(
            NativeRuntimeError::TooLarge
            | NativeRuntimeError::EntryInterface
            | NativeRuntimeError::Input,
        ) => 5,
        Err(_) => 6,
    }
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
    unsafe {
        runtime_entry(
            program,
            program_len,
            input,
            input_len,
            crate::execution::ExecutionLimits::default(),
        )
    }
}

/// # Safety
/// Byte ranges obey g0_runtime_entry's requirements. limits is null or points
/// to an initialized NativeLimits for this call. Returned result is owned.
/// A null limits pointer returns an Input error; g0_runtime_entry uses defaults.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_runtime_entry_with_limits(
    program: *const u8,
    program_len: usize,
    input: *const u8,
    input_len: usize,
    limits: *const NativeLimits,
) -> *mut NativeResult {
    let limits = unsafe { limits.as_ref() }
        .ok_or(NativeRuntimeError::Input)
        .and_then(|limits| limits.execution_limits());
    match limits {
        Ok(limits) => unsafe { runtime_entry(program, program_len, input, input_len, limits) },
        Err(error) => Box::into_raw(Box::new(NativeResult { output: Err(error) })),
    }
}
unsafe fn runtime_entry(
    program: *const u8,
    program_len: usize,
    input: *const u8,
    input_len: usize,
    limits: crate::execution::ExecutionLimits,
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
        execute_embedded_with_limits(program, input, limits)
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
/// Writes the exact signed i128 as two unsigned two's-complement limbs, low
/// limb first. Failure leaves the caller's buffer unchanged.
/// # Safety
/// `result` must be null or live. A nonnull output must refer to writable
/// storage for at least output_len u64 values until this call returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_runtime_integer128(
    result: *const NativeResult,
    output: *mut u64,
    output_len: usize,
) -> i32 {
    if result.is_null() || output.is_null() || output_len < 2 {
        return 1;
    }
    if let Ok(values) = &unsafe { &*result }.output
        && let [Value::Integer(value)] = values.as_slice()
    {
        let bits = *value as u128;
        unsafe {
            output.write(bits as u64);
            output.add(1).write((bits >> 64) as u64);
        }
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
