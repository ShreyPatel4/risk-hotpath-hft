use super::checks::price_collar;
use crate::envelope::Envelope;

pub fn assess(envelope: &Envelope) -> bool {
    let within_collar = price_collar(envelope.price, 50.0, 150.0);
    if within_collar {
        println!("accepted envelope: {}", envelope.describe());
    } else {
        println!("rejected envelope: {}", envelope.describe());
    }
    within_collar
}
