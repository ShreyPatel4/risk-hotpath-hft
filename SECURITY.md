# Security Hardening Report — risk-gate v0.1.0

## Overview

The `risk-gate` crate has undergone adversarial red-team testing and security
hardening. This document summarises the vulnerabilities discovered, fixes
applied, and known limitations that users must understand.

## Vulnerabilities Found & Fixed

### 1. NaN config fields silently disabled checks (CRITICAL)

**Vector:** Setting `credit_limit = f64::NAN` in `RiskConfig` caused the
credit check comparison `new_exposure > NaN` to always return `false`,
silently accepting every order regardless of credit exposure.

**Fix:** Added `RiskConfig::is_valid()` method and `Decision::RejectInvalidConfig`
variant. The gate now validates config as the first check in `evaluate()`.
Any NaN or Inf in a config field immediately rejects.

**Files:** `types.rs`, `engine.rs`

### 2. NaN propagation in credit tracker (CRITICAL)

**Vector:** If exposure ever became NaN (e.g., via `set_exposure(NaN)`),
the comparison `NaN > limit` is always `false` — every subsequent order
would be accepted regardless of the credit limit.

**Fix:** Changed credit comparison from `new_exposure > limit` to
`!(new_exposure <= limit)`. The negated form is `true` when `new_exposure`
is NaN, correctly rejecting the order. Also guarded `set_exposure()` to
reject non-finite values.

**File:** `credit.rs`

### 3. DupDetector\<0\> division-by-zero panic (HIGH)

**Vector:** Instantiating `DupDetector::<0>::new()` then calling
`is_duplicate()` triggered `(self.head + 1) % 0` — a division-by-zero
panic.

**Fix:** Added compile-time assertion `assert!(N > 0)` in `DupDetector::new()`.
Zero-capacity rings now fail at compile time with a clear message.

**File:** `dedup.rs`

### 4. FFI null-pointer dereference (HIGH)

**Vector:** Calling `risk_gate_evaluate` with a null gate or order pointer
caused undefined behaviour (segfault).

**Fix:** All FFI functions now check for null pointers and return safe
defaults (`Decision::RejectInvalidConfig` or `0.0`) instead of dereferencing.

**File:** `ffi.rs`

### 5. Ring buffer eviction bypass (KNOWN LIMITATION)

**Vector:** An attacker who knows the dedup ring capacity `N` can submit
`N` distinct orders to evict a previous order's hash, then re-submit the
original without detection.

**Status:** By-design trade-off of fixed-capacity, zero-allocation
architecture. **Mitigation:** Size the ring capacity to
`peak_order_rate × dup_window_duration`. Documented in `dedup.rs`.

### 6. f64 accumulation drift (KNOWN LIMITATION)

**Vector:** Thousands of small orders cause floating-point accumulation
error in the credit tracker. After many additions, tracked exposure may
differ from the true mathematical sum by a few ULPs.

**Status:** Inherent to f64 arithmetic. The drift is bounded and small
(< 1 ULP per addition). At worst, one additional order may be accepted
or rejected near the exact limit boundary.

## Test Coverage

| Suite | Tests | Coverage |
|-------|-------|----------|
| Unit tests | 28 | All modules |
| Property tests | 8 | Proptest invariants |
| Red team: floats | 8 | IEEE 754 edge cases |
| Red team: boundaries | 7 | Exact threshold attacks |
| Red team: credit | 6 | Accumulation, overflow, NaN |
| Red team: dedup | 7 | Eviction, timing, collisions |
| Red team: config | 7 | Adversarial configs |
| Red team: FFI | 5 | Null safety, ABI stability |
| Immunity proofs | 8 | Full-range proptest proofs |
| Stress tests | 6 | 1M-order chaos, saturation |
| Zero-alloc proof | 1 | Custom allocator verification |
| **Total** | **91** | |

## Running the Security Suite

```bash
make redteam-all    # Full red-team suite + adversarial benchmarks
make check          # Format + clippy + all workspace tests
```

## Responsible Disclosure

If you discover a security issue in risk-gate, please email the maintainers
directly rather than opening a public issue.
