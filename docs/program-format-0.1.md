# Native executable container 0.1

`.g0p` is the bootstrap native executable file format. It packages existing
canonical GIR graphs without introducing a textual language or external runtime.
Its compiler target is the current x86-64 bootstrap profile.

## Binary framing

All integers in the container framing are unsigned and little-endian.

| Field | Encoding |
| --- | --- |
| Magic | Four bytes: `G0P` followed by zero |
| Major version | u16, currently 0 |
| Minor version | u16, currently 1 |
| Entry name | u32 UTF-8 byte length followed by name bytes |
| Graph count | u32 |
| Each graph | u32 byte length followed by a `G0G` 0.1 graph blob |

The encoder sorts definitions by graph name and uses the existing canonical
graph encoder for each payload. Definition order has no execution semantics.
Readers accept valid definition orderings and reject trailing bytes. Reencoding
produces the canonical ordering.

## Validation boundary

- Entry name is mandatory, nonempty and resolves to exactly one definition.
- Graph names are unique. All referenced graphs are present in the bundle.
- Existing graph, call-signature, recursion, control and program validation
  apply to every definition, including unreachable ones.
- The bootstrap executable entry has zero input ports and exactly one output.
  Compilation additionally requires implemented machine lowering for its type.
- Version, UTF-8, framing and embedded graphs are validated before compilation.
  Counts impossible for the remaining payload are rejected before collection
  reservation. Truncated input never yields a partial executable document.
- Only definitions reachable from the entry are emitted by the native program
  compiler; closed-bundle validation still considers all definitions.

`ProgramDocument` deliberately contains only entry and graph definitions.
It is not a serializer for all fields of `ProgramContract`; there is no silent
loss of storage schemas, ownership plans, external bindings or policy sections.
Those require explicit future format contracts. There are no library bundles,
external subgraph bindings or implicit entry arguments in version 0.1.

## Execution and authority

Decoding and `g0c check` never execute nodes. Capability requirements embedded
in graphs remain requirements, not grants. The container grants no runtime
authority and does not implement storage or network effects by itself.

`g0c compile` lowers the validated bundle through the existing native program
compiler and exports `g0_machine_main`. Files can be recognized by magic even
with another extension. Known magic takes precedence over a conflicting native
suffix; a damaged `.g0p` still receives a native-program decoding error.

## Examples and regression coverage

`call.g0p` calls a local worker and returns 42. `select.g0p` selects a branch
and returns 42. `loop.g0p` starts with 7, resets the state to 0 in its body,
checks the condition again and returns 0. The CLI test suite assembles and
executes their output on Linux/x86-64. Roundtrip/canonicalization tests also
cover missing/duplicate entries, invalid entry interfaces, missing definitions,
every truncated prefix, invalid lengths/versions and malformed graph payloads.
