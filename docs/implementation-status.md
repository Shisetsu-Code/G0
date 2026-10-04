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
  explicit truncation. The aggregate runtime preserves full signed i128 values;
  checked Add/Sub/Mul return typed results on overflow.
- Multi-graph composition and structured control through native files and the
  Rust bootstrap APIs.
- Linux/x86-64 execution tests for generated assembly. Rust can build the
  compiler on Windows, but the emitted assembly uses ELF/System V conventions.
- Native Win32/GDI graph editor with canonical save, operation/type/interface
  forms, preserved port identities, undo/redo, persisted layout and live worker
  debugging with pause, step, breakpoints, continue and cancellation. UI limits
  are documented in `native-editor.md`.
- Compiler graphs parse native program syntax, build offset-based ASTs, schedule
  dependencies and emit native primitives, multiple graphs, calls, selection,
  matching, loops and maps, or a runtime wrapper. Independent semantic validation
  and native compiler self-hosting remain in progress; see
  `compiler-bootstrap.md` for reproducibility checks and remaining work.

## Native runtime

The native Rust host now implements `g0c run`, immutable structured values and
schema-directed codecs, bounded/cancellable tasks, region lifetimes, encrypted
snapshot transactions and mutually authenticated TLS 1.3. Storage GIR nodes use
one explicit transaction with commit after successful execution. A graph-native
service maps authenticated certificate fingerprints to scoped principals.
Explicit hybrid-only key exchange and bounded typed multiplexed streams now
extend the native transport; see `native-execution.md` for their boundaries.
Integration tests prove the same record survives storage reopen and a real
encrypted loopback round trip.

GIR also executes opaque linear region/buffer handles and bounded pure child
tasks through `ResourceHost`, including CLI/editor/native-wrapper/service
hosting. See `graph-resources.md` for the current interfaces and profiles.
Explicitly hosted children delegate only parent-held requirements through a
per-child host factory. The storage provider uses independent transactions,
commits after valid results and drops writes on child execution failure.

Direct aggregate x86 lowering now executes structured values and machine-level
calls, selection, matching, bounded loops and maps with a linked bounded runtime.
Native compilation rejects unsupported effect bindings; see `native-aggregate-abi.md`.
Online migration/type-change profiles, trusted witness providers and
advanced scheduling/remote-execution profiles still require implementation.
Current profiles reject unsupported requirements instead of weakening them.

Private/Secret field encryption and opaque credential verifiers execute in the
native store, including explicit GIR credential operations and bounded work.
See `protected-storage.md` for the permissions, formats and memory boundaries.
The native store also binds commits to an external rollback witness, detects
restored/deleted snapshots and fails closed on unacknowledged commits. Deployment
supplies a trusted witness provider; see `store-witness.md`.
Offline migrations execute pure G0 transformation graphs over real records,
preserve identities and managed values, validate integrity and replace snapshots
atomically. G0S 0.4 binds structural schema identity; see `store-migrations.md`.

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
Aggregate programs select the direct value-runtime backend. Its typed entry and
context ABI is distinct from the scalar compatibility entry; compiled assembly
must link the native runtime library.

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

Complete independent semantic validation and native self-hosting for the G0
compiler, then finish the whole-branch review and verification. The full
continuous checklist is in `docs/superpowers/plans/2026-10-04-language-runtime.md`.
Policy sections remain host bindings, never authority obtained by decoding.
