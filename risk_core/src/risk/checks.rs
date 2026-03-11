use crate::envelope::OrderRequest;
use std::collections::HashMap;

/// Check that the order price is within an acceptable collar around reference price.
pub fn price_collar(price: f64, lower: f64, upper: f64) -> bool {
    price >= lower && price <= upper
}

/// Check that order quantity does not exceed the maximum allowed.
pub fn max_quantity(qty: u32, max_qty: u32) -> bool {
    qty <= max_qty
}

/// Check that order notional (price * qty) does not exceed a limit.
pub fn max_notional(price: f64, qty: u32, limit: f64) -> bool {
    price * qty as f64 <= limit
}

/// Check that the trader has not exceeded their credit limit.
/// `used` is their current outstanding exposure, `limit` is the cap.
pub fn credit_limit(used: f64, order_notional: f64, limit: f64) -> bool {
    used + order_notional <= limit
}

/// Simple duplicate suppression: returns true if order is a duplicate.
/// Key is (trader_id, symbol, side, price, qty). Entries older than `window_us` are purged.
pub fn is_duplicate(order: &OrderRequest, recent: &mut HashMap<u64, u64>, window_us: u64) -> bool {
    // Purge expired entries
    let cutoff = order.timestamp_us.saturating_sub(window_us);
    recent.retain(|_, ts| *ts >= cutoff);

    let key = compute_dedup_key(order);
    if let std::collections::hash_map::Entry::Vacant(e) = recent.entry(key) {
        e.insert(order.timestamp_us);
        false
    } else {
        true
    }
}

fn compute_dedup_key(order: &OrderRequest) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    order.trader_id.hash(&mut hasher);
    order.symbol.hash(&mut hasher);
    (order.side as u8).hash(&mut hasher);
    order.price.to_bits().hash(&mut hasher);
    order.qty.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::Side;

    #[test]
    fn test_price_collar() {
        assert!(price_collar(100.0, 90.0, 110.0));
        assert!(price_collar(90.0, 90.0, 110.0));
        assert!(price_collar(110.0, 90.0, 110.0));
        assert!(!price_collar(89.99, 90.0, 110.0));
        assert!(!price_collar(110.01, 90.0, 110.0));
    }

    #[test]
    fn test_max_quantity() {
        assert!(max_quantity(100, 500));
        assert!(max_quantity(500, 500));
        assert!(!max_quantity(501, 500));
        assert!(max_quantity(0, 500));
    }

    #[test]
    fn test_max_notional() {
        assert!(max_notional(100.0, 10, 1500.0));
        assert!(max_notional(100.0, 10, 1000.0)); // exactly 1000
        assert!(!max_notional(100.0, 11, 1000.0)); // 1100 > 1000
    }

    #[test]
    fn test_credit_limit() {
        assert!(credit_limit(500.0, 400.0, 1000.0));
        assert!(credit_limit(500.0, 500.0, 1000.0)); // exactly at limit
        assert!(!credit_limit(500.0, 501.0, 1000.0));
        assert!(credit_limit(0.0, 1000.0, 1000.0));
    }

    fn make_order(id: u64, ts: u64, price: f64, qty: u32) -> OrderRequest {
        OrderRequest {
            id,
            timestamp_us: ts,
            symbol: "AAPL".to_string(),
            side: Side::Bid,
            price,
            qty,
            trader_id: "T1".to_string(),
        }
    }

    #[test]
    fn test_duplicate_detection() {
        let mut recent = HashMap::new();
        let window = 1_000_000; // 1 second

        let o1 = make_order(1, 100_000, 150.0, 10);
        assert!(!is_duplicate(&o1, &mut recent, window));

        // Same order again within window -> duplicate
        let o2 = make_order(2, 200_000, 150.0, 10);
        assert!(is_duplicate(&o2, &mut recent, window));
    }

    #[test]
    fn test_duplicate_expires() {
        let mut recent = HashMap::new();
        let window = 1_000_000;

        let o1 = make_order(1, 100_000, 150.0, 10);
        assert!(!is_duplicate(&o1, &mut recent, window));

        // Same fields but after window expires
        let o2 = make_order(2, 1_200_000, 150.0, 10);
        assert!(!is_duplicate(&o2, &mut recent, window));
    }

    #[test]
    fn test_different_orders_not_duplicate() {
        let mut recent = HashMap::new();
        let window = 1_000_000;

        let o1 = make_order(1, 100_000, 150.0, 10);
        assert!(!is_duplicate(&o1, &mut recent, window));

        // Different price -> not duplicate
        let mut o2 = make_order(2, 200_000, 151.0, 10);
        assert!(!is_duplicate(&o2, &mut recent, window));

        // Different qty -> not duplicate
        o2.price = 150.0;
        o2.qty = 20;
        o2.timestamp_us = 300_000;
        assert!(!is_duplicate(&o2, &mut recent, window));
    }
}
