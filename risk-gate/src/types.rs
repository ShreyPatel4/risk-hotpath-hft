//! Core types for the risk gate. All types are `Copy`, `repr(C)`, and allocation-free.

/// Side of an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub enum Side {
    Buy = 0,
    Sell = 1,
}

/// An order to be evaluated by the risk gate.
///
/// Uses integer IDs instead of strings — the caller is responsible for
/// mapping symbols and traders to numeric IDs before calling the gate.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Order {
    pub order_id: u64,
    pub symbol_id: u32,
    pub trader_id: u32,
    pub price: f64,
    pub quantity: u64,
    pub side: Side,
}

/// Result of a risk evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub enum Decision {
    Accept = 0,
    RejectMaxQuantity = 1,
    RejectMaxNotional = 2,
    RejectPriceCollar = 3,
    RejectCreditLimit = 4,
    RejectDuplicate = 5,
    RejectZeroQuantity = 6,
    RejectInvalidPrice = 7,
}

impl Decision {
    /// Returns true if the order was accepted.
    #[inline(always)]
    pub fn is_accept(self) -> bool {
        matches!(self, Decision::Accept)
    }

    /// Returns true if the order was rejected.
    #[inline(always)]
    pub fn is_reject(self) -> bool {
        !self.is_accept()
    }

    /// Short name of the rejection rule, or "accept".
    pub fn rule_name(self) -> &'static str {
        match self {
            Decision::Accept => "accept",
            Decision::RejectMaxQuantity => "max_qty",
            Decision::RejectMaxNotional => "max_notional",
            Decision::RejectPriceCollar => "price_collar",
            Decision::RejectCreditLimit => "credit_limit",
            Decision::RejectDuplicate => "duplicate",
            Decision::RejectZeroQuantity => "zero_qty",
            Decision::RejectInvalidPrice => "invalid_price",
        }
    }
}

/// Configuration for all risk checks. All values are plain `Copy` types.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct RiskConfig {
    /// Maximum allowed order quantity.
    pub max_quantity: u64,
    /// Maximum allowed notional value (price * quantity).
    pub max_notional: f64,
    /// Per-trader cumulative credit exposure limit.
    pub credit_limit: f64,
    /// Price collar: lower bound as fraction of reference price (e.g. 0.95 = -5%).
    pub collar_lower: f64,
    /// Price collar: upper bound as fraction of reference price (e.g. 1.05 = +5%).
    pub collar_upper: f64,
    /// Duplicate detection time window in nanoseconds.
    pub dup_window_ns: u64,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_quantity: 10_000,
            max_notional: 5_000_000.0,
            credit_limit: 1_000_000_000.0,
            collar_lower: 0.95,
            collar_upper: 1.05,
            dup_window_ns: 1_000_000_000, // 1 second
        }
    }
}
