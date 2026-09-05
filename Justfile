backend address="127.0.0.1:7878":
    cargo run -p exchange-backend -- --bind {{address}}

backend-trace address="127.0.0.1:7878":
    NN_BACKEND_TRACE=1 cargo run -p exchange-backend -- --bind {{address}}

backend-stress address="127.0.0.1:7878":
    NN_BACKEND_PRINTER_STRESS=1 cargo run -p exchange-backend -- --bind {{address}}

backend-debug address="127.0.0.1:7878":
    NN_BACKEND_PRINTER_STRESS=1 NN_BACKEND_TRACE=1 cargo run -p exchange-backend -- --bind {{address}}

frontend-check:
    /bin/odin check frontend

frontend-font:
    python scripts/extract_ttc_face.py /usr/share/fonts/TTF/Iosevka-Regular.ttc target/Iosevka-Regular.ttf

frontend-build: frontend-check frontend-font
    /bin/odin build frontend -out:target/north-neeladesh-frontend

frontend backend_address="127.0.0.1:7878": frontend-build
    NN_BACKEND_ADDRESS={{backend_address}} ./target/north-neeladesh-frontend

frontend-trace backend_address="127.0.0.1:7878": frontend-build
    NN_BACKEND_ADDRESS={{backend_address}} NN_FRONTEND_TRACE=1 ./target/north-neeladesh-frontend

build: frontend-build

protocol-harness:
    cargo run -p exchange-protocol-harness

protocol-manual address="127.0.0.1:7878":
    cargo run -p exchange-protocol-harness -- --connect {{address}}

check: frontend-check
    cargo check --workspace

backend-test:
    cargo test -p exchange-backend

test:
    cargo test --workspace
