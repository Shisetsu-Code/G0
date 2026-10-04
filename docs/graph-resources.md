# Graph resources

G0G 0.3 adds `RegionOpen`, `RegionAllocate`, `RegionWrite`, `RegionRead`,
`RegionClose`, `TaskSpawn` and `TaskJoin` (tags 49..55). Older graph versions
retain their original operation sets; a new operation under an old version
header is rejected. G0P 0.3 can contain these graphs with typed entry arguments.

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
| RegionClose | region | Bool |

Region nodes declare `MemoryWrite`. Their dependency edges order mutable
operations. Reads make owned copies that remain valid after closing the region.
Copies and allocations are charged before allocation. Native Rust APIs also
support child and secret regions; GIR exposes public root regions in this stage.

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

`ResourceHost` bounds a scope to 16,384 cumulative opaque handle identities.
Its resource allocation quota includes region copies, conservative metadata
charges, and reserved child task memory. The Executor's graph-value quota is
separate; the caller configures both by supplying explicit limits. Cancellation
is shared with child tasks. `ChainedHost` routes region/task nodes to that scope
and other effects to a supplied storage host. CLI run, editor execution, native
wrappers and graph services use this resource host with explicit/default-deny
capabilities.
