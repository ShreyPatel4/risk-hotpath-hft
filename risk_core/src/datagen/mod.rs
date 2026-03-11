//! Hyper-realistic synthetic market data generator.
//!
//! Generates multi-day JSONL datasets with Geometric Brownian Motion price dynamics,
//! diurnal volume patterns, sector correlations, and anomaly injection for
//! risk engine evaluation.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::envelope::{Action, MarketEvent, OrderRequest, Side, SimEvent};

// ─── Configuration & Output Types ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataGenConfig {
    /// Directory to write JSONL + metadata into.
    pub output_dir: PathBuf,
    /// Number of trading days to generate.
    pub num_days: u32,
    /// Target total event count across all days.
    pub target_events: u64,
    /// RNG seed for deterministic reproduction.
    pub seed: u64,
}

impl Default for DataGenConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("data/generated"),
            num_days: 15,
            target_events: 40_000_000,
            seed: 42,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataGenSummary {
    pub total_events: u64,
    pub total_market_events: u64,
    pub total_order_events: u64,
    pub num_days: u32,
    pub num_symbols: usize,
    pub num_traders: usize,
    pub anomalies_injected: AnomalyCounts,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnomalyCounts {
    pub flash_crashes: u32,
    pub fat_finger_orders: u32,
    pub momentum_spikes: u32,
    pub liquidity_gaps: u32,
    pub correlated_sector_crashes: u32,
    pub red_herrings: u32,
}

// ─── Symbol / Sector Configuration ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolConfig {
    pub ticker: String,
    pub sector: String,
    pub base_price: f64,
    /// Annualised volatility (e.g. 0.30 = 30%).
    pub volatility: f64,
    /// Annualised drift.
    pub drift: f64,
    /// Average daily volume in shares.
    pub avg_daily_volume: u64,
}

/// Sector index for correlation matrix lookups.
const SECTORS: [&str; 10] = [
    "Technology",
    "Healthcare",
    "Financials",
    "Consumer Discretionary",
    "Consumer Staples",
    "Industrials",
    "Energy",
    "Materials",
    "Utilities",
    "Real Estate",
];

fn sector_index(sector: &str) -> usize {
    SECTORS.iter().position(|&s| s == sector).unwrap_or(0)
}

