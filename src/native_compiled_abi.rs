//! Typed entry bridge for directly compiled graphs; no GIR interpretation.
use crate::{
    execution::{ExecutionLimits, RuntimeError},
    gir::SemanticType,
    native_aggregate_runtime::NativeContext,
    native_runtime::{NativeLimits, NativeResult, NativeRuntimeError},
    program_binary::decode_program,
    value::Value,
    value_codec::{CodecLimits, decode_value},
};

pub type NativeEntry = unsafe extern "C" fn(*mut NativeContext, *const u64, u64) -> u64;

/// # Safety
/// Nonempty byte ranges are readable throughout the call. entry must be the
/// directly compiled entry for this exact canonical program's metadata indices,
/// obey the NativeContext ABI and return a live, checked value handle. limits
/// is null (default execution limits), or readable initialized NativeLimits.
/// The returned NativeResult is owned and must be freed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_invoke(
    program: *const u8,
    program_len: usize,
    input: *const u8,
    input_len: usize,
    entry: Option<NativeEntry>,
    limits: *const NativeLimits,
) -> *mut NativeResult {
    let result = (|| {
        if program_len > crate::bootstrap_compiler::MAX_SOURCE_BYTES || input_len > 8 * 1024 * 1024
        {
            return Err(NativeRuntimeError::TooLarge);
        }
        if (program_len != 0 && program.is_null()) || (input_len != 0 && input.is_null()) {
            return Err(NativeRuntimeError::Input);
        }
        let entry = entry.ok_or(NativeRuntimeError::Input)?;
        let limits = match unsafe { limits.as_ref() } {
            Some(limits) => limits.execution_limits()?,
            None => ExecutionLimits::default(),
        };
        let source = if program_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(program, program_len) }
        };
        let input = if input_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(input, input_len) }
        };
        let document = decode_program(source).map_err(NativeRuntimeError::Document)?;
        let program = document
            .validated_contract()
            .map_err(NativeRuntimeError::Document)?;
        let graph = program
            .graphs
            .iter()
            .find(|graph| Some(&graph.name) == program.entry_graph.as_ref())
            .ok_or(NativeRuntimeError::EntryInterface)?;
        if graph.outputs.len() != 1 {
            return Err(NativeRuntimeError::EntryInterface);
        }
        let output_type = graph.outputs[0].ty.clone();
        // Bound input allocation before copying raw bytes or decoding a value.
        if (input.len() as u64)
            .checked_add(std::mem::size_of::<Value>() as u64)
            .is_none_or(|bytes| bytes > limits.max_value_bytes)
        {
            return Err(NativeRuntimeError::Runtime(RuntimeError::MemoryLimit));
        }
        let args = match graph.inputs.as_slice() {
            [] if input.is_empty() => vec![],
            [port] if port.ty == SemanticType::Bytes => vec![Value::Bytes(input.into())],
            [port] => vec![
                decode_value(
                    input,
                    &port.ty,
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
        let schemas = program.schemas.clone();
        let mut context =
            NativeContext::new(program, limits).map_err(NativeRuntimeError::Runtime)?;
        let handles = args
            .into_iter()
            .map(|value| context.insert_value(value))
            .collect::<Result<Vec<_>, _>>()
            .map_err(NativeRuntimeError::Runtime)?;
        let handle = unsafe { entry(&mut context, handles.as_ptr(), handles.len() as u64) };
        if let Some(error) = context.error {
            return Err(NativeRuntimeError::Runtime(error));
        }
        let value = context.value(handle).ok_or(NativeRuntimeError::Input)?;
        if !value.fits(&output_type, &schemas) {
            return Err(NativeRuntimeError::Runtime(RuntimeError::InvalidHostResult));
        }
        Ok(vec![value.clone()])
    })();
    Box::into_raw(Box::new(NativeResult { output: result }))
}
