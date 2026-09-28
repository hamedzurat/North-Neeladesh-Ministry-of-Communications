set positional-arguments

backend-debug address="0.0.0.0:7878" voice_address="0.0.0.0:7879":
    PYTHONPATH=python NN_STORY_CLASSIFIER_COMMAND="uv run --project python --no-sync python -m voice_workers.classifier" NN_VOICE_STT_COMMAND="uv run --project python --no-sync python -m voice_workers.stt" NN_VOICE_DIALOGUE_COMMAND="uv run --project python --no-sync python -m voice_workers.dialogue" NN_VOICE_DIALOGUE_PERSISTENT=1 NN_OLLAMA_MODEL="qwen3.5:4b" NN_VOICE_TTS_COMMAND="uv run --project python --no-sync python -m voice_workers.pocket_tts" NN_VOICE_TTS_PERSISTENT=1 cargo run --quiet -p exchange-backend --bin exchange-backend -- --bind {{ address }} --voice-bind {{ voice_address }} --voice-upload-bind 0.0.0.0:7883 --debug-bind 127.0.0.1:7880

story-test-backend address="0.0.0.0:7878" voice_address="0.0.0.0:7879" text_address="127.0.0.1:7880" debug_address="127.0.0.1:7882":
    PYTHONPATH=python NN_STORY_CLASSIFIER_COMMAND="python -m voice_workers.classifier" NN_VOICE_STT_COMMAND="uv run --project python --no-sync python -m voice_workers.stt" NN_VOICE_DIALOGUE_COMMAND="python -m voice_workers.dialogue" NN_VOICE_DIALOGUE_PERSISTENT=1 NN_VOICE_TTS_COMMAND="uv run --project python --no-sync python -m voice_workers.pocket_tts" NN_VOICE_TTS_PERSISTENT=1 cargo run --quiet -p exchange-backend --bin exchange-backend -- --bind {{ address }} --voice-bind {{ voice_address }} --voice-upload-bind 0.0.0.0:7883 --text-bind {{ text_address }} --debug-bind {{ debug_address }}

story-test path="fallen_mother_ems_report_succeeds" log="story-test.log":
    PYTHONPATH=python NN_STORY_CLASSIFIER_COMMAND="python -m voice_workers.classifier" cargo run --quiet -p exchange-test-frontend -- --connect 127.0.0.1:7878 --text-connect 127.0.0.1:7880 --debug-connect 127.0.0.1:7882 --path {{ path }} --log {{ log }} --player-command "python -m voice_workers.player"

story-audio:
    PYTHONPATH=python uv run --project python --no-sync python scripts/generate_story_audio.py

story-test-all:
    #!/usr/bin/env bash
    set -euo pipefail
    rm -f story-test-all.log
    for path in fallen_mother_ems_report_succeeds fallen_mother_ems_report_fails fallen_mother_police_report_succeeds fallen_mother_no_water_help_leads_to_bad_ending fallen_mother_unrelated_questions fallen_mother_random_conversation bela_bose_completes_professor_routing bela_bose_misdirection_reaches_bad_ending bela_bose_answers_professor_questions bela_bose_tap_monitors_call bela_bose_reverse_tap_wiring_monitors_call bela_bose_late_tap_monitors_call bela_bose_rewires_tap_monitoring bela_bose_patience_expires bela_bose_arnab_patience_expires bela_bose_wrong_bela_destination bela_bose_correct_bela_destination bela_bose_answers_bela_questions bela_bose_handles_arnab_unrelated_questions dirty_work_good_ending dirty_work_neutral_ending dirty_work_bad_ending nahid_police_report_stops_scams nahid_five_completed_scams_end_in_penalty nahid_dialogue_then_police_report_stops_scams; do
        temp="/tmp/opencode/story-test-${path}.log"
        just story-test "${path}" "$temp"
        cat "$temp" >> story-test-all.log
        printf '\n' >> story-test-all.log

    done

ollama-benchmark:
    ./scripts/benchmark_ollama_models.sh

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

story-debug-surface backend_address="127.0.0.1:7882" address="127.0.0.1:7881":
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

pocket-tts *sentences:
    uv run --project python --no-sync python scripts/test_pocket_tts.py --all "$@"

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
