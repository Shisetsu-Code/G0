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
- [x] 7. Native graphical graph editor/debugger: create/open/edit/save canonical graphs, diagnostics and source attribution. Verify native UI and saved documents.
- [ ] 8. Compiler implemented in G0: executable compiler graphs, staged bootstrap and reproducible output comparison. Scope of implemented language must be documented, then expanded to cover the compiler itself.
- [ ] 9. Whole-branch review, full tests/clippy, Linux native execution CI, update PR and deliver patches and implementation report.

## Execution ledger

2026-10-04: Existing native `.g0g`/`.g0p` compilation and CLI passed 295 Windows tests and Linux native execution CI. Tasks above are the remaining list; no item is marked complete by design alone.

Ruling: Reuse the current isolated clone and draft-PR branch — it contains the accepted continuation and is clean — cost if wrong: move the branch to a managed worktree before integration.

Task 1: complete — cargo test: 301 passed; cargo clippy --all-targets -- -D warnings: clean. Native Rust execution is distinct from x86 assembly compilation.

Tasks 2..6: initial native profiles implemented — 324 tests passed; clippy all-targets clean. One real typed record traversed authenticated TLS, GIR storage effects, a durable commit and store reopen.
Ruling: Ship explicit initial storage/transport profiles and reject advanced unsupported requirements — preserves security instead of treating validation as implementation — cost if wrong: extend the profiles before applications requiring protected fields, relations, migration or multiplexing can run.
- [x] GIR region/buffer handles and pure task spawn/join, including canonical files and native hosts.
- [x] Persistent relations, cardinality, authorized delete plans and GIR set/traverse operations.
- [x] Private/Secret field envelopes and opaque credential verifiers, including GIR operations.
- [x] Runtime protocol for independently trusted rollback witnesses; deployed providers remain host bindings.
- [x] Executable bounded offline migrations through pure G0 graphs and committed structural schema identity.
- [x] GIR child/secret regions with inherited lifetime/quota and protected read/write types.
- [x] Scoped child task profile for owned memory effects, bounded reservations and handle containment.
- [x] Explicit child effect-host factory and storage transaction provider, with delegated grants and completion failure propagation.
- [x] Required hybrid key exchange and bounded typed multiplexing with independent stream closure.
- [x] Aggregate x86 lowering: Linux CI assembles and executes primitive, Map,
  Loop and multigraph fixtures through the direct value-runtime ABI.

Task 7: initial Win32/GDI editor implemented and smoke-tested on Windows: create/connect/save/execute 42 + 7 = 49, native canvas rendered to BMP. Canonical editing model, undo/redo and bounded graph/node trace tested. Full toolbox constructors, persisted screen layout and live breakpoints remain. Collection Map/TextJoin operations added for compiler construction; order, empty input, wrong body types and execution limits tested.

Task 8: executable G0 native-wrapper backend added with canonical compiler source, typed G0P 0.3 entry arguments, opaque native runtime ABI and atomic CLI output. Its graph source compiles itself reproducibly through the runtime; Linux CI additionally links and compares three stages. Direct native optimizing lowering and a G0 implementation of decoding/validation remain; the interpreter-backed wrapper is explicitly not marked full self-hosting.

Editor completion: native forms cover operations, semantic types, node/graph
interfaces, effects, required capabilities and schema declarations. Persisted
bounded layout and live worker debugging support pause, step, breakpoints,
continue and cancellation. Twenty editor tests, native HWND/GDI smoke and
Clippy passed; review additionally exposed and fixed long-literal Apply
truncation with an actual native-control regression check. Explicit form port
IDs now preserve imported nonconsecutive interfaces and node contracts.

