mod envelope;
mod feed;
mod replay;
mod risk;
mod store;
mod telemetry;

use envelope::Envelope;
use telemetry::metrics;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "simulate".to_string());

    match mode.as_str() {
        "replay" => {
            replay::runner::replay_from("./data/snapshots")?;
        }
        "export" => {
            metrics::init();
            println!("metrics exporter is listening on 9898");
        }
        _ => {
            metrics::init();

            let mut book = feed::orderbook::OrderBook::default();
            let envelope = Envelope::new(1, 101.25, 100);
            book.update(envelope.clone());
            if let Some(latest) = book.latest() {
                println!("latest envelope in book: {}", latest.describe());
            }

            if risk::engine::assess(&envelope) {
                metrics::record_simulation();
                store::clickhouse::write_event(&envelope)?;
            }

            feed::simulator::run().await?;
            replay::runner::replay_from("./data/snapshots")?;
        }
    }

    println!("risk-core exited cleanly");
    Ok(())
}
