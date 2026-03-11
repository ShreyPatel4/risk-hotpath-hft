use crate::envelope::{Action, MarketEvent, Side};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PriceLevel {
    pub price: f64,
    pub qty: u32,
}

#[derive(Debug, Clone, Default)]
pub struct BookSide {
    levels: Vec<PriceLevel>,
    max_depth: usize,
}

impl BookSide {
    fn new(max_depth: usize) -> Self {
        Self {
            levels: Vec::with_capacity(max_depth),
            max_depth,
        }
    }

    fn find_index(&self, price: f64) -> Option<usize> {
        self.levels
            .iter()
            .position(|l| (l.price - price).abs() < 1e-9)
    }

    pub fn apply(&mut self, price: f64, qty: u32, action: Action, ascending: bool) {
        match action {
            Action::Delete => {
                if let Some(idx) = self.find_index(price) {
                    self.levels.remove(idx);
                }
            }
            Action::Add | Action::Update => {
                if let Some(idx) = self.find_index(price) {
                    self.levels[idx].qty = qty;
                } else {
                    self.levels.push(PriceLevel { price, qty });
                }
                if ascending {
                    self.levels
                        .sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
                } else {
                    self.levels
                        .sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap());
                }
                self.levels.truncate(self.max_depth);
            }
        }
    }

    pub fn best(&self) -> Option<&PriceLevel> {
        self.levels.first()
    }

    pub fn depth(&self) -> &[PriceLevel] {
        &self.levels
    }
}

#[derive(Debug, Clone)]
pub struct SymbolBook {
    pub bids: BookSide,
    pub asks: BookSide,
}

impl SymbolBook {
    fn new(max_depth: usize) -> Self {
        Self {
            bids: BookSide::new(max_depth),
            asks: BookSide::new(max_depth),
        }
    }

    pub fn best_bid(&self) -> Option<f64> {
        self.bids.best().map(|l| l.price)
    }

    pub fn best_ask(&self) -> Option<f64> {
        self.asks.best().map(|l| l.price)
    }

    pub fn spread(&self) -> Option<f64> {
        match (self.best_ask(), self.best_bid()) {
            (Some(ask), Some(bid)) => Some(ask - bid),
            _ => None,
        }
    }

