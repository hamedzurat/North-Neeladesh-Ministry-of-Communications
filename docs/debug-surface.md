# Debug surface

The debug dashboard observes the authoritative telephone exchange backend. It
shows the current Run, Shift, Calls, routing wires, printer output, frontend
transport, voice status, and diagnostics.

Use `just backend-debug` and `just debug-surface` for local development. The
dashboard's reset and time controls are intended for isolated checks; normal
play proceeds through the physical routing controls.

`just content-validation` runs the neutral exchange integration tests.
