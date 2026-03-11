use crate::envelope::RiskDecision;
use once_cell::sync::Lazy;
use prometheus::{
    Encoder, Histogram, HistogramOpts, IntCounter, IntCounterVec, Opts, Registry, TextEncoder,
};
use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::thread;

static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

static MARKET_EVENTS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::with_opts(Opts::new(
        "risk_market_events_total",
        "Total market data events processed",
    ))
    .unwrap()
});

static ORDERS_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::with_opts(Opts::new(
        "risk_orders_total",
        "Total order requests evaluated",
    ))
    .unwrap()
});

static ACCEPT_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::with_opts(Opts::new(
        "risk_accept_total",
        "Total orders accepted by risk engine",
    ))
    .unwrap()
});

static REJECT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "risk_reject_total",
            "Total orders rejected by risk engine, by rule",
        ),
        &["rule"],
    )
    .unwrap()
});

static CHECK_LATENCY_US: Lazy<Histogram> = Lazy::new(|| {
    let buckets = vec![
        1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 5000.0,
    ];
    Histogram::with_opts(
        HistogramOpts::new(
            "risk_check_latency_us",
            "Risk check latency in microseconds",
        )
        .buckets(buckets),
    )
    .unwrap()
});

static BOOK_UPDATES_TOTAL: Lazy<IntCounter> = Lazy::new(|| {
    IntCounter::with_opts(Opts::new(
        "risk_book_updates_total",
        "Total order book updates applied",
    ))
    .unwrap()
});

pub fn init() {
    let collectors: Vec<Box<dyn prometheus::core::Collector>> = vec![
        Box::new(MARKET_EVENTS_TOTAL.clone()),
        Box::new(ORDERS_TOTAL.clone()),
        Box::new(ACCEPT_TOTAL.clone()),
        Box::new(REJECT_TOTAL.clone()),
        Box::new(CHECK_LATENCY_US.clone()),
        Box::new(BOOK_UPDATES_TOTAL.clone()),
    ];
    for c in collectors {
        let _ = REGISTRY.register(c);
    }
    start_exporter();
}

pub fn record_market_event() {
    MARKET_EVENTS_TOTAL.inc();
    BOOK_UPDATES_TOTAL.inc();
}

pub fn record_risk_decision(decision: &RiskDecision) {
    ORDERS_TOTAL.inc();
    CHECK_LATENCY_US.observe(decision.latency_us as f64);
    if decision.accepted {
        ACCEPT_TOTAL.inc();
    } else if let Some(ref rule) = decision.rule_hit {
        REJECT_TOTAL.with_label_values(&[rule]).inc();
    }
}

fn start_exporter() {
    let addr: SocketAddr = "0.0.0.0:9898".parse().expect("valid metrics addr");
    thread::spawn(move || {
        let listener = match std::net::TcpListener::bind(addr) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("failed to bind metrics exporter on {addr}: {e}");
                return;
            }
        };
        println!("[metrics] exporter listening on {addr}");
        for stream in listener.incoming().flatten() {
            if let Err(err) = handle_http(stream) {
                eprintln!("[metrics] error: {err}");
            }
        }
    });
}

fn handle_http(mut stream: std::net::TcpStream) -> std::io::Result<()> {
    let reader = BufReader::new(&stream);
    let mut path = String::new();
    if let Some(Ok(line)) = reader.lines().next() {
        // Parse "GET /path HTTP/1.1"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            path = parts[1].to_string();
        }
    }

    match path.as_str() {
        "/health" => {
            let body = r#"{"status":"ok"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes())
        }
        _ => write_metrics(stream),
    }
}

fn write_metrics(mut stream: std::net::TcpStream) -> std::io::Result<()> {
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();
    encoder.encode(&metric_families, &mut buffer).ok();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\n\r\n{}",
        buffer.len(),
        String::from_utf8_lossy(&buffer)
    );
    stream.write_all(response.as_bytes())
}
