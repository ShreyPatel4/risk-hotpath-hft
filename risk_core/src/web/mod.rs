use crate::feed::orderbook::OrderBook;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Path, State},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

const INDEX_HTML: &str = include_str!("../../static/index.html");

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LiveStats {
    pub total_events: u64,
    pub total_orders: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub avg_latency_us: f64,
    pub events_per_sec: f64,
    pub reject_by_rule: HashMap<String, u64>,
}

/// Confusion matrix counters for risk gate evaluation quality.
///
/// Ground truth is derived from order characteristics:
/// - "should accept" = price valid, qty within limits, notional within limits,
///   price within collar of mid-price (if available)
/// - "should reject" = violates any of the above
///
/// The risk gate may also reject for stateful reasons (credit exhaustion, duplicates)
/// which can cause "false negatives" — valid orders rejected due to accumulated state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfusionMatrix {
    pub tp: u64, // accepted, and should have been accepted
    pub fp: u64, // accepted, but should have been rejected
    pub r#fn: u64, // rejected, but should have been accepted
    pub tn: u64, // rejected, and should have been rejected
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

pub struct AppState {
    pub book: Arc<RwLock<OrderBook>>,
    pub stats: Arc<RwLock<LiveStats>>,
    pub confusion: Arc<RwLock<ConfusionMatrix>>,
    pub logs: Arc<RwLock<VecDeque<LogEntry>>>,
    pub ws_tx: broadcast::Sender<String>,
    pub sim_running: Arc<AtomicBool>,
    offload_enabled: AtomicBool,
}

// ---------------------------------------------------------------------------
// Constructor helpers
// ---------------------------------------------------------------------------

pub fn new_app_state() -> Arc<AppState> {
    let (ws_tx, _) = broadcast::channel::<String>(4096);
    Arc::new(AppState {
        book: Arc::new(RwLock::new(OrderBook::default())),
        stats: Arc::new(RwLock::new(LiveStats::default())),
        confusion: Arc::new(RwLock::new(ConfusionMatrix::default())),
        logs: Arc::new(RwLock::new(VecDeque::with_capacity(512))),
        ws_tx,
        sim_running: Arc::new(AtomicBool::new(false)),
        offload_enabled: AtomicBool::new(false),
    })
}

/// Broadcast a pre-serialised JSON string to all connected WebSocket clients.
pub fn broadcast(state: &AppState, msg: &str) {
    // Ignore errors – they just mean no receivers are listening.
    let _ = state.ws_tx.send(msg.to_owned());
}

// ---------------------------------------------------------------------------
// Server entry-point
// ---------------------------------------------------------------------------

pub async fn start_server(state: Arc<AppState>, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/stats", get(stats_handler))
        .route("/api/symbols", get(symbols_handler))
        .route("/api/book/{symbol}", get(book_handler))
        .route("/api/logs", get(logs_handler))
        .route("/api/files", get(files_handler))
        .route("/api/confusion", get(confusion_handler))
        .route("/api/offload", post(offload_handler))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("Web dashboard listening on http://0.0.0.0:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn stats_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let stats = state.stats.read().await;
    Json(stats.clone())
}

async fn symbols_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let book = state.book.read().await;
    let syms: Vec<String> = book.symbols().into_iter().cloned().collect();
    Json(syms)
}

#[derive(Serialize)]
struct BookLevel {
    price: f64,
    qty: u32,
}

#[derive(Serialize)]
struct BookSnapshot {
    bids: Vec<BookLevel>,
    asks: Vec<BookLevel>,
    best_bid: Option<f64>,
    best_ask: Option<f64>,
    spread: Option<f64>,
    mid_price: Option<f64>,
}

async fn book_handler(
    State(state): State<Arc<AppState>>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    let book = state.book.read().await;
    let snap = match book.get(&symbol) {
        Some(sb) => BookSnapshot {
            bids: sb
                .bids
                .depth()
                .iter()
                .map(|l| BookLevel {
                    price: l.price,
                    qty: l.qty,
                })
                .collect(),
            asks: sb
                .asks
                .depth()
                .iter()
                .map(|l| BookLevel {
                    price: l.price,
                    qty: l.qty,
                })
                .collect(),
            best_bid: sb.best_bid(),
            best_ask: sb.best_ask(),
            spread: sb.spread(),
            mid_price: sb.mid_price(),
        },
        None => BookSnapshot {
            bids: vec![],
            asks: vec![],
            best_bid: None,
            best_ask: None,
            spread: None,
            mid_price: None,
        },
    };
    Json(snap)
}

async fn logs_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let logs = state.logs.read().await;
    let entries: Vec<LogEntry> = logs.iter().rev().take(500).cloned().collect();
    Json(entries)
}

async fn files_handler() -> impl IntoResponse {
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir("data") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    files.push(name.to_string());
                }
            }
        }
    }
    files.sort();
    Json(files)
}

async fn confusion_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cm = state.confusion.read().await;
    Json(cm.clone())
}

#[derive(Serialize)]
struct OffloadResponse {
    enabled: bool,
}

async fn offload_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let prev = state.offload_enabled.fetch_xor(true, Ordering::SeqCst);
    let new_val = !prev;
    tracing::info!("Database offload toggled → {}", new_val);
    Json(OffloadResponse { enabled: new_val })
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.ws_tx.subscribe();
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(text) => {
                        if socket.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("WebSocket client lagged by {} messages", n);
                    }
                    Err(_) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(_)) => {} // ignore client messages
                    _ => break,       // client disconnected or error
                }
            }
        }
    }
}
