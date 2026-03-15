//! Fixed-capacity duplicate order detector using a ring buffer. No heap allocation.

/// Ring buffer that tracks recent order IDs and timestamps for duplicate detection.
///
/// When the buffer is full, the oldest entry is overwritten. This means orders older
/// than `N` entries are automatically "forgotten" — size the ring appropriately for
/// your expected dup window and order rate.
///
/// # Capacity requirement
///
/// The ring size `N` must be >= `order_rate * dup_window_duration`. If an attacker
/// can submit `N` distinct orders within the window, they can evict a previous
/// order's hash from the ring and re-submit it without detection. This is an
/// inherent trade-off of fixed-capacity, zero-allocation design. Size `N`
/// conservatively for your peak order rate.
///
/// # Panics
///
/// `N` must be > 0. A zero-capacity ring will fail at compile time.
pub struct DupDetector<const N: usize> {
    ring: [(u64, u64); N], // (order_hash, timestamp_ns)
    head: usize,
    len: usize,
}

impl<const N: usize> Default for DupDetector<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> DupDetector<N> {
    /// Create a new empty detector.
    pub const fn new() -> Self {
        assert!(N > 0, "DupDetector ring size must be > 0");
        Self {
            ring: [(0, 0); N],
            head: 0,
            len: 0,
        }
    }

    /// Check if an order with the given hash is a duplicate within the time window.
    ///
    /// `order_hash` should be a hash of the order's dedup key fields (e.g. trader + symbol + side + price + qty).
    /// `now_ns` is the current timestamp in nanoseconds.
    /// `window_ns` is the dedup window duration in nanoseconds.
    ///
    /// If not a duplicate, the order is recorded in the ring and `false` is returned.
    /// If a duplicate, `true` is returned and the ring is not modified.
    #[inline]
    pub fn is_duplicate(&mut self, order_hash: u64, now_ns: u64, window_ns: u64) -> bool {
        let cutoff = now_ns.saturating_sub(window_ns);

        // Scan the ring for a match within the window
        let scan_len = self.len.min(N);
        for i in 0..scan_len {
            let (hash, ts) = self.ring[i];
            if hash == order_hash && ts >= cutoff {
                return true;
            }
        }

        // Not a duplicate — record it
        self.ring[self.head] = (order_hash, now_ns);
        self.head = (self.head + 1) % N;
        if self.len < N {
            self.len += 1;
        }
        false
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.ring = [(0, 0); N];
        self.head = 0;
        self.len = 0;
    }
}

/// Compute a fast hash of an order's dedup key fields.
/// Uses FNV-1a for speed (no cryptographic strength needed).
#[inline]
pub fn order_dedup_hash(
    trader_id: u32,
    symbol_id: u32,
    side: u8,
    price_bits: u64,
    qty: u64,
) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325; // FNV offset basis
    let mix = |h: &mut u64, v: u64| {
        *h ^= v;
        *h = h.wrapping_mul(0x100000001b3); // FNV prime
    };
    mix(&mut h, trader_id as u64);
    mix(&mut h, symbol_id as u64);
    mix(&mut h, side as u64);
    mix(&mut h, price_bits);
    mix(&mut h, qty);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_duplicate() {
        let mut d = DupDetector::<8>::new();
        assert!(!d.is_duplicate(100, 1000, 500));
        assert!(!d.is_duplicate(200, 1100, 500));
    }

    #[test]
    fn test_duplicate_within_window() {
        let mut d = DupDetector::<8>::new();
        assert!(!d.is_duplicate(100, 1000, 500));
        assert!(d.is_duplicate(100, 1200, 500)); // same hash, within 500ns window
    }

    #[test]
    fn test_duplicate_outside_window() {
        let mut d = DupDetector::<8>::new();
        assert!(!d.is_duplicate(100, 1000, 500));
        assert!(!d.is_duplicate(100, 1600, 500)); // same hash, but 600ns > 500ns window
    }

    #[test]
    fn test_ring_wraps() {
        let mut d = DupDetector::<4>::new();
        for i in 0..10 {
            assert!(!d.is_duplicate(i, i * 100, 50));
        }
        // Oldest entries have been evicted
        assert!(!d.is_duplicate(0, 1000, 50)); // hash 0 was evicted long ago
    }

    #[test]
    fn test_hash_deterministic() {
        let h1 = order_dedup_hash(1, 2, 0, 100_f64.to_bits(), 50);
        let h2 = order_dedup_hash(1, 2, 0, 100_f64.to_bits(), 50);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_differs_on_any_field() {
        let base = order_dedup_hash(1, 2, 0, 100_f64.to_bits(), 50);
        assert_ne!(base, order_dedup_hash(2, 2, 0, 100_f64.to_bits(), 50));
        assert_ne!(base, order_dedup_hash(1, 3, 0, 100_f64.to_bits(), 50));
        assert_ne!(base, order_dedup_hash(1, 2, 1, 100_f64.to_bits(), 50));
        assert_ne!(base, order_dedup_hash(1, 2, 0, 101_f64.to_bits(), 50));
        assert_ne!(base, order_dedup_hash(1, 2, 0, 100_f64.to_bits(), 51));
    }
}
