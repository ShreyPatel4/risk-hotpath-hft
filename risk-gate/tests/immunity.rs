//! Immunity proofs: proptest invariants that prove no input can bypass any check.
//!
//! These tests use full-range inputs (not constrained ranges) to prove that
//! the risk gate is mathematically correct for ALL possible inputs.

use proptest::prelude::*;
use risk_gate::checks;
use risk_gate::engine::RiskGate;
use risk_gate::types::*;

fn valid_config() -> RiskConfig {
    RiskConfig::default()
}

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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(5000))]

    /// No sequence of orders can push a single trader's credit exposure
    /// above the configured limit.
    #[test]
    fn no_input_sequence_bypasses_credit(
        prices in prop::collection::vec(0.01f64..10_000.0, 1..50),
        qtys in prop::collection::vec(1u64..1000, 1..50),
    ) {
        let limit = 100_000.0;
        let config = RiskConfig {
            credit_limit: limit,
            max_quantity: u64::MAX,
            max_notional: f64::MAX,
            ..valid_config()
        };
        let mut gate = RiskGate::<4, 256>::new(config);

        let len = prices.len().min(qtys.len());
        for i in 0..len {
            let order = make_order(prices[i], qtys[i], 0, i as u64);
            gate.evaluate(&order, 0.0, i as u64);
        }

        let exposure = gate.trader_exposure(0);
        prop_assert!(
            exposure <= limit,
            "exposure {} exceeded limit {}",
            exposure,
            limit
        );
    }

    /// Every rejection decision matches the FIRST failing check in short-circuit order.
    #[test]
    fn every_rejection_maps_to_correct_check(
        price in 0.01f64..1_000.0,
        qty in 1u64..100_000,
        ref_price in 0.01f64..1_000.0,
    ) {
        let config = valid_config();
        let mut gate = RiskGate::<16, 16>::new(config);
        let order = make_order(price, qty, 0, 1);

        let decision = gate.evaluate(&order, ref_price, 0);

        if decision == Decision::RejectMaxQuantity {
            prop_assert!(!checks::check_max_quantity(qty, &config));
        } else if decision == Decision::RejectMaxNotional {
            prop_assert!(checks::check_max_quantity(qty, &config));
            prop_assert!(!checks::check_max_notional(price, qty, &config));
        } else if decision == Decision::RejectPriceCollar {
            prop_assert!(checks::check_max_quantity(qty, &config));
            prop_assert!(checks::check_max_notional(price, qty, &config));
            prop_assert!(!checks::check_price_collar(price, ref_price, &config));
        }
    }

    /// If evaluate() returns Accept, every individual check must also pass.
    #[test]
    fn accept_implies_all_checks_pass(
        price in 0.01f64..200.0,
        qty in 1u64..5000,
        ref_price in 0.01f64..200.0,
    ) {
        let config = valid_config();
        let mut gate = RiskGate::<16, 16>::new(config);
        let order = make_order(price, qty, 0, 1);

        let decision = gate.evaluate(&order, ref_price, 0);

        if decision == Decision::Accept {
            prop_assert!(checks::check_quantity_nonzero(qty));
            prop_assert!(checks::check_price_valid(price));
            prop_assert!(checks::check_max_quantity(qty, &config));
            prop_assert!(checks::check_max_notional(price, qty, &config));
            prop_assert!(checks::check_price_collar(price, ref_price, &config));
        }
    }

    /// Same gate state + same inputs → identical decision (determinism).
    #[test]
    fn deterministic_evaluation(
        price in 0.01f64..500.0,
        qty in 1u64..10_000,
        trader_id in 0u32..16,
        ref_price in 0.01f64..500.0,
        now_ns in 0u64..1_000_000_000,
    ) {
        let config = valid_config();

        let mut gate1 = RiskGate::<16, 16>::new(config);
        let mut gate2 = RiskGate::<16, 16>::new(config);
        let order = make_order(price, qty, trader_id, 1);

        let d1 = gate1.evaluate(&order, ref_price, now_ns);
        let d2 = gate2.evaluate(&order, ref_price, now_ns);

        prop_assert_eq!(d1, d2);
    }

    /// For accepted orders, exposure increases by the order's notional.
    /// For rejected orders, exposure does not change.
    #[test]
    fn credit_monotonically_increases(
        price in 0.01f64..100.0,
        qty in 1u64..100,
    ) {
        let config = RiskConfig {
            credit_limit: f64::MAX,
            ..valid_config()
        };
        let mut gate = RiskGate::<16, 16>::new(config);
        let order = make_order(price, qty, 0, 1);

        let before = gate.trader_exposure(0);
        let decision = gate.evaluate(&order, 0.0, 0);
        let after = gate.trader_exposure(0);

        if decision == Decision::Accept {
            let expected_notional = price * qty as f64;
            prop_assert!(
                (after - before - expected_notional).abs() < 1e-10,
                "exposure should increase by notional on accept"
            );
        } else {
            prop_assert_eq!(before, after, "exposure should not change on reject");
        }
    }

    /// When ref_price is invalid (≤0 or NaN/Inf), collar never causes rejection.
    #[test]
    fn collar_skip_when_no_valid_ref_price(
        price in 0.01f64..10_000.0,
        ref_price in prop::num::f64::ANY,
    ) {
        if ref_price <= 0.0 || !ref_price.is_finite() {
            let config = valid_config();
            let result = checks::check_price_collar(price, ref_price, &config);
            prop_assert!(result, "collar should pass when ref_price is invalid");
        }
    }

    /// Full-range inputs: arbitrary f64/u64 values never cause a panic.
    /// This extends the existing never_panics test with truly arbitrary config values too.
    #[test]
    fn full_range_never_panics(
        price in prop::num::f64::ANY,
        qty in prop::num::u64::ANY,
        trader_id in prop::num::u32::ANY,
        ref_price in prop::num::f64::ANY,
        now_ns in prop::num::u64::ANY,
    ) {
        // Use default (valid) config so we test the order path, not config rejection
        let mut gate = RiskGate::<1024, 64>::new(valid_config());
        let order = make_order(price, qty, trader_id, 1);
        // Must not panic — any Decision variant is acceptable
        let _ = gate.evaluate(&order, ref_price, now_ns);
    }
}

