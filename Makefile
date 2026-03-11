.PHONY: up down run-sim gui replay demo replay-all hot-swap export-metrics bench bench-gate test fmt clippy check sample-data generate-data

up:
	docker compose up -d

down:
	docker compose down

# --- Core commands ---

run-sim:
	cargo run -p risk_core -- run-sim

gui:
	cargo run -p risk_core -- gui

replay:
	cargo run -p risk_core -- replay --input data/sample_events.jsonl

# --- Demo: the money commands ---

demo:
	cargo run --release -p risk_core -- replay --input data/generated/day_01.jsonl

replay-all:
	cargo run --release -p risk_core -- replay --input data/generated

hot-swap:
	cargo run --release -p hot_swap_demo

# --- Data ---

sample-data:
	cargo run -p risk_core -- write-sample-replay --output data/sample_events.jsonl --count 500

generate-data:
	cargo run --release -p risk_core -- generate-dataset

# --- Observability ---

export-metrics:
	cargo run -p risk_core -- export-metrics

# --- Quality ---

bench:
	cargo bench --workspace

bench-gate:
	cargo bench -p risk-gate

test:
	cargo test --workspace

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

check: fmt clippy test
