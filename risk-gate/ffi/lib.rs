//! C FFI bindings for risk-gate.
//!
//! Build with: `cargo build --release -p risk-gate --features ffi`
//! Generate header with: `cbindgen --crate risk-gate --output risk-gate/ffi/risk_gate.h`

#![cfg(feature = "ffi")]

use risk_gate::types::{Decision, Order, RiskConfig};
use risk_gate::RiskGate;

/// Opaque handle to a RiskGate instance.
/// The caller owns this and must call `risk_gate_destroy` when done.
pub struct RiskGateHandle {
    inner: RiskGate<1024, 256>,
}

/// Create a new risk gate with the given configuration.
/// Returns a heap-allocated opaque handle. This is the ONLY allocation.
///
/// # Safety
/// The caller must eventually call `risk_gate_destroy` on the returned pointer.
#[no_mangle]
pub extern "C" fn risk_gate_new(config: RiskConfig) -> *mut RiskGateHandle {
    let handle = Box::new(RiskGateHandle {
        inner: RiskGate::new(config),
    });
    Box::into_raw(handle)
}

/// Evaluate an order against the risk gate.
///
/// # Safety
/// `gate` must be a valid pointer returned by `risk_gate_new`.
/// `order` must be a valid pointer to an Order struct.
#[no_mangle]
pub unsafe extern "C" fn risk_gate_evaluate(
    gate: *mut RiskGateHandle,
    order: *const Order,
    ref_price: f64,
    now_ns: u64,
) -> Decision {
    let gate = unsafe { &mut *gate };
    let order = unsafe { &*order };
    gate.inner.evaluate(order, ref_price, now_ns)
}

/// Replace the risk configuration atomically.
///
/// # Safety
/// `gate` must be a valid pointer returned by `risk_gate_new`.
#[no_mangle]
pub unsafe extern "C" fn risk_gate_set_config(gate: *mut RiskGateHandle, config: RiskConfig) {
    let gate = unsafe { &mut *gate };
    gate.inner.set_config(config);
}

/// Get a trader's current credit exposure.
///
/// # Safety
/// `gate` must be a valid pointer returned by `risk_gate_new`.
#[no_mangle]
pub unsafe extern "C" fn risk_gate_trader_exposure(
    gate: *const RiskGateHandle,
    trader_id: u32,
) -> f64 {
    let gate = unsafe { &*gate };
    gate.inner.trader_exposure(trader_id)
}

/// Reset all credit exposures to zero.
///
/// # Safety
/// `gate` must be a valid pointer returned by `risk_gate_new`.
#[no_mangle]
pub unsafe extern "C" fn risk_gate_reset_credit(gate: *mut RiskGateHandle) {
    let gate = unsafe { &mut *gate };
    gate.inner.reset_all_credit();
}

/// Destroy the risk gate and free memory.
///
/// # Safety
/// `gate` must be a valid pointer returned by `risk_gate_new`.
/// After calling this, the pointer is invalid.
#[no_mangle]
pub unsafe extern "C" fn risk_gate_destroy(gate: *mut RiskGateHandle) {
    if !gate.is_null() {
        drop(unsafe { Box::from_raw(gate) });
    }
}
