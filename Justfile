backend-debug address="127.0.0.1:7878" voice_address="127.0.0.1:7879":
    PYTHONPATH=python NN_STORY_CLASSIFIER_COMMAND="uv run --project python --no-sync python -m voice_workers.classifier" NN_VOICE_STT_COMMAND="uv run --project python --no-sync python -m voice_workers.stt" NN_VOICE_DIALOGUE_COMMAND="uv run --project python --no-sync python -m voice_workers.dialogue" NN_VOICE_DIALOGUE_PERSISTENT=1 NN_OLLAMA_MODEL="qwen3.5:4b" NN_VOICE_TTS_COMMAND="uv run --project python --no-sync python -m voice_workers.pocket_tts" NN_VOICE_TTS_PERSISTENT=1 cargo run --quiet -p exchange-backend --bin exchange-backend -- --bind {{ address }} --voice-bind {{ voice_address }} --debug-bind 127.0.0.1:7880

story-test-backend address="127.0.0.1:7878" voice_address="127.0.0.1:7879" text_address="127.0.0.1:7880" debug_address="127.0.0.1:7882":
    PYTHONPATH=python NN_STORY_CLASSIFIER_COMMAND="python -m voice_workers.classifier" NN_VOICE_STT_COMMAND="uv run --project python --no-sync python -m voice_workers.stt" NN_VOICE_DIALOGUE_COMMAND="uv run --project python --no-sync python -m voice_workers.dialogue" NN_VOICE_DIALOGUE_PERSISTENT=1 NN_VOICE_TTS_COMMAND="uv run --project python --no-sync python -m voice_workers.pocket_tts" NN_VOICE_TTS_PERSISTENT=1 cargo run --quiet -p exchange-backend --bin exchange-backend -- --bind {{ address }} --voice-bind {{ voice_address }} --text-bind {{ text_address }} --debug-bind {{ debug_address }}

story-test path="ems_success" log="story-test.log":
    PYTHONPATH=python NN_STORY_CLASSIFIER_COMMAND="python -m voice_workers.classifier" cargo run --quiet -p exchange-test-frontend -- --path {{ path }} --debug-connect 127.0.0.1:7882 --log {{ log }} --player-command "python -m voice_workers.player"

story-audio:
    PYTHONPATH=python uv run --project python --no-sync python scripts/generate_neel_university_audio.py

story-test-all:
    #!/usr/bin/env bash
    set -euo pipefail
    rm -f story-test-all.log
    for path in ems_success ems_failure police_success water_no_help unrelated_questions random_conversation neel_direct neel_misdirection neel_tap neel_tap_reverse neel_tap_late neel_rewire neel_rewire_late neel_patience neel_arnab_patience neel_bela_1031 neel_bela_1032 neel_bela_1032_questions intertwined_success; do
        temp="/tmp/opencode/story-test-${path}.log"
        just story-test "${path}" "$temp"
        cat "$temp" >> story-test-all.log
        printf '\n' >> story-test-all.log
    done

frontend-raylib:
    test -f /usr/lib/libraylib.so || (echo "missing /usr/lib/libraylib.so; install raylib 6.0" >&2 && exit 1)
    rm -rf target/odin-vendor
    mkdir -p target/odin-vendor
    cp -a /usr/lib/odin/vendor/raylib target/odin-vendor/raylib
    ln -sf /usr/lib/libraylib.so target/odin-vendor/raylib/linux/libraylib.so.600
    ln -sf /usr/lib/libraylib.so target/odin-vendor/raylib/linux/libraylib.a
    cc -shared -fPIC $(pkg-config --cflags libpipewire-0.3) frontend/pipewire_capture.c $(pkg-config --libs libpipewire-0.3) -lpthread -o target/libnn_pipewire_capture.so

frontend-check: frontend-raylib
    /bin/odin check frontend -collection:nn_vendor=target/odin-vendor

frontend-font:
    python scripts/extract_ttc_face.py /usr/share/fonts/TTF/Iosevka-Regular.ttc target/Iosevka-Regular.ttf

frontend-build: frontend-check frontend-font
    /bin/odin build frontend -collection:nn_vendor=target/odin-vendor -define:RAYLIB_SHARED=true -extra-linker-flags:"-Ltarget -Wl,-rpath,'$$ORIGIN'" -out:target/north-neeladesh-frontend

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

voice-setup:
    uv run --project python python -m voice_workers.setup

voice-preflight:
    uv run --project python --no-sync python -m voice_workers.preflight

check: frontend-check
    cargo check --workspace

backend-test:
    cargo test -p exchange-backend

backend-tests:
    cargo test -p exchange-backend

content-validation:
    cargo test -p exchange-backend --test neutral_exchange --test simple_hardware_demo

frontend-checks:
    /bin/odin check frontend

voice-checks:
    cargo test -p exchange-backend --lib voice_workers

test:
    cargo test --workspace
