use crate::envelope::{MarketEvent, RiskDecision};

/// ClickHouse writer for persisting risk events and market data.
/// When the `clickhouse` feature is disabled, all operations are no-ops.
#[cfg(feature = "clickhouse")]
mod inner {
    use super::*;
    use clickhouse::Client;

    pub struct ClickHouseWriter {
        client: Client,
    }

    impl ClickHouseWriter {
        pub fn new(url: &str) -> Self {
            let client = Client::default().with_url(url);
            Self { client }
        }

        pub async fn create_tables(&self) -> anyhow::Result<()> {
            self.client
                .query(
                    "CREATE TABLE IF NOT EXISTS risk_events (
                        timestamp_us UInt64,
                        order_id UInt64,
                        symbol String,
                        accepted UInt8,
                        rule_hit String,
                        reason String,
                        latency_us UInt64
                    ) ENGINE = MergeTree()
                    ORDER BY (timestamp_us, order_id)",
                )
                .execute()
                .await?;

            self.client
                .query(
                    "CREATE TABLE IF NOT EXISTS market_ticks (
                        timestamp_us UInt64,
                        symbol String,
                        side String,
                        price Float64,
                        qty UInt32,
                        action String
                    ) ENGINE = MergeTree()
                    ORDER BY (timestamp_us, symbol)",
                )
                .execute()
                .await?;

            println!("[clickhouse] tables created or verified");
            Ok(())
        }

        pub async fn write_risk_event(&self, decision: &RiskDecision) -> anyhow::Result<()> {
            let accepted: u8 = u8::from(decision.accepted);
            let rule_hit = decision.rule_hit.as_deref().unwrap_or("");
            let reason = decision.reason.replace('\'', "''");
            let sql = format!(
                "INSERT INTO risk_events VALUES ({}, {}, '{}', {}, '{}', '{}', {})",
                decision.latency_us,
                decision.order_id,
                decision.symbol,
                accepted,
                rule_hit,
                reason,
                decision.latency_us
            );
            self.client.query(&sql).execute().await?;
            Ok(())
        }

        pub async fn write_market_event(&self, event: &MarketEvent) -> anyhow::Result<()> {
            let sql = format!(
                "INSERT INTO market_ticks VALUES ({}, '{}', '{}', {}, {}, '{:?}')",
                event.timestamp_us, event.symbol, event.side, event.price, event.qty, event.action
            );
            self.client.query(&sql).execute().await?;
            Ok(())
        }
    }

    pub fn new_writer(url: &str) -> ClickHouseWriter {
        ClickHouseWriter::new(url)
    }
}

#[cfg(not(feature = "clickhouse"))]
mod inner {
    use super::*;

    pub struct ClickHouseWriter;

    impl ClickHouseWriter {
        pub async fn create_tables(&self) -> anyhow::Result<()> {
            Ok(())
        }

        pub async fn write_risk_event(&self, _decision: &RiskDecision) -> anyhow::Result<()> {
            Ok(())
        }

        pub async fn write_market_event(&self, _event: &MarketEvent) -> anyhow::Result<()> {
            Ok(())
        }
    }

    pub fn new_writer(_url: &str) -> ClickHouseWriter {
        println!("[clickhouse] feature disabled, persistence is off");
        ClickHouseWriter
    }
}

pub use inner::{new_writer, ClickHouseWriter};
