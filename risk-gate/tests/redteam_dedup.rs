//! Red Team: Duplicate detection evasion attacks.
//!
//! Tests that exploit ring buffer mechanics, timing windows, and hash
//! properties to try to bypass duplicate detection.

use risk_gate::dedup::{order_dedup_hash, DupDetector};
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

/// Exploit ring buffer eviction: submit N distinct orders to flush the ring,
/// then re-submit the original order — it should NOT be detected as duplicate
/// because its entry was evicted.
#[test]
fn test_ring_buffer_eviction_exploit() {
    let window = 1_000_000u64; // 1ms window
    let config = RiskConfig {
        dup_window_ns: window,
        credit_limit: f64::MAX,
        max_notional: f64::MAX,
        max_quantity: u64::MAX,
        ..Default::default()
    };
    let mut gate = RiskGate::<256, 4>::new(config); // ring size = 4

    // Submit the target order
    let target = make_order(100.0, 10, 0, 1);
    assert_eq!(gate.evaluate(&target, 0.0, 1000), Decision::Accept);

    // Submit 4 distinct orders to evict the target's hash from the ring
    for i in 0..4u64 {
        let flusher = make_order(100.0 + (i as f64 * 10.0), 1, 1, 100 + i);
        gate.evaluate(&flusher, 0.0, 1000 + i);
    }

    // Re-submit the target within the time window — evicted, so NOT a duplicate
    gate.reset_all_credit(); // isolate from credit effects
    let d = gate.evaluate(&target, 0.0, 1500);
    assert_eq!(
        d,
        Decision::Accept,
        "evicted order should not be detected as duplicate"
    );
}

/// N orders stored, re-submit #1 → duplicate; N+1 orders, re-submit #1 → not duplicate.
#[test]
fn test_ring_exactly_at_capacity() {
    let mut dedup = DupDetector::<4>::new();

    // Insert 4 distinct hashes (fills the ring exactly)
    for i in 0..4u64 {
        assert!(!dedup.is_duplicate(i + 100, 1000, 5000));
    }

    // Re-submit hash 100 within window → should be duplicate (still in ring)
    assert!(dedup.is_duplicate(100, 1000, 5000));

    // Insert one more (evicts hash 100)
    assert!(!dedup.is_duplicate(999, 1000, 5000));

    // Re-submit hash 100 → should NOT be duplicate (evicted)
    assert!(!dedup.is_duplicate(100, 1000, 5000));
}

/// Test exact nanosecond boundary of dedup window.
/// At cutoff: ts >= cutoff is true (duplicate). One ns later: not duplicate.
#[test]
fn test_dup_window_exact_boundary_ns() {
    let mut dedup = DupDetector::<8>::new();
    let window = 500u64;

    // Insert at t=1000
    assert!(!dedup.is_duplicate(42, 1000, window));

    // Check at t=1500: cutoff = 1500 - 500 = 1000. ts=1000 >= 1000 → duplicate
    assert!(dedup.is_duplicate(42, 1500, window));

    // Remove the duplicate entry by inserting it (it was detected, ring not modified)
    // Check at t=1501: cutoff = 1501 - 500 = 1001. ts=1000 >= 1001 → false → not duplicate
    let mut dedup2 = DupDetector::<8>::new();
    assert!(!dedup2.is_duplicate(42, 1000, window));
    assert!(!dedup2.is_duplicate(42, 1501, window));
}

/// dup_window_ns = 0: cutoff = now - 0 = now. Only exact same timestamp matches.
#[test]
fn test_dup_window_zero() {
    let mut dedup = DupDetector::<8>::new();

    // Insert at t=1000
    assert!(!dedup.is_duplicate(42, 1000, 0));

    // Same hash at t=1001: cutoff = 1001 - 0 = 1001. ts=1000 >= 1001 → false
    assert!(!dedup.is_duplicate(42, 1001, 0));

    // Same hash at exact same timestamp: cutoff = 1000 - 0 = 1000. ts=1000 >= 1000 → true
    let mut dedup2 = DupDetector::<8>::new();
    assert!(!dedup2.is_duplicate(42, 1000, 0));
    assert!(dedup2.is_duplicate(42, 1000, 0));
}

/// dup_window_ns = u64::MAX: cutoff = now.saturating_sub(u64::MAX) = 0.
/// Everything in the ring with ts >= 0 matches → effectively infinite window.
#[test]
fn test_dup_window_u64_max() {
    let mut dedup = DupDetector::<8>::new();

    // Insert at t=0
    assert!(!dedup.is_duplicate(42, 0, u64::MAX));

    // Re-submit at t=u64::MAX: cutoff = u64::MAX.saturating_sub(u64::MAX) = 0
    // ts=0 >= 0 → duplicate
    assert!(dedup.is_duplicate(42, u64::MAX, u64::MAX));
}

/// All orders submitted at now_ns = 0.
#[test]
fn test_dup_timestamp_zero() {
    let mut dedup = DupDetector::<8>::new();

    // First submission at t=0
    assert!(!dedup.is_duplicate(42, 0, 1000));

    // Same hash at t=0 → cutoff = 0.saturating_sub(1000) = 0. ts=0 >= 0 → duplicate
    assert!(dedup.is_duplicate(42, 0, 1000));

    // Different hash at t=0 → not duplicate
    assert!(!dedup.is_duplicate(99, 0, 1000));
}

/// Brute-force search for FNV-1a hash collision to document false positive rate.
/// FNV-1a on 5 u64 inputs can have collisions; find one and verify behavior.
#[test]
fn test_hash_collision_search() {
    // Search for a collision by varying trader_id and qty
    let base_hash = order_dedup_hash(0, 0, 0, 100_f64.to_bits(), 1);

    let mut found_collision = false;
    for trader_id in 1..100_000u32 {
        let h = order_dedup_hash(trader_id, 0, 0, 100_f64.to_bits(), 1);
        if h == base_hash {
            // Found a collision! Two different orders hash to the same value.
            // This means a duplicate detection false positive is possible.
            found_collision = true;

            // Verify: inserting order A, then checking order B (different trader_id
            // but same hash) would be a false positive
            let mut dedup = DupDetector::<8>::new();
            assert!(!dedup.is_duplicate(base_hash, 1000, 5000));
            // Same hash from different order → falsely detected as duplicate
            assert!(dedup.is_duplicate(h, 1000, 5000));
            break;
        }
    }

    // It's OK if no collision is found in 100K iterations — FNV-1a has a 64-bit
    // output space so collisions are rare. The test documents the search.
    if !found_collision {
        // Verify at least that distinct inputs produce distinct hashes (probabilistic)
        let h1 = order_dedup_hash(0, 0, 0, 100_f64.to_bits(), 1);
        let h2 = order_dedup_hash(1, 0, 0, 100_f64.to_bits(), 1);
        assert_ne!(h1, h2);
    }
}
