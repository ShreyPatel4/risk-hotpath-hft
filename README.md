# risk-hotpath-hft

> In electronic trading, every order passes through a risk gate before reaching
> the exchange — this gate must evaluate in nanoseconds or the order misses the market.

**risk-gate** is a 500-line `no_std` Rust crate that evaluates pre-trade risk in **37ns**
with zero heap allocations. This workspace is the simulation harness that proves it —
120 symbols, 50 traders, streaming replay at 1.5M events/sec, and a live web dashboard.

## What it proves

| Claim | Evidence |
|-------|----------|
| **37ns** full 7-check evaluation | `cargo bench -p risk-gate` (Criterion) |
| **23M evals/sec** sustained throughput | `cargo run --release -p hot_swap_demo` |
| **P99 = 42ns**, zero impact during config swap | Hot-swap demo: swap at window 3, no P99 delta |
| **Zero heap allocation** | `#![no_std]`, no Vec/HashMap/String in risk-gate |
| **All checks property-tested** | 8 proptest invariants, no input bypasses any rule |
| **Streams 780K events in 0.5s** | `cargo run --release -- replay --input data/generated/day_01.jsonl` |
| **82% accept / 18% reject** (healthy split) | Rejections across max_qty + price_collar, not dominated by one rule |

## Quick Start

```bash
# 1. Benchmark the gate (the pitch)
cargo bench -p risk-gate

# 2. Hot-swap demo (23M ops/sec, swap config mid-stream)
cargo run --release -p hot_swap_demo

# 3. Generate realistic dataset (120 symbols, GBM dynamics, anomalies)
cargo run --release -p risk_core -- generate-dataset --days 1 --target-events 500000

# 4. Replay through the gate — streaming, with latency + rule breakdown
cargo run --release -p risk_core -- replay --input data/generated/day_01.jsonl

# 5. Replay a full 15-day dataset (directory mode)
cargo run --release -p risk_core -- generate-dataset --days 15
cargo run --release -p risk_core -- replay --input data/generated

# 6. Live web dashboard
cargo run -p risk_core -- gui    # http://localhost:8080

# 7. Observability stack (Docker)
docker compose up -d             # Prometheus :9090, Grafana :3000
cargo run -p risk_core -- run-sim
```

## Sample Replay Output

```
=== Replay Summary ===
Total events:   781,154        (processed in 0.52s)
Market events:  733,188
Order events:   47,966
  Accepted:     39,255         (81.8%)
  Rejected:     8,711          (18.2%)

Throughput:     1,509,498 events/sec
Risk checks:    92,689 orders/sec

Latency (per risk check):
  min: 0 us   p50: 1 us   p99: 1 us   max: 45 us   avg: 0.01 us

Rejections by rule:
  max_qty              7,606
  price_collar         1,105
```

## The Crate: risk-gate

See [risk-gate/README.md](risk-gate/README.md) for the full API.

```rust
use risk_gate::{RiskGate, Order, Side, RiskConfig};

let mut gate = RiskGate::<1024, 256>::new(RiskConfig::default());
let order = Order { order_id: 1, symbol_id: 0, trader_id: 0,
                    price: 150.0, quantity: 100, side: Side::Buy };
let decision = gate.evaluate(&order, 150.0, 0);  // 37ns
```

**37ns** | `no_std` | zero heap | `#[repr(C)]` | C FFI | property-tested

### 7 checks (short-circuits on first failure)

| Check | What it catches |
|-------|----------------|
| Zero quantity | qty = 0 |
| Invalid price | NaN, Inf, negative, zero |
| Max quantity | qty > 10,000 (configurable) |
| Max notional | price * qty > $5M (configurable) |
| Price collar | price outside +/-5% of mid (configurable) |
| Credit limit | per-trader cumulative exposure > $1B (configurable) |
| Duplicate | same order within 1s window |

### Benchmarks