    pub fn mid_price(&self) -> Option<f64> {
        match (self.best_ask(), self.best_bid()) {
            (Some(ask), Some(bid)) => Some((ask + bid) / 2.0),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OrderBook {
    books: HashMap<String, SymbolBook>,
    max_depth: usize,
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new(10)
    }
}

impl OrderBook {
    pub fn new(max_depth: usize) -> Self {
        Self {
            books: HashMap::new(),
            max_depth,
        }
    }

    pub fn apply(&mut self, event: &MarketEvent) {
        let book = self
            .books
            .entry(event.symbol.clone())
            .or_insert_with(|| SymbolBook::new(self.max_depth));

        match event.side {
            Side::Bid => book.bids.apply(event.price, event.qty, event.action, false),
            Side::Ask => book.asks.apply(event.price, event.qty, event.action, true),
        }
    }

    pub fn get(&self, symbol: &str) -> Option<&SymbolBook> {
        self.books.get(symbol)
    }

    pub fn symbols(&self) -> Vec<&String> {
        self.books.keys().collect()
    }

    pub fn best_bid(&self, symbol: &str) -> Option<f64> {
        self.books.get(symbol).and_then(|b| b.best_bid())
    }

    pub fn best_ask(&self, symbol: &str) -> Option<f64> {
        self.books.get(symbol).and_then(|b| b.best_ask())
    }

    pub fn mid_price(&self, symbol: &str) -> Option<f64> {
        self.books.get(symbol).and_then(|b| b.mid_price())
    }

    pub fn spread(&self, symbol: &str) -> Option<f64> {
        self.books.get(symbol).and_then(|b| b.spread())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{Action, Side};

    fn make_event(symbol: &str, side: Side, price: f64, qty: u32, action: Action) -> MarketEvent {
        MarketEvent {
            timestamp_us: 0,
            symbol: symbol.to_string(),
            side,
            price,
            qty,
            action,
        }
    }

    #[test]
    fn test_add_bids_and_asks() {
        let mut book = OrderBook::new(5);
        book.apply(&make_event("AAPL", Side::Bid, 150.0, 100, Action::Add));
        book.apply(&make_event("AAPL", Side::Bid, 149.5, 200, Action::Add));
        book.apply(&make_event("AAPL", Side::Ask, 150.5, 50, Action::Add));
        book.apply(&make_event("AAPL", Side::Ask, 151.0, 75, Action::Add));

        assert_eq!(book.best_bid("AAPL"), Some(150.0));
        assert_eq!(book.best_ask("AAPL"), Some(150.5));
        assert!((book.spread("AAPL").unwrap() - 0.5).abs() < 1e-9);
        assert!((book.mid_price("AAPL").unwrap() - 150.25).abs() < 1e-9);
    }

    #[test]
    fn test_update_price_level() {
        let mut book = OrderBook::new(5);
        book.apply(&make_event("AAPL", Side::Bid, 150.0, 100, Action::Add));
        assert_eq!(book.get("AAPL").unwrap().bids.best().unwrap().qty, 100);

        book.apply(&make_event("AAPL", Side::Bid, 150.0, 250, Action::Update));
        assert_eq!(book.get("AAPL").unwrap().bids.best().unwrap().qty, 250);
    }

    #[test]
    fn test_delete_price_level() {
        let mut book = OrderBook::new(5);
        book.apply(&make_event("AAPL", Side::Bid, 150.0, 100, Action::Add));
        book.apply(&make_event("AAPL", Side::Bid, 149.0, 200, Action::Add));
        assert_eq!(book.best_bid("AAPL"), Some(150.0));

        book.apply(&make_event("AAPL", Side::Bid, 150.0, 0, Action::Delete));
        assert_eq!(book.best_bid("AAPL"), Some(149.0));
    }

    #[test]
    fn test_max_depth_truncation() {
        let mut book = OrderBook::new(3);
        for i in 0..10 {
            let price = 100.0 + i as f64;
            book.apply(&make_event("AAPL", Side::Bid, price, 10, Action::Add));
        }
        let bids = book.get("AAPL").unwrap().bids.depth();
        assert_eq!(bids.len(), 3);
        assert!((bids[0].price - 109.0).abs() < 1e-9);
        assert!((bids[1].price - 108.0).abs() < 1e-9);
        assert!((bids[2].price - 107.0).abs() < 1e-9);
    }

    #[test]
    fn test_multiple_symbols() {
        let mut book = OrderBook::new(5);
        book.apply(&make_event("AAPL", Side::Bid, 150.0, 100, Action::Add));
        book.apply(&make_event("GOOG", Side::Bid, 2800.0, 50, Action::Add));

        assert_eq!(book.best_bid("AAPL"), Some(150.0));
        assert_eq!(book.best_bid("GOOG"), Some(2800.0));
        assert_eq!(book.best_bid("MSFT"), None);
    }

    #[test]
    fn test_empty_book() {
        let book = OrderBook::new(5);
        assert_eq!(book.best_bid("AAPL"), None);
        assert_eq!(book.best_ask("AAPL"), None);
        assert_eq!(book.spread("AAPL"), None);
        assert_eq!(book.mid_price("AAPL"), None);
    }

    #[test]
    fn test_asks_sorted_ascending() {
        let mut book = OrderBook::new(5);
        book.apply(&make_event("AAPL", Side::Ask, 155.0, 10, Action::Add));
        book.apply(&make_event("AAPL", Side::Ask, 152.0, 20, Action::Add));
        book.apply(&make_event("AAPL", Side::Ask, 158.0, 30, Action::Add));

        let asks = book.get("AAPL").unwrap().asks.depth();
        assert!((asks[0].price - 152.0).abs() < 1e-9);
        assert!((asks[1].price - 155.0).abs() < 1e-9);
        assert!((asks[2].price - 158.0).abs() < 1e-9);
    }
}
