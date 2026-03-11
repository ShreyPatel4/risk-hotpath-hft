use criterion::{criterion_group, criterion_main, Criterion};
use risk_core::envelope::{Action, MarketEvent, OrderRequest, Side};
use risk_core::feed::orderbook::OrderBook;
use risk_core::feed::simulator::{SimConfig, Simulator};
use risk_core::risk::checks::price_collar;
use risk_core::risk::engine::{RiskConfig, RiskEngine};

fn bench_price_collar(c: &mut Criterion) {
    c.bench_function("price_collar", |b| {
        b.iter(|| price_collar(100.0, 90.0, 110.0))
    });
}

fn bench_orderbook_apply(c: &mut Criterion) {
    c.bench_function("orderbook_apply", |b| {
        let mut book = OrderBook::new(10);
        let event = MarketEvent {
            timestamp_us: 0,
            symbol: "AAPL".to_string(),
            side: Side::Bid,
            price: 150.0,
            qty: 100,
            action: Action::Add,
        };
        b.iter(|| book.apply(&event));
    });
}

fn bench_risk_evaluate(c: &mut Criterion) {
    c.bench_function("risk_evaluate", |b| {
        let mut book = OrderBook::new(10);
        book.apply(&MarketEvent {
            timestamp_us: 0,
            symbol: "AAPL".to_string(),
            side: Side::Bid,
            price: 149.0,
            qty: 100,
            action: Action::Add,
        });
        book.apply(&MarketEvent {
            timestamp_us: 0,
            symbol: "AAPL".to_string(),
            side: Side::Ask,
            price: 151.0,
            qty: 100,
            action: Action::Add,
        });
        let mut engine = RiskEngine::new(RiskConfig::default());
        let order = OrderRequest {
            id: 1,
            timestamp_us: 1000,
            symbol: "AAPL".to_string(),
            side: Side::Bid,
            price: 150.0,
            qty: 10,
            trader_id: "T0".to_string(),
        };
        b.iter(|| {
            engine.reset_exposure();
            engine.evaluate(&order, &book)
        });
    });
}

fn bench_simulator_next(c: &mut Criterion) {
    c.bench_function("simulator_next_event", |b| {
        let mut sim = Simulator::new(SimConfig::default());
        b.iter(|| sim.next_event());
    });
}

criterion_group!(
    benches,
    bench_price_collar,
    bench_orderbook_apply,
    bench_risk_evaluate,
    bench_simulator_next
);
criterion_main!(benches);
