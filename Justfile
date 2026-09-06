backend address="127.0.0.1:7878" voice_address="127.0.0.1:7879":
    NN_VOICE_RANDOM_SPEAKER=1 cargo run -p exchange-backend -- --bind {{ address }} --voice-bind {{ voice_address }}

backend-trace address="127.0.0.1:7878" voice_address="127.0.0.1:7879":
    NN_BACKEND_TRACE=1 cargo run -p exchange-backend -- --bind {{ address }} --voice-bind {{ voice_address }}

backend-stress address="127.0.0.1:7878" voice_address="127.0.0.1:7879":
    NN_BACKEND_PRINTER_STRESS=1 cargo run -p exchange-backend -- --bind {{ address }} --voice-bind {{ voice_address }}

backend-debug address="127.0.0.1:7878" voice_address="127.0.0.1:7879":
    NN_BACKEND_PRINTER_STRESS=1 NN_BACKEND_TRACE=1 cargo run -p exchange-backend -- --bind {{ address }} --voice-bind {{ voice_address }}

frontend-check:
    /bin/odin check frontend

frontend-font:
    python scripts/extract_ttc_face.py /usr/share/fonts/TTF/Iosevka-Regular.ttc target/Iosevka-Regular.ttf

frontend-build: frontend-check frontend-font
    /bin/odin build frontend -out:target/north-neeladesh-frontend

frontend backend_address="127.0.0.1:7878": frontend-build
    NN_BACKEND_ADDRESS={{ backend_address }} ./target/north-neeladesh-frontend

frontend-trace backend_address="127.0.0.1:7878": frontend-build
    NN_BACKEND_ADDRESS={{ backend_address }} NN_FRONTEND_TRACE=1 ./target/north-neeladesh-frontend

build: frontend-build

protocol-harness:
    cargo run -p exchange-protocol-harness

protocol-manual address="127.0.0.1:7878":
    cargo run -p exchange-protocol-harness -- --connect {{ address }}

voice-daemon backend_address="127.0.0.1:7879" stt_command="uv run --project python --no-sync python -m voice_workers.stt" dialogue_command="uv run --project python --no-sync python -m voice_workers.dialogue" tts_command="uv run --project python --no-sync python -m voice_workers.tts":
    NN_VOICE_TTS_PERSISTENT=1 NN_VOICE_BACKEND_ADDRESS={{ backend_address }} NN_VOICE_STT_COMMAND="{{ stt_command }}" NN_VOICE_DIALOGUE_COMMAND="{{ dialogue_command }}" NN_VOICE_TTS_COMMAND="{{ tts_command }}" cargo run -p exchange-voice-daemon

voice-demo backend_address="127.0.0.1:7879" dialogue_command="uv run --project python --no-sync python -m voice_workers.dialogue" tts_command="uv run --project python --no-sync python -m voice_workers.tts":
    NN_VOICE_TTS_PERSISTENT=1 NN_VOICE_DEMO_RANDOM_VOICE=1 NN_VOICE_BACKEND_ADDRESS={{ backend_address }} NN_VOICE_CAPTURE_COMMAND="sh scripts/voice_smoke_capture.sh" NN_VOICE_STT_COMMAND="sh scripts/voice_smoke_stt.sh" NN_VOICE_DIALOGUE_COMMAND="{{ dialogue_command }}" NN_VOICE_TTS_COMMAND="{{ tts_command }}" cargo run -p exchange-voice-daemon

voice-setup:
    uv run --project python python -m voice_workers.setup

voice-smoke backend_address="127.0.0.1:7879" capture_command="sh scripts/voice_smoke_capture.sh" stt_command="sh scripts/voice_smoke_stt.sh" dialogue_command="sh scripts/voice_smoke_dialogue.sh" tts_command="sh scripts/voice_smoke_tts.sh":
    NN_VOICE_BACKEND_ADDRESS={{ backend_address }} NN_VOICE_CAPTURE_COMMAND="{{ capture_command }}" NN_VOICE_STT_COMMAND="{{ stt_command }}" NN_VOICE_DIALOGUE_COMMAND="{{ dialogue_command }}" NN_VOICE_TTS_COMMAND="{{ tts_command }}" cargo run -p exchange-voice-daemon

voice-preflight:
    uv run --project python --no-sync python -m voice_workers.preflight

check: frontend-check
    cargo check --workspace

backend-test:
    cargo test -p exchange-backend

test:
    cargo test --workspace
