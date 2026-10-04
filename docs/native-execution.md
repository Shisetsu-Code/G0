# Native bootstrap execution

`g0c run program.g0p` executes GIR directly in the native Rust host. `compile`
continues to emit System V x86-64 assembly through the existing compiler.
The optimized scalar path emits machine arithmetic directly. Programs using
aggregate values now use a separate native backend: graph calls, branches,
loops and maps execute as machine control flow, with bounded value primitives
supplied by a linked runtime library. See `native-aggregate-abi.md`.

The executor supports scalar arithmetic, checked conversion, explicit truncation,
calls, selection, variant matching and bounded loops. Runtime failures carry graph
and node identities without copying input values into diagnostics. Capabilities
declared by nodes are requirements, never grants. Only an explicit host grant can
authorize an effect. The host's returned values must fit every output contract.

Execution has cumulative step and allocation budgets, a maximum call depth and a
cancellation token. Accounting conservatively includes graph bookkeeping and
immutable payloads, including shared values again when they cross output ports.
It is an execution allocation bound, not a measurement of the process RSS.

## Values and operations

Arrays, slices and vectors have immutable shared payloads. `MakeArray` constructs
them; `Index` returns `Option` for an array/byte access; `Length` counts array
elements or bytes. Text cannot be byte-indexed through these operations.

`TextConcat`, `BytesConcat`, `EncodeUtf8`, `DecodeUtf8` and `FormatInteger` make
text/binary boundaries explicit. `DecodeUtf8` returns `Result<Text, Bytes>` and
preserves invalid bytes as the error value.
`UnwrapOr` accepts either Option or Result: it yields a present/success payload,
or the explicitly supplied fallback. The error payload remains an error value
until the graph deliberately chooses this operation.

G0G 0.8 adds checked `BytesSlice` (start and length), `ArrayConcat` (two
collections into a dynamic typed slice), `Range` (0 through count-1) and
`BytesFromArray` (statically constrained 0..255 elements). Slicing rejects
negative, overflowing and out-of-bounds offsets while accepting an empty slice
at the end. Range and byte construction charge allocation before reserving
storage and consume cumulative steps while constructing their elements.

G0G 0.9 adds `CheckedAdd`, `CheckedSub` and `CheckedMul`. These accept Integer
inputs and return `Result<Integer(i128::MIN,i128::MAX),Bool>`: successful values
are exact; overflow returns `Err(true)`. They allow graph algorithms to examine
overflow explicitly without weakening the interval proofs of ordinary arithmetic.
`ResultIsOk` returns the discriminant of any Result as Bool without unwrapping
its payload. Both the executor and native value primitives implement these rules.

Native compilation also selects the value backend for Integer ranges extending
beyond signed i64. The scalar compatibility entry checks i64 representability;
typed hosts retain the full i128 value. `g0_runtime_integer128(result, limbs, 2)`
writes two uint64 limbs in low/high order, preserving the signed two's-complement
representation. An invalid result or short buffer returns failure without writes.

`MakeRecord { schema, fields }` maps fields to inputs ordered by port ID. Required
fields must be supplied and every field type is checked against the program's
single schema registry. `Field` returns the field value or `Option` for an
optional field. `MakeVariant { schema, tag }` supplies one typed payload;
`VariantPayload` returns an optional payload for a tag. Existing `Match` selects
by tag and passes its explicitly declared payload ports to the selected branch.

`Some`, `None`, `Ok`, `Err` and `UnwrapOr` construct and inspect explicit results.
Secret and credential values redact debug output; public serialization rejects
their types, including sensitive types in absent optional fields.

## Regions and tasks

`RegionArena` owns buffers through non-clonable handles. Foreign handles, closed
regions and out-of-range writes are rejected. Child allocations consume ancestor
budgets. Closing a parent closes descendants. Rust borrows prevent write/release
while a buffer view remains borrowed. Secret regions inherit their restriction,
wipe buffers before release and reject moving their contents into an ordinary
owned buffer; their debug views redact contents. Host code that explicitly
borrows secret bytes is trusted to preserve that restriction.

