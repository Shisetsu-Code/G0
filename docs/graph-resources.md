# Graph resources

G0G 0.3 adds `RegionOpen`, `RegionAllocate`, `RegionWrite`, `RegionRead`,
`RegionClose`, `TaskSpawn` and `TaskJoin` (tags 49..55). Older graph versions
retain their original operation sets; a new operation under an old version
header is rejected. G0P 0.3 can contain these graphs with typed entry arguments.
G0G 0.5 adds `RegionOpenSecret` (60) and `RegionOpenChild { secret }` (61).
The child secrecy flag has canonical values 0/1; inheritance can never weaken a
secret parent.

Region handles have type `Unique<Reference<g0.region>>`; buffer handles have
type `Unique<Reference<g0.buffer>>`. Values are opaque: literals and codecs cannot
mint or serialize them, diagnostic output does not reveal their identity, and
each operation consumes its input handles. The next operation receives renewed
handles. The host rejects consumed handles, foreign hosts, mismatched regions,
closed regions and out-of-bounds access. Graph validation rejects fan-out of
Unique values, including handles nested in named records or variants.

The public-region profile has these interfaces:

| Operation | Inputs | Outputs |
| --- | --- | --- |
| RegionOpen | nonnegative byte quota | region |
| RegionAllocate | region, nonnegative size | region, buffer |
| RegionWrite | region, buffer, nonnegative offset, Bytes | region, buffer |
| RegionRead | region, buffer | region, buffer, owned Bytes |
| RegionClose | region, optional Bool sequencing dependency | Bool |

Region nodes declare `MemoryWrite`. Their dependency edges order mutable
operations. Reads make owned copies that remain valid after closing the region.
Copies and allocations are charged before allocation.

`RegionOpenSecret` has the root-open interface but returns
`Unique<Reference<g0.secret-region>>`. Secret buffers have the corresponding
`g0.secret-buffer` kind. The same allocate/write/read/close operations preserve
these kinds; writes require `Secret<Bytes>` and reads return `Secret<Bytes>`.
Validation rejects declaring the read result as public Bytes. Protected read
copies remain redacted and cannot use public transport/value codecs. They are
immutable application values; their lifetime is distinct from the arena's owned
buffers and does not promise wiping on drop. Owned arena buffers are wiped when
their secret region closes or the resource scope is destroyed.

`RegionOpenChild` consumes a parent and a byte quota, returning a renewed parent
and a new child. A secret parent always produces a secret child; a public parent
can explicitly request a secret child. Child allocations consume every ancestor
quota, and closing a parent closes all descendants. An optional Bool dependency
on close orders parent destruction after a child's last operation without
duplicating its linear handle.

TaskSpawn refers to a closed pure graph and reserves explicit cumulative child
step and allocation budgets. The output is
`Unique<Reference<g0.task:BODY>>`. It requires the exact host capability
`LocalExecution("spawn", BODY, "tasks")`; joining requires the corresponding
`"join"` capability. Declarations grant no authority. Children execute without
ambient capabilities, and joining consumes the handle once. Child runtime
errors preserve their original graph/node attribution. Scope destruction
cancels and joins remaining children.

Optional trailing Bool sequencing inputs/outputs order task effects separately
from payload values. This allows two tasks to be spawned before either is
joined while preserving an explicit effect order. The child receives only its
declared payload inputs. Children performing host effects require an additional
task execution profile; this profile deliberately accepts pure child graphs.

G0G 0.6 adds `TaskSpawnScoped` and `TaskJoinScoped` (62..63). Their bodies may
perform MemoryWrite operations in a separately owned ResourceHost. The task
kind is `g0.scoped-task:BODY` and exact capabilities use `spawn-scoped` and
`join-scoped`. Both nodes declare LocalExecution plus the body's MemoryWrite
summary. Child steps remain bounded and allocation reservations cover both the
child executor and resource scope (twice the requested value-byte quota).
No parent grants are inherited. External host effects require a separate
explicit child-host binding and are rejected by this profile.

Each scoped child owns and destroys its regions, preserving secret wipe behavior.
Native handles cannot be passed into or returned from scoped tasks, including
handles hidden inside records, variants or protected wrappers. Returned owned
values remain valid after scope destruction; a handle escape fails explicitly.
Cancellation comes from the parent task group; destroying a completed child's
private resource group does not cancel its parent or siblings.

`ResourceHost` bounds a scope to 16,384 cumulative opaque handle identities.
Its resource allocation quota includes region copies, conservative metadata
charges, and reserved child task memory. The Executor's graph-value quota is
separate; the caller configures both by supplying explicit limits. Cancellation
is shared with child tasks. `ChainedHost` routes region/task nodes to that scope
and other effects to a supplied storage host. CLI run, editor execution, native
wrappers and graph services use this resource host with explicit/default-deny
capabilities.
