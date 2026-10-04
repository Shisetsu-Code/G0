# Executable offline migrations

`NativeStore::migrate` transforms an entire resource inside the already locked
native store. `MigrationPermit` requires an exact Storage capability:
action `migrate:RESOURCE:FROM:TO`, resource equal to the canonical store root,
scope `store`. This is administrative authority over every tenant's affected
records, separate from ordinary record permissions. The opaque permit binds
the root, resource and versions; it cannot authorize a different migration.

`StoreMigration` supplies the new resource/data schemas, a validated offline
migration plan, a canonical `ProgramDocument`, limits and cancellation. The
entry consumes the previous record under an explicitly named source-schema
alias and returns the new resource record. The source definition must exactly
match the previous data schema except its alias; new definitions must match the
target registry. Pure G0 operations perform field extraction, construction and
backfill. All program graphs must have no effects or capability requirements.

The current profile supports field additions/backfills, explicit stable-tag
renames, explicit destructive removals, index rebuilding and existing-type field
protection changes to `g0.native.fields` version 1. Required additions need the
declared add/backfill/promote sequence. Type changes, online mode and changes to
tenant isolation or managed-field bindings are rejected. Managed values must
remain unchanged; credential outputs must remain opaque verifiers. Recursive
records referencing the migrated type require suitable source-schema modeling;
the engine never coerces a value that does not fit the declared graph input.

One validated executor processes every record with cumulative allocation/step
budgets and shared cancellation. Record IDs, ownership, scopes and relation
metadata are preserved. All values, unique indexes and relations are checked
before any replacement. On success every transformed row gets the next version,
the snapshot is durably replaced, an optional witness advances and the security
epoch invalidates pre-migration transactions. Any validation/runtime failure
leaves the old schema, snapshot and existing transactions in place. A durable
write with uncertain acknowledgement instead poisons the instance and requires
explicit recovery under the resulting schema.

G0S 0.4 adds an authenticated 32-byte schema/structural-store commitment after
the generation and before the row count. It binds canonical data schemas and
versions, protections, tenancy, managed bindings, relations and index intents.
Policy expressions and mutability remain explicit host authorization bindings.
Even an empty resource cannot silently reopen under an obsolete data schema.
Readers retain 0.1..0.3 support and upgrade on the next successful commit.

Tests migrate real persisted records to a required private field, reopen with
the target schema and reject the obsolete schema. Missing authority, undeclared
changes, cancellation and exhaustion after an earlier transformed record all
preserve the original file. Pre-migration transactions cannot commit afterward.
