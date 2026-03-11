use crate::envelope::{RiskDecision, SimEvent};
use crate::feed::orderbook::OrderBook;
use crate::risk::engine::{RiskConfig, RiskEngine};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

// Latency histogram bucket boundaries (microseconds)
const LATENCY_BUCKETS: [u64; 10] = [0, 1, 2, 5, 10, 25, 50, 100, 500, 1000];

/// Summary of a replay run with throughput and latency statistics.
#[derive(Debug, Default)]
pub struct ReplaySummary {
    pub total_events: usize,
    pub market_events: usize,
    pub order_events: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub reject_by_rule: HashMap<String, usize>,
    pub parse_errors: usize,
    // Timing
    pub wall_clock_secs: f64,
    // Latency (streaming — no per-decision Vec needed)
    pub latency_min: u64,
    pub latency_max: u64,
    pub latency_sum: u64,
    pub latency_count: u64,
    pub latency_histogram: [u64; 11], // 10 buckets + overflow
    // Optional: stored decisions (only for baseline comparison)
    pub decisions: Option<Vec<RiskDecision>>,
}

impl ReplaySummary {
    fn record_latency(&mut self, us: u64) {
        self.latency_count += 1;
        self.latency_sum += us;
        if self.latency_count == 1 {
            self.latency_min = us;
            self.latency_max = us;
        } else {
            self.latency_min = self.latency_min.min(us);
            self.latency_max = self.latency_max.max(us);
        }
        // Histogram bucket
        let idx = LATENCY_BUCKETS
            .iter()
            .rposition(|&b| us >= b)
            .map(|i| i + 1)
            .unwrap_or(0);
        self.latency_histogram[idx.min(10)] += 1;
    }

    fn latency_avg(&self) -> f64 {
        if self.latency_count == 0 {
            0.0
        } else {
            self.latency_sum as f64 / self.latency_count as f64
        }
    }

    /// Estimate a percentile from the histogram using linear interpolation.
    fn latency_percentile(&self, p: f64) -> u64 {
        if self.latency_count == 0 {
            return 0;
        }
        let target = (p / 100.0 * self.latency_count as f64).ceil() as u64;
        let mut cumulative: u64 = 0;
        let boundaries = [0, 1, 2, 5, 10, 25, 50, 100, 500, 1000, u64::MAX];
        for (i, &count) in self.latency_histogram.iter().enumerate() {
            cumulative += count;
            if cumulative >= target {
                return boundaries[i];
            }
        }
        self.latency_max
    }

    /// Merge another summary into this one (for multi-file replay).
    pub fn merge(&mut self, other: &ReplaySummary) {
        self.total_events += other.total_events;
        self.market_events += other.market_events;
        self.order_events += other.order_events;
        self.accepted += other.accepted;
        self.rejected += other.rejected;
        self.parse_errors += other.parse_errors;
        self.wall_clock_secs += other.wall_clock_secs;
        for (rule, count) in &other.reject_by_rule {
            *self.reject_by_rule.entry(rule.clone()).or_insert(0) += count;
        }

        // Merge latency
        if other.latency_count > 0 {
            if self.latency_count == 0 {
                self.latency_min = other.latency_min;
                self.latency_max = other.latency_max;
            } else {
                self.latency_min = self.latency_min.min(other.latency_min);
                self.latency_max = self.latency_max.max(other.latency_max);
            }
        }
        self.latency_sum += other.latency_sum;
        self.latency_count += other.latency_count;
        for (i, &c) in other.latency_histogram.iter().enumerate() {
            self.latency_histogram[i] += c;
        }
    }
}

impl std::fmt::Display for ReplaySummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f)?;
        writeln!(f, "=== Replay Summary ===")?;
        writeln!(
            f,
            "Total events:   {:<14} (processed in {:.2}s)",
            format_num(self.total_events),
            self.wall_clock_secs
        )?;
        writeln!(f, "Market events:  {}", format_num(self.market_events))?;
        let accept_pct = if self.order_events > 0 {
            self.accepted as f64 / self.order_events as f64 * 100.0
        } else {
            0.0
        };
        let reject_pct = 100.0 - accept_pct;
        writeln!(f, "Order events:   {}", format_num(self.order_events))?;
        writeln!(
            f,
            "  Accepted:     {:<14} ({:.1}%)",
            format_num(self.accepted),
            accept_pct
        )?;
        writeln!(
            f,
            "  Rejected:     {:<14} ({:.1}%)",
            format_num(self.rejected),
            reject_pct
        )?;

        // Throughput
        if self.wall_clock_secs > 0.0 {
            writeln!(f)?;
            writeln!(
                f,
                "Throughput:     {} events/sec",
                format_num((self.total_events as f64 / self.wall_clock_secs) as usize)
            )?;
            if self.order_events > 0 {
                writeln!(
                    f,
                    "Risk checks:    {} orders/sec",
                    format_num((self.order_events as f64 / self.wall_clock_secs) as usize)
                )?;
            }
        }

        // Latency
        if self.latency_count > 0 {
            writeln!(f)?;
            writeln!(f, "Latency (per risk check):")?;
            writeln!(
                f,
                "  min: {} us   p50: {} us   p99: {} us   max: {} us   avg: {:.2} us",
                self.latency_min,
                self.latency_percentile(50.0),
                self.latency_percentile(99.0),
                self.latency_max,
                self.latency_avg(),
            )?;
        }

        // Rejections by rule
        if !self.reject_by_rule.is_empty() {
            writeln!(f)?;
            writeln!(f, "Rejections by rule:")?;
            let mut rules: Vec<_> = self.reject_by_rule.iter().collect();
            rules.sort_by(|a, b| b.1.cmp(a.1)); // descending by count
            for (rule, count) in rules {
                writeln!(f, "  {rule:<20} {}", format_num(*count))?;
            }
        }

        if self.parse_errors > 0 {
            writeln!(f)?;
            writeln!(f, "Parse errors:   {}", self.parse_errors)?;
        }

        Ok(())
    }
}

