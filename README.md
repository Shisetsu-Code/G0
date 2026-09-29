# G0

G0 is an experimental graph-first programming language and compiler.

The source of truth is a typed computation graph, not textual control flow. The text format in this repository is **Graph Assembly**, a bootstrap/import/export representation for constructing graphs until the native graph tooling exists.

## Current principles

- Graph-first semantics; text is not the language model.
- Static, explicit types.
- No implicit casts.
- No hidden allocation or garbage collector.
- No `null` or `undefined`.
- Checked behavior is preferred over silent overflow.
- Effects and capabilities will be explicit.
- Dependencies define legal parallelism.
- Modern hardware baseline; bootstrap target: `x86_64-v3`.
- Legacy/insecure protocols will not enter the core platform.
- No LLVM dependency in the bootstrap compiler.

## Bootstrap compiler

`g0c` currently implements the smallest complete path:

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

Supported graph operations in 0.1:

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

The compiler currently uses no third-party Rust crates.

## Near-term roadmap

1. Freeze Graph IR invariants and binary canonical format.
2. Add explicit integer overflow semantics.
3. Add graph inputs/outputs and callable subgraphs.
4. Add comparisons, selection and graph-native iteration.
5. Add ownership/lifetime regions and explicit effects.
6. Add a real machine IR and register allocator.
7. Add SIMD/vector nodes and dependency-driven scheduling.
8. Add native graph editor/tooling.
9. Add capability-based filesystem/network/runtime APIs.
10. Self-host the compiler.

See `docs/architecture.md` for the architectural constraints.
