frontend:
	odin run frontend

backend:
	./scripts/run-backend.sh

check:
	cargo test --manifest-path backend/Cargo.toml
	odin check frontend

build:
	mkdir -p build
	cargo build --release --manifest-path backend/Cargo.toml --bin backend
	odin build frontend -out:build/cabinet-frontend
