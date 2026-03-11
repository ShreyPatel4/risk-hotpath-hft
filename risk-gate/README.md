# risk-gate

Zero-allocation pre-trade risk gate for high-frequency trading systems.

- **37ns** full evaluation (all 7 checks, accept path) — benchmarked on Apple Silicon
- **23M evals/sec** sustained throughput with P99 = 42ns
- `no_std` compatible — embed in FPGA soft-cores, kernel modules, or WASM
- All types `#[repr(C)]` — C FFI header included, link from C/C++/Python/Java
- Fixed-capacity credit tracker and duplicate detector (no HashMap, no Vec, no heap)
- Property-tested with proptest: no input combination bypasses any check
- ~500 lines of Rust, zero `unsafe` except at the FFI boundary
- Atomic config hot-swap under load with zero throughput impact

## Use it (Rust)

```rust
use risk_gate::{RiskGate, Order, Side, RiskConfig, Decision};

let config = RiskConfig::default();
let mut gate = RiskGate::<1024, 256>::new(config);  // 1024 traders, 256-entry dup ring

let order = Order {
    order_id: 1,
    symbol_id: 0,      // caller maps "AAPL" -> 0
    trader_id: 0,      // caller maps "TRADER_1" -> 0
    price: 150.0,
    quantity: 100,
    side: Side::Buy,
};

let decision = gate.evaluate(&order, 150.0, /* now_ns */ 0);
assert!(decision.is_accept());
```

## Embed it (C/C++)

```c
#include "risk_gate.h"

RiskConfig cfg = { .max_quantity = 10000, .max_notional = 5000000.0, ... };
RiskGateHandle* gate = risk_gate_new(cfg);

Order order = { .order_id = 1, .price = 150.0, .quantity = 100, .side = SIDE_BUY, ... };
Decision d = risk_gate_evaluate(gate, &order, 150.0, 0);

risk_gate_destroy(gate);
```

Build: `cargo build --release -p risk-gate --features ffi`

## Checks (evaluated in order, short-circuits on first failure)

| # | Check | Latency | Description |
|---|-------|---------|-------------|
| 1 | Zero quantity | <1ns | Business invariant: qty must be > 0 |
| 2 | Invalid price | <1ns | Rejects NaN, Inf, negative, zero |
| 3 | Max quantity | <1ns | `qty <= config.max_quantity` |
| 4 | Max notional | <1ns | `price * qty <= config.max_notional` |
| 5 | Price collar | ~2ns | `price` within `[ref * lower, ref * upper]` |
| 6 | Credit limit | ~5ns | Per-trader cumulative exposure cap |
| 7 | Duplicate | ~20ns | Ring buffer scan within time window |

## Benchmarks

```
gate_evaluate_accept      37.0 ns    (full pipeline, all checks pass)
gate_evaluate_reject_qty   0.3 ns    (short-circuit on first failing check)
gate_full_pipeline         1.7 ns    (mixed orders, varying symbols/traders)
credit_check_and_update    0.3 ns    (single array lookup)
```

## Hot-Swap Under Load

Config swap is a single struct copy. Zero throughput impact:

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

Run: `cargo run --release -p hot_swap_demo`

## Design Constraints

- **No `std`**: The crate compiles with `#![no_std]` by default
- **No heap**: `CreditTracker<N>` uses `[f64; N]`, `DupDetector<N>` uses `[(u64,u64); N]`
- **No strings**: Symbol and trader IDs are `u32` — the caller maps names to indices
- **All `Copy`**: `Order`, `Decision`, `RiskConfig`, `Side` are all `Copy` + `repr(C)`
- **Deterministic**: Same inputs always produce the same output (no randomness, no time queries)
- **ZERO runtime dependencies**: no serde, no tokio, no allocator

## Property Tests

8 proptest properties verify invariants that must hold for ALL possible inputs:

- Max quantity is a pure threshold (passes iff `qty <= max`)
- Price collar rejects anything outside the band
- Credit limit cannot be bypassed by any combination of prior exposure + new order
- Zero quantity is always rejected
- Invalid prices (NaN, Inf, <=0) are always rejected
- Pathological inputs (u64::MAX, f64::MAX, NaN) never cause a panic
- Duplicate detection works within the configured time window
- Notional check is consistent with `price * qty`
