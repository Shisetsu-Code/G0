# G0 Core Constitution 0.1

Status: **frozen design contract for the bootstrap phase**.

This document defines the principles that the compiler, runtime and standard platform must preserve. A change that violates one of these rules is a language-design change, not an implementation detail.

## 1. Maxims

1. **The program is a typed graph.** Text is only a transport/editing representation.
2. **Security is structural and secure-by-default.** Unsafe/insecure behavior is never the implicit path.
3. **Import never executes.** Loading definitions grants neither execution nor authority.
4. **Execution never grants authority.** Capabilities are explicit inputs and cannot appear from ambient state.
5. **No ambient authority.** Network, storage, clock, entropy, device, process and remote execution require capabilities.
6. **Default deny.** Absence of an authorization rule means no access.
7. **Storage is an enforcement boundary.** Authorization, tenant isolation, constraints and protected fields are enforced at the storage layer, not only in UI/API code.
8. **No implicit shared mutable state.** Shared mutation requires a concurrency primitive.
9. **No hidden control effects.** No hidden exceptions, hidden retries, import initializers, global constructors or implicit remote execution.
10. **No undefined behavior as a normal programming model.** Potentially invalid operations are rejected, checked or represented explicitly.
11. **No implicit lossy conversion.** Type and representation changes that can lose information require proof or an explicit operation.
12. **Logical meaning is separate from physical representation.** The compiler may choose widths, layouts, scheduling and implementations when equivalence is proven.
13. **Invariants are stronger than optimization.** An optimizer may search only inside the set of semantically valid and security-valid programs.
14. **The programmer declares intent, invariants and optimization freedom.** The toolchain chooses implementation details inside that space.
15. **Concrete technologies stay out of Core when a semantic contract can represent them.** Core expresses secure connection, storage, identity, stream, signature, etc.; platform profiles choose concrete mechanisms.
16. **Profiles may evolve without redefining program semantics.** Crypto, network, storage, accelerator and OS policies are versioned independently.
17. **Security properties cannot silently degrade under load or optimization.**
18. **Sensitive values carry restrictions with their type.** Secrets cannot accidentally become logs, debug output, ordinary serialization or unrestricted network output.
19. **Authorization is attached to resources and relations, not UI routes.**
20. **Race freedom is the default.** Concurrent conflicting writers are invalid unless mediated by an explicit safe primitive.
21. **Retries of non-idempotent effects are never implicit.**
22. **Remote code is inert until identified, verified, authorized and executed through an explicit capability.**
23. **Builds and selected optimization profiles are reproducible and lockable.**
24. **The safe path must also be the shortest path for humans and code-generating models.**
25. **Compatibility before 1.0 is subordinate to removing bad abstractions.**
26. **Core has one semantic text model.** Text is Unicode semantically; boundary codecs are explicit and limited. UTF-8 is the default, UTF-16/UTF-32 may be enabled for interoperability, and external legacy codecs exist only as explicit adapters. Arbitrary binary data is Bytes.
27. **Text is not byte-indexed.** User-facing text boundaries are grapheme-based; raw byte operations require Bytes.
28. **Mathematical precision is a contract.** Exactness, determinism, rounding and tolerated error are explicit semantic requirements, not backend accidents.
29. **Impossible exactness is rejected.** Transcendental results use declared precision/error contracts rather than pretending finite representation is exact.
30. **Profiling is graph-native.** Runtime cost, memory, I/O and latency are attributable to graph nodes and critical paths.
31. **Debugging cannot weaken information-flow rules.** Secret and credential values stay redacted even in tracing/profiling.
32. **Ownership follows graph topology.** Move/borrow validity is proven from dependencies; unordered conflicting ownership uses are invalid.\n33. **G0 is a native ecosystem, not a compatibility layer.** Python, SQL, HTTP, HTML/CSS/JS, DOM/browser compatibility and equivalent legacy stacks are not design requirements.\n34. **One logical structure should survive end to end.** Storage, service, transport and client boundaries must not force redundant schema/DTO/serialization models when one typed G0 structure can preserve the semantics.\n35. **Backend and data are one analyzable graph.** Storage mutations, service logic, transport and client-visible state should remain attributable in AST/dataflow/effect analysis instead of becoming opaque glue layers.\n36. **Measure instead of guessing.** Performance-oriented structural changes are accepted only after semantic/security gates and empirical compile/run/benchmark evidence.\n37. **Technical debt is not purchased for ecosystem familiarity.** Historical compatibility is never sufficient justification for adding duplicate syntax, protocols, formats or execution models.

## 2. Three classes of design property

Every configurable property belongs to one of these classes:

- `fixed<T>`: may not change.
- `bounded<T>`: optimizer may choose a value inside declared/proven bounds.
- `free<T>`: implementation may vary if semantic equivalence and all invariants are proven.

Example:

```text
PasswordVerification {
    security_profile = fixed(High)
    memory_cost      = bounded(128 MiB .. 1 GiB)
    parallelism      = bounded(1 .. 8)
    internal_layout  = free
}
```

## 3. Hard gates before optimization

Every generated candidate must pass these gates in order:

1. graph validity;
2. type validity;
3. ownership/lifetime validity;
4. effect/capability validity;
5. authorization and storage-policy validity;
6. concurrency/race validity;
7. security-profile validity;
8. user invariants;
9. functional tests/proofs required by the build.

Only candidates that pass all gates may enter performance optimization.

## 4. What is deliberately not frozen in Core

These are profile/backend concerns and must remain replaceable:

- concrete network protocols;
- concrete cryptographic algorithms;
- password-hash algorithm implementations;
- database engines;
- filesystem APIs;
- GPU APIs;
- object formats;
- operating-system APIs;
- exact instruction-set extensions;
- search/optimization algorithm;
- scheduler implementation.

Core freezes their semantic contracts, not their current implementations.
