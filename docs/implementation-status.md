# Implementation status

The immediate objective is to finish G0's language and runtime. Mosca integration
is deferred until programs can be represented, compiled and executed through
those native facilities.

## Executable today

- Original Graph Assembly parsing, validation and arithmetic compilation.
- Canonical binary GIR encoding and decoding.
- `g0c check` and `compile` for a single native `.g0g` graph.
- Native `.g0p` program files containing an explicit entry and multiple graphs,
  with calls, selection and bounded loops compiled from files.
- Native scalar GIR optimization, MIR, register allocation and x86-64 emission.
- Integer arithmetic, comparisons, Boolean operations, checked conversion and
  explicit truncation, subject to supported physical widths.
- Multi-graph composition and structured control through native files and the
  Rust bootstrap APIs.
- Linux/x86-64 execution tests for generated assembly. Rust can build the
  compiler on Windows, but the emitted assembly uses ELF/System V conventions.

## Contracts requiring runtime implementation

Storage schemas, authorization, tenant isolation, transaction rules, encryption
requirements, replay restrictions, ownership plans and task budgets have
validation code and tests. They still need concrete persistence, cryptographic
transport, memory operations and execution machinery connected to compiler
lowering. A validated policy is not an implementation of its runtime effect.

## Current native-file boundary

The `.g0g` format represents one graph. `.g0p` represents a closed executable
bundle with an explicit entry, multiple definitions and no external bindings.
The bootstrap entry has no input ports and exactly one output. The CLI
checks graph references and control contracts with the existing program
validator; a missing subgraph definition fails before emission. Compilation
uses the existing scalar pipeline and exports `g0_machine_main`.

The CLI rejects malformed options. Decode, validation and lowering failures
leave an existing output file intact, and an output resolving to the input
path is rejected. Successful assembly output replaces its directory entry
atomically, preserving the input even if output is a hardlink alias. Failed
replacement cleans up the temporary file. Import/check does not execute graph
operations.

The checked-in `examples/truncate.g0g` returns 31. Program examples `call.g0p`
and `select.g0p` return 42; `loop.g0p` performs one state transition and returns
0. CLI integration tests assemble and execute these examples on Linux.

## Next milestone

Implement executable structured values and memory, then the runtime effects
required by a small native application. Policy/schema sections in future program
formats must be explicit rather than granting runtime authority during decoding.