fn format_num(n: usize) -> String {
    if n >= 1_000_000 {
        format!(
            "{},{:03},{:03}",
            n / 1_000_000,
            (n / 1_000) % 1_000,
            n % 1_000
        )
    } else if n >= 1_000 {
        format!("{},{:03}", n / 1_000, n % 1_000)
    } else {
        format!("{n}")
    }
}

/// Parse a JSONL file into a vector of SimEvents (for small files / tests).
pub fn parse_events(path: &Path) -> anyhow::Result<(Vec<SimEvent>, usize)> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    let mut errors = 0;

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        match serde_json::from_str::<SimEvent>(trimmed) {
            Ok(event) => events.push(event),
            Err(e) => {
                eprintln!("[replay] parse error: {e} on line: {trimmed}");
                errors += 1;
            }
        }
    }

    Ok((events, errors))
}

/// Replay a single JSONL file through the risk engine (streaming, memory-efficient).
pub fn replay_from(path: &str, risk_config: Option<RiskConfig>) -> anyhow::Result<ReplaySummary> {
    let file_path = Path::new(path);

    // If it's a directory, delegate to replay_from_dir
    if file_path.is_dir() {
        return replay_from_dir(path, risk_config);
    }

    if !file_path.exists() {
        anyhow::bail!("replay input not found: {path}");
    }

    let config = risk_config.unwrap_or_default();
    let mut engine = RiskEngine::new(config);
    let mut book = OrderBook::new(10);

    let summary = replay_file_streaming(file_path, &mut engine, &mut book, false)?;

    println!("{summary}");
    Ok(summary)
}

/// Replay all JSONL files in a directory (sorted), sharing engine state across days.
pub fn replay_from_dir(
    dir: &str,
    risk_config: Option<RiskConfig>,
) -> anyhow::Result<ReplaySummary> {
    let dir_path = Path::new(dir);
    if !dir_path.is_dir() {
        anyhow::bail!("replay directory not found: {dir}");
    }

    let mut files: Vec<_> = std::fs::read_dir(dir_path)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    files.sort();

    if files.is_empty() {
        anyhow::bail!("no .jsonl files found in {dir}");
    }

    println!("[replay] processing {} files from {dir}", files.len());

    let config = risk_config.unwrap_or_default();
    let mut engine = RiskEngine::new(config);
    let mut book = OrderBook::new(10);
    let mut aggregate = ReplaySummary::default();

    for (i, file) in files.iter().enumerate() {
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        let day_summary = replay_file_streaming(file, &mut engine, &mut book, false)?;
        println!(
            "[replay] {name}: {} events ({} orders, {} accepted, {} rejected) in {:.2}s",
            format_num(day_summary.total_events),
            format_num(day_summary.order_events),
            format_num(day_summary.accepted),
            format_num(day_summary.rejected),
            day_summary.wall_clock_secs
        );
        aggregate.merge(&day_summary);

        // Reset credit exposure between days (realistic: overnight reset)
        if i < files.len() - 1 {
            engine.reset_exposure();
        }
    }

    println!("{aggregate}");
    Ok(aggregate)
}

/// Stream a single file through the risk engine without loading it all into memory.
fn replay_file_streaming(
    path: &Path,
    engine: &mut RiskEngine,
    book: &mut OrderBook,
    keep_decisions: bool,
) -> anyhow::Result<ReplaySummary> {
    let file = File::open(path)?;
    let reader = BufReader::with_capacity(256 * 1024, file);
    let mut summary = ReplaySummary::default();
    if keep_decisions {
        summary.decisions = Some(Vec::new());
    }

    let start = Instant::now();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let event = match serde_json::from_str::<SimEvent>(trimmed) {
            Ok(e) => e,
            Err(_) => {
                summary.parse_errors += 1;
                continue;
            }
        };

        summary.total_events += 1;

        match event {
            SimEvent::Market(ref m) => {
                book.apply(m);
                summary.market_events += 1;
            }
            SimEvent::Order(ref o) => {
                summary.order_events += 1;
                let decision = engine.evaluate(o, book);

                // Latency tracking
                summary.record_latency(decision.latency_us);

                if decision.accepted {
                    summary.accepted += 1;
                } else {
                    summary.rejected += 1;
                    if let Some(ref rule) = decision.rule_hit {
                        *summary.reject_by_rule.entry(rule.clone()).or_insert(0) += 1;
                    }
                }

                if let Some(ref mut decisions) = summary.decisions {
                    decisions.push(decision);
                }
            }
        }
    }

    summary.wall_clock_secs = start.elapsed().as_secs_f64();
    Ok(summary)
}

