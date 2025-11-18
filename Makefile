.PHONY: up run_sim bench replay export

up:
docker compose up -d

run_sim:
cargo run -p risk_core

bench:
cargo bench -p risk_core

replay:
cargo run -p risk_core -- replay

export:
cargo run -p risk_core -- export
