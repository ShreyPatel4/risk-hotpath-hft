use std::time::Duration;

use crate::envelope::Envelope;
use crate::telemetry::metrics;

pub async fn run() -> anyhow::Result<()> {
    let mut tick: u64 = 0;
    while tick < 2 {
        let envelope = Envelope::new(tick, 100.0 + tick as f64, 10 + tick as u32);
        metrics::record_simulation();
        println!("simulated trade: {}", envelope.describe());
        tick += 1;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
}
