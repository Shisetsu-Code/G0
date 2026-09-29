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

The bootstrap implementation currently has one output per node and one type (`i64`). This is deliberately temporary.

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

The current bootstrap temporarily lowers the typed graph directly to x86-64 assembly. That shortcut is intentionally isolated in `src/backend.rs`.

## 4. Hardware baseline

Initial CPU target: `x86_64-v3`.

The project intentionally does not promise compatibility with obsolete ISA baselines. Future platform profiles may add newer baselines without weakening the current one.

## 5. Security/runtime direction

The future standard platform will expose secure capabilities rather than generic ambient authority.

Examples:

- network capability
- filesystem capability
- clock capability
- entropy capability
- accelerator capability

Insecure/legacy protocol variants do not belong in the core platform. Secure transport policy must still be versioned independently from language semantics so cryptographic and protocol recommendations can evolve without redesigning the language.

## 6. Compatibility policy

"Zero technical debt" is treated as a design constraint, not a literal guarantee.

G0 will prefer:

- explicit versioned platform profiles;
- small stable semantic cores;
- replaceable policy layers;
- no permanent compatibility promises before 1.0;
- deprecation/removal before fossilizing poor abstractions.

This is intended to prevent today's "modern" choices from becoming tomorrow's hard-coded legacy.