```
gate_evaluate_accept      37.0 ns    (all 7 checks pass)
gate_evaluate_reject_qty   0.3 ns    (short-circuit)
gate_full_pipeline         1.7 ns    (mixed realistic load)
credit_check_and_update    0.3 ns    (single array lookup)
```

### Hot-Swap Under Load

```
Window         Throughput        P50        P99        Max  Config
---------------------------------------------------------------------------
0-1s               22.7M        0ns       42ns    45042ns  max_qty=1000
1-2s               23.6M        0ns       42ns    46167ns  max_qty=1000
2-3s               23.5M        0ns       42ns   315708ns  max_qty=1000
3-4s               23.4M        0ns       42ns    28875ns  max_qty=5000 <-- SWAP
4-5s               23.7M        0ns       42ns    30417ns  max_qty=5000 <-- SWAP
5-6s               23.8M        0ns       42ns    37583ns  max_qty=5000 <-- SWAP
```

## The Harness: risk_core

Simulation and observability layer that proves the gate at scale:

- **Data generation** (`datagen/`): 120 real tickers across 10 sectors, Geometric Brownian Motion
  with drift/volatility/sector correlations, diurnal U-shaped volume, 50 trader profiles
  (market makers, institutional, retail, algo), anomaly injection (flash crashes, fat fingers,
  momentum spikes, liquidity gaps)
- **Streaming replay** (`replay/`): line-by-line BufReader, handles multi-GB files without loading
  into memory, reports throughput + latency percentiles per check, directory mode for multi-day runs
- **Relay** (`relay/`): configurable speed multiplier or max-throughput batch mode from files/directories
- **Web dashboard** (`web/` + `static/`): axum server with WebSocket streaming, Chart.js charts,
  live order book depth, decision stream, log viewer
- **Observability** (`telemetry/`): Prometheus metrics on `:9898` (6 metrics including latency
  histogram), Grafana dashboard, health endpoint, optional ClickHouse persistence

## Architecture

```
  ┌──────────────┐     ┌───────────┐
  │  Data Gen    │     │  Relay    │  stream / batch
  │  120 symbols │────▶│  Layer    │  from files
  │  GBM + anom. │     └─────┬─────┘
  └──────────────┘           │ SimEvent (line-by-line)
                             ▼
                  ┌──────────────────────┐
                  │    Order Book        │  shallow depth
                  │  (bid/ask levels)    │  per symbol
                  └──────────┬───────────┘
                             │ mid price
                             ▼
               ┌──────────────────────────┐
               │  risk-gate (37ns, no_std) │  7 checks
               │  ┌─────────────────────┐ │  zero alloc
               │  │ CreditTracker<1024> │ │  #[repr(C)]
               │  │ DupDetector<256>    │ │  property-tested
               │  └─────────────────────┘ │
               └──┬─────┬─────┬───────────┘
                  │     │     │
     ┌────────────┘     │     └──────────────┐
     ▼                  ▼                    ▼
┌──────────┐    ┌──────────────┐    ┌──────────────┐
│Prometheus│    │  ClickHouse  │    │  Web GUI     │
│  :9898   │    │  (optional)  │    │  :8080       │
└──────────┘    └──────────────┘    └──────────────┘
```

## Repo Structure