/// Returns the full 120-symbol universe: 12 symbols per sector with real tickers
/// and realistic base prices / volatilities.
pub fn default_symbols() -> Vec<SymbolConfig> {
    let mut syms = Vec::with_capacity(120);

    // Helper to push a batch for one sector.
    macro_rules! sector {
        ($sector:expr, [ $( ($tick:expr, $price:expr, $vol:expr, $adv:expr) ),* $(,)? ]) => {
            $(
                syms.push(SymbolConfig {
                    ticker: $tick.to_string(),
                    sector: $sector.to_string(),
                    base_price: $price,
                    volatility: $vol,
                    drift: 0.05,
                    avg_daily_volume: $adv,
                });
            )*
        };
    }

    sector!(
        "Technology",
        [
            ("AAPL", 178.0, 0.28, 55_000_000),
            ("MSFT", 370.0, 0.25, 22_000_000),
            ("NVDA", 480.0, 0.45, 40_000_000),
            ("GOOG", 140.0, 0.30, 25_000_000),
            ("META", 350.0, 0.38, 18_000_000),
            ("AVGO", 900.0, 0.32, 4_000_000),
            ("ADBE", 570.0, 0.33, 3_500_000),
            ("CRM", 260.0, 0.34, 5_000_000),
            ("AMD", 120.0, 0.42, 50_000_000),
            ("INTC", 44.0, 0.35, 30_000_000),
            ("ORCL", 115.0, 0.28, 8_000_000),
            ("CSCO", 50.0, 0.22, 18_000_000),
        ]
    );

    sector!(
        "Healthcare",
        [
            ("UNH", 520.0, 0.22, 3_500_000),
            ("JNJ", 155.0, 0.18, 7_000_000),
            ("LLY", 580.0, 0.32, 3_000_000),
            ("PFE", 28.0, 0.30, 25_000_000),
            ("ABBV", 155.0, 0.25, 5_000_000),
            ("MRK", 108.0, 0.23, 8_000_000),
            ("TMO", 530.0, 0.26, 1_500_000),
            ("ABT", 108.0, 0.22, 4_500_000),
            ("DHR", 250.0, 0.27, 2_500_000),
            ("BMY", 52.0, 0.28, 10_000_000),
            ("AMGN", 280.0, 0.24, 2_800_000),
            ("GILD", 78.0, 0.26, 6_000_000),
        ]
    );

    sector!(
        "Financials",
        [
            ("JPM", 170.0, 0.25, 10_000_000),
            ("BAC", 33.0, 0.28, 35_000_000),
            ("WFC", 45.0, 0.27, 18_000_000),
            ("GS", 380.0, 0.30, 2_500_000),
            ("MS", 88.0, 0.30, 8_000_000),
            ("C", 52.0, 0.28, 15_000_000),
            ("BLK", 750.0, 0.26, 800_000),
            ("SCHW", 68.0, 0.30, 7_000_000),
            ("AXP", 185.0, 0.25, 3_000_000),
            ("USB", 38.0, 0.27, 6_500_000),
            ("PNC", 150.0, 0.25, 2_000_000),
            ("TFC", 35.0, 0.28, 5_000_000),
        ]
    );

    sector!(
        "Consumer Discretionary",
        [
            ("AMZN", 155.0, 0.32, 48_000_000),
            ("TSLA", 240.0, 0.55, 95_000_000),
            ("HD", 340.0, 0.24, 4_000_000),
            ("MCD", 290.0, 0.20, 3_000_000),
            ("NKE", 105.0, 0.30, 6_000_000),
            ("SBUX", 95.0, 0.28, 6_500_000),
            ("LOW", 220.0, 0.26, 3_200_000),
            ("TJX", 95.0, 0.24, 4_500_000),
            ("BKNG", 3500.0, 0.30, 400_000),
            ("CMG", 2200.0, 0.32, 300_000),
            ("MAR", 220.0, 0.28, 1_500_000),
            ("ROST", 140.0, 0.26, 2_000_000),
        ]
    );

    sector!(
        "Consumer Staples",
        [
            ("PG", 155.0, 0.18, 6_000_000),
            ("KO", 58.0, 0.17, 12_000_000),
            ("PEP", 170.0, 0.18, 4_500_000),
            ("COST", 570.0, 0.22, 2_500_000),
            ("WMT", 165.0, 0.20, 7_000_000),
            ("PM", 95.0, 0.20, 4_000_000),
            ("MO", 42.0, 0.22, 7_500_000),
            ("MDLZ", 72.0, 0.19, 5_000_000),
            ("CL", 78.0, 0.18, 3_500_000),
            ("KMB", 125.0, 0.19, 1_800_000),
            ("GIS", 66.0, 0.18, 3_000_000),
            ("SYY", 75.0, 0.22, 2_500_000),
        ]
    );

    sector!(
        "Industrials",
        [
            ("CAT", 280.0, 0.28, 3_500_000),
            ("GE", 120.0, 0.30, 6_000_000),
            ("HON", 200.0, 0.24, 3_000_000),
            ("UPS", 155.0, 0.26, 3_200_000),
            ("BA", 210.0, 0.35, 5_000_000),
            ("RTX", 88.0, 0.24, 4_500_000),
            ("DE", 380.0, 0.28, 1_800_000),
            ("LMT", 450.0, 0.22, 1_200_000),
            ("MMM", 100.0, 0.26, 3_000_000),
            ("FDX", 260.0, 0.28, 2_000_000),
            ("GD", 260.0, 0.22, 1_000_000),
            ("NSC", 240.0, 0.24, 1_200_000),
        ]
    );

    sector!(
        "Energy",
        [
            ("XOM", 105.0, 0.28, 15_000_000),
            ("CVX", 150.0, 0.27, 8_000_000),
            ("COP", 115.0, 0.32, 6_000_000),
            ("SLB", 50.0, 0.35, 10_000_000),
            ("EOG", 120.0, 0.33, 3_500_000),
            ("MPC", 150.0, 0.30, 3_000_000),
            ("PSX", 120.0, 0.28, 2_500_000),
            ("VLO", 135.0, 0.30, 3_000_000),
            ("PXD", 230.0, 0.32, 2_000_000),
            ("OXY", 60.0, 0.38, 12_000_000),
            ("HAL", 38.0, 0.35, 6_000_000),
            ("DVN", 45.0, 0.36, 8_000_000),
        ]
    );

    sector!(
        "Materials",
        [
            ("LIN", 400.0, 0.22, 1_500_000),
            ("APD", 280.0, 0.24, 1_200_000),
            ("SHW", 320.0, 0.24, 1_000_000),
            ("FCX", 40.0, 0.38, 10_000_000),
            ("NEM", 42.0, 0.32, 6_000_000),
            ("NUE", 170.0, 0.32, 2_000_000),
            ("DOW", 55.0, 0.28, 4_000_000),
            ("DD", 72.0, 0.26, 2_500_000),
            ("ECL", 190.0, 0.22, 1_200_000),
            ("PPG", 140.0, 0.24, 1_500_000),
            ("VMC", 230.0, 0.26, 800_000),
            ("MLM", 480.0, 0.26, 500_000),
        ]
    );

    sector!(
        "Utilities",
        [
            ("NEE", 72.0, 0.20, 8_000_000),
            ("DUK", 98.0, 0.17, 3_000_000),
            ("SO", 72.0, 0.17, 4_000_000),
            ("D", 48.0, 0.19, 4_500_000),
            ("AEP", 85.0, 0.18, 2_500_000),
            ("SRE", 75.0, 0.19, 2_000_000),
            ("EXC", 38.0, 0.20, 5_000_000),
            ("XEL", 62.0, 0.18, 2_500_000),
            ("ED", 95.0, 0.16, 1_500_000),
            ("WEC", 88.0, 0.17, 1_200_000),
            ("ES", 65.0, 0.18, 2_000_000),
            ("AWK", 135.0, 0.20, 800_000),
        ]
    );

    sector!(
        "Real Estate",
        [
            ("PLD", 125.0, 0.26, 4_000_000),
            ("AMT", 210.0, 0.24, 2_000_000),
            ("CCI", 110.0, 0.26, 2_500_000),
            ("EQIX", 780.0, 0.24, 600_000),
            ("SPG", 140.0, 0.28, 2_500_000),
            ("PSA", 280.0, 0.22, 1_000_000),
            ("O", 55.0, 0.20, 5_000_000),
            ("WELL", 90.0, 0.24, 2_000_000),
            ("DLR", 130.0, 0.26, 1_500_000),
            ("AVB", 190.0, 0.24, 700_000),
            ("EQR", 65.0, 0.22, 1_500_000),
            ("VTR", 48.0, 0.26, 2_000_000),
        ]
    );

    assert_eq!(syms.len(), 120);
    syms
}

