use crate::envelope::{OrderRequest, RiskDecision};
use crate::feed::orderbook::OrderBook;
use risk_gate::types::Decision as GateDecision;
use risk_gate::{Order as GateOrder, RiskConfig as GateConfig, RiskGate, Side as GateSide};
use std::collections::HashMap;
use std::time::Instant;

/// Configuration for the risk engine (user-facing, maps to risk-gate RiskConfig).
#[derive(Debug, Clone)]
pub struct RiskConfig {
    pub price_collar_pct: f64,
    pub max_order_qty: u32,
    pub max_notional: f64,
    pub credit_limit_per_trader: f64,
    pub duplicate_window_us: u64,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            price_collar_pct: 0.05,
            max_order_qty: 10_000,
            max_notional: 5_000_000.0,
            credit_limit_per_trader: 1_000_000_000.0,
            duplicate_window_us: 1_000_000,
        }
    }
}

impl RiskConfig {
    fn to_gate_config(&self) -> GateConfig {
        GateConfig {
            max_quantity: self.max_order_qty as u64,
            max_notional: self.max_notional,
            credit_limit: self.credit_limit_per_trader,
            collar_lower: 1.0 - self.price_collar_pct,
            collar_upper: 1.0 + self.price_collar_pct,
            dup_window_ns: self.duplicate_window_us * 1000, // us → ns
        }
    }
}

/// Thin wrapper around `risk_gate::RiskGate` that adds:
/// - String→u32 ID mapping for symbols and traders
/// - `Instant`-based latency measurement
/// - Conversion to/from the harness's serde-friendly types
pub struct RiskEngine {
    gate: RiskGate<1024, 256>,
    symbol_map: HashMap<String, u32>,
    trader_map: HashMap<String, u32>,
    next_symbol_id: u32,
    next_trader_id: u32,
}

impl RiskEngine {
    pub fn new(config: RiskConfig) -> Self {
        Self {
            gate: RiskGate::new(config.to_gate_config()),
            symbol_map: HashMap::new(),
            trader_map: HashMap::new(),
            next_symbol_id: 0,
            next_trader_id: 0,
        }
    }

    fn symbol_id(&mut self, symbol: &str) -> u32 {
        let next = &mut self.next_symbol_id;
        *self
            .symbol_map
            .entry(symbol.to_string())
            .or_insert_with(|| {
                let id = *next;
                *next += 1;
                id
            })
    }

    fn trader_id(&mut self, trader: &str) -> u32 {
        let next = &mut self.next_trader_id;
        *self
            .trader_map
            .entry(trader.to_string())
            .or_insert_with(|| {
                let id = *next;
                *next += 1;
                id
            })
    }

    pub fn evaluate(&mut self, order: &OrderRequest, book: &OrderBook) -> RiskDecision {
        let start = Instant::now();

        let ref_price = book.mid_price(&order.symbol).unwrap_or(0.0);
        let sym_id = self.symbol_id(&order.symbol);
        let trd_id = self.trader_id(&order.trader_id);

        let gate_order = GateOrder {
            order_id: order.id,
            symbol_id: sym_id,
            trader_id: trd_id,
            price: order.price,
            quantity: order.qty as u64,
            side: match order.side {
                crate::envelope::Side::Bid => GateSide::Buy,
                crate::envelope::Side::Ask => GateSide::Sell,
            },
        };

        let now_ns = order.timestamp_us * 1000; // us → ns
        let decision = self.gate.evaluate(&gate_order, ref_price, now_ns);
        let elapsed = start.elapsed().as_micros() as u64;

        match decision {
            GateDecision::Accept => RiskDecision::accept(order.id, order.symbol.clone(), elapsed),
            reject => RiskDecision::reject(
                order.id,
                order.symbol.clone(),
                reject.rule_name(),
                format!("{:?}", reject),
                elapsed,
            ),
        }
    }

    pub fn reset_exposure(&mut self) {
        self.gate.reset_all_credit();
    }