Compiler/runtime checkpoint: G0 binary syntax reader, AST offset descriptors and
topological scheduler execute as G0 graphs. A direct G0 emitter covers primitive
operations, multiple graphs, calls, selection, matching, loops and maps. Linux
has assembled and executed sparse-ID call/select/loop output. Compiler-native
self-generation still exhausts its explicit step quota; full semantic validation
and its remaining program contracts are in progress. Host limits distinguish ordinary execution from explicit
compiler reservations. The Rust aggregate native backend has machine-level
control, typed entries, private tagged result packs and cancellation; Linux
assembly execution passed CI run 37201209650 on Linux. The full run also passed
Windows editor verification and three linked wrapper-bootstrap stages, including
rejection of a legacy compiler input without publishing diagnostic text as assembly.

Independent runtime review: 89 tests in 15 suites passed for the completed
storage, task, transport, codec, editor and ABI paths. Follow-up review verified
full 4 MiB byte emission (41,943,033 output bytes for all-255 input) within
12,584,075 steps. Dense lookup preserves general lookup behavior; 21 differential
probes passed, and a 1000-node dense lookup used 22 steps while the old scan
exhausted 200. The compiler's remaining semantic and self-generation work is
excluded from these completion claims.

Runtime integration: G0G 0.3 resource operations now execute through opaque host-bound linear handles. Tests cover region lifetime, bounds, foreign/stale handles, schema-contained linear fan-out, explicit task permissions, ordered concurrent spawning, one-time join and child error propagation. Native wrappers, CLI, editor and graph services route these operations to the actual resource scope. Linux and Windows CI succeeded for the preceding 336-test editor/compiler checkpoint (run 37193464436).

Storage relations: actual encrypted row metadata, serializable updates, scope/resource/cardinality enforcement and authorized Restrict/Detach/Cascade deletion implemented. Cascades are bounded to 128 entities before recursive planning. G0S 0.1 fixture read and G0S 0.2 commit/reopen upgrade verified; GIR set/traverse executes within the same storage transaction. Full suite: 345 tests pass.

Protected storage: G0S 0.3 adds independently authenticated field envelopes and verifier-only credentials; canonical G0G 0.4 adds credential operations without granting authority. Plaintext/credential persistence, context substitution, explicit actions, field policy and work budgets tested. Rollback witness integration checks exact authenticated snapshot commitments and withholds acknowledgement when external durable advancement fails.

Verification: 348 local tests pass and all-target Clippy is clean. Both credential GIR operations execute through StorageHost. The preceding resources/relations checkpoint also passed Linux and Windows CI (run 37194866493).

Migration integration: administrative capability binds root/resource/versions; a pure closed G0 graph transforms previous-schema records into the new schema. One executor shares limits across the complete resource. Validation, cancellation and mid-migration exhaustion preserve the previous snapshot. A required private-field backfill survives reopen; obsolete schemas and pre-migration transactions are rejected. G0S 0.4 adds authenticated schema and structural-store identity while retaining earlier snapshots. Linux/Windows CI also passed the protected-storage/witness checkpoint (run 37195779389).

G0G 0.5 region integration: graph constructors create secret roots and scoped children; opaque secret kinds prevent declaring protected reads as public. Typed graph execution tests create, allocate, write/read and close child/parent scopes in explicit order; direct tests reject descendants after parent destruction and public encoding of secret reads. The migration checkpoint passed Linux and Windows CI (run 37196257939).

G0G 0.6 scoped tasks: owned child ResourceHost executes region effects; twice the child value quota reserves executor and resource allocations. Pure and scoped task kinds/capabilities remain distinct. Tests prove actual child read/write results, default-deny spawning, summary validation, child exhaustion and rejection of escaping child buffers. External effects remain explicit host-binding work.

G0G 0.7 hosted tasks: explicit per-child factories receive only declared parent-held grants. StorageTaskFactory binds a fixed principal/scope/allowlist, stages a separate transaction and commits after valid output and cancellation checks. Real graph tests cover absent factories, denied delegation, durable success, staged-write discard after exhaustion and authority revocation at completion. Successful child commits remain independent of later parent failure; nested execution is rejected in this profile.

