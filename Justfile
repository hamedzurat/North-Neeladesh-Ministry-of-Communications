backend:
    cargo run -p exchange-backend -- --bind 127.0.0.1:7878

protocol-harness:
    cargo run -p exchange-protocol-harness

protocol-manual address="127.0.0.1:7878":
    cargo run -p exchange-protocol-harness -- --connect {{address}}

check:
    cargo check --workspace

backend-test:
    cargo test -p exchange-backend

test:
    cargo test --workspace
