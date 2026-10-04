# Implementation status

The immediate objective is to finish G0's language and runtime. Mosca integration
is deferred until programs can be represented, compiled and executed through
those native facilities.

## Executable today

- Original Graph Assembly parsing, validation and arithmetic compilation.
- Canonical binary GIR encoding and decoding.
- `g0c check` and `compile` for a single native `.g0g` graph.
- Native scalar GIR optimization, MIR, register allocation and x86-64 emission.
- Integer arithmetic, comparisons, Boolean operations, checked conversion and
  explicit truncation, subject to supported physical widths.
- Multi-graph composition and structured control through the Rust bootstrap APIs.
- Linux/x86-64 execution tests for generated assembly. Rust can build the
  compiler on Windows, but the emitted assembly uses ELF/System V conventions.

## Contracts requiring runtime implementation

Storage schemas, authorization, tenant isolation, transaction rules, encryption
requirements, replay restrictions, ownership plans and task budgets have
validation code and tests. They still need concrete persistence, cryptographic
transport, memory operations and execution machinery connected to compiler
lowering. A validated policy is not an implementation of its runtime effect.

## Current native-file boundary

The `.g0g` format represents one graph, not a complete program bundle. The CLI
checks graph references and control contracts with the existing program
validator; a missing subgraph definition fails before emission. Compilation
uses the existing scalar pipeline and exports `g0_machine_main`.

The CLI rejects malformed options. Decode, validation and lowering failures
leave an existing output file intact, and an output resolving to the input
path is rejected. Successful assembly output replaces its directory entry
atomically, preserving the input even if output is a hardlink alias. Failed
replacement cleans up the temporary file. Import/check does not execute graph
operations.

The checked-in `examples/truncate.g0g` returns 31. The CLI integration test
actually assembles and executes it on Linux, rather than only inspecting the
assembly text.

## Next milestone

Define a native program container containing an explicit entry and multiple
canonical graph definitions. Preserve the existing program validation gates,
reject incomplete bundles and compile calls/control flow without Rust-side
graph construction. Keep additional policy/schema sections explicit rather
than granting runtime authority during decoding.

After that milestone, implement executable structured values and memory,
then the runtime effects required by a small native application.
