frontend:
	odin run frontend

backend:
	cargo run --manifest-path backend/Cargo.toml --bin backend

stt:
	./scripts/run-stt.sh

dialogue:
	./scripts/run-dialogue.sh

tts:
	./scripts/run-tts.sh

check:
	cargo test --manifest-path backend/Cargo.toml
	odin check frontend

build:
	mkdir -p build
	cargo build --release --manifest-path backend/Cargo.toml --bin backend
	odin build frontend -out:build/cabinet-frontend
