//! The risk gate: evaluates orders against all checks in a single inline call.

use crate::checks;
use crate::credit::CreditTracker;
use crate::dedup::{order_dedup_hash, DupDetector};
use crate::types::{Decision, Order, RiskConfig};

/// Zero-allocation pre-trade risk gate.
///
/// Generic parameters:
/// - `TRADERS`: maximum number of distinct trader IDs (fixed-size credit array)
/// - `DUP_RING`: capacity of the duplicate detection ring buffer
///
/// All state is stack-allocated. No heap, no `Box`, no `Vec`, no `HashMap`.
///
/// # Example
/// ```
/// use risk_gate::{RiskGate, Order, Side, RiskConfig, Decision};
///
/// let config = RiskConfig::default();
/// let mut gate = RiskGate::<256, 64>::new(config);
///
/// let order = Order {
///     order_id: 1,
///     symbol_id: 0,
///     trader_id: 0,
///     price: 150.0,
///     quantity: 100,
///     side: Side::Buy,
/// };
///
/// let decision = gate.evaluate(&order, 150.0, 0);
/// assert!(decision.is_accept());
/// ```
pub struct RiskGate<const TRADERS: usize, const DUP_RING: usize> {
    config: RiskConfig,
    credit: CreditTracker<TRADERS>,
    dups: DupDetector<DUP_RING>,
}

impl<const T: usize, const D: usize> RiskGate<T, D> {
    /// Create a new risk gate with the given configuration.
    pub const fn new(config: RiskConfig) -> Self {
        Self {
            config,
            credit: CreditTracker::new(),
            dups: DupDetector::new(),
        }
    }

    /// Evaluate an order against all risk checks.
    ///
    /// `ref_price`: reference price for collar check (typically mid-price from book).
    ///              Pass 0.0 or negative to skip the collar check.
    /// `now_ns`:    current timestamp in nanoseconds (for duplicate detection window).
    ///
    /// Returns `Decision::Accept` or `Decision::Reject*` with the first failing rule.
    /// Checks are ordered by expected failure frequency for early short-circuit.
    #[inline]
    pub fn evaluate(&mut self, order: &Order, ref_price: f64, now_ns: u64) -> Decision {
        // Config validation — reject if NaN/Inf could silently disable checks
        if !self.config.is_valid() {
            return Decision::RejectInvalidConfig;
        }

        // Sanity checks first (cheapest)
        if !checks::check_quantity_nonzero(order.quantity) {
            return Decision::RejectZeroQuantity;
        }
        if !checks::check_price_valid(order.price) {
            return Decision::RejectInvalidPrice;
        }

        // Max quantity (single comparison)
        if !checks::check_max_quantity(order.quantity, &self.config) {
            return Decision::RejectMaxQuantity;
        }

        // Max notional (one multiply + comparison)
        if !checks::check_max_notional(order.price, order.quantity, &self.config) {
            return Decision::RejectMaxNotional;
        }

        // Price collar (two multiplies + two comparisons)
        if !checks::check_price_collar(order.price, ref_price, &self.config) {
            return Decision::RejectPriceCollar;
        }

        // Credit limit (array lookup + addition + comparison, updates state)
        let notional = order.price * order.quantity as f64;
        if !self.credit.check_and_update(
            order.trader_id as usize,
            notional,
            self.config.credit_limit,
        ) {
            return Decision::RejectCreditLimit;
        }

        // Duplicate detection (ring buffer scan, updates state)
        let hash = order_dedup_hash(
            order.trader_id,
            order.symbol_id,
            order.side as u8,
            order.price.to_bits(),
            order.quantity,
        );
        if self
            .dups
            .is_duplicate(hash, now_ns, self.config.dup_window_ns)
        {
            return Decision::RejectDuplicate;
        }

        Decision::Accept
    }

    /// Get current config (read-only).
    pub fn config(&self) -> &RiskConfig {
        &self.config
    }

    /// Replace the configuration atomically.
    /// This is a simple copy — for lock-free hot-swap under concurrent load,
    /// use the `HotSwapConfig` wrapper.
    pub fn set_config(&mut self, config: RiskConfig) {
        self.config = config;
    }

    /// Get a trader's current credit exposure.
    pub fn trader_exposure(&self, trader_id: u32) -> f64 {
        self.credit.exposure(trader_id as usize)
    }

    /// Reset a trader's credit exposure to zero.
    pub fn reset_trader(&mut self, trader_id: u32) {
        self.credit.reset(trader_id as usize);
    }

    /// Reset all traders' credit exposure.
    pub fn reset_all_credit(&mut self) {
        self.credit.reset_all();
    }

