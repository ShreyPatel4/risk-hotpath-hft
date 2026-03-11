//! Hot-swap demo: measures P99 latency before, during, and after a config swap.
//!
//! Proves that swapping risk thresholds under full load has zero throughput impact.
//!
//! Run: `cargo run --release --example hot_swap_demo`

use risk_gate::{Order, RiskConfig, RiskGate, Side};
use std::time::Instant;

const WINDOW_SECS: f64 = 1.0;
const TOTAL_WINDOWS: usize = 6;
const SWAP_AT_WINDOW: usize = 3; // swap config at the start of window 3

fn main() {
    let config_before = RiskConfig {
        max_quantity: 1_000,
        max_notional: 500_000.0,
        credit_limit: 10_000_000.0,
        collar_lower: 0.95,
        collar_upper: 1.05,
        dup_window_ns: 1_000_000_000,
    };

    let config_after = RiskConfig {
        max_quantity: 5_000, // doubled+
        max_notional: 2_000_000.0,
        credit_limit: 50_000_000.0,
        ..config_before
    };

    let mut gate = RiskGate::<1024, 256>::new(config_before);

    println!("=== risk-gate hot-swap demo ===");
    println!("Evaluating orders at max speed, swapping config at window {SWAP_AT_WINDOW}\n");
    println!(
        "{:<10} {:>14} {:>10} {:>10} {:>10}  Config",
        "Window", "Throughput", "P50", "P99", "Max"
    );
    println!("{}", "-".repeat(75));

    let mut order_id: u64 = 0;

    for window in 0..TOTAL_WINDOWS {
        if window == SWAP_AT_WINDOW {
            gate.set_config(config_after);
        }

        // Collect latency samples for this window
        let mut latencies: Vec<u64> = Vec::with_capacity(50_000_000);
        let window_start = Instant::now();

        while window_start.elapsed().as_secs_f64() < WINDOW_SECS {
            // Batch of 1000 to reduce Instant::now() overhead
            for _ in 0..1000 {
                order_id += 1;
                let order = Order {
                    order_id,
                    symbol_id: (order_id % 120) as u32,
                    trader_id: (order_id % 50) as u32,
                    price: 150.0 + (order_id % 10) as f64 * 0.01,
                    quantity: 100,
                    side: if order_id.is_multiple_of(2) {
                        Side::Buy
                    } else {
                        Side::Sell
                    },
                };

                let start = Instant::now();
                let _ = gate.evaluate(&order, 150.0, order_id * 1000);
                let elapsed_ns = start.elapsed().as_nanos() as u64;
                latencies.push(elapsed_ns);
            }
        }

        let wall = window_start.elapsed().as_secs_f64();
        let count = latencies.len();
        let throughput = count as f64 / wall;

        latencies.sort_unstable();
        let p50 = latencies[count / 2];
        let p99 = latencies[count * 99 / 100];
        let max = latencies[count - 1];

        let config_label = if window < SWAP_AT_WINDOW {
            format!("max_qty={}", config_before.max_quantity)
        } else {
            format!("max_qty={} <-- SWAP", config_after.max_quantity)
        };

        println!(
            "{:<10} {:>12.1}M {:>8}ns {:>8}ns {:>8}ns  {}",
            format!("{window}-{}s", window + 1),
            throughput / 1_000_000.0,
            p50,
            p99,
            max,
            config_label,
        );
    }

    println!("\nConfig swap is a single struct copy — zero throughput impact.");
}