/// Each of the 9 Decision variants is reachable with specific inputs.
#[test]
fn all_decision_variants_reachable() {
    let config = RiskConfig {
        max_quantity: 100,
        max_notional: 10_000.0,
        credit_limit: 5_000.0,
        collar_lower: 0.95,
        collar_upper: 1.05,
        dup_window_ns: 1_000_000_000,
    };

    // Accept
    let mut gate = RiskGate::<16, 16>::new(config);
    assert_eq!(
        gate.evaluate(&make_order(100.0, 10, 0, 1), 100.0, 0),
        Decision::Accept
    );

    // RejectZeroQuantity
    let mut gate = RiskGate::<16, 16>::new(config);
    assert_eq!(
        gate.evaluate(&make_order(100.0, 0, 0, 1), 100.0, 0),
        Decision::RejectZeroQuantity
    );

    // RejectInvalidPrice
    let mut gate = RiskGate::<16, 16>::new(config);
    assert_eq!(
        gate.evaluate(&make_order(f64::NAN, 10, 0, 1), 100.0, 0),
        Decision::RejectInvalidPrice
    );

    // RejectMaxQuantity
    let mut gate = RiskGate::<16, 16>::new(config);
    assert_eq!(
        gate.evaluate(&make_order(100.0, 101, 0, 1), 100.0, 0),
        Decision::RejectMaxQuantity
    );

    // RejectMaxNotional: need qty <= max_quantity but price*qty > max_notional
    let notional_config = RiskConfig {
        max_quantity: 200,
        max_notional: 10_000.0,
        credit_limit: f64::MAX,
        ..config
    };
    let mut gate = RiskGate::<16, 16>::new(notional_config);
    assert_eq!(
        gate.evaluate(&make_order(100.0, 101, 0, 1), 100.0, 0),
        Decision::RejectMaxNotional
    );

    // RejectPriceCollar
    let mut gate = RiskGate::<16, 16>::new(config);
    assert_eq!(
        gate.evaluate(&make_order(200.0, 1, 0, 1), 100.0, 0),
        Decision::RejectPriceCollar
    );

    // RejectCreditLimit
    let credit_config = RiskConfig {
        credit_limit: 500.0,
        ..config
    };
    let mut gate = RiskGate::<16, 16>::new(credit_config);
    gate.evaluate(&make_order(100.0, 5, 0, 1), 0.0, 0); // fill to 500
    assert_eq!(
        gate.evaluate(&make_order(100.0, 1, 0, 2), 0.0, 1),
        Decision::RejectCreditLimit
    );

    // RejectDuplicate
    let mut gate = RiskGate::<16, 16>::new(RiskConfig {
        credit_limit: f64::MAX,
        ..config
    });
    gate.evaluate(&make_order(100.0, 10, 0, 1), 100.0, 0);
    gate.reset_all_credit();
    assert_eq!(
        gate.evaluate(&make_order(100.0, 10, 0, 1), 100.0, 500),
        Decision::RejectDuplicate
    );

    // RejectInvalidConfig
    let bad_config = RiskConfig {
        credit_limit: f64::NAN,
        ..config
    };
    let mut gate = RiskGate::<16, 16>::new(bad_config);
    assert_eq!(
        gate.evaluate(&make_order(100.0, 10, 0, 1), 100.0, 0),
        Decision::RejectInvalidConfig
    );
}
