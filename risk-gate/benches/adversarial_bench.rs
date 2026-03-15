//! Adversarial benchmarks: worst-case latency paths, cache pressure,
//! and branch predictor stress.

use criterion::{criterion_group, criterion_main, Criterion};
use risk_gate::engine::RiskGate;
use risk_gate::types::*;

fn make_order(price: f64, qty: u64, trader_id: u32, order_id: u64) -> Order {
    Order {
        order_id,
        symbol_id: 0,
        trader_id,
        price,
        quantity: qty,
        side: Side::Buy,
    }
}

/// Worst case: all 7 checks pass, including full ring scan (ring at capacity with no match).
fn bench_worst_case_accept(c: &mut Criterion) {
    let config = RiskConfig {
        max_quantity: u64::MAX,
        max_notional: f64::MAX,
        credit_limit: f64::MAX,
        collar_lower: 0.0,
        collar_upper: f64::MAX,
        dup_window_ns: 0, // window=0 means only exact-ts dups
    };
    let mut gate = RiskGate::<16, 64>::new(config);

    // Pre-fill the ring buffer to capacity with distinct entries
    for i in 0..64u64 {
        let order = make_order(50.0 + i as f64, 1, 0, i);
        gate.evaluate(&order, 100.0, i);
    }
    gate.reset_all_credit();

    let mut i = 1000u64;
    c.bench_function("worst_case_accept", |b| {
        b.iter(|| {
            i += 1;
            let order = make_order(100.0, 1, 0, i);
            gate.evaluate(&order, 100.0, i)
        })
    });
}

/// Best case: NaN price → earliest short-circuit at check 2 (after config validation).
fn bench_nan_price_shortest_reject(c: &mut Criterion) {
    let config = RiskConfig::default();
    let mut gate = RiskGate::<16, 16>::new(config);
    let order = make_order(f64::NAN, 10, 0, 1);

    c.bench_function("nan_price_shortest_reject", |b| {
        b.iter(|| gate.evaluate(&order, 100.0, 0))
    });
}

/// Ring buffer at capacity, hash not present → must scan all N entries.
fn bench_full_ring_dedup_scan(c: &mut Criterion) {
    let config = RiskConfig {
        dup_window_ns: u64::MAX, // infinite window
        credit_limit: f64::MAX,
        max_notional: f64::MAX,
        max_quantity: u64::MAX,
        collar_lower: 0.0,
        collar_upper: f64::MAX,
    };
    let mut gate = RiskGate::<16, 256>::new(config);

    // Fill ring with 256 distinct entries
    for i in 0..256u64 {
        let order = make_order(50.0 + i as f64 * 0.01, 1, 0, i);
        gate.evaluate(&order, 0.0, i);
    }
    gate.reset_all_credit();

    // This order has a hash not in the ring → full scan
    let new_order = make_order(999.0, 1, 1, 9999);

    c.bench_function("full_ring_dedup_scan_256", |b| {
        b.iter(|| {
            // Need to reset because the order gets added to ring after first eval
            gate.clear_dups();
            for i in 0..256u64 {
                let order = make_order(50.0 + i as f64 * 0.01, 1, 0, i);
                gate.evaluate(&order, 0.0, i);
            }
            gate.reset_all_credit();
            gate.evaluate(&new_order, 0.0, 1000)
        })
    });
}

/// Large trader set causes cache pressure: [f64; 4096] = 32KB (exceeds L1 on most CPUs).
fn bench_large_trader_set_cache_pressure(c: &mut Criterion) {
    let config = RiskConfig {
        credit_limit: f64::MAX,
        max_notional: f64::MAX,
        max_quantity: u64::MAX,
        collar_lower: 0.0,
        collar_upper: f64::MAX,
        dup_window_ns: 0,
    };
    let mut gate = RiskGate::<4096, 64>::new(config);

    let mut i = 0u64;
    c.bench_function("large_trader_4096_cache_pressure", |b| {
        b.iter(|| {
            i += 1;
            let trader = (i % 4096) as u32;
            let order = make_order(100.0, 1, trader, i);
            gate.evaluate(&order, 0.0, i)
        })
    });
}

/// Alternating accept and reject to stress branch predictor.
fn bench_alternating_accept_reject(c: &mut Criterion) {
    let config = RiskConfig {
        max_quantity: 100,
        credit_limit: f64::MAX,
        ..Default::default()
    };
    let accept_order = make_order(100.0, 50, 0, 1);
    let reject_order = make_order(100.0, 101, 0, 2); // qty > max

    let mut gate = RiskGate::<16, 16>::new(config);
    let mut toggle = false;

    c.bench_function("alternating_accept_reject", |b| {
        b.iter(|| {
            toggle = !toggle;
            if toggle {
                gate.reset_all_credit();
                gate.clear_dups();
                gate.evaluate(&accept_order, 0.0, 0)
            } else {
                gate.evaluate(&reject_order, 0.0, 1)
            }
        })
    });
}

criterion_group!(
    adversarial,
    bench_worst_case_accept,
    bench_nan_price_shortest_reject,
    bench_full_ring_dedup_scan,
    bench_large_trader_set_cache_pressure,
    bench_alternating_accept_reject,
);
criterion_main!(adversarial);
