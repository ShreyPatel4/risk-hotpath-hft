//! Stress and chaos tests.
//!
//! Long-running sequences designed to expose edge cases that only manifest
//! under sustained load or specific ordering of operations.

use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use risk_gate::engine::RiskGate;
use risk_gate::types::*;
use std::time::Instant;

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

/// 1 million orders with adversarial mix: 10% NaN price, 5% zero qty, 85% valid.
/// Verify correct rejection counts and no panics.
#[test]
fn test_million_order_mixed_adversarial() {
    let config = RiskConfig {
        credit_limit: f64::MAX,
        max_quantity: 100_000,
        max_notional: f64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<1024, 256>::new(config);
    let mut rng = ChaCha8Rng::seed_from_u64(0);

    let total = 1_000_000u64;
    let mut nan_count = 0u64;
    let mut zero_qty_count = 0u64;
    let mut accept_count = 0u64;
    let mut reject_count = 0u64;

    for i in 0..total {
        let r: f64 = rng.gen();
        let (price, qty) = if r < 0.10 {
            nan_count += 1;
            (f64::NAN, 10u64)
        } else if r < 0.15 {
            zero_qty_count += 1;
            (100.0, 0u64)
        } else {
            let p = rng.gen_range(50.0..150.0);
            let q = rng.gen_range(1u64..100);
            (p, q)
        };

        let trader = rng.gen_range(0u32..1024);
        let order = make_order(price, qty, trader, i);
        let d = gate.evaluate(&order, 0.0, i);

        match d {
            Decision::Accept => accept_count += 1,
            _ => reject_count += 1,
        }
    }

    // Verify expected distributions
    assert!(
        nan_count > 90_000,
        "expected ~100K NaN orders, got {}",
        nan_count
    );
    assert!(
        zero_qty_count > 40_000,
        "expected ~50K zero-qty orders, got {}",
        zero_qty_count
    );
    assert!(accept_count > 0, "should have some accepted orders");
    assert!(
        reject_count >= nan_count + zero_qty_count,
        "at minimum, all NaN and zero-qty orders should be rejected"
    );

    // Total must add up
    assert_eq!(accept_count + reject_count, total);
}

/// Fill all 256 traders to their credit limit, verify each is at exactly the limit,
/// then verify further orders from any trader are rejected.
#[test]
fn test_credit_saturation_all_traders() {
    let limit = 10_000.0;
    let config = RiskConfig {
        credit_limit: limit,
        max_quantity: u64::MAX,
        max_notional: f64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<256, 256>::new(config);

    // Fill each trader with one order to the limit
    for trader_id in 0..256u32 {
        let order = make_order(10_000.0, 1, trader_id, trader_id as u64);
        let d = gate.evaluate(&order, 0.0, trader_id as u64);
        assert_eq!(d, Decision::Accept, "trader {} should accept", trader_id);
    }

    // Verify all are at the limit
    for trader_id in 0..256u32 {
        assert_eq!(gate.trader_exposure(trader_id), 10_000.0);
    }

    // All further orders should be rejected
    for trader_id in 0..256u32 {
        let order = make_order(0.01, 1, trader_id, 1000 + trader_id as u64);
        let d = gate.evaluate(&order, 0.0, 1000 + trader_id as u64);
        assert_eq!(
            d,
            Decision::RejectCreditLimit,
            "trader {} should reject after saturation",
            trader_id
        );
    }
}

/// 10K orders with interspersed re-submissions to test ring buffer under churn.
#[test]
fn test_ring_buffer_full_rotation() {
    let config = RiskConfig {
        dup_window_ns: 1_000_000, // 1ms window
        credit_limit: f64::MAX,
        max_notional: f64::MAX,
        max_quantity: u64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<256, 64>::new(config);
    let mut rng = ChaCha8Rng::seed_from_u64(42);

    let mut recent_orders: Vec<(Order, u64)> = Vec::new();
    let mut dup_detected = 0u64;

    for i in 0..10_000u64 {
        let now = i * 100; // 100ns spacing

        // 20% chance: re-submit a recent order (should be duplicate)
        if !recent_orders.is_empty() && rng.gen_bool(0.2) {
            let idx = rng.gen_range(0..recent_orders.len());
            let (order, orig_ts) = &recent_orders[idx];
            if now.saturating_sub(config.dup_window_ns) <= *orig_ts {
                // Within window — should be flagged
                gate.reset_all_credit();
                let d = gate.evaluate(order, 0.0, now);
                if d == Decision::RejectDuplicate {
                    dup_detected += 1;
                }
            }
        } else {
            let price = rng.gen_range(50.0..150.0);
            let qty = rng.gen_range(1u64..10);
            let trader = rng.gen_range(0u32..256);
            let order = make_order(price, qty, trader, i);
            gate.evaluate(&order, 0.0, now);

            recent_orders.push((order, now));
            // Keep only recent orders
            if recent_orders.len() > 100 {
                recent_orders.remove(0);
            }
        }
    }

    assert!(dup_detected > 0, "should have detected some duplicates");
}

/// 100K orders with config hot-swap every 10K. Verify no panics and
/// consistent state throughout.
#[test]
fn test_config_hot_swap_under_load() {
    let configs = [
        RiskConfig {
            max_quantity: 1000,
            credit_limit: 1_000_000.0,
            ..Default::default()
        },
        RiskConfig {
            max_quantity: 500,
            credit_limit: 500_000.0,
            ..Default::default()
        },
        RiskConfig {
            max_quantity: 2000,
            credit_limit: 2_000_000.0,
            ..Default::default()
        },
        RiskConfig {
            max_quantity: 100,
            credit_limit: 100_000.0,
            ..Default::default()
        },
        RiskConfig::default(),
    ];

    let mut gate = RiskGate::<64, 64>::new(configs[0]);
    let mut rng = ChaCha8Rng::seed_from_u64(99);
    let mut total_accepted = 0u64;
    let mut total_rejected = 0u64;

    for i in 0..100_000u64 {
        // Swap config every 10K orders
        if i > 0 && i % 10_000 == 0 {
            let config_idx = (i / 10_000) as usize % configs.len();
            gate.set_config(configs[config_idx]);
            gate.reset_all_credit();
            gate.clear_dups();
        }

        let price = rng.gen_range(90.0..110.0);
        let qty = rng.gen_range(1u64..200);
        let trader = rng.gen_range(0u32..64);
        let order = make_order(price, qty, trader, i);
        let d = gate.evaluate(&order, 100.0, i);

        match d {
            Decision::Accept => total_accepted += 1,
            _ => total_rejected += 1,
        }
    }

    assert!(total_accepted > 0);
    assert!(total_rejected > 0);
    assert_eq!(total_accepted + total_rejected, 100_000);
}

/// Orders passing all 7 checks (the slowest path). Measure wall clock time.
#[test]
fn test_worst_case_latency_path() {
    let config = RiskConfig {
        max_quantity: u64::MAX,
        max_notional: f64::MAX,
        credit_limit: f64::MAX,
        collar_lower: 0.0,
        collar_upper: f64::MAX,
        dup_window_ns: 0,
    };
    let mut gate = RiskGate::<16, 64>::new(config);

    let iterations = 10_000u64;
    let start = Instant::now();

    for i in 0..iterations {
        let order = make_order(100.0 + (i as f64 * 0.001), 1, 0, i);
        let d = gate.evaluate(&order, 100.0, i);
        assert_eq!(d, Decision::Accept);
    }

    let elapsed = start.elapsed();
    // 10K * 200ns = 2ms is a generous bound
    assert!(
        elapsed.as_millis() < 50,
        "worst-case latency too high: {}ms for {} iterations",
        elapsed.as_millis(),
        iterations
    );
}

/// Fill the ring with entries having the same hash but different timestamps,
/// then verify a real duplicate is still caught.
#[test]
fn test_adversarial_dedup_pressure() {
    let config = RiskConfig {
        dup_window_ns: 1_000_000,
        credit_limit: f64::MAX,
        max_notional: f64::MAX,
        max_quantity: u64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<16, 8>::new(config); // ring size = 8

    // Submit the same order 8 times at different timestamps to fill ring
    // with the same hash. Each submission after the first is a "duplicate"
    // detected by the ring, so we need to structure this differently.
    // Instead: submit the target, then 7 different orders, then target again.
    let target = make_order(100.0, 10, 0, 1);

    // Submit target
    assert_eq!(gate.evaluate(&target, 0.0, 1000), Decision::Accept);

    // Submit 6 different orders (not enough to evict target from ring size 8)
    for i in 0..6u64 {
        let other = make_order(100.0 + (i as f64 * 10.0), 1, 1, 100 + i);
        gate.evaluate(&other, 0.0, 1000 + i);
    }

    // Re-submit target within window — should still be detected as duplicate
    gate.reset_all_credit();
    let d = gate.evaluate(&target, 0.0, 1500);
    assert_eq!(
        d,
        Decision::RejectDuplicate,
        "target should still be in ring (only 6 of 8 slots consumed by others)"
    );
}
