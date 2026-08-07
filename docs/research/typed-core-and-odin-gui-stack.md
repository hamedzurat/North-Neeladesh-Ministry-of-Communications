# Typed core and Odin GUI stack options

Research date: 2026-08-08

## Question

Which combination of Odin, Rust, C++, GUI libraries, build tools, IPC, and packaging best supports this game's authoritative single-writer core, Odin development GUI, local AI runtimes, Linux deployment, testing, and later ESP32 frontend?

## Recommendation

Use this as the **provisional default**, subject to the proof spikes below:

- **Rust authoritative backend:** a pure deterministic game-domain crate plus a Tokio host executable. The host is the only writer of live state and owns timers, persistence, frontend connections, AI-job lifecycle, and cancellation.
- **Odin GUI simulator:** a thin client built with Odin's official `vendor:raylib` binding and its bundled `raygui` controls. It sends player commands and renders snapshots/events; it never owns game rules or saves.
- **Local AI worker processes:** run STT, LLM, and TTS in the runtime best supported by each selected engine. Supervise them from the Rust host and use typed, cancellable request/response protocols. These are **local workers, not microservices**: they run on one laptop, have no independent deployment or public network API, and are started/stopped with the game.
- **Content and persistence:** TOML is authored content; SQLite is saves, event journal, and diagnostics. In Rust, use Serde plus `toml` and `rusqlite` with its `bundled` SQLite feature.
- **Transport:** use a versioned logical protocol independent of transport. Start with length-prefixed JSON over loopback TCP for the Odin GUI and framed stdio for AI workers. Add a serial transport adapter for the ESP32 later. Keep golden protocol fixtures shared by all implementations.
- **Deployment:** no Docker in the live path. Pin the Rust toolchain and `Cargo.lock`, pin a monthly Odin release, vendor any non-official Odin dependencies, and produce one self-contained release directory plus a launch/health-check script for the known Linux laptop.

This division keeps the custom switchboard UI pleasant to build in Odin while assigning async orchestration and dependency-heavy infrastructure to the ecosystem that currently covers them most coherently.

## What Odin actually provides

The premise that Odin has useful GUI support is correct with an important qualification: it is **officially maintained vendor code, not a built-in widget toolkit**.

Odin's official vendor collection includes SDL2/SDL3, GLFW, raylib, miniaudio, and an Odin-native port of microui. The raylib package includes `raygui.odin`; its package API exposes controls such as buttons, sliders, list views, spinners, and text boxes. Raylib itself is intended for prototyping, tooling, graphical applications, embedded systems, and education. This is a good fit for a custom switchboard surface rather than a conventional desktop form application. [Odin vendor collection](https://pkg.odin-lang.org/vendor/), [Odin raylib binding and example](https://github.com/odin-lang/Odin/tree/master/vendor/raylib), [raylib/raygui API](https://pkg.odin-lang.org/vendor/raylib/), [microui package](https://pkg.odin-lang.org/vendor/microui/)

