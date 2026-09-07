backend address="127.0.0.1:7878" voice_address="127.0.0.1:7879":
    NN_VOICE_RANDOM_SPEAKER=1 NN_VOICE_STT_COMMAND="uv run --project python --no-sync python -m voice_workers.stt" NN_VOICE_DIALOGUE_COMMAND="uv run --project python --no-sync python -m voice_workers.dialogue" NN_VOICE_TTS_COMMAND="uv run --project python --no-sync python -m voice_workers.tts" NN_VOICE_TTS_PERSISTENT=1 cargo run -p exchange-backend -- --bind {{ address }} --voice-bind {{ voice_address }}

backend-trace address="127.0.0.1:7878" voice_address="127.0.0.1:7879":
    NN_BACKEND_TRACE=1 NN_VOICE_STT_COMMAND="uv run --project python --no-sync python -m voice_workers.stt" NN_VOICE_DIALOGUE_COMMAND="uv run --project python --no-sync python -m voice_workers.dialogue" NN_VOICE_TTS_COMMAND="uv run --project python --no-sync python -m voice_workers.tts" NN_VOICE_TTS_PERSISTENT=1 cargo run -p exchange-backend -- --bind {{ address }} --voice-bind {{ voice_address }}

backend-stress address="127.0.0.1:7878" voice_address="127.0.0.1:7879":
    NN_BACKEND_PRINTER_STRESS=1 NN_VOICE_STT_COMMAND="uv run --project python --no-sync python -m voice_workers.stt" NN_VOICE_DIALOGUE_COMMAND="uv run --project python --no-sync python -m voice_workers.dialogue" NN_VOICE_TTS_COMMAND="uv run --project python --no-sync python -m voice_workers.tts" NN_VOICE_TTS_PERSISTENT=1 cargo run -p exchange-backend -- --bind {{ address }} --voice-bind {{ voice_address }}

backend-debug address="127.0.0.1:7878" voice_address="127.0.0.1:7879" debug_address="127.0.0.1:7880":
    NN_BACKEND_PRINTER_STRESS=1 NN_BACKEND_TRACE=1 NN_VOICE_STT_COMMAND="uv run --project python --no-sync python -m voice_workers.stt" NN_VOICE_DIALOGUE_COMMAND="uv run --project python --no-sync python -m voice_workers.dialogue" NN_VOICE_TTS_COMMAND="uv run --project python --no-sync python -m voice_workers.tts" NN_VOICE_TTS_PERSISTENT=1 cargo run -p exchange-backend -- --bind {{ address }} --voice-bind {{ voice_address }} --debug-bind {{ debug_address }}

frontend-check:
    /bin/odin check frontend

frontend-font:
    python scripts/extract_ttc_face.py /usr/share/fonts/TTF/Iosevka-Regular.ttc target/Iosevka-Regular.ttf

frontend-build: frontend-check frontend-font
    /bin/odin build frontend -out:target/north-neeladesh-frontend

frontend backend_address="127.0.0.1:7878": frontend-build
    NN_BACKEND_ADDRESS={{ backend_address }} ./target/north-neeladesh-frontend

odin: frontend

debug-surface backend_address="127.0.0.1:7880" address="127.0.0.1:7881":
    cargo run -p exchange-debug-surface -- --backend {{ backend_address }} --bind {{ address }}

frontend-trace backend_address="127.0.0.1:7878": frontend-build
    NN_BACKEND_ADDRESS={{ backend_address }} NN_FRONTEND_TRACE=1 ./target/north-neeladesh-frontend

build: frontend-build

protocol-harness:
    cargo run -p exchange-protocol-harness

protocol-manual address="127.0.0.1:7878":
    cargo run -p exchange-protocol-harness -- --connect {{ address }}

voice-daemon backend_address="127.0.0.1:7879" capture_command="" playback_command="":
    NN_VOICE_BACKEND_ADDRESS={{ backend_address }} NN_VOICE_CAPTURE_COMMAND="{{ capture_command }}" NN_VOICE_PLAYBACK_COMMAND="{{ playback_command }}" cargo run -p exchange-voice-daemon

voice-demo backend_address="127.0.0.1:7879" capture_command="sh scripts/voice_smoke_capture.sh" playback_command="":
    NN_VOICE_BACKEND_ADDRESS={{ backend_address }} NN_VOICE_CAPTURE_COMMAND="{{ capture_command }}" NN_VOICE_PLAYBACK_COMMAND="{{ playback_command }}" cargo run -p exchange-voice-daemon

voice-setup:
    uv run --project python python -m voice_workers.setup

voice-smoke backend_address="127.0.0.1:7879" capture_command="sh scripts/voice_smoke_capture.sh" playback_command="sh scripts/voice_smoke_playback.sh":
    NN_VOICE_BACKEND_ADDRESS={{ backend_address }} NN_VOICE_CAPTURE_COMMAND="{{ capture_command }}" NN_VOICE_PLAYBACK_COMMAND="{{ playback_command }}" cargo run -p exchange-voice-daemon

voice-preflight:
    uv run --project python --no-sync python -m voice_workers.preflight

check: frontend-check
    cargo check --workspace

backend-test:
    cargo test -p exchange-backend

backend-tests:
    cargo test -p exchange-backend

content-validation:
    cargo test -p exchange-backend --test story_graph

frontend-checks:
    /bin/odin check frontend

voice-checks:
    cargo test -p exchange-voice-daemon

test:
    cargo test --workspace
