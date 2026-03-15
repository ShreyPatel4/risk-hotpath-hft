//! Red Team: IEEE 754 float edge-case attacks.
//!
//! Every test here attempts to exploit floating-point edge cases to bypass
//! or crash the risk gate. After hardening, all should pass cleanly.

use risk_gate::engine::RiskGate;
use risk_gate::types::*;

fn valid_config() -> RiskConfig {
    RiskConfig::default()
}

fn make_order(price: f64, qty: u64) -> Order {
    Order {
        order_id: 1,
        symbol_id: 0,
        trader_id: 0,
        price,
        quantity: qty,
        side: Side::Buy,
    }
}

/// All NaN bit patterns must be rejected as invalid price.
#[test]
fn test_nan_payloads_all_variants() {
    let mut gate = RiskGate::<16, 16>::new(valid_config());

    // Standard NaN
    assert_eq!(
        gate.evaluate(&make_order(f64::NAN, 10), 100.0, 0),
        Decision::RejectInvalidPrice
    );

    // Quiet NaN (positive)
    let qnan_pos = f64::from_bits(0x7FF8_0000_0000_0001);
    assert!(qnan_pos.is_nan());
    assert_eq!(
        gate.evaluate(&make_order(qnan_pos, 10), 100.0, 0),
        Decision::RejectInvalidPrice
    );

    // Quiet NaN (negative)
    let qnan_neg = f64::from_bits(0xFFF8_0000_0000_0001);
    assert!(qnan_neg.is_nan());
    assert_eq!(
        gate.evaluate(&make_order(qnan_neg, 10), 100.0, 0),
        Decision::RejectInvalidPrice
    );

    // Signaling NaN (positive)
    let snan_pos = f64::from_bits(0x7FF0_0000_0000_0001);
    assert!(snan_pos.is_nan());
    assert_eq!(
        gate.evaluate(&make_order(snan_pos, 10), 100.0, 0),
        Decision::RejectInvalidPrice
    );

    // Signaling NaN (negative)
    let snan_neg = f64::from_bits(0xFFF0_0000_0000_0001);
    assert!(snan_neg.is_nan());
    assert_eq!(
        gate.evaluate(&make_order(snan_neg, 10), 100.0, 0),
        Decision::RejectInvalidPrice
    );

    // Custom NaN payload (max payload)
    let custom_nan = f64::from_bits(0x7FFF_FFFF_FFFF_FFFF);
    assert!(custom_nan.is_nan());
    assert_eq!(
        gate.evaluate(&make_order(custom_nan, 10), 100.0, 0),
        Decision::RejectInvalidPrice
    );
}

/// Subnormal prices are finite and positive — they pass price validation
/// but should still be subject to notional/collar checks.
#[test]
fn test_subnormal_prices() {
    let config = RiskConfig {
        max_notional: 1.0, // very low limit
        ..valid_config()
    };
    let mut gate = RiskGate::<16, 16>::new(config);

    let subnormal = f64::MIN_POSITIVE * 0.5;
    assert!(subnormal > 0.0 && subnormal.is_finite());
    assert!(subnormal < f64::MIN_POSITIVE); // truly subnormal

    // Subnormal price * 1 qty = tiny notional, should pass notional check
    let order = make_order(subnormal, 1);
    let d = gate.evaluate(&order, 0.0, 0); // no ref price → collar skipped
    assert_eq!(d, Decision::Accept);
}

/// Infinity prices (both signs) must be rejected as invalid.
#[test]
fn test_infinity_both_signs() {
    let mut gate = RiskGate::<16, 16>::new(valid_config());

    assert_eq!(
        gate.evaluate(&make_order(f64::INFINITY, 10), 100.0, 0),
        Decision::RejectInvalidPrice
    );
    assert_eq!(
        gate.evaluate(&make_order(f64::NEG_INFINITY, 10), 100.0, 0),
        Decision::RejectInvalidPrice
    );

    // Infinity as ref_price should cause collar to be skipped (ref_price not finite)
    let d = gate.evaluate(&make_order(100.0, 1), f64::INFINITY, 0);
    assert_eq!(d, Decision::Accept);

    // Use different price to avoid dedup collision with above
    let d = gate.evaluate(&make_order(101.0, 1), f64::NEG_INFINITY, 1);
    assert_eq!(d, Decision::Accept);
}

