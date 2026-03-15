//! Zero-allocation verification.
//!
//! Installs a custom global allocator that panics on any heap allocation.
//! If `evaluate()` completes 1000 iterations without panicking, the
//! zero-allocation guarantee is proven.
//!
//! This file MUST be a separate integration test binary because only one
//! `#[global_allocator]` can exist per binary.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, Ordering};

use risk_gate::engine::RiskGate;
use risk_gate::types::*;

/// Allocator that panics when allocations are forbidden.
struct PanicAllocator;

static ALLOC_ALLOWED: AtomicBool = AtomicBool::new(true);

unsafe impl GlobalAlloc for PanicAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !ALLOC_ALLOWED.load(Ordering::Relaxed) {
            panic!(
                "HEAP ALLOCATION DETECTED during risk gate evaluation! size={} align={}",
                layout.size(),
                layout.align()
            );
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static A: PanicAllocator = PanicAllocator;

/// Prove that RiskGate::evaluate() performs zero heap allocations.
#[test]
fn test_evaluate_zero_alloc() {
    // Set up gate while allocations are still allowed (for test harness)
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

    // NOW: forbid all allocations
    ALLOC_ALLOWED.store(false, Ordering::SeqCst);

    // Run 1000 evaluations — if any allocates, we panic
    for (i, order) in orders.iter().enumerate() {
        let ref_price = 100.0 + (i as f64 * 0.005);
        let now_ns = i as u64 * 1_000_000; // 1ms spacing
        let _ = gate.evaluate(order, ref_price, now_ns);
    }

    // Also test config swap under no-alloc
    let new_config = RiskConfig {
        max_quantity: 50_000,
        ..config
    };
    gate.set_config(new_config);

    // Run more evaluations with new config
    for (i, order) in orders.iter().enumerate() {
        let _ = gate.evaluate(order, 100.0, 2_000_000 + i as u64);
    }

    // Re-enable allocations for test cleanup
    ALLOC_ALLOWED.store(true, Ordering::SeqCst);
}
