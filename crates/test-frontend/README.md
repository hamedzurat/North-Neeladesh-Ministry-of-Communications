# Text test frontend

This binary drives the real backend without a microphone or speakers. It plays a
complete story path. It supports both the Shapla Apartments service story and
the Neel University routing story. The backend performs story transitions,
delayed ringing, directory routing, and dialogue generation; the frontend only
supplies human actions.

Start the backend with the text listener enabled:

```sh
cargo run -p exchange-backend -- --text-bind 127.0.0.1:7880 --debug-bind 127.0.0.1:7881
```

Run interactively:

```sh
cargo run -p exchange-test-frontend -- --log run.log
```

Use an LLM adapter instead of the terminal prompt:

```sh
cargo run -p exchange-test-frontend -- \
  --player-command ./player-utterance \
  --log run.log
```

Available complete paths:

```sh
just story-test path=ems_success log=story-test-ems-success.log
just story-test path=ems_failure log=story-test-ems-failure.log
just story-test path=police_success log=story-test-police-success.log
just story-test path=water_no_help log=story-test-water-no-help.log
just story-test path=unrelated_questions log=story-test-unrelated-questions.log
just story-test path=random_conversation log=story-test-random-conversation.log
just story-test neel_direct /tmp/neel-direct.log
just story-test neel_misdirection /tmp/neel-misdirection.log
just story-test neel_tap /tmp/neel-tap.log
just story-test neel_tap_late /tmp/neel-tap-late.log
just story-test neel_bela_1031 /tmp/neel-wrong-bela.log
just story-test neel_bela_1032 /tmp/neel-correct-bela.log
just story-test neel_bela_1032_questions /tmp/neel-questions.log
just story-test-all
```

Generate the authored Neel University TAP recordings after changing voice
profiles in `exchange.toml` with:

```sh
just story-audio
```

Generated recordings are runtime assets and are intentionally ignored by Git.

`story-test-all` runs every path in one backend session and writes the combined
transcript to `story-test-all.log`.

The debug connection is used only to select the story thread. It never selects
or skips a story beat. Each path reaches its second beat through normal backend
conversation and classification.

The command receives JSON on stdin:

```json
{
  "caller_line": 3,
  "destination_line": 11,
  "state_revision": 2,
  "conversation": [],
  "task": "ask the caller for the exact location"
}
```

It must write JSON to stdout:

```json
{"text":"Please connect me to the station."}
```

The adapter generates only the player's words. The backend remains responsible
for routing, dialogue response generation, and game state.

During a run, the terminal prints:

```text
PLAYER LLM    // ...
CLASSIFIER    // success
SUBSCRIBER    // ...
```

The same gameplay is recorded in the `.log` file.

The story classifier is a separate generic command. Configure it with
`NN_STORY_CLASSIFIER_COMMAND`. It receives:

```json
{"prompt":"...","text":"Send EMS to Shapla Apartments."}
```

and must return one word in this JSON shape:

```json
{"classification":"success"}
```

The active service task accepts `success` or `failure`. EMS and Police use
different prompts, selected by the button the player pressed.

The bundled Ollama adapter can be used directly:

```sh
export PYTHONPATH=python
export NN_STORY_CLASSIFIER_COMMAND="python -m voice_workers.classifier"
```
