//! Red Team: Exact boundary condition attacks.
//!
//! Every test probes the exact threshold of a configurable limit — one value
//! that should accept, and the adjacent value that should reject.

use risk_gate::engine::RiskGate;
use risk_gate::types::*;

fn make_order_with_trader(price: f64, qty: u64, trader_id: u32, order_id: u64) -> Order {
    Order {
        order_id,
        symbol_id: 0,
        trader_id,
        price,
        quantity: qty,
        side: Side::Buy,
    }
}

fn make_order(price: f64, qty: u64) -> Order {
    make_order_with_trader(price, qty, 0, 1)
}

/// qty == max_quantity accepts; qty == max_quantity + 1 rejects.
#[test]
fn test_max_quantity_exact_boundary() {
    let max = 5000u64;
    let config = RiskConfig {
        max_quantity: max,
        credit_limit: f64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<16, 16>::new(config);

    assert_eq!(
        gate.evaluate(&make_order(1.0, max), 0.0, 0),
        Decision::Accept
    );
    assert_eq!(
        gate.evaluate(&make_order(1.0, max + 1), 0.0, 1),
        Decision::RejectMaxQuantity
    );
}

/// price * qty == max_notional accepts; price * (qty+1) rejects.
#[test]
fn test_max_notional_exact_boundary() {
    // Use integer-valued price for exact f64 representation
    let config = RiskConfig {
        max_notional: 10_000.0,
        max_quantity: u64::MAX,
        credit_limit: f64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<16, 16>::new(config);

    // 100.0 * 100 = 10_000.0 == max_notional → accept
    assert_eq!(
        gate.evaluate(&make_order(100.0, 100), 0.0, 0),
        Decision::Accept
    );

    // 100.0 * 101 = 10_100.0 > max_notional → reject
    let mut gate2 = RiskGate::<16, 16>::new(config);
    assert_eq!(
        gate2.evaluate(&make_order(100.0, 101), 0.0, 0),
        Decision::RejectMaxNotional
    );
}

/// One ULP below collar lower bound rejects; exactly at lower bound accepts.
#[test]
fn test_collar_exact_both_sides_ulp() {
    let config = RiskConfig {
        collar_lower: 0.95,
        collar_upper: 1.05,
        credit_limit: f64::MAX,
        ..Default::default()
    };
    let ref_price = 200.0;
    let lower = ref_price * 0.95; // 190.0
    let upper = ref_price * 1.05; // 210.0

    let mut gate = RiskGate::<16, 16>::new(config);

    // Exactly at lower → accept
    assert_eq!(
        gate.evaluate(&make_order(lower, 1), ref_price, 0),
        Decision::Accept
    );

    // One ULP below lower → reject
    let below_lower = f64::from_bits(lower.to_bits() - 1);
    assert_eq!(
        gate.evaluate(&make_order(below_lower, 1), ref_price, 1),
        Decision::RejectPriceCollar
    );

    // Exactly at upper → accept
    assert_eq!(
        gate.evaluate(&make_order(upper, 1), ref_price, 2),
        Decision::Accept
    );

    // One ULP above upper → reject
    let above_upper = f64::from_bits(upper.to_bits() + 1);
    assert_eq!(
        gate.evaluate(&make_order(above_upper, 1), ref_price, 3),
        Decision::RejectPriceCollar
    );
}

/// Fill credit to exactly limit - notional, then submit exact notional → accept.
/// Then any further positive notional → reject.
#[test]
fn test_credit_exactly_at_limit() {
    let limit = 1000.0;
    let config = RiskConfig {
        credit_limit: limit,
        ..Default::default()
    };
    let mut gate = RiskGate::<16, 16>::new(config);

    // First order: notional = 100.0 * 9 = 900.0
    let d = gate.evaluate(&make_order(100.0, 9), 0.0, 0);
    assert_eq!(d, Decision::Accept);
    assert_eq!(gate.trader_exposure(0), 900.0);

    // Second order: notional = 100.0 * 1 = 100.0, total = 1000.0 == limit → accept
    let d = gate.evaluate(&make_order(100.0, 1), 0.0, 1);
    assert_eq!(d, Decision::Accept);
    assert_eq!(gate.trader_exposure(0), 1000.0);

    // Third order: any positive notional → reject
    let d = gate.evaluate(&make_order(0.01, 1), 0.0, 2);
    assert_eq!(d, Decision::RejectCreditLimit);
}

/// u64::MAX quantity with max_quantity=u64::MAX passes qty check but fails notional
/// (because price * u64::MAX overflows to infinity).
#[test]
fn test_u64_max_quantity() {
    let config = RiskConfig {
        max_quantity: u64::MAX,
        max_notional: 1e18,
        credit_limit: f64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<16, 16>::new(config);

    // price=1.0, qty=u64::MAX → notional = 1.8e19 > 1e18 → reject notional
    let d = gate.evaluate(&make_order(1.0, u64::MAX), 0.0, 0);
    assert_eq!(d, Decision::RejectMaxNotional);
}

/// trader_id=u32::MAX cast to usize is 4294967295, which exceeds any reasonable
/// TRADERS capacity → RejectCreditLimit.
#[test]
fn test_u32_max_trader_id() {
    let config = RiskConfig {
        credit_limit: f64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<256, 16>::new(config);

    let order = make_order_with_trader(100.0, 1, u32::MAX, 1);
    let d = gate.evaluate(&order, 0.0, 0);
    assert_eq!(d, Decision::RejectCreditLimit);
}

/// symbol_id=u32::MAX should only affect the dedup hash, not cause any panic.
#[test]
fn test_u32_max_symbol_id() {
    let mut gate = RiskGate::<16, 16>::new(RiskConfig {
        credit_limit: f64::MAX,
        ..Default::default()
    });

    let order = Order {
        order_id: 1,
        symbol_id: u32::MAX,
        trader_id: 0,
        price: 100.0,
        quantity: 1,
        side: Side::Buy,
    };

    // Should not panic
    let d = gate.evaluate(&order, 0.0, 0);
    assert_eq!(d, Decision::Accept);
}
