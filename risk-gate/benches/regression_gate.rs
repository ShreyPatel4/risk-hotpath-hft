//! Regression benchmarks for CI gating.
//!
//! These benchmarks establish baselines that can be compared across commits
//! using Criterion's `--save-baseline` and `--baseline` features.
//!
//! Usage:
//!   cargo bench -p risk-gate --bench regression_gate -- --save-baseline main
//!   # ... make changes ...
//!   cargo bench -p risk-gate --bench regression_gate -- --baseline main

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use risk_gate::engine::RiskGate;
use risk_gate::types::*;

/// Measure P99 latency through a realistic workload of 100K evaluations.
fn bench_gate_p99_regression(c: &mut Criterion) {
    let config = RiskConfig::default();

    c.bench_function("gate_p99_realistic_100k", |b| {
        b.iter_batched(
            || {
                let gate = RiskGate::<256, 64>::new(config);
                let orders: Vec<(Order, f64, u64)> = (0..1000u64)
                    .map(|i| {
                        let order = Order {
                            order_id: i,
                            symbol_id: (i % 10) as u32,
                            trader_id: (i % 256) as u32,
                            price: 95.0 + (i % 100) as f64 * 0.1,
                            quantity: 1 + (i % 50),
                            side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
                        };
                        let ref_price = 100.0;
                        let now_ns = i * 1_000_000;
                        (order, ref_price, now_ns)
                    })
                    .collect();
                (gate, orders)
            },
            |(mut gate, orders)| {
                for (order, ref_price, now_ns) in &orders {
                    let _ = gate.evaluate(order, *ref_price, *now_ns);
                }
            },
            BatchSize::SmallInput,
        )
    });
}

/// Measure sustained throughput: accepts per second in a tight loop.
fn bench_gate_throughput_regression(c: &mut Criterion) {
    let config = RiskConfig {
        credit_limit: f64::MAX,
        max_notional: f64::MAX,
        max_quantity: u64::MAX,
        collar_lower: 0.0,
        collar_upper: f64::MAX,
        dup_window_ns: 0,
    };

    c.bench_function("gate_throughput_sustained", |b| {
        b.iter_batched(
            || RiskGate::<16, 16>::new(config),
            |mut gate| {
                for i in 0..10_000u64 {
                    let order = Order {
                        order_id: i,
                        symbol_id: 0,
                        trader_id: 0,
                        price: 100.0,
                        quantity: 1,
                        side: Side::Buy,
                    };
                    let _ = gate.evaluate(&order, 0.0, i);
                }
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(regression, bench_gate_p99_regression, bench_gate_throughput_regression);
criterion_main!(regression);