    pub fn trader_exposure(&self, trader_id: &str) -> f64 {
        match self.trader_map.get(trader_id) {
            Some(&id) => self.gate.trader_exposure(id),
            None => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{Action, MarketEvent, Side};

    fn setup_book() -> OrderBook {
        let mut book = OrderBook::new(5);
        book.apply(&MarketEvent {
            timestamp_us: 0,
            symbol: "AAPL".to_string(),
            side: Side::Bid,
            price: 149.0,
            qty: 100,
            action: Action::Add,
        });
        book.apply(&MarketEvent {
            timestamp_us: 0,
            symbol: "AAPL".to_string(),
            side: Side::Ask,
            price: 151.0,
            qty: 100,
            action: Action::Add,
        });
        book
    }

    fn make_order(id: u64, price: f64, qty: u32) -> OrderRequest {
        OrderRequest {
            id,
            timestamp_us: id * 1000,
            symbol: "AAPL".to_string(),
            side: Side::Bid,
            price,
            qty,
            trader_id: "TRADER_0".to_string(),
        }
    }

    #[test]
    fn test_accept_valid_order() {
        let book = setup_book();
        let mut engine = RiskEngine::new(RiskConfig::default());
        let order = make_order(1, 150.0, 10);
        let decision = engine.evaluate(&order, &book);
        assert!(decision.accepted);
        assert!(decision.rule_hit.is_none());
    }

    #[test]
    fn test_reject_max_qty() {
        let book = setup_book();
        let config = RiskConfig {
            max_order_qty: 100,
            ..Default::default()
        };
        let mut engine = RiskEngine::new(config);
        let order = make_order(1, 150.0, 101);
        let decision = engine.evaluate(&order, &book);
        assert!(!decision.accepted);
        assert_eq!(decision.rule_hit.as_deref(), Some("max_qty"));
    }

    #[test]
    fn test_reject_max_notional() {
        let book = setup_book();
        let config = RiskConfig {
            max_notional: 1000.0,
            ..Default::default()
        };
        let mut engine = RiskEngine::new(config);
        let order = make_order(1, 150.0, 100);
        let decision = engine.evaluate(&order, &book);
        assert!(!decision.accepted);
        assert_eq!(decision.rule_hit.as_deref(), Some("max_notional"));
    }

    #[test]
    fn test_reject_price_collar() {
        let book = setup_book();
        let mut engine = RiskEngine::new(RiskConfig::default());
        let order = make_order(1, 160.0, 10);
        let decision = engine.evaluate(&order, &book);
        assert!(!decision.accepted);
        assert_eq!(decision.rule_hit.as_deref(), Some("price_collar"));
    }

    #[test]
    fn test_reject_credit_limit() {
        let book = setup_book();
        let config = RiskConfig {
            credit_limit_per_trader: 5000.0,
            ..Default::default()
        };
        let mut engine = RiskEngine::new(config);
        let o1 = make_order(1, 150.0, 10);
        assert!(engine.evaluate(&o1, &book).accepted);
        let o2 = make_order(2, 150.0, 30);
        let decision = engine.evaluate(&o2, &book);
        assert!(!decision.accepted);
        assert_eq!(decision.rule_hit.as_deref(), Some("credit_limit"));
    }

    #[test]
    fn test_no_book_skips_collar() {
        let book = OrderBook::new(5);
        let mut engine = RiskEngine::new(RiskConfig::default());
        let order = make_order(1, 999.0, 10);
        let decision = engine.evaluate(&order, &book);
        assert!(decision.accepted);
    }

    #[test]
    fn test_exposure_tracking() {
        let book = setup_book();
        let mut engine = RiskEngine::new(RiskConfig::default());
        let order = make_order(1, 150.0, 10);
        engine.evaluate(&order, &book);
        assert!((engine.trader_exposure("TRADER_0") - 1500.0).abs() < 1e-9);
    }
}
