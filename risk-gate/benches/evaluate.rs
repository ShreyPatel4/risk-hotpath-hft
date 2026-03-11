use criterion::{criterion_group, criterion_main, Criterion};
use risk_gate::{Order, RiskConfig, RiskGate, Side};

fn bench_evaluate_accept(c: &mut Criterion) {
    let config = RiskConfig::default();
    let mut gate = RiskGate::<256, 64>::new(config);
    let order = Order {
        order_id: 1,
        symbol_id: 0,
        trader_id: 0,
        price: 150.0,
        quantity: 100,
        side: Side::Buy,
    };
    c.bench_function("gate_evaluate_accept", |b| {
        b.iter(|| {
            gate.reset_all_credit();
            gate.clear_dups();
            gate.evaluate(&order, 150.0, 0)
        })
    });
}

fn bench_evaluate_reject_qty(c: &mut Criterion) {
    let config = RiskConfig {
        max_quantity: 50,
        ..Default::default()
    };
    let mut gate = RiskGate::<256, 64>::new(config);
    let order = Order {
        order_id: 1,
        symbol_id: 0,
        trader_id: 0,
        price: 150.0,
        quantity: 100, // exceeds max
        side: Side::Buy,
    };
    c.bench_function("gate_evaluate_reject_qty", |b| {
        b.iter(|| gate.evaluate(&order, 150.0, 0))
    });
}

fn bench_evaluate_full_pipeline(c: &mut Criterion) {
    let config = RiskConfig::default();
    let mut gate = RiskGate::<1024, 256>::new(config);
    let mut id = 0u64;
    c.bench_function("gate_full_pipeline", |b| {
        b.iter(|| {
            id += 1;
            let order = Order {
                order_id: id,
                symbol_id: (id % 120) as u32,
                trader_id: (id % 50) as u32,
                price: 150.0 + (id % 10) as f64 * 0.01,
                quantity: 100,
                side: if id.is_multiple_of(2) {
                    Side::Buy
                } else {
                    Side::Sell
                },
            };
            gate.evaluate(&order, 150.0, id * 1000)
        })
    });
}

fn bench_credit_check(c: &mut Criterion) {
    let mut tracker = risk_gate::credit::CreditTracker::<1024>::new();
    c.bench_function("credit_check_and_update", |b| {
        b.iter(|| {
            tracker.reset(0);
            tracker.check_and_update(0, 1000.0, 10_000_000.0)
        })
    });
}

criterion_group!(
    benches,
    bench_evaluate_accept,
    bench_evaluate_reject_qty,
    bench_evaluate_full_pipeline,
    bench_credit_check,
);
criterion_main!(benches);
