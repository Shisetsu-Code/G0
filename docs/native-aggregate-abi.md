# Native aggregate ABI

The immutable value arena reuses handles for repeated Bool values and Integer
values from -32 through 1023. Its fixed cache contains 1058 handles (8464 bytes)
and never merges distinct scalar types. Every value production still consumes
the same cumulative logical allocation budget before reuse is considered;
interning reduces duplicate physical storage without relaxing execution quotas.

`native_aggregate::compile_program` produces Linux/System V x86-64 assembly.
Each graph has its own machine function. Machine instructions schedule nodes,
call graphs, select branches, match tags, iterate bounded loops, and map values.
Individual immutable-value primitives are linked from `libg0.a`. This path does
not call `Executor` or deserialize a program to execute it through an interpreter.
The embedded document is metadata for primitive operations and type checks.

The emitted assembly must be linked with a matching G0 static library:

```
cargo build --offline --lib
cc program.s driver.c target/debug/libg0.a -ldl -lpthread -lm -o program
```

## Entry and ownership

The C entry points are:

```
void *g0_compiled_context(uint64_t steps, uint64_t bytes, uint64_t depth);
uint64_t g0_compiled_entry_handle(void *context);
uint64_t g0_compiled_entry_with_inputs(void *context,
                                     const uint64_t *handles, uint64_t count);
uint64_t g0_native_failed(void *context);
int64_t g0_native_result_integer(void *context, uint64_t handle);
void g0_native_context_free(void *context);
```

A context owns its immutable value arena and internal call-result packs. Value
handles are one-based arena indices; zero means failure. Handles cannot be used
after freeing the context or with another context. Native graph functions take
`(context, input_handles, input_count)`; inputs are ordered by port ID. The host
must provide a live exclusive context made for the exact embedded document. Rust
hosts can use `NativeContext::insert_value` and inspect `value(handle)` and
`error`. Parameterized entries use `g0_compiled_entry_with_inputs`.

`g0_machine_main` provides the existing no-argument scalar convenience ABI. It
creates a default context, checks the final value fits `i64` (Bool gives zero or
one), frees the context, and traps on any error or incompatible output. Typed
aggregate results must use the handle entry, keeping the context alive until
the host has copied the result. A parameterized entry cannot run through the
no-argument convenience wrapper.

Internal graph calls returning exactly one port return that value handle.
Calls with zero or multiple output ports return a separate private pack ID with
its high bit set, so a pack cannot resolve as a language value handle.
`g0_native_pack`, `g0_native_pack_item`, and `g0_native_pack_check` construct,
inspect, and validate these tuples. Packs are never language Array values.
Their output order is port ID order. Multi-state Loop updates occur from a
completed pack so the new state never overwrites a still-needed old state.

## Budgets and validation

The compiler validates the complete program before emitting any assembly.
Primitive inputs, graph arguments, graph results, and result packs are checked
against their semantic types and named schemas. Node metadata indices use the
canonical encoded document order, not node IDs. Allocation failures, checked
arithmetic, invalid UTF-8 results, out-of-range slice accesses, and exceeded
loop bounds retain their runtime semantics. Negative Index produces None.

All graph entries, primitives and control nodes consume the shared step budget. Map iterations
and Loop condition tests also consume steps. Value allocations and private packs
consume a conservative cumulative resident-value budget, including pack-table
metadata for zero-output calls; frame bytes consume
that budget while a call is active. Individual frames are limited to one MiB,
active native frame accounting is limited to four MiB, and call depth is capped
at 128. Collection sizes are checked against available memory before allocation.
Graph frame sizes and graph input/output port ordinals are cached lazily from
immutable program metadata. The first eligible entry charges a bounded dense
metadata table; each used graph additionally charges its ordinal vectors once,
before allocation. Eligibility requires room for the frame and twice the new
cache footprint, leaving working space beyond the cache. Tight budgets use the
original sorting and frame calculation path. Cached metadata consumes the value
budget, so an execution that enables caching can exhaust memory earlier than
one without it. Value production charges and recursive type checks are unchanged.
`NativeContext::cancellation` gives Rust hosts a shared cancellation token. Calls
and primitive/collection iteration ticks stop when it is cancelled. An error is
sticky, preserves the first graph/node diagnostic where applicable,
and causes enclosing generated graph calls to return zero.

Arena values are immutable shared payloads, released with the context. The
backend does not grant external capabilities. Nodes declaring capabilities or
effects are rejected, as are unsupported resource, region, task, secret, or
credential operations. Program contract sections absent from the embedded
document are rejected instead of silently discarded. These boundaries require
additional native implementations before this backend can compile those
effects; they do not imply the full language/runtime is complete.

## Execution verification

`tests/native_aggregate.rs` checks Array/Index/Option behavior, negative and
missing indices, shared quota failures, generated Map scheduling, and multi-state
Loop packs. On Linux x86-64 it assembles and links generated aggregate programs
against the real Rust primitive runtime, executes array/index, Map, and multi-state
Loop results, and checks that a deliberately exhausted step budget returns a
failure handle. Windows can build the compiler and run primitive tests, but
cannot execute the emitted System V code directly.
