//! # risk-gate
//!
//! Zero-allocation pre-trade risk gate for high-frequency trading systems.
//!
//! - `no_std` compatible — embed in FPGA soft-cores, kernel modules, or WASM
//! - <200ns per evaluation, 7 checks (including sanity guards)
//! - All types are `#[repr(C)]` — ready for C FFI from day one
//! - Fixed-capacity credit tracker and duplicate detector (no HashMap, no Vec)
//! - Property-tested: no input combination bypasses any check
//!
//! ## Quick Start
//!
//! ```
//! use risk_gate::{RiskGate, Order, Side, RiskConfig, Decision};
//!
//! let config = RiskConfig::default();
//! let mut gate = RiskGate::<256, 64>::new(config);
//!
//! let order = Order {
//!     order_id: 1,
//!     symbol_id: 0,
//!     trader_id: 0,
//!     price: 150.0,
//!     quantity: 100,
//!     side: Side::Buy,
//! };
//!
//! let decision = gate.evaluate(&order, 150.0, 0);
//! assert!(decision.is_accept());
//! ```

#![cfg_attr(not(feature = "std"), no_std)]

pub mod checks;
pub mod credit;
pub mod dedup;
pub mod engine;
#[cfg(feature = "ffi")]
pub mod ffi;
pub mod types;

// Re-export the main API at crate root
pub use engine::RiskGate;
pub use types::{Decision, Order, RiskConfig, Side};