// ─── Trader Profiles ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraderProfile {
    pub id: String,
    pub kind: TraderKind,
    /// Fraction of orders that are orders vs. market events they trigger.
    pub order_rate: f64,
    /// Typical quantity multiplier relative to symbol average.
    pub qty_multiplier: f64,
    /// How far from mid-price the trader typically places orders (bps).
    pub spread_bps: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraderKind {
    MarketMaker,
    Institutional,
    Retail,
    Algo,
}

fn default_traders() -> Vec<TraderProfile> {
    let mut traders = Vec::with_capacity(50);

    // 10 market makers
    for i in 0..10 {
        traders.push(TraderProfile {
            id: format!("MM-{:03}", i),
            kind: TraderKind::MarketMaker,
            order_rate: 0.8,
            qty_multiplier: 2.0,
            spread_bps: 2.0,
        });
    }
    // 15 institutional
    for i in 0..15 {
        traders.push(TraderProfile {
            id: format!("INST-{:03}", i),
            kind: TraderKind::Institutional,
            order_rate: 0.3,
            qty_multiplier: 5.0,
            spread_bps: 5.0,
        });
    }
    // 15 retail
    for i in 0..15 {
        traders.push(TraderProfile {
            id: format!("RET-{:03}", i),
            kind: TraderKind::Retail,
            order_rate: 0.15,
            qty_multiplier: 0.3,
            spread_bps: 10.0,
        });
    }
    // 10 algo
    for i in 0..10 {
        traders.push(TraderProfile {
            id: format!("ALGO-{:03}", i),
            kind: TraderKind::Algo,
            order_rate: 0.6,
            qty_multiplier: 1.5,
            spread_bps: 1.0,
        });
    }

    assert_eq!(traders.len(), 50);
    traders
}

