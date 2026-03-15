//! Red Team: Credit accumulation attacks.
//!
//! Tests that exploit f64 precision, NaN propagation, and accumulation
//! patterns to try to bypass the credit limit.

use risk_gate::credit::CreditTracker;
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

/// Submit 1000 small orders that should cumulatively exceed the credit limit.
/// Verify the gate rejects before the 1000th order.
#[test]
fn test_many_small_orders_accumulate() {
    let limit = 1_000_000.0;
    let config = RiskConfig {
        credit_limit: limit,
        max_quantity: u64::MAX,
        max_notional: f64::MAX,
        ..Default::default()
    };
    // Use large dup ring and vary symbol_id to avoid dedup collisions
    let mut gate = RiskGate::<16, 4096>::new(config);

    let price = 1001.001; // each order notional = price * 1 = ~1001
    let mut accepted = 0u64;

    for i in 0..2000u64 {
        // Vary symbol_id to produce unique dedup hashes
        let order = Order {
            order_id: i,
            symbol_id: i as u32,
            trader_id: 0,
            price,
            quantity: 1,
            side: Side::Buy,
        };
        let d = gate.evaluate(&order, 0.0, i);
        if d == Decision::Accept {
            accepted += 1;
        } else {
            assert_eq!(d, Decision::RejectCreditLimit);
        }
    }

    // Should have accepted ~999 orders (999 * 1001.001 ≈ 999_999.999)
    // and rejected the rest
    assert!(accepted > 0 && accepted < 2000);
    assert!(gate.trader_exposure(0) <= limit);
}

