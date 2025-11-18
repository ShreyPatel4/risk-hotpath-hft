pub fn price_collar(price: f64, lower: f64, upper: f64) -> bool {
    price >= lower && price <= upper
}

#[cfg(test)]
mod tests {
    use super::price_collar;

    #[test]
    fn price_within_bounds() {
        assert!(price_collar(100.0, 90.0, 110.0));
        assert!(!price_collar(120.0, 90.0, 110.0));
    }
}
