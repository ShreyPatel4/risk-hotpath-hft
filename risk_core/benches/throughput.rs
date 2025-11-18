use criterion::{criterion_group, criterion_main, Criterion};
use risk_core::risk::checks::price_collar;

fn bench_price_collar(c: &mut Criterion) {
    c.bench_function("price_collar", |b| {
        b.iter(|| price_collar(100.0, 90.0, 110.0))
    });
}

criterion_group!(benches, bench_price_collar);
criterion_main!(benches);
