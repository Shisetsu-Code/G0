# Language and runtime completion

**Goal:** Finish the native G0 stack before Mosca, including graph authoring and a compiler implemented in G0. Execute continuously; no approval between improvements.

**Architecture:** Typed GIR remains the source of truth. Rust is the bootstrap host. Runtime values preserve semantic types across execution, storage and transport. Authority is supplied by the host explicitly. Native platform profiles supply cryptography and operating-system bindings. No browser or legacy protocol is required.

**Tech stack:** Existing dependency-free Rust compiler, canonical G0 binary documents, System V x86-64 backend; additional native profiles only when implemented and verified.

**Spec:** Core constitution, architecture, implementation-status and program-format documents. User explicitly includes native graphical editor and self-hosting compiler.

## Global constraints

- Preserve checked arithmetic, explicit truncation, default-deny authority and graph-native diagnostics.
- Decoding/importing never executes or grants permissions.
- Bounded execution and allocation; explicit cancellation and region lifetimes.
- Never claim contracts or stubs are executable implementations.
- Keep Mosca out. Keep the draft PR reviewable; do not merge.

## Review focus

Malformed and cyclic inputs; resource exhaustion before allocations; nested calls and capability propagation; tenant isolation; transaction conflicts and crash recovery; secret serialization; transport identity and replay; graphical changes preserving canonical graph identity; compiler bootstrap reproducibility.

## Tasks and completion evidence

- [x] 1. `src/execution.rs`, `src/value.rs`, CLI: execute scalar graphs and calls/select/match/bounded loops on a native Rust host. Tests compare checked arithmetic and CLI results; invalid inputs, exhaustion and absent authority must fail explicitly.
- [x] 2. GIR/binary/validation/runtime: executable arrays, records, variants, text and bytes; one schema registry; typed canonical value codec. Round-trip, malformed input and incorrect construction/access tests.
- [x] 3. Runtime regions, ownership, budgets, tasks and cancellation: executable APIs, lifetime violations and authority attenuation tests.
- [x] 4. Native storage and transactions: schema/policy enforcement, persistence, atomic commits, version conflicts, tenant isolation and recovery tests.
- [x] 5. Native secure transport: maintained cryptographic platform profile, authentication, framing, replay rejection and loopback integration tests.
- [x] 6. End-to-end native application: same typed structure through execution, storage, transport and client; integration test proves real behavior.
- [ ] 7. Native graphical graph editor/debugger: create/open/edit/save canonical graphs, diagnostics and source attribution. Verify native UI and saved documents.
- [ ] 8. Compiler implemented in G0: executable compiler graphs, staged bootstrap and reproducible output comparison. Scope of implemented language must be documented, then expanded to cover the compiler itself.
- [ ] 9. Whole-branch review, full tests/clippy, Linux native execution CI, update PR and deliver patches and implementation report.

## Execution ledger

2026-10-04: Existing native `.g0g`/`.g0p` compilation and CLI passed 295 Windows tests and Linux native execution CI. Tasks above are the remaining list; no item is marked complete by design alone.

Ruling: Reuse the current isolated clone and draft-PR branch — it contains the accepted continuation and is clean — cost if wrong: move the branch to a managed worktree before integration.

Task 1: complete — cargo test: 301 passed; cargo clippy --all-targets -- -D warnings: clean. Native Rust execution is distinct from x86 assembly compilation.

Tasks 2..6: initial native profiles implemented — 324 tests passed; clippy all-targets clean. One real typed record traversed authenticated TLS, GIR storage effects, a durable commit and store reopen.
Ruling: Ship explicit initial storage/transport profiles and reject advanced unsupported requirements — preserves security instead of treating validation as implementation — cost if wrong: extend the profiles before applications requiring protected fields, relations, migration or multiplexing can run.
- [ ] Remaining runtime integration: GIR region/task handles; aggregate x86 lowering; protected fields/credentials and relations; migration/rollback witnesses; advanced transport profiles.

Task 7: initial Win32/GDI editor implemented and smoke-tested on Windows: create/connect/save/execute 42 + 7 = 49, native canvas rendered to BMP. Canonical editing model, undo/redo and bounded graph/node trace tested. Full toolbox constructors, persisted screen layout and live breakpoints remain. Collection Map/TextJoin operations added for compiler construction; order, empty input, wrong body types and execution limits tested.

Task 8: executable G0 native-wrapper backend added with canonical compiler source, typed G0P 0.3 entry arguments, opaque native runtime ABI and atomic CLI output. Its graph source compiles itself reproducibly through the runtime; Linux CI additionally links and compares three stages. Direct native optimizing lowering and a G0 implementation of decoding/validation remain; the interpreter-backed wrapper is explicitly not marked full self-hosting.
