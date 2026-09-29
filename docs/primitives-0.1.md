# G0 Primitive Set 0.1

Status: proposed primitive vocabulary for GIR 0.1.

The goal is a deliberately small semantic core. Higher-level facilities must be expressible by composing these primitives.

## 1. Graph primitives

### Graph
A typed directed graph with explicit inputs, outputs, effects and capability requirements.

### Node
An operation instance.

### Port<T>
Typed input or output endpoint.

### Edge<T>
Typed dependency/data edge between compatible ports.

### Subgraph
A graph usable as a node. This is the primary function/composition mechanism.

### Select
Selects one of multiple subgraphs from a condition without introducing arbitrary graph cycles.

### Match
Typed multi-branch selection.

### Loop
Structured iteration node. Raw graph cycles are invalid unless owned by a structured iteration primitive.

## 2. Value/type primitives

### Bool

### Integer
A semantic integer optionally constrained by a provable range. Physical bit width may be compiler-selected.

### Float
A semantic floating-point value with declared precision/error semantics. Physical representation may be compiler-selected where allowed.

### Text
Valid Unicode text; default canonical external encoding profile is platform-defined.

### Bytes
Opaque binary data.

### Array<T,N>

### Slice<T>

### Vector<T,N>
Semantic vector, not an ISA intrinsic.

### Record
Named product type.

### Variant
Tagged sum type.

### Option<T>

### Result<T,E>

### Reference<T>
Verified reference to another resource/entity.

### Secret<T>
Sensitive value with non-export/log/debug restrictions by default.

### Credential<T>
Non-readable authentication secret; supports verification, not value recovery.

## 3. State and memory primitives

### Value
Immutable by default.

### Unique<T>
Single-owner value/resource.

### Borrow<T>
Temporary non-owning access.

### Shared<T>
Explicit shared ownership.

### Region
Lifetime/arena boundary.

### State<T>
Explicit mutable state.

### Atomic<T>
Atomic shared state with defined operations.

### Versioned<T>
State guarded by generation/version checks.

## 4. Effect primitives

Pure computation has no effect primitive.

Effect classes:

- MemoryWrite
- Storage
- Network
- Clock
- Entropy
- Device
- Process
- RemoteExecution
- Accelerator
- Audit

Effects must be explicit in graph contracts.

## 5. Authority primitives

### Identity
Stable identity claim.

### Principal
Identity plus current authenticated context.

### Capability<Action, Resource, Scope>
Authority to perform a specific action over a specific resource/scope.

### Role
Named bundle of capabilities. Application logic should depend on capabilities rather than comparing role names.

### Policy
Rule that derives allow/deny from principal, resource, relation and scope.

### Scope
Authority boundary such as tenant, project, conversation, account or resource set.

### Relation<A,B>
Typed relationship usable by authorization.

### Resource<T>
Policy-bearing entity.

### AuditEvent
Structured security/privileged-operation record.

## 6. Concurrency primitives

### Task
Schedulable graph execution unit.

### Channel<T>
Typed message transfer.

### Actor<T>
Serialized ownership of mutable state.

### Exclusive<T>
Exclusive mutable access.

### Barrier
Explicit synchronization point.

### Transaction<T>
Atomic logical operation over transactional state/storage.

### Lease<T>
Time/generation-bounded distributed ownership.

No arbitrary lock primitive is part of the initial high-level Core.

## 7. Flow/communication primitives

All external communication is secure by platform contract.

### SecureConnection
Authenticated/confidential/integrity-protected connection contract.

### Stream<T>
Ordered stream abstraction.

### Datagram<T>
Message/datagram abstraction.

### Request<I,O>
Request/response interaction contract.

### Event<T>
One-way event contract.

### Session<T>
Authenticated stateful interaction context.

Concrete protocol names are not Core primitives.

## 8. Storage primitives

### Store
Logical storage capability.

### Entity
Stored typed resource.

### Field<T>
Typed entity field.

### Index
Declarative lookup/index intent.

### UniqueConstraint
Uniqueness invariant.

### Constraint
General storage invariant.

### Relation
Verified entity relation/reference.

### Transaction
Atomic storage mutation.

### EncryptionPolicy
Field/resource protection requirement.

### StoragePolicy
Authorization/isolation policy enforced by the storage implementation.

Storage queries are graph operations, not raw SQL in Core.

## 9. Execution and code-loading primitives

### Import<T>
Makes definitions/interfaces available. Never executes.

### Instantiate<T>
Creates an instance/resource without granting undeclared authority.

### Execute<T>
Explicit execution capability.

### LocalExecute<T>

### RemoteExecute<Target,Artifact>

### ArtifactId

### ContentHash

### SignatureIdentity

Remote code execution requires identity, integrity verification, declared interface, capability grant and execution scope.

## 10. Optimization primitives

### Fixed<T>
Immutable parameter.

### Bounded<T>
Parameter the optimizer may choose within explicit bounds.

### Tunable<T>
A bounded/searchable value exposed to the tuning engine.

### Free<T>
Implementation choice left to the compiler under equivalence constraints.

### Constraint
Hard requirement.

### Objective
Optimization target.

### Benchmark
Measured workload/fitness source.

### OptimizationProfile
Set of objectives, constraints, hardware/workload identity and selected values.

Optimization algorithms are toolchain implementation details, not language semantics.
