use clap::{Parser, Subcommand};
use risk_core::envelope::SimEvent;
use risk_core::feed::orderbook::OrderBook;
use risk_core::feed::simulator::{SimConfig, Simulator};
use risk_core::replay::runner;
use risk_core::risk::engine::{RiskConfig, RiskEngine};
use risk_core::store::clickhouse;
use risk_core::telemetry::metrics;
use risk_core::web;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(
    name = "risk-core",
    version,
    about = "Real-time risk & telemetry engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the simulator with live risk checks and metrics
    RunSim {
        #[arg(long, default_value = "100.0", env = "SIM_RATE")]
        rate: f64,
        #[arg(long, default_value = "42", env = "SIM_SEED")]
        seed: u64,
        #[arg(long, default_value = "false", env = "SIM_BURST")]
        burst: bool,
        #[arg(long, default_value = "10000", env = "RISK_MAX_QTY")]
        max_qty: u32,
        #[arg(long, default_value = "5000000.0", env = "RISK_MAX_NOTIONAL")]
        max_notional: f64,
        #[arg(long, default_value = "1000000000.0", env = "RISK_CREDIT_LIMIT")]
        credit_limit: f64,
        #[arg(long, default_value = "http://localhost:8123", env = "CLICKHOUSE_URL")]
        clickhouse_url: String,
    },
    /// Start the Prometheus metrics exporter only
    ExportMetrics,
    /// Replay events from a JSONL file through the risk engine
    Replay {
        #[arg(long)]
        input: String,
        #[arg(long)]
        baseline: Option<String>,
    },
    /// Write sample replay data to a file
    WriteSampleReplay {
        #[arg(long, default_value = "data/sample_events.jsonl")]
        output: String,
        #[arg(long, default_value = "500")]
        count: usize,
        #[arg(long, default_value = "42")]
        seed: u64,
    },
    /// Start the web GUI with interactive simulator and dashboard
    Gui {
        /// Web server port
        #[arg(long, default_value = "8080", env = "GUI_PORT")]
        port: u16,
        /// Simulator events per second
        #[arg(long, default_value = "200.0")]
        rate: f64,
        /// RNG seed
        #[arg(long, default_value = "42")]
        seed: u64,
        /// Enable burst mode
        #[arg(long, default_value = "false")]
        burst: bool,
        /// Max order qty for risk
        #[arg(long, default_value = "10000")]
        max_qty: u32,
        /// Max notional
        #[arg(long, default_value = "5000000.0")]
        max_notional: f64,
        /// Credit limit per trader
        #[arg(long, default_value = "10000000.0")]
        credit_limit: f64,
    },
    /// Generate a realistic synthetic dataset (120 symbols, 15 days, 150M+ events)
    GenerateDataset {
        /// Output directory
        #[arg(long, default_value = "data/generated")]
        output_dir: String,
        /// Number of trading days
        #[arg(long, default_value = "15")]
        days: u32,
        /// Target number of events (0 = auto based on realistic HFT volume)
        #[arg(long, default_value = "0")]
        target_events: usize,
        /// RNG seed
        #[arg(long, default_value = "42")]
        seed: u64,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::RunSim {
            rate,
            seed,
            burst,
            max_qty,
            max_notional,
            credit_limit,
            clickhouse_url,
        } => {
            run_sim(
                rate,
                seed,
                burst,
                max_qty,
                max_notional,
                credit_limit,
                &clickhouse_url,
            )
            .await?;
        }
        Commands::ExportMetrics => {
            export_metrics().await?;
        }
        Commands::Replay { input, baseline } => {
            let summary = runner::replay_from(&input, None)?;
            if let Some(baseline_path) = baseline {
                runner::compare_to_baseline(&summary, &baseline_path)?;
            }
        }
        Commands::WriteSampleReplay {
            output,
            count,
            seed,
        } => {
            write_sample_replay(&output, count, seed)?;
        }
        Commands::Gui {
            port,
            rate,
            seed,
            burst,
            max_qty,
            max_notional,
            credit_limit,
        } => {
            run_gui(port, rate, seed, burst, max_qty, max_notional, credit_limit).await?;
        }
        Commands::GenerateDataset {
            output_dir,
            days,
            target_events,
            seed,
        } => {
            generate_dataset(&output_dir, days, target_events, seed)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// run-sim (headless)
// ---------------------------------------------------------------------------
async fn run_sim(
    rate: f64,
    seed: u64,
    burst: bool,
    max_qty: u32,
    max_notional: f64,
    credit_limit: f64,
    clickhouse_url: &str,
) -> anyhow::Result<()> {
    println!("=== risk-core simulator ===");
    println!("rate={rate} seed={seed} burst={burst}");
    println!("risk: max_qty={max_qty} max_notional={max_notional} credit_limit={credit_limit}");

    metrics::init();

    let ch = clickhouse::new_writer(clickhouse_url);
    ch.create_tables().await.unwrap_or_else(|e| {
        eprintln!("[clickhouse] table creation failed (non-fatal): {e}");
    });

    let sim_config = SimConfig {
        rate,
        seed,
        burst_enabled: burst,
        ..Default::default()
    };
    let risk_config = RiskConfig {
        max_order_qty: max_qty,
        max_notional,
        credit_limit_per_trader: credit_limit,
        ..Default::default()
    };

    let mut sim = Simulator::new(sim_config);
    let mut book = OrderBook::new(10);
    let mut engine = RiskEngine::new(risk_config);

    let mut total: u64 = 0;
    let mut accepted: u64 = 0;
    let mut rejected: u64 = 0;

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    println!("[sim] running... press Ctrl+C to stop\n");

    loop {
        let delay = sim.next_delay_ms();
        tokio::select! {
            _ = &mut ctrl_c => {
                println!("\n[sim] shutting down...");
                break;
            }
            _ = tokio::time::sleep(Duration::from_millis(delay)) => {
                let event = sim.next_event();
                total += 1;

                match &event {
                    SimEvent::Market(m) => {
                        book.apply(m);
                        metrics::record_market_event();
                        let _ = ch.write_market_event(m).await;
                    }
                    SimEvent::Order(o) => {
                        let decision = engine.evaluate(o, &book);
                        metrics::record_risk_decision(&decision);
                        if decision.accepted {
                            accepted += 1;
                        } else {
                            rejected += 1;
                        }
                        let _ = ch.write_risk_event(&decision).await;
                    }
                }

                if total.is_multiple_of(500) {
                    let symbols = book.symbols();
                    println!(
                        "[sim] events={total} orders_accepted={accepted} orders_rejected={rejected} symbols={}",
                        symbols.len()
                    );
                    for sym in &symbols {
                        if let Some(sb) = book.get(sym) {
                            println!(
                                "  {sym}: bid={:?} ask={:?} spread={:?}",
                                sb.best_bid(),
                                sb.best_ask(),
                                sb.spread()
                            );
                        }
                    }
                }
            }
        }
    }

    println!("\n=== Final Summary ===");
    println!("Total events:     {total}");
    println!("Orders accepted:  {accepted}");
    println!("Orders rejected:  {rejected}");
    println!("Metrics available at http://localhost:9898/metrics");

    Ok(())
}

// ---------------------------------------------------------------------------
// gui mode (web dashboard + live simulator)
// ---------------------------------------------------------------------------
async fn run_gui(
    port: u16,
    rate: f64,
    seed: u64,
    burst: bool,
    max_qty: u32,
    max_notional: f64,
    credit_limit: f64,
) -> anyhow::Result<()> {
    // Initialize metrics exporter (Prometheus on :9898)
    metrics::init();

    // Shared application state for web server
    let state = web::new_app_state();

    // Start the web server in a background task
    let web_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = web::start_server(web_state, port).await {
            eprintln!("[web] server error: {e}");
        }
    });

    println!("=== risk-core GUI mode ===");
    println!("Dashboard:  http://localhost:{port}");
    println!("Metrics:    http://localhost:9898/metrics");
    println!("Health:     http://localhost:9898/health");
    println!("Press Ctrl+C to stop\n");

    // Start the simulator in the main task
    let sim_config = SimConfig {
        rate,
        seed,
        burst_enabled: burst,
        ..Default::default()
    };
    let risk_config = RiskConfig {
        max_order_qty: max_qty,
        max_notional,
        credit_limit_per_trader: credit_limit,
        ..Default::default()
    };

    state
        .sim_running
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let mut sim = Simulator::new(sim_config);
    let mut engine = RiskEngine::new(risk_config);
    let mut latency_sum: f64 = 0.0;
    let mut latency_count: u64 = 0;
    let mut rate_window_start = Instant::now();
    let mut rate_window_events: u64 = 0;

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        let delay = sim.next_delay_ms();
        tokio::select! {
            _ = &mut ctrl_c => {
                println!("\n[gui] shutting down...");
                break;
            }
            _ = tokio::time::sleep(Duration::from_millis(delay)) => {
                let event = sim.next_event();
                rate_window_events += 1;

                // Compute events/sec over a rolling window
                let elapsed = rate_window_start.elapsed().as_secs_f64();
                let events_per_sec = if elapsed > 0.0 {
                    rate_window_events as f64 / elapsed
                } else {
                    0.0
                };
                if elapsed > 2.0 {
                    rate_window_start = Instant::now();
                    rate_window_events = 0;
                }

                match &event {
                    SimEvent::Market(m) => {
                        {
                            let mut book = state.book.write().await;
                            book.apply(m);
                        }
                        metrics::record_market_event();
                        let mut stats = state.stats.write().await;
                        stats.total_events += 1;
                        stats.events_per_sec = events_per_sec;
                    }
                    SimEvent::Order(o) => {
                        let decision = {
                            let book = state.book.read().await;
                            engine.evaluate(o, &book)
                        };
                        metrics::record_risk_decision(&decision);

                        latency_sum += decision.latency_us as f64;
                        latency_count += 1;

                        let mut stats = state.stats.write().await;
                        stats.total_events += 1;
                        stats.total_orders += 1;
                        stats.events_per_sec = events_per_sec;
                        stats.avg_latency_us = latency_sum / latency_count as f64;
                        if decision.accepted {
                            stats.accepted += 1;
                        } else {
                            stats.rejected += 1;
                            if let Some(ref rule) = decision.rule_hit {
                                *stats.reject_by_rule.entry(rule.clone()).or_insert(0) += 1;
                            }
                        }

                        // Broadcast decision to WebSocket clients
                        let msg = serde_json::json!({
                            "type": "decision",
                            "data": {
                                "order_id": decision.order_id,
                                "symbol": decision.symbol,
                                "side": format!("{}", o.side),
                                "price": o.price,
                                "qty": o.qty,
                                "accepted": decision.accepted,
                                "rule_hit": decision.rule_hit,
                                "reason": decision.reason,
                                "latency_us": decision.latency_us,
                            }
                        });
                        web::broadcast(&state, &msg.to_string());
                    }
                }

                // Broadcast stats every 100 events
                {
                    let stats = state.stats.read().await;
                    if stats.total_events.is_multiple_of(100) {
                        let msg = serde_json::json!({
                            "type": "stats",
                            "data": *stats
                        });
                        web::broadcast(&state, &msg.to_string());
                    }
                }
            }
        }
    }

    let stats = state.stats.read().await;
    println!("\n=== Final Summary ===");
    println!("Total events:     {}", stats.total_events);
    println!("Orders evaluated: {}", stats.total_orders);
    println!("Accepted:         {}", stats.accepted);
    println!("Rejected:         {}", stats.rejected);

    Ok(())
}

