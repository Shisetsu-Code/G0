# G0 architecture — bootstrap contract

Status: experimental, pre-1.0.

## 1. Program model

A G0 program is a directed typed graph.

A node is defined by:

- operation
- typed input edges
- typed output
- effect set
- constraints/attributes

The original text bootstrap uses `i64` and one output per node. Native GIR has
explicit typed ports, integer ranges and richer semantic types. Executable
machine lowering currently covers a scalar subset; declaring a semantic type
does not imply that its runtime representation is implemented.

Textual order has no semantic meaning. Graph Assembly therefore permits forward references.

## 2. Invariants

The executable computation graph must:

1. be well typed;
2. have resolvable edges;
3. be acyclic unless a future structured iteration node explicitly owns the cycle semantics;
4. make effects explicit;
5. make mutability explicit;
6. never depend on unspecified evaluation order.

Raw back-edge cycles are rejected. Iteration will be represented by a structured graph node rather than an accidental cycle.

## 3. Planned IR stack

```
G0 Graph
   |
   v
GIR       typed, target-neutral graph semantics
   |
   v
MIR       memory, vectors, scheduling, ABI
   |
   v
MachineIR target instructions/register classes
   |
   v
x86-64 / future targets
```

The native compiler follows GIR -> MIR -> register allocation -> MachineIR ->
x86-64 assembly. `g0c` can read a canonical `.g0g` graph directly into this
pipeline. The original Graph Assembly bootstrap still uses the isolated direct
emitter in `src/backend.rs`.

## 4. Product scope: backend + data first

G0 is not intended to become a compatibility language for the existing software stack.

The primary target is a native backend/data platform whose major layers share the same semantic model:

```
typed data
   <-> storage
   <-> services
   <-> transport
   <-> client/runtime
```

A logical structure should not need to be independently redefined as a SQL schema, ORM model, DTO, JSON payload, frontend model, and persistence object.

The compiler/runtime should preserve one typed structural identity across boundaries whenever semantics allow it.

Examples of properties that may attach structurally to data/resources include:

- persistence
- indexing
- mutability
- authorization
- ownership
- synchronization
- replication
- confidentiality
- encryption policy
- consistency requirements
- observability restrictions

These properties must be visible to validation, dataflow analysis, optimization and tooling.

## 5. Native platform, not legacy compatibility

G0 deliberately does **not** take compatibility with Python, SQL, HTTP, HTML, CSS, JavaScript, browser DOMs or existing browser engines as a platform requirement.

Those systems may be useful as temporary bootstrap references or experimental comparison worlds, but they are not architectural targets.

The intended platform owns its own:

- storage/database model
- transport
- service model
- identity and authorization
- encryption/security profiles
- streaming
- runtime
- client protocol
- eventual native client/rendering environment

If a browser-like client is built, it is for the G0 ecosystem rather than a requirement to reproduce the historical web platform.

The design objective is to avoid importing historical syntax, protocol layering and compatibility obligations unless a concrete G0 requirement independently justifies the same semantic feature.

## 6. End-to-end structural dataflow

Backend and data are treated as one graph problem rather than a chain of unrelated technologies.

A field may participate in a graph such as:

```
client event
    -> typed mutation
    -> authorization
    -> validation
    -> storage transition
    -> derived computation
    -> synchronization
    -> client update
```

The compiler/toolchain should be able to attribute dependencies, effects, cost and policy across this path.

This is central to both optimization and Mosca integration: the dataflow must not become opaque merely because the value crossed a storage, process or transport boundary.

## 7. Hardware baseline

Initial CPU target: `x86_64-v3`.

The project intentionally does not promise compatibility with obsolete ISA baselines. Future platform profiles may add newer baselines without weakening the current one.

## 8. Security/runtime direction

The future standard platform will expose secure capabilities rather than generic ambient authority.

Examples:

- network capability
- storage capability
- clock capability
- entropy capability
- accelerator capability

Security, encryption, authority and connection semantics should be structural rather than repeatedly rebuilt in application glue code.

Concrete cryptographic mechanisms may evolve through platform profiles without changing the logical program semantics.

## 9. Optimization model

Optimization is empirical and graph-native.

The toolchain should expose enough structure to answer:

- which AST/GIR/MIR nodes dominate runtime;
- where memory is retained or copied;
- which dependencies form the critical path;
- which effects prevent reordering;
- which regions can be optimized independently;
- whether a candidate preserves all semantic and security invariants.

Optimization candidates are compiled, executed, tested and measured. A proposed implementation is not accepted because it is theoretically attractive; it is accepted because it passes hard gates and improves the declared objective under measurement.

This model is designed to support selective search instead of repeatedly searching the entire program.

## 10. Compatibility policy and technical debt

"Zero technical debt" is an operating objective: avoid knowingly preserving obsolete or redundant mechanisms solely for compatibility.

G0 will prefer:

- a small stable semantic core;
- one canonical mechanism per semantic need where practical;
- explicit versioned platform profiles;
- replaceable policy/implementation layers;
- no permanent compatibility promises before 1.0;
- removal of poor abstractions before they fossilize;
- structural features over duplicated glue layers.

Compatibility with external ecosystems is not a goal by itself.

The criterion for adding a feature is whether the G0 ecosystem requires the semantic capability, not whether another ecosystem already has an API for it.
