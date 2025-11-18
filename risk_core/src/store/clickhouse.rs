use crate::envelope::Envelope;

#[cfg(feature = "clickhouse")]
pub fn write_event(envelope: &Envelope) -> anyhow::Result<()> {
    let client = clickhouse::Client::default().with_url("http://localhost:8123");
    println!("would write event {:?} to clickhouse", envelope);
    let _ = client; // silence unused warning when feature enabled
    Ok(())
}

#[cfg(not(feature = "clickhouse"))]
pub fn write_event(_envelope: &Envelope) -> anyhow::Result<()> {
    Ok(())
}
