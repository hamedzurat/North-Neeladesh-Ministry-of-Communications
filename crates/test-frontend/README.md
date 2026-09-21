# Text test frontend

This binary drives the backend without a microphone or speakers. It performs one
call, asks a player-utterance command for the caller's text, sends that text to
the backend, and records the run in CSV.

Start the backend with the text listener enabled:

```sh
cargo run -p exchange-backend -- --text-bind 127.0.0.1:7880
```

Run interactively:

```sh
cargo run -p exchange-test-frontend -- --csv run.csv
```

Use an LLM adapter instead of the terminal prompt:

```sh
cargo run -p exchange-test-frontend -- \
  --player-command ./player-utterance \
  --csv run.csv
```

The command receives JSON on stdin:

```json
{
  "caller_line": 3,
  "destination_line": 11,
  "state_revision": 2,
  "conversation": []
}
```

It must write JSON to stdout:

```json
{"text":"Please connect me to the station."}
```

The adapter generates only the player's words. The backend remains responsible
for routing, dialogue response generation, and game state.