Compiler optimization checkpoint: dense node/port lookup verifies candidate IDs
before taking its fast path, and the scheduler preserves the general fallback
for reordered dependencies. Independent wire review passed 13 semantic tests
and 44 adversarial cases with duplicate/missing targets, sparse IDs and shuffled
edges. Linux CI 37205459062 passed the four native aggregate tests, including
actual full-width i128 C ABI execution, and twelve direct G0 control tests.
Native compiler self-generation still exhausted its explicit 64-million-step
reservation; tasks 8 and 9 remain open pending complete semantic validation and
successful native generation comparison.
Independent i128 decoder review verified 8,360 byte-pattern and boundary cases.
Each valid word consumed 141 steps; static intervals prove every intermediate
Mul/Add fits i128. A 16-byte slice rejects truncated and invalid-offset reads
before decoding. The earlier loop exhausted a 180-step regression reservation.

Further compiler work: canonical graph-name proof and binary lookup pass six
tests in two suites, including Unicode/NUL prefixes, missing names, 56 table
sizes and bounded comparison over a four-MiB source. The unsigned-word reader
now uses exact static byte/accumulator intervals instead of eight redundant
checked conversions; 1,032 bit-pattern/boundary cases fit forty steps, with the
low-level helper's existing zero-padding behavior preserved. Twenty tests in
five suites pass for the combined names, schemas, named-node and call-cycle
definitions. Actual type-depth validation and emission caches remain in progress.

A release wrapper profile for the 1,939,646-byte expanded definition completed
in 17,682,256 steps and 20,438,584,134 cumulative logical bytes. The explicit
compiler host now reserves 32 million steps and 32 GiB; ordinary defaults are
unchanged. A separate reader profile of a 2,001,821-byte definition exhausted a
diagnostic 64-GiB allocation ceiling at 48,277,802 steps, so this partial run
does not establish the full reader cost or native self-compilation success.
The latest published Windows job masked failing intermediate tests because
PowerShell continued to later successful commands; local CI changes propagate
each exit code. Tasks 8 and 9 remain open and the PR remains a draft.

The same frozen 2,039,950-byte definition completed the full reader in
112,972,522 steps and 158,883,266,981 logical bytes with diagnostic ceilings of
128 million steps and 512 GiB. Its syntax-only reader required 11,512,498 steps
and 15,820,412,439 logical bytes. These measurements justify reducing repeated
semantic work; neither raises production ABI quotas. A general fixed-width
little-endian i128 decode primitive is being added to reduce integer-range
validation costs while retaining the compiler algorithm in G0.

Native scalar storage now interns Bool and Integer values from -32 to 1023,
using a fixed 1058-handle cache. Eighteen tests in four suites and four independent
review tests pass, covering exact cumulative quota exhaustion, typed pack
separation, all cache slots, full i128 boundaries, steps and cancellation.
Allocation accounting still charges every production before physical reuse.

GIR 0.10 checkpoint 8024a2b adds an exact 16-byte little-endian full-width
integer decoder, validated in the executor, native ABI, binary format and
editor. The published tree matches the local tree exactly. Its full direct
compiler profile, with a 2,019,721-byte self input, exhausted a diagnostic
128-million-step limit after charging 175,377,866,716 cumulative logical bytes.
Native self-compilation remains unproved; the next optimization shares parsed
program/schema tables between semantic passes while retaining all rejection
gates. Independent raw-input review also found a type-depth boundary mismatch:
Rust rejects a leaf at depth 128 but the G0 predicate accepted it. This must be
fixed before replacing host validation or making the direct compiler default.

The depth boundary is now corrected: nested prefixes permit leaves below depth
128, matching Rust. Seven raw-input review tests passed in 11.69 seconds,
comparing Rust validation with direct execution of G0 compile-direct over
manually encoded bytes. Coverage includes node/call cycles, wiring and container
boundaries, schemas and named types, integer bounds, depth versus width,
precision, opcode version gates, Record/Variant payload contracts and
Select/Loop/Map contracts. Native self-generation and full-branch checks are
still pending.