/// f64::MAX price with u64::MAX quantity overflows notional to infinity → reject.
#[test]
fn test_max_f64_price_overflows_notional() {
    let config = RiskConfig {
        max_quantity: u64::MAX,
        ..valid_config()
    };
    let mut gate = RiskGate::<16, 16>::new(config);

    let order = make_order(f64::MAX, u64::MAX);
    let notional = f64::MAX * (u64::MAX as f64);
    assert!(notional.is_infinite()); // confirms overflow

    let d = gate.evaluate(&order, 0.0, 0);
    assert_eq!(d, Decision::RejectMaxNotional);
}

/// Test prices at exact ULP boundaries of collar edges.
#[test]
fn test_epsilon_at_collar_boundaries() {
    let config = RiskConfig {
        collar_lower: 0.95,
        collar_upper: 1.05,
        credit_limit: f64::MAX,
        ..valid_config()
    };
    let ref_price = 100.0;
    let upper_bound = ref_price * config.collar_upper; // 105.0
    let lower_bound = ref_price * config.collar_lower; // 95.0

    // At upper bound: accept
    let mut gate = RiskGate::<16, 16>::new(config);
    assert_eq!(
        gate.evaluate(&make_order(upper_bound, 1), ref_price, 0),
        Decision::Accept
    );

    // One ULP above upper bound: reject
    let one_ulp_above = f64::from_bits(upper_bound.to_bits() + 1);
    assert_eq!(
        gate.evaluate(&make_order(one_ulp_above, 1), ref_price, 1),
        Decision::RejectPriceCollar
    );

    // At lower bound: accept
    assert_eq!(
        gate.evaluate(&make_order(lower_bound, 1), ref_price, 2),
        Decision::Accept
    );

    // One ULP below lower bound: reject
    let one_ulp_below = f64::from_bits(lower_bound.to_bits() - 1);
    assert_eq!(
        gate.evaluate(&make_order(one_ulp_below, 1), ref_price, 3),
        Decision::RejectPriceCollar
    );
}

/// Negative zero is not > 0.0 in IEEE 754, so it must be rejected.
#[test]
fn test_negative_zero_price() {
    let mut gate = RiskGate::<16, 16>::new(valid_config());
    let neg_zero = -0.0_f64;
    assert!(neg_zero == 0.0); // -0.0 == 0.0 is true
    assert!(!neg_zero.is_sign_positive() || neg_zero == 0.0); // -0.0 is not positive

    assert_eq!(
        gate.evaluate(&make_order(neg_zero, 10), 100.0, 0),
        Decision::RejectInvalidPrice
    );
}

/// After Fix 1: NaN in any config field triggers RejectInvalidConfig.
#[test]
fn test_nan_config_now_rejected() {
    let valid_order = make_order(100.0, 10);

    // NaN in max_notional
    let mut gate = RiskGate::<16, 16>::new(RiskConfig {
        max_notional: f64::NAN,
        ..valid_config()
    });
    assert_eq!(
        gate.evaluate(&valid_order, 100.0, 0),
        Decision::RejectInvalidConfig
    );

    // NaN in credit_limit — previously this silently accepted all orders!
    let mut gate = RiskGate::<16, 16>::new(RiskConfig {
        credit_limit: f64::NAN,
        ..valid_config()
    });
    assert_eq!(
        gate.evaluate(&valid_order, 100.0, 0),
        Decision::RejectInvalidConfig
    );

    // NaN in collar_lower
    let mut gate = RiskGate::<16, 16>::new(RiskConfig {
        collar_lower: f64::NAN,
        ..valid_config()
    });
    assert_eq!(
        gate.evaluate(&valid_order, 100.0, 0),
        Decision::RejectInvalidConfig
    );

    // NaN in collar_upper
    let mut gate = RiskGate::<16, 16>::new(RiskConfig {
        collar_upper: f64::NAN,
        ..valid_config()
    });
    assert_eq!(
        gate.evaluate(&valid_order, 100.0, 0),
        Decision::RejectInvalidConfig
    );
}

/// Infinity in config fields: Inf is not finite → RejectInvalidConfig.
#[test]
fn test_infinity_config_fields() {
    let valid_order = make_order(100.0, 10);

    // +Inf max_notional
    let mut gate = RiskGate::<16, 16>::new(RiskConfig {
        max_notional: f64::INFINITY,
        ..valid_config()
    });
    assert_eq!(
        gate.evaluate(&valid_order, 100.0, 0),
        Decision::RejectInvalidConfig
    );

    // -Inf credit_limit
    let mut gate = RiskGate::<16, 16>::new(RiskConfig {
        credit_limit: f64::NEG_INFINITY,
        ..valid_config()
    });
    assert_eq!(
        gate.evaluate(&valid_order, 100.0, 0),
        Decision::RejectInvalidConfig
    );
}