// ─── Diurnal Volume Profile ─────────────────────────────────────────────────

/// U-shaped intraday volume multiplier.
/// `frac` is fraction of trading day elapsed [0, 1].
fn diurnal_volume(frac: f64) -> f64 {
    1.0 + 2.5 * (-50.0 * frac * frac).exp() + 2.0 * (-50.0 * (1.0 - frac) * (1.0 - frac)).exp()
}

// ─── Poisson Inverse-Transform Sampler ──────────────────────────────────────

/// Samples from Poisson(lambda) using the inverse-transform method.
fn poisson_sample(rng: &mut StdRng, lambda: f64) -> u32 {
    if lambda <= 0.0 {
        return 0;
    }
    let l = (-lambda).exp();
    let mut k: u32 = 0;
    let mut p: f64 = 1.0;
    loop {
        k += 1;
        p *= rng.gen::<f64>();
        if p < l {
            return k - 1;
        }
        // Safety valve for very large lambda to avoid near-infinite loops.
        if k > (lambda * 10.0) as u32 + 1000 {
            return k;
        }
    }
}

// ─── Standard Normal via Box-Muller ─────────────────────────────────────────

fn standard_normal(rng: &mut StdRng) -> f64 {
    let u1: f64 = rng.gen::<f64>().max(1e-300);
    let u2: f64 = rng.gen::<f64>();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

// ─── Anomaly Scheduling ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct AnomalySchedule {
    flash_crashes: Vec<(u32, usize, u64)>, // (day, symbol_idx, timestamp_us)
    momentum_spikes: Vec<(u32, usize, u64)>, // 5 total
    correlated_crashes: Vec<(u32, usize, u64)>, // (day, sector_idx, timestamp_us) — 2
    fat_finger_days: Vec<(u32, usize, u64)>, // scattered
    liquidity_gap_days: Vec<(u32, usize, u64)>, // scattered
    red_herring_days: Vec<(u32, usize, u64)>, // scattered
}

