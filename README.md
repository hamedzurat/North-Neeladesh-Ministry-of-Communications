# North Neeladesh Ministry of Communications

Offline MVP of the Cabinet Frontend and Rust authority backend.

## Use

```sh
just check      # Rust tests and Odin frontend check
just build      # Release-build both programs
just mvp        # Start backend and frontend together
```

To run the processes separately, start the backend in one terminal, then the
frontend in another:

```sh
just backend
just frontend
```

`just mvp` starts the same two local processes: it runs the Rust backend in the
background, opens the Odin frontend, and stops the backend when the frontend
exits. The frontend and backend communicate only over loopback TCP
(`127.0.0.1:48129`); no external network access is required.
