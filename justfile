stt_port := "18080"
llm_port := "18081"
tts_port := "18082"
backend_port := "48129"

frontend:
	odin run frontend

backend:
	NN_MVP_STT_PORT={{stt_port}} NN_MVP_LLM_PORT={{llm_port}} NN_MVP_TTS_PORT={{tts_port}} NN_MVP_BACKEND_PORT={{backend_port}} cargo run --manifest-path backend/Cargo.toml --bin backend

stt:
	NN_MVP_STT_PORT={{stt_port}} ./scripts/run-stt.sh

llm:
	NN_MVP_LLM_PORT={{llm_port}} ./scripts/run-llm.sh

tts:
	NN_MVP_TTS_PORT={{tts_port}} ./scripts/run-tts.sh

check:
	cargo test --manifest-path backend/Cargo.toml
	odin check frontend

build:
	mkdir -p build
	cargo build --release --manifest-path backend/Cargo.toml --bin backend
	odin build frontend -out:build/cabinet-frontend
