# Native graph editor

On Windows, `cargo run --bin g0-editor -- [document.g0g|program.g0p]` opens a
Win32/GDI graph editor. Loading reconstructs definitions without executing them.
The editor can create and configure operations, semantic types, graph/node
interfaces, effects, capability requirements and schema declarations. It connects
typed ports, preserves explicit imported port IDs, switches program graphs,
supports undo/redo, validates and saves canonical documents. Invalid intermediate edits remain
editable; validation must succeed before saving or execution.

Select an output port, then an input port to connect them. Graph outputs are
ports too. Drag nodes to arrange them and use the mouse wheel to scroll. Open
and save use native file dialogs. Node arrangements persist in a bounded atomic
layout sidecar; `.g0g` and `.g0p` preserve semantic graphs.

Run executes the selected graph with no arguments or granted capabilities. The
result and bounded trace show graph names, node IDs and typed outputs. Live
worker debugging supports pause, step, breakpoints, continue and stop, including
cancellation while paused. Recorded trace navigation remains available. Credential and
secret values are redacted in diagnostics. Input graphs or effects needing host
authority must be executed through the explicit runtime APIs.

The toolbox supplies common pure-operation presets; the generic forms cover
all supported operations and types. Literal editing preserves bounded long text
and hexadecimal bytes without truncation. See [the detailed editor guide](editor-native.md)
for form syntax, limits and host-authority boundaries. The graphical
frontend is currently Windows only; the graph editing model is platform neutral.

`g0-editor --smoke-test canvas.bmp graph.g0g` runs a hidden native window,
constructs and saves a graph computing 42 + 7, invokes its real execution command,
and renders its own canvas through GDI. It neither captures another application
nor installs any system settings. Model tests verify canonical round trips,
undo/redo, invalid edits, and default-deny loading.
