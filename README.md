# G0

G0 is an experimental graph-first programming language and compiler.

The source of truth is a typed computation graph, not textual control flow. The text format in this repository is **Graph Assembly**, a bootstrap/import/export representation. The native Windows editor authors and debugs canonical graphs directly.

## Current principles

- Graph-first semantics; text is not the language model.
- Static, explicit types.
- No implicit casts.
- No hidden allocation or garbage collector.
- No `null` or `undefined`.
- Checked behavior is preferred over silent overflow.
- Effects and capabilities are explicit.
- Dependencies define legal parallelism.
- Modern hardware baseline; bootstrap target: `x86_64-v3`.
- Legacy/insecure protocols will not enter the core platform.
- No LLVM dependency in the bootstrap compiler.

## Bootstrap compiler

`g0c check` and `g0c compile` accept native program files (`.g0p`) and
canonical graph files (`.g0g`).
The existing `.g0` Graph Assembly path remains bootstrap tooling.

The native path is:

```
canonical binary graph (.g0g)
      |
      v
 decoder + graph/program validation
      |
      v
 constant folding + invariant checks
      |
      v
 MIR -> register allocation -> MachineIR
      |
      v
 x86-64 assembly
```

Native scalar compilation supports integer arithmetic, checked division and
remainder, integer comparisons, Boolean operations, checked conversion and
explicit bit truncation. Rust APIs also support composing multiple graphs and
structured control flow. A `.g0p` file contains multiple graph definitions and
one explicit entry, enabling native calls, selection and bounded loops from
files. A `.g0g` file contains one graph; references to absent definitions are
rejected in either format.

Try the checked-in program examples directly:

```sh
cargo run --release -- check examples/call.g0p
cargo run --release -- compile examples/call.g0p -o target/call.s
cargo run --release -- compile examples/select.g0p -o target/select.s
cargo run --release -- compile examples/loop.g0p -o target/loop.s
```

The call and selection examples return 42. The loop starts at 7, executes a
body that resets the state to 0, then exits and returns 0. These outputs are
assembled and executed in the Linux CLI integration tests. See
`docs/program-format-0.1.md` for the container contract and current limits.

Try the checked-in native example without running a Rust graph generator:

```sh
cargo run --release -- check examples/truncate.g0g
cargo run --release -- compile examples/truncate.g0g -o target/truncate.s
```

The example explicitly truncates 511 to an unsigned 5-bit integer and returns
31 through `g0_machine_main`. Native magic is recognized independently of the
filename extension; the `.g0g` extension also ensures damaged native files are
reported as native decoding errors. `check` validates semantics and references;
`compile` additionally requires implemented machine lowering for every value
and operation. A structurally valid graph is not necessarily executable yet.

The original bootstrap text path is:

```
Graph Assembly
      |
      v
 parser/resolver
      |
      v
 typed graph
      |
      v
 validator (including cycle detection)
      |
      v
 constant folding
      |
      v
 direct x86-64 assembly emitter
```

Supported operations in the bootstrap text format:

- `const i64`
- `add`
- `sub`
- `mul`
- `return`

Forward references are allowed because textual ordering must not define execution ordering.

## Example

```g0
g0 0.1
target x86_64-v3

add %answer %left %right
const %left i64 20
const %right i64 22

return %answer
```

Compile:

```sh
cargo run --release -- compile examples/answer.g0 -o target/answer.s
cc -c target/answer.s -o target/answer.o
```

The optimizer reduces the example graph to a constant result before code generation.

## Why Rust for g0c v0?

Rust is bootstrap machinery, not a design dependency. The intended path is:

```
g0c v0: Rust
g0c v1: Rust + G0
g0c v2: G0 self-hosting
```

The bootstrap platform uses pinned `ring` and `rustls` for snapshot encryption
and mutually authenticated native transport. Their versions are locked in
`Cargo.lock`; they do not define G0 Core semantics.

## Current development priorities

Finish the language and its runtime before adding Mosca integration:

1. Executable record/variant layouts, text, bytes, collections and memory operations.
2. Runtime ownership/regions, task execution and capability enforcement.
3. Native persistent storage, transactions and secure transport implementations.
4. End-to-end applications with one structural model across runtime boundaries.
5. Native graph authoring/debugging tools and eventual self-hosting.

The native host now executes structured values, bounded tasks and regions,
encrypted storage transactions and typed messages over mutually authenticated
TLS 1.3. An integration test carries one record through graph execution,
persistence, transport and client reception. Some advanced profiles remain
contracts and fail closed when unsupported. See
[`docs/native-execution.md`](docs/native-execution.md) and
[`docs/implementation-status.md`](docs/implementation-status.md).

The Windows native editor runs with `cargo run --bin g0-editor`. The initial G0
compiler backend is checked in as `compiler/native-wrapper.g0p`; use
`cargo run -- bootstrap input.g0p -o output.s` to generate its interpreter-backed
native wrapper. See [`docs/native-editor.md`](docs/native-editor.md) and
[`docs/compiler-bootstrap.md`](docs/compiler-bootstrap.md) for the explicit
direct native profile. Its G0 parser, validator, scheduler and emitter compile
the compiler's own definition; linked Linux generations produce identical
assembly. The default bootstrap entry remains the runtime wrapper.

See `docs/architecture.md` for the architectural constraints.