`TaskGroup` starts native OS threads with reserved child budgets and attenuated
capabilities. Children cannot acquire grants absent from the parent. Groups share
cancellation and join all children on drop. Join returns typed results or an
explicit error. Budgets are reserved cumulatively and are not recycled on join.

GIR region/buffer handles and pure task spawn/join execute through ResourceHost;
see `graph-resources.md`. Child and secret region constructors execute with
inherited quotas/lifetimes and protected read types. Scoped child tasks execute
owned region effects, enforce handle containment and reserve both child quotas.
Hosted child tasks accept explicitly delegated effects through a per-child host
factory; the storage factory stages and commits each child's own transaction.
See `graph-resources.md` for delegation and independent-commit boundaries.

## Canonical formats

G0G 0.2 adds operation tags 30..48, in the order listed in `Operation` from
`MakeArray` through `TextJoin`. `Map` applies a closed graph to each collection
element in order with shared authority and budgets; `TextJoin` measures its
output before allocation. Existing tags and semantic types remain unchanged.
The decoder retains support for G0G 0.1.

G0G 0.3 adds region/task operations 49..55 and relation operations 56..57.
G0G 0.4 adds credential operations 58..59; readers retain versions 0.1..0.3.
Older version headers cannot introduce the new operation tags.
G0G 0.5 adds secret-root and child-region constructors (60..61), preserving all
previous operation encodings and retaining readers for 0.1..0.4.
G0G 0.6 adds scoped task spawn/join (62..63) and retains readers for 0.1..0.5.
G0G 0.7 adds explicitly hosted task spawn/join (64..65), retaining 0.1..0.6.
G0G 0.8 adds collection primitives (66..69), retaining 0.1..0.7.
G0G 0.9 adds checked arithmetic and ResultIsOk (70..73), retaining 0.1..0.8.

G0G 0.10 adds `DecodeInteger128Le` (opcode 74, no operation payload).
It consumes exactly 16 Bytes in little-endian two's-complement order and
produces an Integer whose declared range is exactly i128.MIN through i128.MAX.
Other input lengths fail with `Bounds`; narrow output ranges fail validation.
The pure operation uses the existing execution budgets and cancellation checks,
including the native backend's generic primitive dispatch. Versions 0.1..0.9
reject opcode 74; their existing operations remain accepted by the 0.10 decoder.

G0P 0.2 appends a schema registry after the graph list. It consists of a u32 count;
each schema has a u32-length UTF-8 name, u32 version and u32 field count. Each field
has u32 tag, u32-length UTF-8 name, u32-length semantic type encoding and a one-byte
requirement (0 required, 1 optional). Schema names are sorted; fields use ascending
stable tags. G0P 0.1 remains readable with an empty registry.

G0P 0.3 uses the same representation and permits typed entry inputs and multiple
outputs. Versions 0.1 and 0.2 retain their original entry interface restriction.
See `compiler-bootstrap.md` for the executable G0 backend and its current limits.

G0V 0.1 contains `G0V\0`, u16 major 0, u16 minor 1, a u32-length semantic type
encoding and a value. Integers use 16 little-endian bytes; booleans use 0/1;
text/bytes use u32 byte length; collections use u32 count; optional values use
0/1; results use 0 for success and 1 for error. Records carry schema version,
field count and ascending tags followed by schema-directed values; variants carry
schema version, selected tag and payload. No redundant DTO or generic JSON model
is introduced. Decoding checks expected types, versions, canonical order, bounds,
UTF-8, recursion, element counts and trailing bytes.

## Storage profile

