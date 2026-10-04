# Compiler bootstrap

`compiler/native-wrapper.g0p` contains executable G0 compiler definitions.
`examples/bootstrap_compiler.rs` constructs those definitions and regenerates
their canonical binary document. The builder does not inspect or transform
compiler input. Tests require the checked-in source to match the generated
definition byte for byte.

The default `compile` entry accepts bytes and returns System V x86-64 assembly.
Its G0 reader checks G0P 0.3 framing, GIR magic and supported versions, UTF-8
strings, operation payload framing, type prefix trees, schemas, endpoint tags,
capability/effect tags and complete consumption of graph blobs and the container.
Separate G0 reader graphs construct node and edge offset tables. A G0 scheduler
orders dependencies before their users and detects cycles when a complete pass
makes no progress. Tests execute these GIR algorithms with raw bytes or tables
provided directly to the executor.

The default output remains an interpreter-backed native wrapper. It embeds
the document and invokes the linked G0 runtime. The Rust bootstrap host still
decodes and semantically validates source before invoking this entry. The G0
reader does not yet replace the complete semantic validator, including type
assignment, arithmetic proofs, graph linking, operation shapes and linearity.
Reproducing the wrapper compiler through multiple stages does not establish
full compiler self-hosting.

## Direct native emitter implemented in G0

`compile_direct_native` and the example's `direct` command execute a separate
G0 backend. The G0 reader builds offset tables, the G0 scheduler computes node
order, and G0 graphs resolve IDs to metadata indices and wire edge arguments
into native call frames. Emitted instructions call individual immutable value
primitives and the typed graph ABI. They do not invoke `Executor` to execute
the compiled program. Embedded bytes supply metadata for operations and type
checks.

This backend currently accepts one graph, contiguous port IDs, one output per
node and graph, and at most 32 inputs per node. It supports immutable primitives,
including arithmetic, collections and structured values. Unsupported control
programs are rejected by a G0 domain check. Multiple graphs, effects and the full
language require additional G0 lowering and validation. The separate Rust
`native_aggregate` backend has broader native control coverage; that coverage
has not yet been implemented in the G0 compiler.

```sh
cargo run --example bootstrap_compiler -- direct input.g0p > target/program.s
cargo build --lib
cc target/program.s driver.c target/debug/libg0.a -ldl -lpthread -lm -o program
```

On Linux x86-64, `tests/bootstrap_native_emitter.rs` assembles and links output
from the G0 emitter, executes a reversed-ID arithmetic graph and checks that
an exhausted host step budget causes failure. Run `cargo build --lib` before
Linux integration tests. Windows runs the reader, scheduler and emitter tests
but cannot execute System V assembly directly.

## Wrapper bootstrap stages

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

CI runs the linked stages and compares assembly. These checks establish wrapper
reproducibility. Fully native self-compilation requires the G0 backend to cover
the compiler's own control graphs and to validate source semantics independently
of the Rust bootstrap host.

## Host limits and ABI

Compiler hosting accepts at most one MiB of source or compiler document. Its
explicit budget is 16 million steps, eight GiB of cumulative logical allocation
and call depth 128. Logical accounting includes repeated graph metadata and
forwarded shared values; it is not an estimate of resident memory. The reader
slices each graph blob before parsing it to avoid repeatedly charging the entire
container through nested reader calls.

Rust compiler hosting uses a worker with an explicit 16 MiB stack and joins
that worker before returning. Spawn and panic failures become host errors.
The current compiler callback DAG has depth 38, below the shared depth limit,
but its interpreter frames exceed Windows' two MiB main-thread stack in debug
builds. The worker bounds this host resource without changing ordinary runtime
execution limits or requiring a process-wide stack setting.

Ordinary runtime entries retain their default budgets: one million steps and
64 MiB of logical allocation. They do not inherit the compiler's larger budget.
The compiler driver explicitly supplies host limits through
`g0_compiled_entry_with_limits(input, length, limits)`. `NativeLimits` contains
three `uint64_t` fields in order: steps, value bytes and call depth. These limits
confer no capabilities.

Both compiled profiles return an opaque owned `NativeResult`. Runtime exports
provide status, a borrowed Text/Bytes view, a checked i64 result and explicit
freeing. Callers must provide valid byte ranges and respect handle lifetimes.
Compilation never executes source effects or turns declarations into grants.
Invalid input, exhausted budgets and unsupported native domains produce errors;
CLI output replacement occurs only after successful compilation.
