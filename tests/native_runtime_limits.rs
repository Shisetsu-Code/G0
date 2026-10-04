#[path = "support/program_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use g0::{
    execution::{ExecutionLimits, RuntimeError},
    native_runtime::*,
};
#[test]
fn explicit_host_limits_do_not_come_from_embedded_program() {
    let bytes = g0::program_binary::encode_program(&fixture::call_program()).unwrap();
    assert!(execute_embedded(&bytes, &[]).is_ok());
    assert!(matches!(
        execute_embedded_with_limits(
            &bytes,
            &[],
            ExecutionLimits {
                max_steps: 1,
                ..Default::default()
            }
        ),
        Err(NativeRuntimeError::Runtime(RuntimeError::StepLimit))
    ));
    unsafe {
        let limits = NativeLimits {
            max_steps: 1,
            max_value_bytes: 64 * 1024 * 1024,
            max_call_depth: 128,
        };
        let result =
            g0_runtime_entry_with_limits(bytes.as_ptr(), bytes.len(), std::ptr::null(), 0, &limits);
        assert_eq!(g0_runtime_status(result), 1);
        g0_runtime_free(result);
        let result = g0_runtime_entry_with_limits(
            bytes.as_ptr(),
            bytes.len(),
            std::ptr::null(),
            0,
            std::ptr::null(),
        );
        assert_eq!(g0_runtime_status(result), 1);
        g0_runtime_free(result);
    }
}
