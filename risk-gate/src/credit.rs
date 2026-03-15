//! Fixed-capacity credit tracker. No heap allocation.
//!
//! Uses a flat array indexed by `trader_id`. The caller must map trader
//! identifiers to indices in `0..N` before calling into this tracker.

/// Tracks cumulative notional exposure per trader in a fixed-size array.
pub struct CreditTracker<const N: usize> {
    exposure: [f64; N],
}

impl<const N: usize> Default for CreditTracker<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> CreditTracker<N> {
    /// Create a new tracker with all exposures at zero.
    pub const fn new() -> Self {
        Self { exposure: [0.0; N] }
    }

    /// Check if the trader can take on additional notional without exceeding the limit.
    /// If the check passes, the exposure is atomically updated.
    /// Returns `true` if within limit (order accepted), `false` if limit breached.
    #[inline(always)]
    pub fn check_and_update(&mut self, trader_idx: usize, notional: f64, limit: f64) -> bool {
        if trader_idx >= N {
            return false;
        }
        let new_exposure = self.exposure[trader_idx] + notional;
        // NaN-safe: !(NaN <= x) is true, so NaN exposure always rejects.
        // This prevents NaN propagation from silently disabling credit checks.
        // The negated comparison is intentional — `new_exposure > limit` would
        // return false for NaN, silently accepting orders.
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(new_exposure <= limit) {
            return false;
        }
        self.exposure[trader_idx] = new_exposure;
        true
    }

    /// Get current exposure for a trader.
    #[inline(always)]
    pub fn exposure(&self, trader_idx: usize) -> f64 {
        if trader_idx >= N {
            0.0
        } else {
            self.exposure[trader_idx]
        }
    }

    /// Reset exposure for a specific trader to zero.
    #[inline(always)]
    pub fn reset(&mut self, trader_idx: usize) {
        if trader_idx < N {
            self.exposure[trader_idx] = 0.0;
        }
    }

    /// Reset all trader exposures to zero.
    pub fn reset_all(&mut self) {
        self.exposure = [0.0; N];
    }

    /// Set exposure directly (for testing / state restoration).
    /// Rejects NaN and infinite values to prevent poisoning the tracker.
    #[inline(always)]
    pub fn set_exposure(&mut self, trader_idx: usize, value: f64) {
        if trader_idx < N && value.is_finite() {
            self.exposure[trader_idx] = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_credit() {
        let mut tracker = CreditTracker::<4>::new();
        assert!(tracker.check_and_update(0, 500.0, 1000.0));
        assert_eq!(tracker.exposure(0), 500.0);
        assert!(tracker.check_and_update(0, 500.0, 1000.0)); // exactly at limit
        assert_eq!(tracker.exposure(0), 1000.0);
        assert!(!tracker.check_and_update(0, 0.01, 1000.0)); // over limit
    }

    #[test]
    fn test_independent_traders() {
        let mut tracker = CreditTracker::<4>::new();
        assert!(tracker.check_and_update(0, 900.0, 1000.0));
        assert!(tracker.check_and_update(1, 900.0, 1000.0)); // different trader
        assert!(!tracker.check_and_update(0, 200.0, 1000.0)); // trader 0 over
        assert!(tracker.check_and_update(1, 100.0, 1000.0)); // trader 1 at limit
    }

    #[test]
    fn test_out_of_bounds() {
        let mut tracker = CreditTracker::<2>::new();
        assert!(!tracker.check_and_update(99, 1.0, 1000.0)); // out of bounds
        assert_eq!(tracker.exposure(99), 0.0);
    }

    #[test]
    fn test_reset() {
        let mut tracker = CreditTracker::<4>::new();
        tracker.check_and_update(0, 500.0, 1000.0);
        tracker.reset(0);
        assert_eq!(tracker.exposure(0), 0.0);
        assert!(tracker.check_and_update(0, 1000.0, 1000.0));
    }

    #[test]
    fn test_reset_all() {
        let mut tracker = CreditTracker::<4>::new();
        tracker.check_and_update(0, 500.0, 1000.0);
        tracker.check_and_update(1, 300.0, 1000.0);
        tracker.reset_all();
        assert_eq!(tracker.exposure(0), 0.0);
        assert_eq!(tracker.exposure(1), 0.0);
    }
}