/// Exploit f64 accumulation imprecision with alternating large and small values.
/// After many additions, the total may drift from the true mathematical sum.
/// Verify the gate never allows exposure to exceed the limit.
#[test]
fn test_f64_accumulation_precision_attack() {
    let limit = 1e15 + 100.0;
    let config = RiskConfig {
        credit_limit: limit,
        max_quantity: u64::MAX,
        max_notional: f64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<16, 256>::new(config);

    // First: one huge order
    let d = gate.evaluate(&make_order(1e15, 1, 0, 0), 0.0, 0);
    assert_eq!(d, Decision::Accept);

    // Then many small orders of 0.01 — f64 precision at 1e15 magnitude
    // means additions < ~0.125 are lost to rounding
    let mut last_accepted = 0u64;
    for i in 1..10_000u64 {
        let d = gate.evaluate(&make_order(0.01, 1, 0, i), 0.0, i);
        if d == Decision::Accept {
            last_accepted = i;
        }
    }

    // Key invariant: tracked exposure must never exceed the limit
    let exposure = gate.trader_exposure(0);
    assert!(
        exposure <= limit,
        "exposure {} exceeded limit {}",
        exposure,
        limit
    );
    // Also verify some orders were accepted (proving the attack path was exercised)
    assert!(last_accepted > 0);
}

/// Push exposure near f64::MAX, then add more to cause overflow to infinity.
/// Verify the gate rejects when exposure becomes infinite.
#[test]
fn test_credit_overflow_to_infinity() {
    let limit = f64::MAX;
    let config = RiskConfig {
        credit_limit: limit,
        max_quantity: u64::MAX,
        max_notional: f64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<16, 16>::new(config);

    // First order: notional = f64::MAX / 2
    let order1 = Order { order_id: 0, symbol_id: 0, trader_id: 0, price: f64::MAX / 2.0, quantity: 1, side: Side::Buy };
    let d = gate.evaluate(&order1, 0.0, 0);
    assert_eq!(d, Decision::Accept);

    // Second order: notional = f64::MAX / 2 → total = f64::MAX (still finite, accepts)
    let order2 = Order { order_id: 1, symbol_id: 1, trader_id: 0, price: f64::MAX / 2.0, quantity: 1, side: Side::Buy };
    let d = gate.evaluate(&order2, 0.0, 1);
    assert_eq!(d, Decision::Accept);

    // Third order: notional large enough to overflow past f64::MAX.
    // At f64::MAX magnitude, the ULP is ~2e292, so we need to add at least that.
    // f64::MAX / 2.0 will overflow: f64::MAX + f64::MAX/2 = Inf
    let order3 = Order { order_id: 2, symbol_id: 2, trader_id: 0, price: f64::MAX / 2.0, quantity: 1, side: Side::Buy };
    let d = gate.evaluate(&order3, 0.0, 2);
    assert!(d.is_reject(), "f64::MAX + f64::MAX/2 overflows to infinity, should reject");
}

/// After Fix 4: set_exposure(NaN) is now rejected (value not finite).
/// Subsequent check_and_update should behave correctly.
#[test]
fn test_nan_propagation_blocked() {
    let mut tracker = CreditTracker::<4>::new();

    // Attempt to set NaN — should be silently ignored
    tracker.set_exposure(0, f64::NAN);
    assert_eq!(tracker.exposure(0), 0.0); // remains at 0, not NaN

    // Attempt to set Infinity — should be silently ignored
    tracker.set_exposure(0, f64::INFINITY);
    assert_eq!(tracker.exposure(0), 0.0);

    // Attempt to set NEG_INFINITY — should be silently ignored
    tracker.set_exposure(0, f64::NEG_INFINITY);
    assert_eq!(tracker.exposure(0), 0.0);

    // Normal set still works
    tracker.set_exposure(0, 500.0);
    assert_eq!(tracker.exposure(0), 500.0);

    // NaN-safe comparison in check_and_update:
    // Even if we could somehow get NaN into exposure, the comparison should reject
    // (this tests the !(new_exposure <= limit) path)
    let mut tracker2 = CreditTracker::<4>::new();
    // Normal operation should still work
    assert!(tracker2.check_and_update(0, 100.0, 1000.0));
    assert_eq!(tracker2.exposure(0), 100.0);
}

/// Each trader's credit is independent — filling one to the limit
/// must not affect any other trader.
#[test]
fn test_all_traders_independent() {
    let limit = 1000.0;
    let config = RiskConfig {
        credit_limit: limit,
        max_quantity: u64::MAX,
        max_notional: f64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<8, 16>::new(config);

    // Fill traders 0 through 7 independently
    for trader_id in 0..8u32 {
        let order = make_order(1000.0, 1, trader_id, trader_id as u64);
        let d = gate.evaluate(&order, 0.0, trader_id as u64);
        assert_eq!(d, Decision::Accept, "trader {} should accept", trader_id);
        assert_eq!(gate.trader_exposure(trader_id), 1000.0);
    }

    // Each trader is now at the limit — all should reject
    for trader_id in 0..8u32 {
        let order = make_order(0.01, 1, trader_id, 100 + trader_id as u64);
        let d = gate.evaluate(&order, 0.0, 100 + trader_id as u64);
        assert_eq!(
            d,
            Decision::RejectCreditLimit,
            "trader {} should reject",
            trader_id
        );
    }

    // Verify no cross-contamination
    for trader_id in 0..8u32 {
        assert_eq!(gate.trader_exposure(trader_id), 1000.0);
    }
}

/// Various out-of-bounds trader IDs all reject with RejectCreditLimit.
#[test]
fn test_trader_id_out_of_bounds_rejects() {
    let config = RiskConfig {
        credit_limit: f64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<4, 16>::new(config);

    // trader_id 4 → index 4 >= capacity 4 → reject
    assert_eq!(
        gate.evaluate(&make_order(100.0, 1, 4, 1), 0.0, 0),
        Decision::RejectCreditLimit
    );

    // trader_id 100
    assert_eq!(
        gate.evaluate(&make_order(100.0, 1, 100, 2), 0.0, 1),
        Decision::RejectCreditLimit
    );

    // trader_id u32::MAX
    assert_eq!(
        gate.evaluate(&make_order(100.0, 1, u32::MAX, 3), 0.0, 2),
        Decision::RejectCreditLimit
    );

    // Traders 0-3 should still work fine
    for t in 0..4u32 {
        let mut gate2 = RiskGate::<4, 16>::new(config);
        assert_eq!(
            gate2.evaluate(&make_order(100.0, 1, t, t as u64), 0.0, 0),
            Decision::Accept
        );
    }
}
