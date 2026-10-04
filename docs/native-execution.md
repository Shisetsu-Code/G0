# Native bootstrap execution

`g0c run program.g0p` executes GIR directly in the native Rust host. `compile`
continues to emit System V x86-64 assembly through the existing compiler.
The two paths are distinct: aggregate operations currently execute in the host;
their x86 lowering is not implemented and compilation rejects them explicitly.

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

These host APIs are executable. GIR region allocation/mutation operations and
task handles are still a separate integration step; static ownership plans are
not silently treated as runtime allocations.

## Canonical formats

G0G 0.2 adds operation tags 30..48, in the order listed in `Operation` from
`MakeArray` through `TextJoin`. `Map` applies a closed graph to each collection
element in order with shared authority and budgets; `TextJoin` measures its
output before allocation. Existing tags and semantic types remain unchanged.
The decoder retains support for G0G 0.1.

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

The initial store profile supports public application fields protected by store
encryption, and principal/scope-managed fields. Protected-field/credential
profiles, relation integrity and generated/clock-managed fields are rejected
until their executable implementations are supplied. Snapshot authentication
detects modification; protection against restoring an older valid snapshot
requires a separately trusted monotonic witness and is not yet implemented.

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
do not implement migration, multiplexing or post-quantum profiles.

`StorageHost` binds GIR storage operations to one explicitly committed host
transaction. Create/update return the prospective commit version. Reads may
depend on that version through an explicit graph edge. `GraphService` checks the
authenticated fingerprint before receiving an application value, executes its
selected graph, validates output types, durably commits and only then sends its
reply. A failed execution drops its staged writes. A failed send after commit is
an explicit uncertain acknowledgment, not grounds for an implicit retry.