fn build_anomaly_schedule(rng: &mut StdRng, num_days: u32, num_symbols: usize) -> AnomalySchedule {
    let market_open: u64 = 9 * 3600 + 30 * 60; // 9:30 in seconds from midnight
    let market_close: u64 = 16 * 3600; // 16:00

    let random_time = |rng: &mut StdRng, day: u32| -> u64 {
        let secs = rng.gen_range(market_open..market_close);
        let day_base = day as u64 * 24 * 3600 * 1_000_000;
        day_base + secs * 1_000_000
    };

    let random_day = |rng: &mut StdRng| -> u32 { rng.gen_range(0..num_days) };
    let random_sym = |rng: &mut StdRng| -> usize { rng.gen_range(0..num_symbols) };

    // 3 flash crashes
    let flash_crashes: Vec<_> = (0..3)
        .map(|_| {
            let d = random_day(rng);
            let s = random_sym(rng);
            let t = random_time(rng, d);
            (d, s, t)
        })
        .collect();

    // 5 momentum spikes
    let momentum_spikes: Vec<_> = (0..5)
        .map(|_| {
            let d = random_day(rng);
            let s = random_sym(rng);
            let t = random_time(rng, d);
            (d, s, t)
        })
        .collect();

    // 2 correlated sector crashes (sector_idx stored in .1)
    let correlated_crashes: Vec<_> = (0..2)
        .map(|_| {
            let d = random_day(rng);
            let sec = rng.gen_range(0..SECTORS.len());
            let t = random_time(rng, d);
            (d, sec, t)
        })
        .collect();

    // 8 fat finger orders
    let fat_finger_days: Vec<_> = (0..8)
        .map(|_| {
            let d = random_day(rng);
            let s = random_sym(rng);
            let t = random_time(rng, d);
            (d, s, t)
        })
        .collect();

    // 4 liquidity gaps
    let liquidity_gap_days: Vec<_> = (0..4)
        .map(|_| {
            let d = random_day(rng);
            let s = random_sym(rng);
            let t = random_time(rng, d);
            (d, s, t)
        })
        .collect();

    // 12 red herrings (large but within limits)
    let red_herring_days: Vec<_> = (0..12)
        .map(|_| {
            let d = random_day(rng);
            let s = random_sym(rng);
            let t = random_time(rng, d);
            (d, s, t)
        })
        .collect();

    AnomalySchedule {
        flash_crashes,
        momentum_spikes,
        correlated_crashes,
        fat_finger_days,
        liquidity_gap_days,
        red_herring_days,
    }
}

// ─── Core Generator ─────────────────────────────────────────────────────────

