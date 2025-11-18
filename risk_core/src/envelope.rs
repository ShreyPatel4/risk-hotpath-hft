#[derive(Debug, Clone)]
pub struct Envelope {
    pub id: u64,
    pub price: f64,
    pub size: u32,
}

impl Envelope {
    pub fn new(id: u64, price: f64, size: u32) -> Self {
        Self { id, price, size }
    }

    pub fn describe(&self) -> String {
        format!(
            "Envelope id={} price={} size={}",
            self.id, self.price, self.size
        )
    }
}