/// Compare replay outcomes to an expected baseline file.
pub fn compare_to_baseline(
    summary: &ReplaySummary,
    baseline_path: &str,
) -> anyhow::Result<(usize, usize)> {
    let decisions = summary.decisions.as_ref().ok_or_else(|| {
        anyhow::anyhow!("baseline comparison requires --keep-decisions (decisions not stored)")
    })?;

    let file = File::open(baseline_path)?;
    let reader = BufReader::new(file);
    let mut expected: Vec<RiskDecision> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        expected.push(serde_json::from_str(trimmed)?);
    }

    let mut matches = 0;
    let mut mismatches = 0;

    for (i, (actual, exp)) in decisions.iter().zip(expected.iter()).enumerate() {
        if actual.accepted == exp.accepted && actual.rule_hit == exp.rule_hit {
            matches += 1;
        } else {
            mismatches += 1;
            println!(
                "[replay] mismatch at decision {i}: got accepted={} rule={:?}, expected accepted={} rule={:?}",
                actual.accepted, actual.rule_hit, exp.accepted, exp.rule_hit
            );
        }
    }

    let len_diff = decisions.len().abs_diff(expected.len());
    if len_diff > 0 {
        println!(
            "[replay] decision count mismatch: got {}, expected {}",
            decisions.len(),
            expected.len()
        );
        mismatches += len_diff;
    }

    println!("[replay] baseline comparison: {matches} matches, {mismatches} mismatches");
    Ok((matches, mismatches))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{Action, MarketEvent, OrderRequest, Side, SimEvent};
    use std::io::Write;

    fn write_temp_jsonl(events: &[SimEvent]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for event in events {
            let line = serde_json::to_string(event).unwrap();
            writeln!(f, "{line}").unwrap();
        }
        f.flush().unwrap();
        f
    }

    fn sample_events() -> Vec<SimEvent> {
        vec![
            SimEvent::Market(MarketEvent {
                timestamp_us: 1000,
                symbol: "AAPL".to_string(),
                side: Side::Bid,
                price: 149.0,
                qty: 100,
                action: Action::Add,
            }),
            SimEvent::Market(MarketEvent {
                timestamp_us: 2000,
                symbol: "AAPL".to_string(),
                side: Side::Ask,
                price: 151.0,
                qty: 100,
                action: Action::Add,
            }),
            SimEvent::Order(OrderRequest {
                id: 1,
                timestamp_us: 3000,
                symbol: "AAPL".to_string(),
                side: Side::Bid,
                price: 150.0,
                qty: 10,
                trader_id: "T1".to_string(),
            }),
            SimEvent::Order(OrderRequest {
                id: 2,
                timestamp_us: 4000,
                symbol: "AAPL".to_string(),
                side: Side::Ask,
                price: 200.0, // way outside collar
                qty: 10,
                trader_id: "T2".to_string(),
            }),
        ]
    }

    #[test]
    fn test_parse_events() {
        let events = sample_events();
        let f = write_temp_jsonl(&events);
        let (parsed, errors) = parse_events(f.path()).unwrap();
        assert_eq!(errors, 0);
        assert_eq!(parsed.len(), 4);
    }

    #[test]
    fn test_replay_produces_summary() {
        let events = sample_events();
        let f = write_temp_jsonl(&events);
        let summary = replay_from(f.path().to_str().unwrap(), Some(RiskConfig::default())).unwrap();
        assert_eq!(summary.total_events, 4);
        assert_eq!(summary.market_events, 2);
        assert_eq!(summary.order_events, 2);
        assert_eq!(summary.accepted, 1);
        assert_eq!(summary.rejected, 1);
        assert!(summary.reject_by_rule.contains_key("price_collar"));
        // Latency should be tracked
        assert_eq!(summary.latency_count, 2);
        assert!(summary.wall_clock_secs > 0.0);
    }

    #[test]
    fn test_deterministic_replay() {
        let events = sample_events();
        let f = write_temp_jsonl(&events);
        let path = f.path().to_str().unwrap();

        let s1 = replay_from(path, Some(RiskConfig::default())).unwrap();
        let s2 = replay_from(path, Some(RiskConfig::default())).unwrap();

        assert_eq!(s1.accepted, s2.accepted);
        assert_eq!(s1.rejected, s2.rejected);
        assert_eq!(s1.latency_count, s2.latency_count);
    }

    #[test]
    fn test_replay_missing_file() {
        let result = replay_from("/nonexistent/path.jsonl", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_format_num() {
        assert_eq!(format_num(0), "0");
        assert_eq!(format_num(999), "999");
        assert_eq!(format_num(1_000), "1,000");
        assert_eq!(format_num(15_640_095), "15,640,095");
    }
}