/// Generates a multi-day synthetic dataset.
///
/// Streams events to JSONL files on disk using `BufWriter` for memory efficiency.
pub fn generate_dataset(config: &DataGenConfig) -> Result<DataGenSummary> {
    let symbols = default_symbols();
    let traders = default_traders();
    let num_symbols = symbols.len();
    let num_traders = traders.len();

    fs::create_dir_all(&config.output_dir)
        .with_context(|| format!("creating output dir {:?}", config.output_dir))?;

    let mut rng = StdRng::seed_from_u64(config.seed);
    let schedule = build_anomaly_schedule(&mut rng, config.num_days, num_symbols);

    // Pre-compute per-symbol running prices (start from base).
    let mut prices: Vec<f64> = symbols.iter().map(|s| s.base_price).collect();

    // Events per day (roughly equal split).
    let events_per_day = config.target_events / config.num_days as u64;

    // Trading day constants (microseconds).
    let market_open_sec: u64 = 9 * 3600 + 30 * 60; // 09:30
    let market_close_sec: u64 = 16 * 3600; // 16:00
    let trading_seconds: u64 = market_close_sec - market_open_sec; // 23400

    // Compute per-symbol weight proportional to avg_daily_volume.
    let total_adv: u64 = symbols.iter().map(|s| s.avg_daily_volume).sum();
    let sym_weights: Vec<f64> = symbols
        .iter()
        .map(|s| s.avg_daily_volume as f64 / total_adv as f64)
        .collect();

    let mut total_events: u64 = 0;
    let mut total_market: u64 = 0;
    let mut total_orders: u64 = 0;
    let mut order_id_counter: u64 = 1;
    let mut files = Vec::new();

    let mut anomaly_counts = AnomalyCounts::default();

    for day in 0..config.num_days {
        let day_filename = format!("day_{:02}.jsonl", day + 1);
        let day_path = config.output_dir.join(&day_filename);
        let file =
            File::create(&day_path).with_context(|| format!("creating {}", day_path.display()))?;
        let mut writer = BufWriter::with_capacity(1 << 20, file); // 1 MiB buffer

        let day_base_us = day as u64 * 24 * 3600 * 1_000_000;

        // Pre-generate sector factors for this day (one per second of trading).
        let mut z_market_by_sec: Vec<f64> = Vec::with_capacity(trading_seconds as usize);
        let mut z_sector_by_sec: Vec<Vec<f64>> = Vec::with_capacity(trading_seconds as usize);

        for _ in 0..trading_seconds {
            z_market_by_sec.push(standard_normal(&mut rng));
            let mut sec_factors = Vec::with_capacity(SECTORS.len());
            for _ in 0..SECTORS.len() {
                sec_factors.push(standard_normal(&mut rng));
            }
            z_sector_by_sec.push(sec_factors);
        }

        // Collect anomalies active on this day.
        let day_flash: Vec<_> = schedule
            .flash_crashes
            .iter()
            .filter(|(d, _, _)| *d == day)
            .copied()
            .collect();
        let day_momentum: Vec<_> = schedule
            .momentum_spikes
            .iter()
            .filter(|(d, _, _)| *d == day)
            .copied()
            .collect();
        let day_corr_crash: Vec<_> = schedule
            .correlated_crashes
            .iter()
            .filter(|(d, _, _)| *d == day)
            .copied()
            .collect();
        let day_fat_finger: Vec<_> = schedule
            .fat_finger_days
            .iter()
            .filter(|(d, _, _)| *d == day)
            .copied()
            .collect();
        let day_liquidity: Vec<_> = schedule
            .liquidity_gap_days
            .iter()
            .filter(|(d, _, _)| *d == day)
            .copied()
            .collect();
        let day_red_herring: Vec<_> = schedule
            .red_herring_days
            .iter()
            .filter(|(d, _, _)| *d == day)
            .copied()
            .collect();

        let mut day_events: u64 = 0;

        // Iterate second by second.
        for sec_offset in 0..trading_seconds {
            let wall_sec = market_open_sec + sec_offset;
            let frac = sec_offset as f64 / trading_seconds as f64;
            let vol_mult = diurnal_volume(frac);
            let base_ts = day_base_us + wall_sec * 1_000_000;

            let z_mkt = z_market_by_sec[sec_offset as usize];

            // Per-symbol event generation.
            for (sym_idx, sym) in symbols.iter().enumerate() {
                let sec_idx = sector_index(&sym.sector);
                let z_sec = z_sector_by_sec[sec_offset as usize][sec_idx];
                let z_idio = standard_normal(&mut rng);

                // Composite return factor.
                let z = 0.3 * z_mkt + 0.3 * z_sec + 0.82_f64.sqrt() * z_idio;

                // GBM step: dt = 1/(252 * 23400) trading-year fraction per second.
                let dt = 1.0 / (252.0 * trading_seconds as f64);
                let drift_term = (sym.drift - 0.5 * sym.volatility * sym.volatility) * dt;
                let diffusion_term = sym.volatility * dt.sqrt() * z;
                prices[sym_idx] *= (drift_term + diffusion_term).exp();
                prices[sym_idx] = prices[sym_idx].max(0.01); // floor

                // Expected events this second for this symbol.
                let lambda = sym_weights[sym_idx] * events_per_day as f64 / trading_seconds as f64
                    * vol_mult;
                let n_events = poisson_sample(&mut rng, lambda);

                for ev_i in 0..n_events {
                    let micro_offset = (ev_i as u64 * 1_000_000) / (n_events as u64).max(1);
                    let ts = base_ts + micro_offset.min(999_999);

                    // Decide if this is a market event or order.
                    let trader = &traders[rng.gen_range(0..num_traders)];
                    let is_order = rng.gen::<f64>() < trader.order_rate * 0.15;

                    let side = if rng.gen::<bool>() {
                        Side::Bid
                    } else {
                        Side::Ask
                    };

                    let spread_frac = trader.spread_bps * 1e-4;
                    let price_jitter =
                        prices[sym_idx] * spread_frac * (rng.gen::<f64>() - 0.5) * 2.0;
                    let price = round_price(prices[sym_idx] + price_jitter);

                    let base_qty = (sym.avg_daily_volume as f64 / trading_seconds as f64
                        * vol_mult
                        * trader.qty_multiplier
                        * (0.5 + rng.gen::<f64>())) as u32;
                    let qty = base_qty.max(1);

                    // Check anomalies.
                    let in_window = |anomaly_ts: u64| -> bool {
                        ts >= anomaly_ts && ts < anomaly_ts + 5_000_000
                    };

                    // Flash crash: price drops 8-15% suddenly.
                    for &(_, s_idx, a_ts) in &day_flash {
                        if s_idx == sym_idx && in_window(a_ts) {
                            let crash_factor = 1.0 - rng.gen_range(0.08..0.15);
                            prices[sym_idx] *= crash_factor;
                            if is_order {
                                anomaly_counts.flash_crashes += 1;
                            }
                        }
                    }

                    // Momentum spike: price jumps 5-10% up.
                    for &(_, s_idx, a_ts) in &day_momentum {
                        if s_idx == sym_idx && in_window(a_ts) {
                            let spike = 1.0 + rng.gen_range(0.05..0.10);
                            prices[sym_idx] *= spike;
                            if is_order {
                                anomaly_counts.momentum_spikes += 1;
                            }
                        }
                    }

                    // Correlated sector crash: all symbols in sector drop 4-8%.
                    for &(_, sec_crash_idx, a_ts) in &day_corr_crash {
                        if sec_idx == sec_crash_idx && in_window(a_ts) {
                            let crash_factor = 1.0 - rng.gen_range(0.04..0.08);
                            prices[sym_idx] *= crash_factor;
                            if is_order {
                                anomaly_counts.correlated_sector_crashes += 1;
                            }
                        }
                    }

                    if is_order {
                        // Fat finger: absurd quantity.
                        let mut order_price = price;
                        let mut order_qty = qty;
                        let mut is_fat_finger = false;
                        let mut is_liquidity_gap = false;

                        for &(_, s_idx, a_ts) in &day_fat_finger {
                            if s_idx == sym_idx && in_window(a_ts) {
                                order_qty = qty * rng.gen_range(50..200);
                                is_fat_finger = true;
                                anomaly_counts.fat_finger_orders += 1;
                            }
                        }

                        // Liquidity gap: price far from mid.
                        for &(_, s_idx, a_ts) in &day_liquidity {
                            if s_idx == sym_idx && in_window(a_ts) {
                                let gap_mult = match side {
                                    Side::Bid => 1.0 - rng.gen_range(0.03..0.06),
                                    Side::Ask => 1.0 + rng.gen_range(0.03..0.06),
                                };
                                order_price = round_price(prices[sym_idx] * gap_mult);
                                is_liquidity_gap = true;
                                anomaly_counts.liquidity_gaps += 1;
                            }
                        }

                        // Red herring: large but within limits.
                        if !is_fat_finger && !is_liquidity_gap {
                            for &(_, s_idx, a_ts) in &day_red_herring {
                                if s_idx == sym_idx && in_window(a_ts) {
                                    order_qty = qty * rng.gen_range(8..20);
                                    anomaly_counts.red_herrings += 1;
                                }
                            }
                        }

                        let order = OrderRequest {
                            id: order_id_counter,
                            timestamp_us: ts,
                            symbol: sym.ticker.clone(),
                            side,
                            price: order_price,
                            qty: order_qty,
                            trader_id: trader.id.clone(),
                        };
                        order_id_counter += 1;

                        let event = SimEvent::Order(order);
                        serde_json::to_writer(&mut writer, &event)?;
                        writer.write_all(b"\n")?;
                        total_orders += 1;
                    } else {
                        let action = match rng.gen_range(0u8..10) {
                            0..=6 => Action::Add,
                            7..=8 => Action::Update,
                            _ => Action::Delete,
                        };

                        let market = MarketEvent {
                            timestamp_us: ts,
                            symbol: sym.ticker.clone(),
                            side,
                            price,
                            qty,
                            action,
                        };
                        let event = SimEvent::Market(market);
                        serde_json::to_writer(&mut writer, &event)?;
                        writer.write_all(b"\n")?;
                        total_market += 1;
                    }

                    day_events += 1;
                }
            }
        }

        writer.flush()?;
        total_events += day_events;
        files.push(day_path);
    }

    // Write metadata.
    write_metadata(config, &symbols, &traders, &schedule, &files, total_events)?;

    Ok(DataGenSummary {
        total_events,
        total_market_events: total_market,
        total_order_events: total_orders,
        num_days: config.num_days,
        num_symbols,
        num_traders,
        anomalies_injected: anomaly_counts,
        files,
    })
}