CI 37210897202 verifies the GIR 0.10 integer codec through assembled Linux code
and the full-width C ABI. Native graph-name and graph-reference phases pass;
the reader and emitter still exhaust their 64-million-step production
reservations, and the native self-generation test fails with step-limit code 1.
The Windows job now propagates every exit code and passes editor/runtime/wrapper
checks. These results do not establish a green whole-branch build.

The shared-AST definition's reader alone completed in 92,590,894 steps and
126,945,329,343 logical bytes for a frozen 2,029,711-byte input at diagnostic
limits. Its full compile-direct input has 2,029,718 bytes and exhausted 128
million steps after 175,421,397,306 logical bytes. A remaining unrolled signed
integer-bound comparison in the parser is being replaced by the same general
codec, and duplicate profile checks are being removed only where the global
port-type validator already proves those predicates before emission.

The complete local Windows all-target test run for checkpoint 8024a2b passed
541 tests in 61 suites (880.49 seconds). Current-tree Clippy also passed. This
does not include successful Linux self-generation, nor does it replace the
fresh verification required after the subsequent compiler optimizations.

The next frozen definition is 2,037,656 bytes, SHA-256
3962BACF742EE5F746A7D9EA29CEB09D1F46BB294EC826D06963D141845BC580.
It includes shared program/schema AST tables, the remaining integer-bound codec
replacement, contextual elimination of duplicate type proofs, and a monotonic
incoming-edge interval cache. The real compiler's 397-edge graph preserves exact
argument assembly within 1,500 steps where binary lookup exhausts that budget.
Lazy native graph frame/port metadata is charged once when eligible and uses
the previous path under tight budgets. Fresh verification passes 21 tests in
six focused suites (23.55 seconds) and all-target Clippy. Full native
self-generation remains pending.

The completed observer diagnostic identified 36,482,481 reader-u32 node visits
out of 84,405,216 reader steps (about 43%). Observer materialization adds logical
allocation, so its 137,013,184,627-byte charge is not a baseline memory metric.
The next definition uses GIR 0.11 DecodeUnsigned32Le (opcode 75): exactly four
Bytes decode to Integer[0,u32.MAX], with no effects or capabilities. Reader-u32
uses a guarded codec path and retains its zero-padding fallback, including the
offset 2^48-3 boundary. Its 1,032 patterns fit fifteen steps. Core/editor/codec
verification passes 52 tests; independent review passes 512 random words,
lengths 0..12, source-handle preservation and cumulative cached-value charging.
The compiler's directed regressions and Clippy pass. The new canonical source
is 2,050,623 bytes, SHA-256
5B719AA7B740C0C6F023480308C68CBBCD2F1665D859254A98BED5CA54F6BE9A.
Full local and native checks for this frozen definition are pending. An
intermediate local all-target run encountered an intentionally failing test
executable replaced by concurrent TDD work; it is not counted as verification.

Frozen 0.11 completed all four isolated Linux native phases in CI 37213831379:
reader, names, references and emitter. Full native stage zero still exhausted
64 million steps. Its exact compile-direct definition (2,050,630 bytes) also
completed a diagnostic interpreter run in 209,834,144 steps and
3,550,563,057,868 cumulative logical bytes with 512 million steps/eight TiB
diagnostic limits. This logical accounting is not native retention or RSS.
The next explicit compiler reservation will be measured at 256 million steps,
keeping the ordinary one-million-step/64-MiB defaults unchanged. Native usage
counters are being added so the memory reservation can be verified directly.
The frozen local suite found a stale integer128 review expecting GIR 0.11
rejection; the corrected four-case suite accepts 0.10/0.11 and rejects 0.9/0.12.
The full suite must be repeated after the current metrics/budget changes.

The metrics/budget checkpoint passes the fresh complete Windows suite: 567 tests
in 68 suites, 104.44 seconds of test execution with development/test opt-level 2.
Fresh all-target Clippy with warnings denied is clean. Independent read-only
review found no metrics ABI, snapshot, quota/default or stdout contamination
issues. Native stage-zero/one comparison at 256 million steps and 32 GiB is
still pending; the ordinary one-million-step/64-MiB defaults are unchanged.
