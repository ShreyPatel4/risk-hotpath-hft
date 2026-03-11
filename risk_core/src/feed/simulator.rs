use crate::envelope::{Action, MarketEvent, OrderRequest, Side, SimEvent};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;

/// Configuration for the synthetic data simulator.
#[derive(Debug, Clone)]
pub struct SimConfig {
    pub symbols: Vec<String>,
    pub base_prices: HashMap<String, f64>,
    pub rate: f64,
    pub seed: u64,
    pub burst_enabled: bool,
    pub burst_probability: f64,
    pub burst_size: usize,
    pub order_probability: f64,
}

impl Default for SimConfig {
    fn default() -> Self {
        let symbols = vec!["AAPL".to_string(), "GOOG".to_string(), "MSFT".to_string()];
        let mut base_prices = HashMap::new();
        base_prices.insert("AAPL".to_string(), 185.0);
        base_prices.insert("GOOG".to_string(), 175.0);
        base_prices.insert("MSFT".to_string(), 420.0);

        Self {
            symbols,
            base_prices,
            rate: 100.0,
            seed: 42,
            burst_enabled: false,
            burst_probability: 0.02,
            burst_size: 20,
            order_probability: 0.3,
        }
    }
}

/// Seedable synthetic market data generator.
pub struct Simulator {
    rng: StdRng,
    config: SimConfig,
    tick: u64,
    current_prices: HashMap<String, f64>,
    next_order_id: u64,
    burst_remaining: usize,
}

impl Simulator {
    pub fn new(config: SimConfig) -> Self {
        let current_prices = config.base_prices.clone();
        let rng = StdRng::seed_from_u64(config.seed);
        Self {
            rng,
            config,
            tick: 0,
            current_prices,
            next_order_id: 1,
            burst_remaining: 0,
        }
    }

    /// Compute next inter-arrival delay in milliseconds (exponential distribution).
    pub fn next_delay_ms(&mut self) -> u64 {
        if self.burst_remaining > 0 {
            return 0;
        }
        let u: f64 = self.rng.gen::<f64>().max(1e-10);
        let delay = -u.ln() / self.config.rate * 1000.0;
        (delay as u64).max(1)
    }

    /// Generate the next simulated event.
    pub fn next_event(&mut self) -> SimEvent {
        if self.config.burst_enabled && self.burst_remaining == 0 {
            let p: f64 = self.rng.gen();
            if p < self.config.burst_probability {
                self.burst_remaining = self.config.burst_size;
            }
        }
        if self.burst_remaining > 0 {
            self.burst_remaining -= 1;
        }

        self.tick += 1;
        let timestamp_us = self.tick * 1000;

        let sym_idx = self.rng.gen_range(0..self.config.symbols.len());
        let symbol = self.config.symbols[sym_idx].clone();

        let drift: f64 = (self.rng.gen::<f64>() - 0.5) * 0.10;
        let current = self.current_prices.get(&symbol).copied().unwrap_or(100.0);
        let new_price = (current + drift).max(0.01);
        self.current_prices.insert(symbol.clone(), new_price);

        let is_order: bool = self.rng.gen::<f64>() < self.config.order_probability;

        if is_order {
            let side = if self.rng.gen::<bool>() {
                Side::Bid
            } else {
                Side::Ask
            };
            let qty = self.rng.gen_range(1..=500);
            let price_offset: f64 = (self.rng.gen::<f64>() - 0.5) * 0.50;
            let order_price = ((new_price + price_offset) * 100.0).round() / 100.0;
            let trader_idx = self.rng.gen_range(0..5);

            let order = OrderRequest {
                id: self.next_order_id,
                timestamp_us,
                symbol,
                side,
                price: order_price,
                qty,
                trader_id: format!("TRADER_{trader_idx}"),
            };
            self.next_order_id += 1;
            SimEvent::Order(order)
        } else {
            let side = if self.rng.gen::<bool>() {
                Side::Bid
            } else {
                Side::Ask
            };
            let qty = self.rng.gen_range(10..=1000);
            let spread_half: f64 = self.rng.gen::<f64>() * 0.05 + 0.01;
            let price = match side {
                Side::Bid => ((new_price - spread_half) * 100.0).round() / 100.0,
                Side::Ask => ((new_price + spread_half) * 100.0).round() / 100.0,
            };

            SimEvent::Market(MarketEvent {
                timestamp_us,
                symbol,
                side,
                price,
                qty,
                action: Action::Add,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_with_same_seed() {
        let config = SimConfig::default();
        let mut sim1 = Simulator::new(config.clone());
        let mut sim2 = Simulator::new(config);

        for _ in 0..100 {
            let e1 = serde_json::to_string(&sim1.next_event()).unwrap();
            let e2 = serde_json::to_string(&sim2.next_event()).unwrap();
            assert_eq!(e1, e2);
        }
    }

    #[test]
    fn test_different_seeds_differ() {
        let c1 = SimConfig {
            seed: 1,
            ..Default::default()
        };
        let c2 = SimConfig {
            seed: 2,
            ..Default::default()
        };

        let mut sim1 = Simulator::new(c1);
        let mut sim2 = Simulator::new(c2);

        let mut same_count = 0;
        for _ in 0..50 {
            let e1 = serde_json::to_string(&sim1.next_event()).unwrap();
            let e2 = serde_json::to_string(&sim2.next_event()).unwrap();
            if e1 == e2 {
                same_count += 1;
            }
        }
        assert!(same_count < 50);
    }

    #[test]
    fn test_generates_both_event_types() {
        let config = SimConfig::default();
        let mut sim = Simulator::new(config);
        let mut market_count = 0;
        let mut order_count = 0;
        for _ in 0..200 {
            match sim.next_event() {
                SimEvent::Market(_) => market_count += 1,
                SimEvent::Order(_) => order_count += 1,
            }
        }
        assert!(market_count > 0);
        assert!(order_count > 0);
    }

    #[test]
    fn test_delay_is_positive() {
        let config = SimConfig::default();
        let mut sim = Simulator::new(config);
        for _ in 0..100 {
            assert!(sim.next_delay_ms() > 0);
        }
    }

    #[test]
    fn test_burst_mode() {
        let config = SimConfig {
            burst_enabled: true,
            burst_probability: 1.0,
            burst_size: 5,
            ..Default::default()
        };

        let mut sim = Simulator::new(config);
        let _ = sim.next_event(); // triggers burst
        for _ in 0..4 {
            assert_eq!(sim.next_delay_ms(), 0);
            let _ = sim.next_event();
        }
    }
}