    /// Clear the duplicate detection buffer.
    pub fn clear_dups(&mut self) {
        self.dups.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Side;

    fn default_gate() -> RiskGate<16, 16> {
        RiskGate::new(RiskConfig::default())
    }

    fn order(price: f64, qty: u64) -> Order {
        Order {
            order_id: 1,
            symbol_id: 0,
            trader_id: 0,
            price,
            quantity: qty,
            side: Side::Buy,
        }
    }

    #[test]
    fn test_accept_valid() {
        let mut gate = default_gate();
        assert_eq!(gate.evaluate(&order(100.0, 10), 100.0, 0), Decision::Accept);
    }

    #[test]
    fn test_reject_zero_qty() {
        let mut gate = default_gate();
        assert_eq!(
            gate.evaluate(&order(100.0, 0), 100.0, 0),
            Decision::RejectZeroQuantity
        );
    }

    #[test]
    fn test_reject_invalid_price() {
        let mut gate = default_gate();
        assert_eq!(
            gate.evaluate(&order(f64::NAN, 10), 100.0, 0),
            Decision::RejectInvalidPrice
        );
        assert_eq!(
            gate.evaluate(&order(-1.0, 10), 100.0, 0),
            Decision::RejectInvalidPrice
        );
        assert_eq!(
            gate.evaluate(&order(0.0, 10), 100.0, 0),
            Decision::RejectInvalidPrice
        );
    }

    #[test]
    fn test_reject_max_qty() {
        let config = RiskConfig {
            max_quantity: 100,
            ..Default::default()
        };
        let mut gate = RiskGate::<16, 16>::new(config);
        assert_eq!(
            gate.evaluate(&order(10.0, 101), 10.0, 0),
            Decision::RejectMaxQuantity
        );
        assert_eq!(gate.evaluate(&order(10.0, 100), 10.0, 0), Decision::Accept);
    }

    #[test]
    fn test_reject_max_notional() {
        let config = RiskConfig {
            max_notional: 1000.0,
            ..Default::default()
        };
        let mut gate = RiskGate::<16, 16>::new(config);
        assert_eq!(
            gate.evaluate(&order(100.0, 11), 100.0, 0),
            Decision::RejectMaxNotional
        );
    }

    #[test]
    fn test_reject_price_collar() {
        let mut gate = default_gate();
        // ref=100, collar=±5% → [95, 105]
        assert_eq!(
            gate.evaluate(&order(110.0, 10), 100.0, 0),
            Decision::RejectPriceCollar
        );
        assert_eq!(gate.evaluate(&order(105.0, 10), 100.0, 0), Decision::Accept);
    }

    #[test]
    fn test_reject_credit_limit() {
        let config = RiskConfig {
            credit_limit: 500.0,
            ..Default::default()
        };
        let mut gate = RiskGate::<16, 16>::new(config);
        // First order: notional=400 → accepted, exposure=400
        assert_eq!(gate.evaluate(&order(100.0, 4), 100.0, 0), Decision::Accept);
        // Second order: notional=200, total=600 > 500 → rejected
        assert_eq!(
            gate.evaluate(&order(100.0, 2), 100.0, 0),
            Decision::RejectCreditLimit
        );
    }

    #[test]
    fn test_reject_duplicate() {
        let mut gate = default_gate();
        let o = order(100.0, 10);
        assert_eq!(gate.evaluate(&o, 100.0, 1000), Decision::Accept);
        // Same order again within dup window (1 billion ns = 1s)
        assert_eq!(gate.evaluate(&o, 100.0, 1500), Decision::RejectDuplicate);
    }

    #[test]
    fn test_collar_skipped_without_ref() {
        let mut gate = default_gate();
        // Price way off but no reference → collar skipped → accepted
        assert_eq!(gate.evaluate(&order(999.0, 1), 0.0, 0), Decision::Accept);
    }

    #[test]
    fn test_exposure_tracking() {
        let mut gate = default_gate();
        gate.evaluate(&order(100.0, 10), 100.0, 0); // notional=1000
        assert_eq!(gate.trader_exposure(0), 1000.0);
        gate.reset_trader(0);
        assert_eq!(gate.trader_exposure(0), 0.0);
    }

    #[test]
    fn test_config_swap() {
        let mut gate = default_gate();
        assert_eq!(
            gate.evaluate(&order(100.0, 50_000), 100.0, 0),
            Decision::RejectMaxQuantity
        );

        gate.set_config(RiskConfig {
            max_quantity: 100_000,
            ..Default::default()
        });
        gate.reset_all_credit();
        assert_eq!(
            gate.evaluate(&order(100.0, 50_000), 100.0, 0),
            Decision::Accept
        );
    }
}