fn round_price(p: f64) -> f64 {
    (p * 100.0).round() / 100.0
}

fn write_metadata(
    config: &DataGenConfig,
    symbols: &[SymbolConfig],
    traders: &[TraderProfile],
    _schedule: &AnomalySchedule,
    files: &[PathBuf],
    total_events: u64,
) -> Result<()> {
    #[derive(Serialize)]
    struct Metadata<'a> {
        generation_params: GenParams<'a>,
        symbols: &'a [SymbolConfig],
        traders: &'a [TraderProfile],
        output_files: Vec<String>,
        total_events: u64,
    }

    #[derive(Serialize)]
    struct GenParams<'a> {
        seed: u64,
        num_days: u32,
        target_events: u64,
        output_dir: &'a Path,
    }

    let meta = Metadata {
        generation_params: GenParams {
            seed: config.seed,
            num_days: config.num_days,
            target_events: config.target_events,
            output_dir: &config.output_dir,
        },
        symbols,
        traders,
        output_files: files
            .iter()
            .map(|f| {
                f.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect(),
        total_events,
    };

    let meta_path = config.output_dir.join("metadata.json");
    let file = File::create(&meta_path)?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, &meta)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_default_symbols_count() {
        let syms = default_symbols();
        assert_eq!(syms.len(), 120);

        // Verify 12 per sector.
        let mut counts: HashMap<String, usize> = HashMap::new();
        for s in &syms {
            *counts.entry(s.sector.clone()).or_default() += 1;
        }
        for sector in &SECTORS {
            assert_eq!(
                counts.get(*sector).copied().unwrap_or(0),
                12,
                "sector {} should have 12 symbols",
                sector
            );
        }
    }

    #[test]
    fn test_default_traders_count() {
        let traders = default_traders();
        assert_eq!(traders.len(), 50);
    }

    #[test]
    fn test_diurnal_volume_u_shape() {
        let open = diurnal_volume(0.0);
        let mid = diurnal_volume(0.5);
        let close = diurnal_volume(1.0);
        assert!(
            open > mid,
            "open ({open}) should be higher than midday ({mid})"
        );
        assert!(
            close > mid,
            "close ({close}) should be higher than midday ({mid})"
        );
    }

    #[test]
    fn test_poisson_sample_deterministic() {
        let mut rng = StdRng::seed_from_u64(99);
        let a = poisson_sample(&mut rng, 5.0);
        let mut rng2 = StdRng::seed_from_u64(99);
        let b = poisson_sample(&mut rng2, 5.0);
        assert_eq!(a, b);
    }

    #[test]
    fn test_round_price() {
        assert_eq!(round_price(123.456), 123.46);
        assert_eq!(round_price(0.005), 0.01);
    }

    #[test]
    fn test_generate_small_dataset() {
        let dir = tempfile::tempdir().unwrap();
        let config = DataGenConfig {
            output_dir: dir.path().to_path_buf(),
            num_days: 2,
            target_events: 10_000,
            seed: 42,
        };
        let summary = generate_dataset(&config).unwrap();
        assert_eq!(summary.num_days, 2);
        assert_eq!(summary.num_symbols, 120);
        assert_eq!(summary.num_traders, 50);
        assert!(summary.total_events > 0);
        assert_eq!(
            summary.total_events,
            summary.total_market_events + summary.total_order_events
        );

        // Check files exist.
        assert!(dir.path().join("day_01.jsonl").exists());
        assert!(dir.path().join("day_02.jsonl").exists());
        assert!(dir.path().join("metadata.json").exists());
    }
}
