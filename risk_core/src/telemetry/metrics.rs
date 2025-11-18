use once_cell::sync::Lazy;
use prometheus::{Encoder, IntCounter, Opts, Registry, TextEncoder};
use std::io::Write;
use std::net::SocketAddr;
use std::thread;

static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);
static SIMULATION_COUNTER: Lazy<IntCounter> = Lazy::new(|| {
    let opts = Opts::new(
        "simulated_events",
        "Number of simulated envelopes processed",
    );
    IntCounter::with_opts(opts).expect("counter can be created")
});

pub fn init() {
    if REGISTRY
        .register(Box::new(SIMULATION_COUNTER.clone()))
        .is_ok()
    {
        start_exporter();
    }
}

pub fn record_simulation() {
    SIMULATION_COUNTER.inc();
}

fn start_exporter() {
    let addr: SocketAddr = "0.0.0.0:9898".parse().expect("valid metrics addr");
    thread::spawn(move || {
        if let Ok(listener) = std::net::TcpListener::bind(addr) {
            for stream in listener.incoming().flatten() {
                if let Err(err) = write_metrics(stream) {
                    eprintln!("metrics export error: {err}");
                }
            }
        }
    });
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
