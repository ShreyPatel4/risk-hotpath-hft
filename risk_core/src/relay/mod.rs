use crate::envelope::SimEvent;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Relay source: where events come from.
pub enum RelaySource {
    /// Single JSONL file
    SingleFile(PathBuf),
    /// Directory of day_*.jsonl files (sorted by name)
    Directory(PathBuf),
}

/// Relay mode: how events are delivered.
#[derive(Debug, Clone, Copy)]
pub enum RelayMode {
    /// Stream events respecting inter-event timing, multiplied by speed factor
    Stream { speed: f64 },
    /// Deliver all events as fast as possible in chunks
    Batch { chunk_size: usize },
}

/// A relay that reads events from files and delivers them.
pub struct Relay {
    source: RelaySource,
    mode: RelayMode,
}

impl Relay {
    pub fn new(source: RelaySource, mode: RelayMode) -> Self {
        Self { source, mode }
    }

    /// Resolve the list of files to read, in order.
    fn resolve_files(&self) -> anyhow::Result<Vec<PathBuf>> {
        match &self.source {
            RelaySource::SingleFile(p) => {
                if !p.exists() {
                    anyhow::bail!("relay source file not found: {}", p.display());
                }
                Ok(vec![p.clone()])
            }
            RelaySource::Directory(dir) => {
                if !dir.is_dir() {
                    anyhow::bail!("relay source directory not found: {}", dir.display());
                }
                let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
                    .collect();
                files.sort();
                if files.is_empty() {
                    anyhow::bail!("no .jsonl files found in {}", dir.display());
                }
                Ok(files)
            }
        }
    }

    /// Create a streaming iterator over events from all source files.
    pub fn open(&self) -> anyhow::Result<RelayReader> {
        let files = self.resolve_files()?;
        let total_files = files.len();
        tracing::info!(
            "relay opened {} file(s) in {:?} mode",
            total_files,
            self.mode
        );
        Ok(RelayReader {
            files,
            current_file_idx: 0,
            current_reader: None,
            mode: self.mode,
            events_read: 0,
            parse_errors: 0,
            last_timestamp_us: 0,
        })
    }
}

/// Streaming reader that yields events from relay source files.
pub struct RelayReader {
    files: Vec<PathBuf>,
    current_file_idx: usize,
    current_reader: Option<BufReader<File>>,
    mode: RelayMode,
    pub events_read: u64,
    pub parse_errors: u64,
    last_timestamp_us: u64,
}

impl RelayReader {
    /// Read the next event. Returns None when all files are exhausted.
    pub fn next_event(&mut self) -> anyhow::Result<Option<(SimEvent, u64)>> {
        loop {
            // Ensure we have an open reader
            if self.current_reader.is_none() {
                if self.current_file_idx >= self.files.len() {
                    return Ok(None);
                }
                let path = &self.files[self.current_file_idx];
                tracing::debug!("relay opening file: {}", path.display());
                let file = File::open(path)?;
                self.current_reader = Some(BufReader::with_capacity(256 * 1024, file));
            }

            let reader = self.current_reader.as_mut().unwrap();
            let mut line = String::new();
            let bytes = reader.read_line(&mut line)?;

            if bytes == 0 {
                // EOF on current file, move to next
                self.current_reader = None;
                self.current_file_idx += 1;
                continue;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            match serde_json::from_str::<SimEvent>(trimmed) {
                Ok(event) => {
                    self.events_read += 1;

                    // Extract timestamp for delay calculation
                    let ts = match &event {
                        SimEvent::Market(m) => m.timestamp_us,
                        SimEvent::Order(o) => o.timestamp_us,
                    };

                    // Compute inter-event delay in microseconds
                    let delay_us = if self.last_timestamp_us == 0 {
                        0
                    } else {
                        ts.saturating_sub(self.last_timestamp_us)
                    };
                    self.last_timestamp_us = ts;

                    // Adjust delay based on mode
                    let adjusted_delay_us = match self.mode {
                        RelayMode::Stream { speed } => {
                            if speed <= 0.0 {
                                0
                            } else {
                                (delay_us as f64 / speed) as u64
                            }
                        }
                        RelayMode::Batch { .. } => 0,
                    };

                    return Ok(Some((event, adjusted_delay_us)));
                }
                Err(e) => {
                    self.parse_errors += 1;
                    if self.parse_errors <= 10 {
                        tracing::warn!("relay parse error: {e}");
                    }
                    continue;
                }
            }
        }
    }

    /// Read a batch of events (for batch mode).
    pub fn next_batch(&mut self) -> anyhow::Result<Vec<SimEvent>> {
        let chunk_size = match self.mode {
            RelayMode::Batch { chunk_size } => chunk_size,
            RelayMode::Stream { .. } => 1000,
        };

        let mut batch = Vec::with_capacity(chunk_size);
        for _ in 0..chunk_size {
            match self.next_event()? {
                Some((event, _)) => batch.push(event),
                None => break,
            }
        }
        Ok(batch)
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.current_file_idx, self.files.len())
    }
}

/// List available data files in a directory.
pub fn list_data_files(dir: &Path) -> Vec<String> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{Action, MarketEvent, OrderRequest, Side};
    use std::io::Write;

    #[test]
    fn test_relay_single_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let events = vec![
            SimEvent::Market(MarketEvent {
                timestamp_us: 1000,
                symbol: "AAPL".to_string(),
                side: Side::Bid,
                price: 185.0,
                qty: 100,
                action: Action::Add,
            }),
            SimEvent::Order(OrderRequest {
                id: 1,
                timestamp_us: 2000,
                symbol: "AAPL".to_string(),
                side: Side::Bid,
                price: 185.0,
                qty: 10,
                trader_id: "T1".to_string(),
            }),
        ];
        for e in &events {
            writeln!(f, "{}", serde_json::to_string(e).unwrap()).unwrap();
        }
        f.flush().unwrap();

        let relay = Relay::new(
            RelaySource::SingleFile(f.path().to_path_buf()),
            RelayMode::Batch { chunk_size: 100 },
        );
        let mut reader = relay.open().unwrap();
        let batch = reader.next_batch().unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(reader.events_read, 2);
    }

    #[test]
    fn test_relay_stream_delay() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            f,
            "{}",
            serde_json::to_string(&SimEvent::Market(MarketEvent {
                timestamp_us: 1_000_000,
                symbol: "AAPL".to_string(),
                side: Side::Bid,
                price: 185.0,
                qty: 100,
                action: Action::Add,
            }))
            .unwrap()
        )
        .unwrap();
        writeln!(
            f,
            "{}",
            serde_json::to_string(&SimEvent::Market(MarketEvent {
                timestamp_us: 2_000_000,
                symbol: "AAPL".to_string(),
                side: Side::Ask,
                price: 186.0,
                qty: 50,
                action: Action::Add,
            }))
            .unwrap()
        )
        .unwrap();
        f.flush().unwrap();

        let relay = Relay::new(
            RelaySource::SingleFile(f.path().to_path_buf()),
            RelayMode::Stream { speed: 10.0 }, // 10x speed
        );
        let mut reader = relay.open().unwrap();

        let (_, delay1) = reader.next_event().unwrap().unwrap();
        assert_eq!(delay1, 0); // first event has no delay

        let (_, delay2) = reader.next_event().unwrap().unwrap();
        assert_eq!(delay2, 100_000); // 1_000_000us / 10x = 100_000us
    }
}
