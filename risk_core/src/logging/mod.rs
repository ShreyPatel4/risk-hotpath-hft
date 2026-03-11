use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing_subscriber::fmt::MakeWriter;

const MAX_LOG_ENTRIES: usize = 5000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

/// Thread-safe ring buffer for captured log entries.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    inner: Arc<RwLock<VecDeque<LogEntry>>>,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_LOG_ENTRIES))),
        }
    }

    pub fn inner(&self) -> &Arc<RwLock<VecDeque<LogEntry>>> {
        &self.inner
    }

    pub async fn push(&self, entry: LogEntry) {
        let mut buf = self.inner.write().await;
        if buf.len() >= MAX_LOG_ENTRIES {
            buf.pop_front();
        }
        buf.push_back(entry);
    }

    pub async fn recent(&self, limit: usize) -> Vec<LogEntry> {
        let buf = self.inner.read().await;
        buf.iter().rev().take(limit).cloned().collect()
    }
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// A tracing writer that captures output into the LogBuffer.
/// Used with tracing-subscriber to intercept log lines.
#[derive(Clone)]
pub struct LogBufferWriter {
    buffer: LogBuffer,
    broadcast: Option<tokio::sync::broadcast::Sender<String>>,
}

impl LogBufferWriter {
    pub fn new(
        buffer: LogBuffer,
        broadcast: Option<tokio::sync::broadcast::Sender<String>>,
    ) -> Self {
        Self { buffer, broadcast }
    }
}

/// We implement MakeWriter so tracing-subscriber can write to our buffer.
/// Each write produces a LogEntry parsed from the formatted output.
impl<'a> MakeWriter<'a> for LogBufferWriter {
    type Writer = LogLineWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogLineWriter {
            buffer: self.buffer.clone(),
            broadcast: self.broadcast.clone(),
            buf: Vec::new(),
        }
    }
}

pub struct LogLineWriter {
    buffer: LogBuffer,
    broadcast: Option<tokio::sync::broadcast::Sender<String>>,
    buf: Vec<u8>,
}

impl std::io::Write for LogLineWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let line = String::from_utf8_lossy(&self.buf).to_string();
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            self.buf.clear();
            return Ok(());
        }

        // Parse level from the tracing format: "  LEVEL target: message"
        let (level, target, message) = parse_log_line(&trimmed);

        let entry = LogEntry {
            timestamp: now_iso(),
            level: level.to_string(),
            target: target.to_string(),
            message: message.to_string(),
        };

        // Non-blocking push: spawn a task
        let buffer = self.buffer.clone();
        let broadcast = self.broadcast.clone();
        let entry_clone = entry.clone();
        tokio::spawn(async move {
            buffer.push(entry_clone.clone()).await;
            if let Some(tx) = broadcast {
                let msg = serde_json::json!({
                    "type": "log",
                    "data": entry_clone
                });
                let _ = tx.send(msg.to_string());
            }
        });

        // Also write to stderr for console visibility
        eprintln!("{trimmed}");

        self.buf.clear();
        Ok(())
    }
}

impl Drop for LogLineWriter {
    fn drop(&mut self) {
        let _ = std::io::Write::flush(self);
    }
}

fn parse_log_line(line: &str) -> (&str, &str, &str) {
    // Try to match tracing compact format: "  LEVEL target: message"
    let trimmed = line.trim();
    for level in &["ERROR", "WARN", "INFO", "DEBUG", "TRACE"] {
        if let Some(rest) = trimmed.strip_prefix(level) {
            let rest = rest.trim();
            if let Some((target, message)) = rest.split_once(": ") {
                return (level, target.trim(), message.trim());
            }
            return (level, "", rest);
        }
    }
    ("INFO", "", trimmed)
}

fn now_iso() -> String {
    // Simple UTC timestamp without chrono dependency
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    let days = secs / 86400;
    // Approximate date (good enough for log display)
    let years = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;
    format!("{years:04}-{month:02}-{day:02}T{hours:02}:{mins:02}:{s:02}Z")
}

/// Initialize tracing with the LogBuffer writer.
/// Returns the LogBuffer so it can be shared with the web server.
pub fn init_tracing(broadcast: Option<tokio::sync::broadcast::Sender<String>>) -> LogBuffer {
    let buffer = LogBuffer::new();
    let writer = LogBufferWriter::new(buffer.clone(), broadcast);

    use tracing_subscriber::fmt;
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,risk_core=debug"));

    fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .compact()
        .init();

    buffer
}
