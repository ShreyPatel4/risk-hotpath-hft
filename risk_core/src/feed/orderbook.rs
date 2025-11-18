use crate::envelope::Envelope;

#[derive(Default, Debug)]
pub struct OrderBook {
    last: Option<Envelope>,
}

impl OrderBook {
    pub fn update(&mut self, envelope: Envelope) {
        self.last = Some(envelope);
    }

    pub fn latest(&self) -> Option<&Envelope> {
        self.last.as_ref()
    }
}
