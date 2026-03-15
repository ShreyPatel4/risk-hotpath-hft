//! Zero-allocation verification.
//!
//! Installs a custom global allocator that counts allocations made while
//! the flag is set. If `evaluate()` causes any allocations, the test fails.
//!
//! This file MUST be a separate integration test binary because only one
//! `#[global_allocator]` can exist per binary.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use risk_gate::engine::RiskGate;
use risk_gate::types::*;

/// Allocator that counts allocations when tracking is enabled.
/// Does NOT panic — panicking from an allocator causes double-panic aborts
/// because the panic machinery itself allocates.
struct CountingAllocator;

static TRACKING: AtomicBool = AtomicBool::new(false);
static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACKING.load(Ordering::Relaxed) {
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static A: CountingAllocator = CountingAllocator;

/// Prove that RiskGate::evaluate() performs zero heap allocations.
#[test]
fn test_evaluate_zero_alloc() {
    // Set up gate while tracking is off (test harness may allocate freely)
    let config = RiskConfig::default();
    let mut gate = RiskGate::<256, 64>::new(config);

    let orders: Vec<Order> = (0..1000u64)
        .map(|i| Order {
            order_id: i,
            symbol_id: (i % 10) as u32,
            trader_id: (i % 256) as u32,
            price: 100.0 + (i as f64 * 0.01),
            quantity: 1 + (i % 100),
            side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
        })
        .collect();

    // Reset counter and start tracking
    ALLOC_COUNT.store(0, Ordering::SeqCst);
    TRACKING.store(true, Ordering::SeqCst);

    // Run 1000 evaluations
    for (i, order) in orders.iter().enumerate() {
        let ref_price = 100.0 + (i as f64 * 0.005);
        let now_ns = i as u64 * 1_000_000;
        let _ = gate.evaluate(order, ref_price, now_ns);
    }

    // Also test config swap under tracking
    let new_config = RiskConfig {
        max_quantity: 50_000,
        ..config
    };
    gate.set_config(new_config);

    // Run more evaluations with new config
    for (i, order) in orders.iter().enumerate() {
        let _ = gate.evaluate(order, 100.0, 2_000_000 + i as u64);
    }

    // Stop tracking before any assertion (assert may allocate for error messages)
    TRACKING.store(false, Ordering::SeqCst);
    let count = ALLOC_COUNT.load(Ordering::SeqCst);

    assert_eq!(
        count, 0,
        "risk gate evaluate() performed {count} heap allocations — expected zero"
    );
}
