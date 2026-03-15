//! Red Team: Adversarial RiskConfig values.
//!
//! Tests that probe what happens with extreme, nonsensical, or weaponized
//! configuration values.

use risk_gate::engine::RiskGate;
use risk_gate::types::*;

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

/// Config with all zeros: every order should be rejected.
#[test]
fn test_config_all_zeros() {
    let config = RiskConfig {
        max_quantity: 0,
        max_notional: 0.0,
        credit_limit: 0.0,
        collar_lower: 0.0,
        collar_upper: 0.0,
        dup_window_ns: 0,
    };
    let mut gate = RiskGate::<16, 16>::new(config);

    // qty=1 > max_quantity=0 → reject
    let d = gate.evaluate(&make_order(100.0, 1), 100.0, 0);
    assert_eq!(d, Decision::RejectMaxQuantity);

    // qty=0 → RejectZeroQuantity (before max_quantity check)
    let d = gate.evaluate(&make_order(100.0, 0), 100.0, 0);
    assert_eq!(d, Decision::RejectZeroQuantity);
}

/// Config with all max values: most orders should pass.
#[test]
fn test_config_all_max() {
    let config = RiskConfig {
        max_quantity: u64::MAX,
        max_notional: f64::MAX,
        credit_limit: f64::MAX,
        collar_lower: 0.0,  // widest possible band
        collar_upper: f64::MAX,
        dup_window_ns: 0, // disable dedup (window=0, only exact same ts matches)
    };
    // collar_lower=0.0 is not finite... wait, 0.0 is finite. But is_valid requires >= 0.
    // Actually 0.0 is finite and >= 0, so it passes is_valid.
    // But collar check: lower = ref_price * 0.0 = 0.0, so price >= 0.0 is always true.
    // upper = ref_price * f64::MAX — may overflow but still passes.
    // Wait — f64::MAX is finite, so config is valid? No! Let me check is_valid.
    // collar_upper must be finite. f64::MAX is finite. OK, config is valid.

    let mut gate = RiskGate::<16, 16>::new(config);

    let d = gate.evaluate(&make_order(100.0, 1000), 100.0, 0);
    assert_eq!(d, Decision::Accept);
}

/// After Fix 1: NaN in ANY f64 config field → RejectInvalidConfig.
#[test]
fn test_config_nan_all_rejected() {
    let valid_order = make_order(100.0, 10);
    let base = RiskConfig::default();

    let nan_configs = [
        RiskConfig {
            max_notional: f64::NAN,
            ..base
        },
        RiskConfig {
            credit_limit: f64::NAN,
            ..base
        },
        RiskConfig {
            collar_lower: f64::NAN,
            ..base
        },
        RiskConfig {
            collar_upper: f64::NAN,
            ..base
        },
    ];

    for (i, config) in nan_configs.iter().enumerate() {
        let mut gate = RiskGate::<16, 16>::new(*config);
        assert_eq!(
            gate.evaluate(&valid_order, 100.0, i as u64),
            Decision::RejectInvalidConfig,
            "NaN config variant {} should reject",
            i
        );
    }
}

/// Infinity in config fields: +Inf is not finite → RejectInvalidConfig.
#[test]
fn test_config_infinity_behavior() {
    let valid_order = make_order(100.0, 10);
    let base = RiskConfig::default();

    let inf_configs = [
        RiskConfig {
            max_notional: f64::INFINITY,
            ..base
        },
        RiskConfig {
            credit_limit: f64::INFINITY,
            ..base
        },
        RiskConfig {
            collar_upper: f64::INFINITY,
            ..base
        },
        RiskConfig {
            max_notional: f64::NEG_INFINITY,
            ..base
        },
        RiskConfig {
            collar_lower: f64::NEG_INFINITY,
            ..base
        },
    ];

    for (i, config) in inf_configs.iter().enumerate() {
        let mut gate = RiskGate::<16, 16>::new(*config);
        assert_eq!(
            gate.evaluate(&valid_order, 100.0, i as u64),
            Decision::RejectInvalidConfig,
            "Inf config variant {} should reject",
            i
        );
    }
}

/// Inverted collar band (lower > upper) creates an impossible acceptance range.
/// All orders should be rejected by collar.
#[test]
fn test_config_inverted_collar() {
    let config = RiskConfig {
        collar_lower: 1.10, // "lower" is actually higher
        collar_upper: 0.90, // "upper" is actually lower
        credit_limit: f64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<16, 16>::new(config);

    // ref=100: lower_bound=110, upper_bound=90
    // price >= 110 && price <= 90 is impossible
    let d = gate.evaluate(&make_order(100.0, 1), 100.0, 0);
    assert_eq!(d, Decision::RejectPriceCollar);

    let d = gate.evaluate(&make_order(90.0, 1), 100.0, 1);
    assert_eq!(d, Decision::RejectPriceCollar);

    let d = gate.evaluate(&make_order(110.0, 1), 100.0, 2);
    assert_eq!(d, Decision::RejectPriceCollar);
}

/// Config hot-swap preserves credit exposure and dedup ring state.
#[test]
fn test_config_hot_swap_preserves_state() {
    let config_a = RiskConfig {
        credit_limit: 10_000.0,
        ..Default::default()
    };
    let mut gate = RiskGate::<16, 16>::new(config_a);

    // Accumulate some credit under config A
    let order = make_order(100.0, 10); // notional = 1000
    assert_eq!(gate.evaluate(&order, 0.0, 0), Decision::Accept);
    assert_eq!(gate.trader_exposure(0), 1000.0);

    // Swap to config B with a lower credit limit
    let config_b = RiskConfig {
        credit_limit: 1500.0,
        ..Default::default()
    };
    gate.set_config(config_b);

    // Exposure (1000) was accumulated under A but is still tracked
    assert_eq!(gate.trader_exposure(0), 1000.0);

    // New order: notional=600, total=1600 > 1500 → reject under new limit
    let order2 = make_order(100.0, 6);
    let d = gate.evaluate(&order2, 0.0, 1);
    assert_eq!(d, Decision::RejectCreditLimit);

    // Smaller order: notional=500, total=1500 == 1500 → accept
    let order3 = make_order(100.0, 5);
    let d = gate.evaluate(&order3, 0.0, 2);
    assert_eq!(d, Decision::Accept);
}

/// RiskConfig::is_valid() correctly identifies valid and invalid configs.
#[test]
fn test_config_validation_method() {
    assert!(RiskConfig::default().is_valid());

    assert!(!RiskConfig {
        max_notional: f64::NAN,
        ..Default::default()
    }
    .is_valid());

    assert!(!RiskConfig {
        credit_limit: f64::INFINITY,
        ..Default::default()
    }
    .is_valid());

    assert!(!RiskConfig {
        collar_lower: -1.0,
        ..Default::default()
    }
    .is_valid());

    assert!(!RiskConfig {
        collar_upper: f64::NEG_INFINITY,
        ..Default::default()
    }
    .is_valid());

    // Zero values are valid (they just mean "reject everything")
    assert!(RiskConfig {
        max_notional: 0.0,
        credit_limit: 0.0,
        collar_lower: 0.0,
        collar_upper: 0.0,
        ..Default::default()
    }
    .is_valid());
}
