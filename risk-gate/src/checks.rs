//! Pure risk check functions. No side effects, no allocation, no I/O.
//! Each function returns `true` if the check passes (order is acceptable).

use crate::types::RiskConfig;

/// Reject zero quantity orders (business invariant).
#[inline(always)]
pub fn check_quantity_nonzero(qty: u64) -> bool {
    qty > 0
}

/// Reject orders with non-finite or non-positive price.
#[inline(always)]
pub fn check_price_valid(price: f64) -> bool {
    price.is_finite() && price > 0.0
}

/// Check that order quantity does not exceed the configured maximum.
#[inline(always)]
pub fn check_max_quantity(qty: u64, config: &RiskConfig) -> bool {
    qty <= config.max_quantity
}

/// Check that order notional (price * qty) does not exceed the configured maximum.
#[inline(always)]
pub fn check_max_notional(price: f64, qty: u64, config: &RiskConfig) -> bool {
    price * (qty as f64) <= config.max_notional
}

/// Check that order price falls within the collar around a reference price.
/// Reference price is typically the mid-price from the order book.
/// Returns `true` (pass) if no reference price is available (ref_price <= 0).
#[inline(always)]
pub fn check_price_collar(price: f64, ref_price: f64, config: &RiskConfig) -> bool {
    if ref_price <= 0.0 || !ref_price.is_finite() {
        return true; // no reference → skip collar
    }
    let lower = ref_price * config.collar_lower;
    let upper = ref_price * config.collar_upper;
    price >= lower && price <= upper
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> RiskConfig {
        RiskConfig::default()
    }

    #[test]
    fn test_quantity_nonzero() {
        assert!(!check_quantity_nonzero(0));
        assert!(check_quantity_nonzero(1));
        assert!(check_quantity_nonzero(u64::MAX));
    }

    #[test]
    fn test_price_valid() {
        assert!(!check_price_valid(0.0));
        assert!(!check_price_valid(-1.0));
        assert!(!check_price_valid(f64::NAN));
        assert!(!check_price_valid(f64::INFINITY));
        assert!(!check_price_valid(f64::NEG_INFINITY));
        assert!(check_price_valid(0.01));
        assert!(check_price_valid(999_999.99));
    }

    #[test]
    fn test_max_quantity() {
        let c = cfg();
        assert!(check_max_quantity(1, &c));
        assert!(check_max_quantity(c.max_quantity, &c));
        assert!(!check_max_quantity(c.max_quantity + 1, &c));
    }

    #[test]
    fn test_max_notional() {
        let c = cfg();
        assert!(check_max_notional(100.0, 10, &c)); // 1000 < 5M
        assert!(!check_max_notional(100_000.0, 100, &c)); // 10M > 5M
        assert!(check_max_notional(c.max_notional, 1, &c)); // exactly at limit
    }

    #[test]
    fn test_price_collar() {
        let c = cfg();
        let mid = 100.0;
        assert!(check_price_collar(100.0, mid, &c));
        assert!(check_price_collar(95.0, mid, &c)); // exactly at lower
        assert!(check_price_collar(105.0, mid, &c)); // exactly at upper
        assert!(!check_price_collar(94.99, mid, &c));
        assert!(!check_price_collar(105.01, mid, &c));
    }

    #[test]
    fn test_collar_no_ref_price() {
        let c = cfg();
        // No reference price available → collar is skipped
        assert!(check_price_collar(999.0, 0.0, &c));
        assert!(check_price_collar(999.0, -1.0, &c));
        assert!(check_price_collar(999.0, f64::NAN, &c));
    }
}
