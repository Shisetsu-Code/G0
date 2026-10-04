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
- Native Win32/GDI graph editor with canonical save, validation, undo/redo and
  bounded execution traces. The initial toolbox and UI limits are documented in
  `native-editor.md`.
- Executable compiler backend written as G0 graphs, generating a native wrapper
  linked to the Rust runtime. This stage is not full compiler self-hosting; see
  `compiler-bootstrap.md` for reproducibility checks and remaining work.

## Contracts requiring runtime implementation

The native Rust host now implements `g0c run`, immutable structured values and
schema-directed codecs, bounded/cancellable tasks, region lifetimes, encrypted
snapshot transactions and mutually authenticated TLS 1.3. Storage GIR nodes use
one explicit transaction with commit after successful execution. A graph-native
service maps authenticated certificate fingerprints to scoped principals.
Integration tests prove the same record survives storage reopen and a real
encrypted loopback round trip.

GIR also executes opaque linear region/buffer handles and bounded pure child
tasks through `ResourceHost`, including CLI/editor/native-wrapper/service
hosting. See `graph-resources.md` for the current interfaces and profiles.

Aggregate x86 lowering, migrations, trusted witness providers and
advanced scheduling/remote-execution profiles still require implementation.
Current profiles reject unsupported requirements instead of weakening them.

Private/Secret field encryption and opaque credential verifiers execute in the
native store, including explicit GIR credential operations and bounded work.
See `protected-storage.md` for the permissions, formats and memory boundaries.
The native store also binds commits to an external rollback witness, detects
restored/deleted snapshots and fails closed on unacknowledged commits. Deployment
supplies a trusted witness provider; see `store-witness.md`.

Persistent relations execute cardinality, referential/scope integrity and
authorized Restrict/Detach/Cascade deletion, including GIR link/traversal nodes.
Encrypted snapshots preserve legacy reads and upgrade their relation-aware
envelope on commit. See `store-relations.md` for bounded cascade behavior.

## Current native-file boundary

The `.g0g` format represents one graph. `.g0p` represents a closed executable
bundle with an explicit entry, multiple definitions and no external bindings.
G0P 0.3 supports typed entry interfaces; the optimized bootstrap entry still
requires no input ports and exactly one output. The CLI
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

Expand the editor and G0 compiler beyond their initial profiles, then close the
remaining runtime/profile integrations. The full
continuous checklist is in `docs/superpowers/plans/2026-10-04-language-runtime.md`.
Policy sections remain host bindings, never authority obtained by decoding.
