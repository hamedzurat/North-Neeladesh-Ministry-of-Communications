# Debug surface

The debug dashboard observes the authoritative telephone exchange backend. It
shows the current Run, Shift, Calls, routing wires, printer output, frontend
transport, voice status, shared story mechanics, the retained event journal,
and diagnostics.

The event journal records accepted and rejected input snapshots, call changes,
ring readiness, story transitions, text turns, debug toggles, and failures. It
is reset with the run because it belongs to that run's evidence.

Use `just backend-debug` and `just debug-surface` for local development. The
dashboard's reset and time controls are intended for isolated checks; normal
play proceeds through the physical routing controls.

`just content-validation` runs the neutral exchange integration tests.