Odin also has official JSON encoding/decoding, process creation with pipeable standard streams, threads/channels, sockets, a non-blocking event-loop package, and a built-in test runner. The test runner tracks memory errors and can run package tests in parallel. These capabilities make an Odin backend possible, not merely a GUI. [core package index](https://pkg.odin-lang.org/core/), [`core:encoding/json`](https://pkg.odin-lang.org/core/encoding/json/), [`core:os` process API](https://pkg.odin-lang.org/core/os/), [`core:nbio`](https://pkg.odin-lang.org/core/nbio/), [Odin test runner](https://odin-lang.org/docs/testing/)

The weak point is infrastructure coverage and dependency workflow. The current official `core` and `vendor` indexes list neither TOML nor SQLite. Those would require a community package, a maintained local wrapper over the C libraries, or moving those responsibilities elsewhere. Odin also explicitly has no official package manager and recommends manual, pinned vendoring. That can be reliable, but it increases project-owned integration and upgrade work. [core index](https://pkg.odin-lang.org/core/), [vendor index](https://pkg.odin-lang.org/vendor/), [Odin package-management FAQ](https://odin-lang.org/docs/faq/#is-there-an-official-odin-package-manager)

For audio, the official miniaudio binding supports playback, capture, full duplex, and callback-driven streaming. That makes Odin capable of a future audio adapter, but it does not by itself solve echo cancellation or true conversational full duplex. [Odin miniaudio binding](https://pkg.odin-lang.org/vendor/miniaudio/)

## Stack comparison

| Shape | Strengths for this project | Main costs and risks | Verdict |
| --- | --- | --- | --- |
| **Rust backend + Odin GUI + native AI workers** | Cargo workspaces and one lockfile; typed TOML/JSON via Serde; bundled SQLite; mature async process, socket, channel, cancellation, and serial libraries; memory-safe long-running host; GUI remains Odin | Two compiled languages and a wire contract; protocol fixtures and integration tests are mandatory | **Recommended provisional default** |
| **All Odin backend + GUI + native AI workers** | Fast iteration in one language; official graphics, audio, JSON, processes, testing, sockets, and non-blocking I/O; easiest sharing of in-memory types | No official TOML/SQLite package; manual dependency vendoring; more application-owned async/serial/database integration; coupling the GUI and core is tempting | **Viable only if the Odin-only spike passes** |
| **C++ backend + Odin GUI + native AI workers** | Direct access to SQLite's C API and many native model runtimes; Boost.Asio supplies a cross-platform async model; toml++ supports TOML 1.0; CMake/CTest cover builds/tests | Highest memory/lifetime risk around asynchronous cancellation and callbacks; dependency/build choices are less unified; still pays the cross-language protocol cost | **Fallback when a required C/C++ library cannot be isolated behind a worker** |

Rust's advantage here is coherence, not raw performance. Cargo workspaces share a lockfile and support workspace-wide checks. Tokio provides an event-driven runtime, bounded multi-producer/single-consumer channels with backpressure, Unix/TCP I/O, process supervision, and async serial adapters. Serde derives typed deserialization; the `toml` crate maps it to TOML, and `rusqlite` recommends its `bundled` feature for applications controlling their own database. [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html), [Tokio](https://github.com/tokio-rs/tokio), [Tokio MPSC](https://docs.rs/tokio/latest/src/tokio/sync/mpsc/mod.rs.html), [Tokio process supervision](https://docs.rs/tokio/latest/tokio/process/struct.Command.html), [Serde `Deserialize`](https://docs.rs/serde/latest/serde/trait.Deserialize.html), [`toml` API](https://docs.rs/toml/latest/toml/type.Table.html), [`rusqlite` bundled mode](https://github.com/rusqlite/rusqlite), [`tokio-serial`](https://docs.rs/tokio-serial/latest/tokio_serial/)

C++ is technically capable: Boost.Asio provides synchronous and asynchronous low-level I/O, toml++ supports TOML 1.0 and C++17, SQLite publishes a directly embeddable amalgamation, while CMake presets and CTest can make builds repeatable. It does not produce a project-specific advantage large enough to offset manual lifetime and build-system complexity when AI engines can be isolated in workers. [Boost.Asio](https://www.boost.org/latest/doc/html/boost_asio/overview/basics.html), [toml++](https://marzer.github.io/tomlplusplus/), [SQLite compilation](https://sqlite.org/howtocompile.html), [CMake presets](https://cmake.org/cmake/help/latest/guide/user-interaction/index.html#presets), [CTest](https://cmake.org/cmake/help/latest/module/CTest.html)

## Recommended module and process shape

```text
content/*.toml
      |
      v
Rust content loader/validator ----> pure game-domain reducer
                                          |
Odin GUI -- frontend protocol --> Rust host (only state writer) <-- ESP32 serial adapter
                                          |
                    +---------------------+--------------------+
                    |                     |                    |
                 SQLite             STT/LLM/TTS workers    audio adapters
              saves/journal        framed local IPC       laptop / future ESP32
```

Suggested source layout:

```text
backend/
  crates/domain/       # state, commands, events, deterministic reducer
  crates/content/      # TOML schemas, references, validation
  crates/protocol/     # versioned semantic messages and fixtures
  crates/storage/      # SQLite migrations, saves, journal
  crates/host/         # Tokio loop, jobs, timers, frontend/AI supervision
frontends/
  gui-odin/            # raylib/raygui rendering and command mapping only
  esp32/               # firmware later; same semantics, serial transport
workers/                # launch adapters/configs for selected local engines
protocol/fixtures/      # language-neutral accepted/rejected message examples
content/                # authored TOML hierarchy
```

The domain reducer should not depend on Tokio, SQLite, GUI, audio, or a model runtime. Given `(State, Command)` it returns validated state changes, events, and requested effects. The host executes slow effects concurrently and feeds results back as new commands containing a request/session ID. A stale STT/LLM/TTS result is therefore rejected by ordinary domain validation after hang-up, cancellation, or circuit change.

Use loopback TCP for the first Odin GUI connection because Odin's official high-level networking package directly supports TCP across Linux, Windows, and macOS; its Unix-domain support is exposed at a lower POSIX layer. Binding only to `127.0.0.1` keeps this local. Tokio supports either TCP or Unix streams, so transport can be changed later without changing domain messages. [Odin `core:net`](https://pkg.odin-lang.org/core/net/), [Tokio networking](https://docs.rs/tokio/latest/tokio/net/)

The frontend and ESP32 protocols should share **message meaning**, not necessarily byte encoding. Examples are `PlugCord`, `SelectOperatorCircuit`, `PressPTT`, `TurnGenerator`, `SetTuner`, `StateSnapshot`, and `LampChanged`. Put `protocol_version`, `session_id`, `sequence`, `kind`, and a typed payload in every envelope. Define reconnect as `Hello -> capability negotiation -> full snapshot -> incremental events`. Do not make SQLite the live frontend API.

Treat microphone capture and speaker playback as host-side adapter ports, separate from the visual/control frontend. Start with laptop audio. A speaker driven through the ESP32 is possible only after measuring the chosen USB/serial link, buffering, DAC/I2S path, and added latency; it must not be assumed by the core architecture.

## Proof spikes before locking the language choice

Time-box these and record measurements. They are decision tests, not production implementation.

1. **Odin GUI proof:** render 16 line jacks/lamps, eight patch cords, a tap bridge, directory digits, and two tuners with `vendor:raylib`/raygui. Demonstrate mouse-driven plug/unplug and a stable redraw loop on the target laptop.
2. **Cross-language protocol proof:** have the Odin process connect to a Rust host over loopback, send commands, receive a full snapshot plus incremental events, deliberately disconnect, reconnect, and resynchronize. Run the same golden valid/invalid JSON fixtures through both decoders.
3. **Single-writer and cancellation proof:** implement a tiny pure Rust reducer and Tokio host with fake AI workers that answer late and out of order. Prove stale results cannot mutate state and bounded queues apply backpressure.
4. **Content/storage proof:** load a small cross-file TOML world with stable IDs and deliberate broken references; report useful file/line errors. Save, reload, and replay one shift through bundled SQLite.
5. **ESP32 transport proof:** before cabinet implementation, use a pseudo-terminal or a minimal board to test framing, heartbeat, duplicate/out-of-order handling, unplug/replug, and full state resync. `tokio-serial` exposes async serial I/O and even pseudo-terminal pairs on Unix. [tokio-serial `SerialStream`](https://docs.rs/tokio-serial/latest/tokio_serial/struct.SerialStream.html)
6. **Odin-only counter-spike:** in the same time budget as items 2–4, try an Odin headless host with JSON, one delayed fake worker, the chosen TOML parser, SQLite, and serial stub. Choose all-Odin only if its dependency pinning, diagnostics, tests, cancellation, and packaging are comparably simple—not merely because the happy path runs.

Decision rule: lock **Rust backend + Odin GUI** if the cross-language proof passes and the Odin-only counter-spike exposes meaningful integration ownership. Lock **all Odin** only if it passes the same failure-path tests with clearly lower project complexity. Select **C++** only when a required in-process native dependency cannot be kept behind the worker boundary.

## Reproducible non-container deployment

- Commit `Cargo.lock`, pin `rust-toolchain.toml`, and build/test with `--locked`; Cargo documents `--locked` as enforcing the exact dependency resolution. [Cargo test and lock/offline flags](https://doc.rust-lang.org/cargo/commands/cargo-test.html)
- Pin one Odin monthly release rather than tracking nightly. Keep the compiler version in a checked-in toolchain manifest and vendor/pin non-official Odin code, matching Odin's documented dependency model. [Odin installation/releases](https://odin-lang.org/docs/install/), [Odin package-management FAQ](https://odin-lang.org/docs/faq/#is-there-an-official-odin-package-manager)
- Build one release directory containing the Rust host, Odin GUI fallback, worker launchers/environments, model files or verified model paths, content, migrations, assets, config, license notices, and logs directory.
- Provide one launch command that performs preflight checks for GPU/runtime, model files, microphone/speaker, writable save path, and optional ESP32, then selects Cabinet or GUI frontend.
- Rehearse from a cold reboot with networking disabled. Container images are unnecessary for a single known machine and would add GPU, audio, and USB passthrough failure modes to the live demonstration.

## Decision summary

Odin is a sound choice for the custom GUI, and it could run the entire game. Its official GUI claim should be described accurately as maintained vendor bindings/source ports, especially raylib/raygui and microui. For this project's dependency-heavy, asynchronous backend, Rust currently offers the lowest-risk complete path. Keep AI engines out of the core as supervised local workers, keep all state mutations in one host task, and make the GUI and ESP32 replaceable protocol clients. Validate that shape with the six small failure-oriented spikes before treating the language decision as final.
