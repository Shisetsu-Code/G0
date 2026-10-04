# Compiler bootstrap

`compiler/native-wrapper.g0p` is an executable G0 compiler backend. Its entry
accepts `Bytes` containing a validated native G0 program and returns assembly as
`Text`. All byte formatting, size calculation, ordered mapping, joining and
assembly composition are G0 graph operations. `examples/bootstrap_compiler.rs`
regenerates that canonical graph source; a test requires byte-for-byte equality
with the checked-in document.

The current output profile is System V x86-64 assembly containing the complete
program document and a native entry that invokes the linked G0 runtime. This is
an interpreter-backed native wrapper. It is not the optimizing GIR-to-MIR x86
backend rewritten in G0: validation, decoding, scheduling and primitive
operations still use the Rust bootstrap host. The graph backend can compile
its own source, but this stage does not establish full compiler self-hosting.

```sh
cargo run --release -- bootstrap compiler/native-wrapper.g0p -o target/compiler-stage1.s
cargo build --release --lib
cc target/compiler-stage1.s tests/support/bootstrap_driver.c target/release/libg0.a \
  -ldl -lpthread -lm -o target/compiler-stage1
./target/compiler-stage1 compiler/native-wrapper.g0p > target/compiler-stage2.s
cmp target/compiler-stage1.s target/compiler-stage2.s
cc target/compiler-stage2.s tests/support/bootstrap_driver.c target/release/libg0.a \
  -ldl -lpthread -lm -o target/compiler-stage2
./target/compiler-stage2 compiler/native-wrapper.g0p > target/compiler-stage3.s
cmp target/compiler-stage2.s target/compiler-stage3.s
```

CI executes both linked compiler stages and compares their assembly output.
Local tests execute the same compiler through the runtime and C ABI. These
checks prove reproducibility for this output profile; they do not prove an
independent native implementation of the compiler's algorithms.

Compilation validates the source definition before passing its bytes to the
compiler graph. It never executes source effects or grants source capabilities.
The bootstrap profile accepts at most 128 KiB of source, reserves at most 512
MiB of cumulative runtime allocation and performs at most two million steps.
Malformed input and budget exhaustion are explicit errors; output is replaced
atomically only after successful compilation.

G0P 0.3 allows typed entry inputs and multiple outputs. Readers preserve the
zero-input/single-output entry restriction of versions 0.1 and 0.2. The existing
optimized native entry compiler continues to require zero inputs and one output
and explicitly rejects an incompatible interface. The wrapper runtime supports
zero inputs, one raw Bytes input, or one publicly serializable G0V typed input.

`g0_compiled_entry(input, length)` returns an opaque owned result. The exported
runtime ABI provides status, a borrowed Text/Bytes view, a checked i64 result,
and one explicit free operation. Native callers must meet the documented memory
and handle lifetime preconditions. The runtime grants no capabilities and has
no implicit file, network or process access.

Remaining self-hosting work includes implementing program decoding and semantic
validation in G0, direct native lowering for structured values, porting the
optimizing backend, and repeating the bootstrap without the Rust compiler host.
