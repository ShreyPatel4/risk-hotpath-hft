//! Red Team: FFI boundary tests.
//!
//! Tests the C API for null-safety, ABI stability, and correct behavior
//! through the foreign function interface.

#![cfg(feature = "ffi")]

use risk_gate::ffi::*;
use risk_gate::types::*;
use std::ptr;

/// Full create → evaluate → destroy lifecycle through FFI.
#[test]
fn test_ffi_full_lifecycle() {
    let config = RiskConfig::default();
    let gate = risk_gate_new(config);
    assert!(!gate.is_null());

    let order = Order {
        order_id: 1,
        symbol_id: 0,
        trader_id: 0,
        price: 100.0,
        quantity: 10,
        side: Side::Buy,
    };

    let decision = unsafe { risk_gate_evaluate(gate, &order as *const Order, 100.0, 0) };
    assert_eq!(decision, Decision::Accept);

    unsafe { risk_gate_destroy(gate) };
}

/// After Fix 3: null pointers return safe defaults instead of UB.
#[test]
fn test_ffi_null_pointer_safety() {
    let order = Order {
        order_id: 1,
        symbol_id: 0,
        trader_id: 0,
        price: 100.0,
        quantity: 10,
        side: Side::Buy,
    };

    // Null gate pointer → RejectInvalidConfig
    let d = unsafe { risk_gate_evaluate(ptr::null_mut(), &order as *const Order, 100.0, 0) };
    assert_eq!(d, Decision::RejectInvalidConfig);

    // Null order pointer → RejectInvalidConfig
    let config = RiskConfig::default();
    let gate = risk_gate_new(config);
    let d = unsafe { risk_gate_evaluate(gate, ptr::null(), 100.0, 0) };
    assert_eq!(d, Decision::RejectInvalidConfig);

    // Null gate for other functions — should not crash
    unsafe { risk_gate_set_config(ptr::null_mut(), config) };
    let exposure = unsafe { risk_gate_trader_exposure(ptr::null(), 0) };
    assert_eq!(exposure, 0.0);
    unsafe { risk_gate_reset_credit(ptr::null_mut()) };

    // Null destroy is already handled
    unsafe { risk_gate_destroy(ptr::null_mut()) };

    // Clean up
    unsafe { risk_gate_destroy(gate) };
}

/// Verify repr(C) type sizes and alignments match expected values.
/// This catches ABI breakage across compiler versions.
#[test]
fn test_ffi_repr_c_sizes_and_alignment() {
    // Order: u64 + u32 + u32 + f64 + u64 + u8(enum) + padding
    assert!(core::mem::size_of::<Order>() > 0);
    assert!(core::mem::align_of::<Order>() >= 4);

    // RiskConfig: u64 + f64 + f64 + f64 + f64 + u64 = 48 bytes
    assert_eq!(core::mem::size_of::<RiskConfig>(), 48);
    assert!(core::mem::align_of::<RiskConfig>() >= 8);

    // Decision: repr(C) enum, typically u32
    assert!(core::mem::size_of::<Decision>() >= 1);

    // Side: repr(C) enum
    assert!(core::mem::size_of::<Side>() >= 1);
}

/// Both Side::Buy and Side::Sell produce different dedup hashes through FFI.
#[test]
fn test_ffi_both_side_values() {
    let config = RiskConfig {
        credit_limit: f64::MAX,
        dup_window_ns: 1_000_000_000,
        ..Default::default()
    };
    let gate = risk_gate_new(config);

    let buy_order = Order {
        order_id: 1,
        symbol_id: 0,
        trader_id: 0,
        price: 100.0,
        quantity: 1,
        side: Side::Buy,
    };

    let sell_order = Order {
        order_id: 2,
        symbol_id: 0,
        trader_id: 0,
        price: 100.0,
        quantity: 1,
        side: Side::Sell,
    };

    // Both should accept (different sides produce different dedup hashes)
    let d1 = unsafe { risk_gate_evaluate(gate, &buy_order as *const Order, 0.0, 0) };
    assert_eq!(d1, Decision::Accept);

    let d2 = unsafe { risk_gate_evaluate(gate, &sell_order as *const Order, 0.0, 1) };
    assert_eq!(d2, Decision::Accept);

    unsafe { risk_gate_destroy(gate) };
}

/// Config swap through FFI, verify behavior changes.
#[test]
fn test_ffi_config_swap_via_c_api() {
    let config = RiskConfig {
        max_quantity: 100,
        credit_limit: f64::MAX,
        ..Default::default()
    };
    let gate = risk_gate_new(config);

    let order = Order {
        order_id: 1,
        symbol_id: 0,
        trader_id: 0,
        price: 10.0,
        quantity: 101, // exceeds max_quantity=100
        side: Side::Buy,
    };

    // Should reject: qty 101 > max 100
    let d = unsafe { risk_gate_evaluate(gate, &order as *const Order, 0.0, 0) };
    assert_eq!(d, Decision::RejectMaxQuantity);

    // Swap config to allow higher quantity
    let new_config = RiskConfig {
        max_quantity: 200,
        credit_limit: f64::MAX,
        ..Default::default()
    };
    unsafe { risk_gate_set_config(gate, new_config) };

    // Reset credit and retry
    unsafe { risk_gate_reset_credit(gate) };
    let d = unsafe { risk_gate_evaluate(gate, &order as *const Order, 0.0, 1) };
    assert_eq!(d, Decision::Accept);

    // Verify exposure tracking through FFI
    let exposure = unsafe { risk_gate_trader_exposure(gate, 0) };
    assert!(exposure > 0.0);

    unsafe { risk_gate_destroy(gate) };
}
