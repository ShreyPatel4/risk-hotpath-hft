use proptest::prelude::*;
use risk_gate::checks;
use risk_gate::credit::CreditTracker;
use risk_gate::engine::RiskGate;
use risk_gate::types::*;

fn default_config() -> RiskConfig {
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
    /// Max quantity check is a pure threshold: passes iff qty <= max.
    #[test]
    fn max_qty_always_enforced(qty in 0u64..1_000_000, max in 1u64..1_000_000) {
        let config = RiskConfig { max_quantity: max, ..default_config() };
        let result = checks::check_max_quantity(qty, &config);
        prop_assert_eq!(result, qty <= max);
    }

    /// Price collar rejects any price outside the band, for any reference price.
    #[test]
    fn collar_always_enforced(
        price in 0.01f64..1_000_000.0,
        ref_price in 0.01f64..1_000_000.0,
        lower in 0.5f64..1.0,
        upper in 1.0f64..2.0,
    ) {
        let config = RiskConfig { collar_lower: lower, collar_upper: upper, ..default_config() };
        let result = checks::check_price_collar(price, ref_price, &config);
        let expected = price >= ref_price * lower && price <= ref_price * upper;
        prop_assert_eq!(result, expected);
    }

    /// If credit remaining < order notional, the credit check ALWAYS rejects.
    #[test]
    fn credit_limit_never_bypassed(
        existing in 0.0f64..10_000_000.0,
        notional in 0.01f64..10_000_000.0,
        limit in 0.01f64..10_000_000.0,
    ) {
        let mut tracker = CreditTracker::<4>::new();
        tracker.set_exposure(0, existing);
        let result = tracker.check_and_update(0, notional, limit);
        if existing + notional > limit {
            prop_assert!(!result, "credit check should reject when over limit");
        }
    }

    /// No order with zero quantity should ever be accepted by the gate.
    #[test]
    fn zero_quantity_always_rejected(price in 0.01f64..1_000_000.0) {
        let mut gate = RiskGate::<4, 4>::new(default_config());
        let order = make_order(price, 0, 0, 1);
        let decision = gate.evaluate(&order, price, 0);
        prop_assert_eq!(decision, Decision::RejectZeroQuantity);
    }

    /// No order with invalid price (NaN, Inf, <= 0) should be accepted.
    #[test]
    fn invalid_price_always_rejected(qty in 1u64..1000) {
        let mut gate = RiskGate::<4, 4>::new(default_config());

        let nan_order = make_order(f64::NAN, qty, 0, 1);
        prop_assert_eq!(gate.evaluate(&nan_order, 100.0, 0), Decision::RejectInvalidPrice);

        let inf_order = make_order(f64::INFINITY, qty, 0, 2);
        prop_assert_eq!(gate.evaluate(&inf_order, 100.0, 0), Decision::RejectInvalidPrice);

        let neg_order = make_order(-1.0, qty, 0, 3);
        prop_assert_eq!(gate.evaluate(&neg_order, 100.0, 0), Decision::RejectInvalidPrice);

        let zero_order = make_order(0.0, qty, 0, 4);
        prop_assert_eq!(gate.evaluate(&zero_order, 100.0, 0), Decision::RejectInvalidPrice);
    }

    /// Pathological inputs: the gate must never panic.
    #[test]
    fn never_panics_on_any_input(
        price in prop::num::f64::ANY,
        qty in prop::num::u64::ANY,
        trader_id in 0u32..2000,
        ref_price in prop::num::f64::ANY,
    ) {
        let mut gate = RiskGate::<1024, 64>::new(default_config());
        let order = make_order(price, qty, trader_id, 1);
        // Must not panic — result can be any Decision variant
        let _ = gate.evaluate(&order, ref_price, 0);
    }

    /// Duplicate detection: same order within window → rejected.
    #[test]
    fn duplicate_detected_within_window(
        now in 1_000_000u64..2_000_000_000,
        window in 1_000u64..1_000_000_000,
    ) {
        let config = RiskConfig { dup_window_ns: window, credit_limit: f64::MAX, ..default_config() };
        let mut gate = RiskGate::<4, 64>::new(config);
        let order = make_order(100.0, 1, 0, 42);

        // First eval: accept
        let d1 = gate.evaluate(&order, 100.0, now);
        prop_assert_eq!(d1, Decision::Accept);

        // Same order, within window: must reject (might be credit instead if cumulative)
        gate.reset_all_credit(); // isolate dup check
        let d2 = gate.evaluate(&order, 100.0, now + window / 2);
        prop_assert_eq!(d2, Decision::RejectDuplicate);
    }

    /// Max notional is price * qty: if either is huge, notional check catches it.
    #[test]
    fn notional_check_consistent(
        price in 0.01f64..100_000.0,
        qty in 1u64..1_000_000,
        max_notional in 1.0f64..1_000_000_000.0,
    ) {
        let config = RiskConfig { max_notional, ..default_config() };
        let result = checks::check_max_notional(price, qty, &config);
        let expected = price * (qty as f64) <= max_notional;
        prop_assert_eq!(result, expected);
    }
}
