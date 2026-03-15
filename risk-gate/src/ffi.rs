//! C FFI bindings for risk-gate.
//!
//! Enabled with `--features ffi`. Build a shared library with:
//! ```sh
//! cargo build --release -p risk-gate --features ffi
//! ```

use crate::engine::RiskGate;
use crate::types::{Decision, Order, RiskConfig};

/// Opaque handle to a RiskGate instance.
pub struct RiskGateHandle {
    inner: RiskGate<1024, 256>,
}

/// Create a new risk gate with the given configuration.
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
/// `gate` and `order` must be valid, non-null pointers.
#[no_mangle]
pub unsafe extern "C" fn risk_gate_evaluate(
    gate: *mut RiskGateHandle,
    order: *const Order,
    ref_price: f64,
    now_ns: u64,
) -> Decision {
    if gate.is_null() || order.is_null() {
        return Decision::RejectInvalidConfig;
    }
    let gate = unsafe { &mut *gate };
    let order = unsafe { &*order };
    gate.inner.evaluate(order, ref_price, now_ns)
}

/// Replace the risk configuration.
///
/// # Safety
/// `gate` must be a valid pointer returned by `risk_gate_new`.
#[no_mangle]
pub unsafe extern "C" fn risk_gate_set_config(gate: *mut RiskGateHandle, config: RiskConfig) {
    if gate.is_null() {
        return;
    }
    let gate = unsafe { &mut *gate };
    gate.inner.set_config(config);
}

/// Get a trader's current credit exposure.
///
/// # Safety
/// `gate` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn risk_gate_trader_exposure(
    gate: *const RiskGateHandle,
    trader_id: u32,
) -> f64 {
    if gate.is_null() {
        return 0.0;
    }
    let gate = unsafe { &*gate };
    gate.inner.trader_exposure(trader_id)
}

/// Reset all credit exposures to zero.
///
/// # Safety
/// `gate` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn risk_gate_reset_credit(gate: *mut RiskGateHandle) {
    if gate.is_null() {
        return;
    }
    let gate = unsafe { &mut *gate };
    gate.inner.reset_all_credit();
}

/// Destroy the risk gate and free memory.
///
/// # Safety
/// `gate` must be a valid pointer returned by `risk_gate_new`, or null.
#[no_mangle]
pub unsafe extern "C" fn risk_gate_destroy(gate: *mut RiskGateHandle) {
    if !gate.is_null() {
        drop(unsafe { Box::from_raw(gate) });
    }
}
