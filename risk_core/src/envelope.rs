use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Bid,
    Ask,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Side::Bid => write!(f, "bid"),
            Side::Ask => write!(f, "ask"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Add,
    Update,
    Delete,
}

/// A market data update representing a price level change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketEvent {
    pub timestamp_us: u64,
    pub symbol: String,
    pub side: Side,
    pub price: f64,
    pub qty: u32,
    pub action: Action,
}

/// An order request to be evaluated by the risk engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub id: u64,
    pub timestamp_us: u64,
    pub symbol: String,
    pub side: Side,
    pub price: f64,
    pub qty: u32,
    pub trader_id: String,
}

impl OrderRequest {
    pub fn notional(&self) -> f64 {
        self.price * self.qty as f64
    }
}

/// A tagged union of events produced by the simulator or read from replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SimEvent {
    #[serde(rename = "market")]
    Market(MarketEvent),
    #[serde(rename = "order")]
    Order(OrderRequest),
}

/// Structured risk decision returned by the engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskDecision {
    pub accepted: bool,
    pub order_id: u64,
    pub symbol: String,
    pub rule_hit: Option<String>,
    pub reason: String,
    pub latency_us: u64,
}

impl RiskDecision {
    pub fn accept(order_id: u64, symbol: String, latency_us: u64) -> Self {
        Self {
            accepted: true,
            order_id,
            symbol,
            rule_hit: None,
            reason: "all checks passed".to_string(),
            latency_us,
        }
    }

    pub fn reject(
        order_id: u64,
        symbol: String,
        rule: &str,
        reason: String,
        latency_us: u64,
    ) -> Self {
        Self {
            accepted: false,
            order_id,
            symbol,
            rule_hit: Some(rule.to_string()),
            reason,
            latency_us,
        }
    }
}
