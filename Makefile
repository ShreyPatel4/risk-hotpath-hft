.PHONY: up down run-sim gui replay demo replay-all hot-swap export-metrics bench bench-gate test fmt clippy check sample-data generate-data redteam redteam-ffi immunity stress zero-alloc bench-adversarial bench-regression bench-regression-check redteam-all

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

# --- Red Team Suite ---

redteam:
	cargo test -p risk-gate --test redteam_floats --test redteam_boundaries --test redteam_credit --test redteam_dedup --test redteam_config -- --nocapture

redteam-ffi:
	cargo test -p risk-gate --features ffi --test redteam_ffi -- --nocapture

immunity:
	cargo test -p risk-gate --test immunity -- --nocapture

stress:
	cargo test -p risk-gate --test stress --release -- --nocapture --test-threads=1

zero-alloc:
	cargo test -p risk-gate --test zero_alloc --release -- --nocapture

bench-adversarial:
	cargo bench -p risk-gate --bench adversarial_bench

bench-regression:
	cargo bench -p risk-gate --bench regression_gate -- --save-baseline main

bench-regression-check:
	cargo bench -p risk-gate --bench regression_gate -- --baseline main

redteam-all: redteam redteam-ffi immunity stress zero-alloc bench-adversarial
	@echo "=== Red Team Suite Complete ==="
