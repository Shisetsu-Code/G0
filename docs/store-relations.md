# Persistent graph relations

NativeStore implements schema-declared relations in the same serializable
transaction as record values. `set_relation` replaces a source relation's
targets; `traverse` returns typed target records from the transaction snapshot
and staged changes. Target IDs resolve through the caller's scope and the
declared target resource. Duplicate targets are errors.

Updating links requires the source resource's Update policy and each target's
Read policy. Traversal checks the source's Read policy and applies normal target
field/resource authorization before returning records. Permissions are checked
inside the store, including when GIR invokes these methods.

Commit validates target existence, resource kind, scope isolation and One,
OptionalOne or Many cardinality. Required One relations must be populated before
commit; a transaction may construct records and links together. Relation changes
update the source's version and participate in global generation conflicts.

Deleting a referenced record follows Restrict, Detach or Cascade. Every cascaded
deletion and detached source is authorized separately. The complete plan is
staged only after all authorizations succeed, so a rejected deletion leaves
previous transaction mutations unchanged. This profile bounds a cascade to 128
entities before invoking the recursive contract planner. Cascade cycles and
constraint failures are explicit errors. The snapshot bounds links to 100,000.

G0G 0.3 tags 56 and 57 expose these operations:

| Operation | Inputs | Output | Exact storage capability |
| --- | --- | --- | --- |
| StoreSetRelation | source Text ID, Slice<Text> target IDs, optional version dependency | prospective Integer version | link, source resource, principal scope |
| StoreTraverse | source Text ID, optional version dependency | Slice<Record<Target>> | traverse, source resource, principal scope |

Both declare Storage and execute in StorageHost's explicit transaction. Version
dependency inputs order effects; the transaction's own generation and security
epoch determine freshness. The caller commits only after successful execution.

G0S 0.2 appends canonical relation metadata to each encrypted row. Names and target
keys are sorted, duplicate/noncanonical input is rejected, and aggregate lengths
are bounded before allocation. The AEAD associated data binds the snapshot
version and canonical store path. Readers retain G0S 0.1 support; a successful
commit upgrades the envelope. Modification detection does not by itself provide
a trusted anti-rollback witness.