```
risk-hotpath-hft/
├── risk-gate/                    # THE PRODUCT: no_std, zero-alloc risk gate
│   ├── src/
│   │   ├── lib.rs                #   crate root + re-exports
│   │   ├── engine.rs             #   RiskGate<TRADERS, DUP_RING>
│   │   ├── checks.rs             #   7 pure check functions (#[inline(always)])
│   │   ├── credit.rs             #   CreditTracker<N> — fixed-size [f64; N]
│   │   ├── dedup.rs              #   DupDetector<N> — ring buffer + FNV-1a hash
│   │   ├── types.rs              #   Order, Decision, RiskConfig (all Copy, repr(C))
│   │   └── ffi.rs                #   extern "C" bindings (--features ffi)
│   ├── ffi/risk_gate.h           #   C header
│   ├── tests/property.rs         #   8 proptest invariants
│   └── benches/evaluate.rs       #   Criterion: 4 benchmarks
│
├── risk_core/                    # SIMULATION HARNESS (depends on risk-gate)
│   ├── src/
│   │   ├── main.rs               #   CLI: run-sim, gui, replay, generate-dataset
│   │   ├── risk/engine.rs        #   thin wrapper: String→u32 mapping + Instant timing
│   │   ├── feed/orderbook.rs     #   shallow order book (configurable depth)
│   │   ├── feed/simulator.rs     #   live 3-symbol generator (Poisson timing)
│   │   ├── datagen/mod.rs        #   120-symbol generator (GBM, anomalies, 50 traders)
│   │   ├── replay/runner.rs      #   streaming replay: throughput, latency, rule breakdown
│   │   ├── relay/mod.rs          #   file/dir streaming with speed control
│   │   ├── web/mod.rs            #   axum + WebSocket (REST API + live streaming)
│   │   ├── telemetry/metrics.rs  #   Prometheus exporter (6 metrics + /health)
│   │   └── store/clickhouse.rs   #   optional ClickHouse persistence
│   └── static/index.html         #   web dashboard SPA (Chart.js, 5 tabs)
│
├── examples/
│   ├── hot_swap_demo/            #   config swap under 23M ops/sec load
│   └── c_integration/            #   gcc example linking risk-gate via FFI
│
├── data/
│   ├── sample_events.jsonl       #   200-event replay fixture
│   └── generated/                #   output of generate-dataset (gitignored)
├── docker-compose.yml            #   Prometheus + Grafana + ClickHouse
├── Makefile
└── .github/workflows/ci.yml
```

## Makefile

```bash
make bench-gate      # Criterion benchmarks (37ns)
make hot-swap        # config swap demo (23M ops/sec)
make generate-data   # generate 120-symbol dataset (--release)
make demo            # replay generated data with stats (--release)
make replay-all      # replay all day files (--release)
make gui             # web dashboard on :8080
make run-sim         # headless simulator with metrics
make test            # 77 tests across workspace
make check           # fmt + clippy + test
```

## Embed risk-gate in C/C++

```c
#include "risk_gate.h"

RiskConfig cfg = { .max_quantity = 10000, .max_notional = 5000000.0,
                   .credit_limit = 1000000000.0, .collar_lower = 0.95,
                   .collar_upper = 1.05, .dup_window_ns = 1000000000 };
RiskGateHandle* gate = risk_gate_new(cfg);

Order order = { .order_id = 1, .symbol_id = 0, .trader_id = 0,
                .price = 150.0, .quantity = 100, .side = SIDE_BUY };
Decision d = risk_gate_evaluate(gate, &order, 150.0, 0);

risk_gate_destroy(gate);
```

Build: `cargo build --release -p risk-gate --features ffi`
Example: `cd examples/c_integration && make`

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                    # 77 tests
cargo bench -p risk-gate                  # 4 benchmarks
```

## Project Bootstrap

The `scripts/bootstrap_project.py` helper wires up a GitHub Projects v2 board.
See `--help` for usage.

## Current Limitations

- Datagen not config-aware (order distribution depends on threshold tuning)
- GUI live mode uses 3-symbol simulator, not the 120-symbol dataset
- Credit exposure resets on restart (no durable state)
- ClickHouse writes inline (no background batching)
- Single-threaded event loop in harness

## Roadmap

- [ ] Config-aware order generator (distribute orders across all 7 check outcomes)
- [ ] WASM build for risk-gate (~15KB .wasm)
- [ ] GUI replay mode (stream generated data through dashboard)
- [ ] Lock-free order book with crossbeam channels
- [ ] FIX/ITCH protocol adapters
- [ ] Persistent credit ledger (Redis or RocksDB)
