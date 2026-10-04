# Native graph editor

On Windows, `cargo run --bin g0-editor -- [document.g0g|program.g0p]` opens a
Win32/GDI graph editor. Loading reconstructs definitions without executing them.
The editor can create integer constants and arithmetic nodes, connect typed
ports, delete nodes, change integer literals, switch program graphs, undo/redo,
validate, and save canonical native documents. Invalid intermediate edits remain
editable; validation must succeed before saving or execution.

Select an output port, then an input port to connect them. Graph outputs are
ports too. Drag nodes to arrange them and use the mouse wheel to scroll. Open
and save use native file dialogs. The current node arrangement is session state;
`.g0g` and `.g0p` preserve semantic graphs, not screen coordinates.

Run executes the selected graph with no arguments or granted capabilities. The
result and bounded trace show graph names, node IDs and typed outputs. Next
trace traverses recorded events; it is not a live breakpoint. Credential and
secret values are redacted in diagnostics. Input graphs or effects needing host
authority must be executed through the explicit runtime APIs.

The initial toolbox constructs constants and arithmetic. Imported operations
remain visible and connected by their declared ports; constructors for the rest
of the language and live stepping are subsequent editor work. The graphical
frontend is currently Windows only; the graph editing model is platform neutral.

`g0-editor --smoke-test canvas.bmp graph.g0g` runs a hidden native window,
constructs and saves a graph computing 42 + 7, invokes its real execution command,
and renders its own canvas through GDI. It neither captures another application
nor installs any system settings. Model tests verify canonical round trips,
undo/redo, invalid edits, and default-deny loading.
