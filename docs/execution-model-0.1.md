# G0 Execution Model 0.1

## 1. Entry

An executable has one explicit entry graph.

No imported module, global value, annotation, decorator or library may execute initialization effects merely because it was loaded.

```text
Executable
  entry -> App.Start
```

All startup effects are reachable from the entry graph.

## 2. Dependency execution

Edges define data/dependency requirements.

Independent pure nodes are legally parallel:

```text
A -> C
B -> C

A || B is legal
C waits for both
```

Textual source order has no execution meaning.

## 3. Effects

Effectful nodes participate in explicit effect ordering. The compiler/runtime may reorder only when doing so is proven equivalent under the declared effect semantics.

## 4. Concurrency safety

Two conflicting mutations of the same state cannot be scheduled concurrently unless mediated by a primitive whose semantics define that conflict.

Examples of valid mediation:

- Atomic
- Exclusive
- Actor
- Channel
- Transaction
- Versioned compare-and-set

Raw data races are invalid programs.

## 5. Check/use freshness

Security facts that can become stale cannot cross an unconstrained external effect and still be assumed current.

The compiler may require revalidation or an atomic transaction for patterns equivalent to:

```text
authorize
  -> external wait/effect
  -> mutate protected resource
```

## 6. Errors

Recoverable failure is represented with `Result<T,E>`.

Program-contract violations use explicit trap/abort semantics.

Hidden exception unwinding is not part of Core 0.1.

## 7. Integer arithmetic

Potential overflow behavior is explicit/proven.

The compiler may narrow physical representation only if range analysis proves all required intermediate/result values remain valid or inserts the declared checked behavior.

## 8. Sensitive control flow

Operations over `Secret<T>`, `Credential<T>` and authentication primitives must not introduce avoidable secret-dependent early termination or observable branches when the security profile requires side-channel-resistant behavior.

Security-sensitive verification primitives define this behavior once for all applications.

## 9. Storage access

A storage operation executes with principal/capability/scope context.

Storage-level policy is authoritative. Higher layers may further restrict access but may not widen what storage policy permits.

## 10. Retry semantics

Every external effect is classified by retry semantics.

At minimum:

- idempotent;
- non-idempotent;
- transactional/deduplicated.

Automatic retry of a non-idempotent effect without an explicit idempotency/deduplication contract is invalid.

## 11. Remote execution

A remote artifact is inert until the following graph succeeds:

```text
ArtifactId
   + ContentHash
   + SignatureIdentity
   + Interface
   + RequestedCapabilities
          |
          v
       Verify
          |
          v
      Authorize
          |
          v
       Execute
```

Remote execution authority is scoped to target, artifact/interface and granted capabilities. It is never equivalent to arbitrary command execution.

## 12. Optimization execution

Optimization follows:

```text
semantic graph
   -> validation gates
   -> candidate generation
   -> validation gates
   -> benchmark
   -> selection
   -> optimization lock
```

A candidate that violates a hard invariant is discarded regardless of measured performance.

Production execution uses the locked selected profile unless explicitly deployed in a bounded adaptive mode.
