# North Neeladesh Ministry of Communications

Offline MVP of the Cabinet Frontend and Rust authority backend.

## Use

```sh
just check      # Rust tests and Odin frontend check
just build      # Release-build both programs
```

Start the backend in one terminal, then the frontend in another:

```sh
just backend
just frontend
```

The frontend and backend communicate only over loopback TCP (`127.0.0.1:48129`);
no external network access is required.
