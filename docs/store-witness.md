# Native snapshot rollback witness

`NativeStore::open_with_witness` binds an encrypted snapshot to a caller-supplied
`RollbackWitness`. The witness stores a generation and SHA-256 digest of the
exact authenticated snapshot bytes. Its namespace must identify one store, and
its provider must offer durable atomic compare-and-exchange in a trust domain
that resists rollback independently of the store directory. A normal local file
copied/restored with the snapshot cannot supply that property. This runtime
implements the witness protocol; deployment supplies its trusted provider.

Opening checks exact equality, including the digest for the same generation.
A missing witness can initialize only a new empty store. An existing snapshot
cannot silently enroll itself as trusted. A missing/restored/changed snapshot,
or a snapshot ahead of the witness, produces `RecoveryRequired`.

Commit writes and synchronizes the snapshot first, then advances the witness
from the previous checkpoint to the new one before acknowledging success.
If the witness fails or detects a conflicting checkpoint, commit returns
`CommitUncertain` and poisons the open instance. Reopening requires explicit
trusted recovery; it never infers that the ahead snapshot should be approved.
A crash between the two durable writes likewise requires recovery. The admin
must establish the intended snapshot and witness checkpoint through its trusted
provider before opening again. No automatic retry or rollback repair occurs.

The unanchored `open` profile remains available for deployments that do not
require rollback detection. It authenticates ciphertext and enforces ordinary
transaction versions but cannot detect restoration of an older valid snapshot.

Tests use an independently held witness to exercise actual snapshot restoration,
file deletion, durable-but-unacknowledged commit failure, poisoning and reopen
rejection. The memory witness is a test provider, not a production trust anchor.