`NativeStore` implements encrypted snapshots, exclusive process locking,
serializable optimistic transactions, explicit rollback, version conflicts,
tenant keys, resource/field policy checks, immutable and managed fields and unique
indexes. Commits never retry implicitly. Authorization epoch changes invalidate
transactions. Snapshots authenticate content with AES-256-GCM through pinned
`ring`; roots and keys are explicitly supplied by the host. Snapshots are bound
to their canonical directory in authenticated associated data, so moving a store
requires an explicit migration. Temporary files are flushed before replacement;
Unix synchronizes the directory and Windows uses write-through replacement.
An uncertain commit requires reopening before further operations.

The store supports public, private, secret and credential fields, plus
principal/scope-managed fields. Generated/clock-managed fields remain rejected.
See `protected-storage.md` for field envelopes, verifier-only credentials and
explicit permissions. Optional `open_with_witness` detects restoration against
an independently trusted monotonic witness supplied by the host; unanchored
snapshots alone cannot detect restoring older valid bytes. See `store-witness.md`.

Persistent relations now enforce target/scope/cardinality constraints and
authorized Restrict/Detach/Cascade deletion. GIR can set and traverse relations
in its storage transaction. G0S 0.4 preserves G0S 0.1..0.3 reading and upgrades on
commit. See `store-relations.md` for interfaces and resource limits.

Explicitly authorized offline migrations execute pure G0 transformation graphs
with cumulative budgets, check the complete result and atomically replace the
snapshot. The schema commitment rejects obsolete definitions on reopen; see
`store-migrations.md` for supported changes and failure behavior.

Native transport uses a separately selected platform profile. The pinned
cryptography dependencies are bootstrap platform implementations, not G0 Core
semantics. Cargo.lock records their exact transitive versions.

The implemented transport profile requires mutual certificate authentication,
explicit trust roots, TLS 1.3 and ALPN `g0.native.v1`. It disables early data and
session tickets. Certificate SHA-256 fingerprints identify peers; the service
maps them to explicitly provisioned principals. A frame carries `G0N\0`, a u64
sequence and a u32 G0V payload length. Per-direction sequences reject replays and
reordering within a session. Limits apply before allocation; handshake and frame
I/O have absolute deadlines. A malformed/partially consumed frame closes the
channel. No automatic reconnect or mutation retry is supplied. These mechanisms
do not implement connection migration, datagram or multipath profiles.

The host can explicitly select `TlsProfile::HybridRequired`, which permits only
X25519MLKEM768 through pinned rustls/AWS-LC. A classical-only peer fails the
handshake; no classical fallback is offered. Peer certificates retain their
existing classical authentication algorithms. This profile protects the hybrid
key exchange and does not claim post-quantum certificate signatures. See
[rustls key-exchange documentation](https://docs.rs/rustls/0.23.43/rustls/crypto/aws_lc_rs/kx_group/index.html).

`native_transport::multiplex::MultiplexedChannel` interleaves up to 128
host-provisioned typed streams over one authenticated connection. Both hosts
configure matching stream identifiers/types and per-stream byte/message limits;
the peer cannot provision a new stream by sending an identifier. Each stream
has its own monotonically checked sequence and half-close state. Half-closing
one send direction leaves other streams and the reverse direction available.
Canonical envelopes remain inside the encrypted G0N framing and deadlines;
unexpected identifiers, sequence, payload types or post-close frames close the
connection. Receive returns the next stream event without unbounded per-stream
queues or hidden retries. This profile does not bypass TCP head-of-line blocking
or implement dynamic stream negotiation.
Per-stream message quotas count data values; one final half-close remains
available after that quota. The connection's global frame quota counts both
data and control frames and must include any planned close frames.

`StorageHost` binds GIR storage operations to one explicitly committed host
transaction. Create/update return the prospective commit version. Reads may
depend on that version through an explicit graph edge. `GraphService` checks the
authenticated fingerprint before receiving an application value, executes its
selected graph, validates output types, durably commits and only then sends its
reply. A failed execution drops its staged writes. A failed send after commit is
an explicit uncertain acknowledgment, not grounds for an implicit retry.
