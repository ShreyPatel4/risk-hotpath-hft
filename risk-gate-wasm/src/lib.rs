//! risk-gate-wasm — the real gate, compiled for the browser.
//!
//! Flat scalar C-ABI over `risk-gate` so the hall-of-demos page can run the
//! actual no_std crate (not a JS imitation) via raw `WebAssembly.instantiate`,
//! no wasm-bindgen, no allocator, no glue. One global gate instance: the demo
//! is single-threaded and wasm32-unknown-unknown has no threads.
//!
//! Build:
//! ```sh
//! rustup target add wasm32-unknown-unknown
//! cargo build --release -p risk-gate-wasm --target wasm32-unknown-unknown
//! # artifact: target/wasm32-unknown-unknown/release/risk_gate_wasm.wasm
//! ```

#![no_std]

use core::panic::PanicInfo;

use risk_gate::{Decision, Order, RiskConfig, RiskGate, Side};

/// The demo gate: same const generics as the README example.
static mut GATE: Option<RiskGate<1024, 256>> = None;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

fn config_from(
    max_quantity: u64,
    max_notional: f64,
    credit_limit: f64,
    collar_lower: f64,
    collar_upper: f64,
    dup_window_ns: u64,
) -> RiskConfig {
    RiskConfig {
        max_quantity,
        max_notional,
        credit_limit,
        collar_lower,
        collar_upper,
        dup_window_ns,
    }
}

/// (Re)create the gate with the given configuration. Resets all state.
#[no_mangle]
pub extern "C" fn rg_init(
    max_quantity: u64,
    max_notional: f64,
    credit_limit: f64,
    collar_lower: f64,
    collar_upper: f64,
    dup_window_ns: u64,
) {
    let config = config_from(
        max_quantity,
        max_notional,
        credit_limit,
        collar_lower,
        collar_upper,
        dup_window_ns,
    );
    unsafe {
        GATE = Some(RiskGate::new(config));
    }
}

/// Hot-swap the configuration on the live gate, keeping credit/dedup state.
/// Returns 1 if a gate existed, 0 otherwise.
#[no_mangle]
pub extern "C" fn rg_swap_config(
    max_quantity: u64,
    max_notional: f64,
    credit_limit: f64,
    collar_lower: f64,
    collar_upper: f64,
    dup_window_ns: u64,
) -> u32 {
    let config = config_from(
        max_quantity,
        max_notional,
        credit_limit,
        collar_lower,
        collar_upper,
        dup_window_ns,
    );
    unsafe {
        match GATE.as_mut() {
            Some(gate) => {
                gate.set_config(config);
                1
            }
            None => 0,
        }
    }
}

/// Evaluate one order. Returns the Decision discriminant (0 = accept,
/// 1..=8 per risk_gate::Decision). side: 0 = buy, 1 = sell.
#[no_mangle]
pub extern "C" fn rg_evaluate(
    order_id: u64,
    symbol_id: u32,
    trader_id: u32,
    price: f64,
    quantity: u64,
    side: u32,
    ref_price: f64,
    now_ns: u64,
) -> u32 {
    let order = Order {
        order_id,
        symbol_id,
        trader_id,
        price,
        quantity,
        side: if side == 0 { Side::Buy } else { Side::Sell },
    };
    unsafe {
        match GATE.as_mut() {
            Some(gate) => gate.evaluate(&order, ref_price, now_ns) as u32,
            None => Decision::RejectInvalidConfig as u32,
        }
    }
}

/// Tight-loop benchmark INSIDE wasm: evaluates `n` synthetic orders (seeded
/// LCG, mix of clean and violating orders) without crossing the JS boundary
/// per call. Returns a checksum of decisions so the loop cannot be optimized
/// away. JS wall-clocks the whole call for an honest evals/sec figure in the
/// visitor's own browser.
#[no_mangle]
pub extern "C" fn rg_bench(n: u64, seed: u64) -> u64 {
    let mut s = if seed == 0 { 0x9E3779B97F4A7C15 } else { seed };
    let mut next = move || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        s
    };
    let mut checksum: u64 = 0;
    unsafe {
        let gate = match GATE.as_mut() {
            Some(g) => g,
            None => return u64::MAX,
        };
        for i in 0..n {
            let r = next();
            let ref_price = 100.0 + ((r >> 32) % 100) as f64;
            // ~1 in 8 orders violates the collar, ~1 in 16 the size cap;
            // the rest are clean. Mirrors the repo generator's shape.
            let price = match r % 16 {
                0 | 1 => ref_price * 1.2,
                2 => ref_price * 0.7,
                _ => ref_price * (0.98 + ((r >> 8) % 5) as f64 * 0.01),
            };
            let quantity = if r % 16 == 3 { 50_000 } else { 1 + (r >> 16) % 900 };
            let order = Order {
                order_id: i,
                symbol_id: (r % 120) as u32,
                trader_id: ((r >> 7) % 50) as u32,
                price,
                quantity,
                side: if r % 2 == 0 { Side::Buy } else { Side::Sell },
            };
            checksum = checksum
                .wrapping_add(gate.evaluate(&order, ref_price, i * 1_000) as u64);
        }
    }
    checksum
}
