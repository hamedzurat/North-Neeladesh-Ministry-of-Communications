frontend:
	odin run frontend

backend:
	cargo run --manifest-path backend/Cargo.toml --bin backend

check:
	cargo test --manifest-path backend/Cargo.toml
	odin check frontend

build:
	mkdir -p build
	cargo build --release --manifest-path backend/Cargo.toml --bin backend
	odin build frontend -out:build/cabinet-frontend

mvp: # Starts both processes; stops the backend when the frontend exits.
	#!/usr/bin/env bash
	set -euo pipefail
	cargo run --quiet --manifest-path backend/Cargo.toml --bin backend &
	core_pid=$!
	trap 'kill "$core_pid" 2>/dev/null || true' EXIT INT TERM
	odin run frontend