// ---------------------------------------------------------------------------
// export-metrics
// ---------------------------------------------------------------------------
async fn export_metrics() -> anyhow::Result<()> {
    metrics::init();
    println!("[export] metrics exporter running on http://0.0.0.0:9898/metrics");
    println!("[export] health check at http://0.0.0.0:9898/health");
    println!("[export] press Ctrl+C to stop");
    tokio::signal::ctrl_c().await?;
    println!("[export] shutting down");
    Ok(())
}

// ---------------------------------------------------------------------------
// write-sample-replay
// ---------------------------------------------------------------------------
fn write_sample_replay(output: &str, count: usize, seed: u64) -> anyhow::Result<()> {
    if let Some(parent) = std::path::Path::new(output).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let config = SimConfig {
        seed,
        rate: 1000.0,
        ..Default::default()
    };
    let mut sim = Simulator::new(config);

    let mut file = std::fs::File::create(output)?;
    use std::io::Write;
    for _ in 0..count {
        let event = sim.next_event();
        let line = serde_json::to_string(&event)?;
        writeln!(file, "{line}")?;
    }

    println!("[write-sample] wrote {count} events to {output}");
    Ok(())
}

// ---------------------------------------------------------------------------
// generate-dataset
// ---------------------------------------------------------------------------
fn generate_dataset(
    output_dir: &str,
    days: u32,
    target_events: usize,
    seed: u64,
) -> anyhow::Result<()> {
    use risk_core::datagen::{generate_dataset as gen, DataGenConfig};
    use std::path::PathBuf;

    // Auto-compute realistic volume: ~10M events/day for 120 symbols
    let target: u64 = if target_events == 0 {
        (days as u64) * 10_000_000 // ~10M/day → 150M for 15 days
    } else {
        target_events as u64
    };

    let config = DataGenConfig {
        output_dir: PathBuf::from(output_dir),
        num_days: days,
        target_events: target,
        seed,
    };

    println!("=== Generating realistic dataset ===");
    println!("  days={days}  target_events={target}  seed={seed}  output={output_dir}");
    println!("  This may take several minutes for large datasets...\n");

    let summary = gen(&config)?;

    println!("\n=== Generation Complete ===");
    println!("  Total events:  {}", summary.total_events);
    println!("  Market events: {}", summary.total_market_events);
    println!("  Order events:  {}", summary.total_order_events);
    println!("  Files written: {}", summary.files.len());
    println!("  Output: {output_dir}/");

    Ok(())
}
