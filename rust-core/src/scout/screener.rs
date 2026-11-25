#![allow(dead_code)]

use std::path::Path;

use anyhow::Result;
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::adapters::{bea_calendar::BeaCalendar, finnhub::FinnhubAdapter};
use crate::engine::event_risk::evaluate_with_feeds;
use crate::models::market::AssetClass;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoutCandidate {
    pub asset_class: AssetClass,
    pub ticker: String,
    pub timeframe: String,
    pub correlation_group: String,
    pub last_close: Decimal,
    pub ret_pct: f64,
    pub avg_volume: f64,
    pub atr_pct: f64,
    pub event_risk_blocked: bool,
    pub event_risk_reasons: Vec<String>,
}

pub fn universe_for(asset_class: AssetClass) -> Vec<&'static str> {
    match asset_class {
        AssetClass::Crypto => vec![
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT", "AVAXUSDT",
            "DOGEUSDT", "LINKUSDT", "DOTUSDT",
        ],
        AssetClass::UsStock => vec![
            "NVDA", "AAPL", "MSFT", "AMZN", "META", "TSLA", "AMD", "GOOGL", "NFLX", "QQQ",
        ],
        AssetClass::Forex => vec![
            "EURUSD", "GBPUSD", "USDJPY", "AUDUSD", "USDCHF", "USDCAD", "NZDUSD", "EURJPY",
        ],
        AssetClass::IdxStock => vec![
            "BBCA.JK", "BBRI.JK", "BMRI.JK", "TLKM.JK", "ASII.JK", "BBNI.JK", "GOTO.JK", "ANTM.JK",
        ],
    }
}

pub fn correlation_group(asset_class: AssetClass, ticker: &str) -> String {
    match asset_class {
        AssetClass::Crypto => if ticker.starts_with("BTC") || ticker.starts_with("ETH") {
            "crypto_major"
        } else {
            "crypto_alt"
        }
        .into(),
        AssetClass::UsStock => "us_equity".into(),
        AssetClass::Forex => "fx_usd".into(),
        AssetClass::IdxStock => "idx_equity".into(),
    }
}

pub async fn screen_top_candidates(
    db: &PgPool,
    finnhub: &FinnhubAdapter,
    bea: &BeaCalendar,
    idx_snapshot: &Path,
    asset_class: AssetClass,
    limit: i64,
) -> Result<Vec<ScoutCandidate>> {
    let tickers = universe_for(asset_class);
    let mut out = Vec::new();
    let since = Utc::now() - Duration::days(14);

    for ticker in tickers {
        let rows = sqlx::query_as::<_, CandleRow>(
            r#"SELECT open_time, open, high, low, close, volume
               FROM candles
               WHERE asset_class=$1 AND ticker=$2 AND timeframe='1h' AND open_time >= $3
               ORDER BY open_time ASC
               LIMIT 300"#,
        )
        .bind(asset_class.as_str())
        .bind(ticker)
        .bind(since)
        .fetch_all(db)
        .await?;

        if rows.len() < 24 {
            continue;
        }
        let first = rows.first().unwrap();
        let last = rows.last().unwrap();
        let first_close = first.close.to_string().parse::<f64>().unwrap_or(0.0);
        let last_close = last.close.to_string().parse::<f64>().unwrap_or(0.0);
        if first_close <= 0.0 || last_close <= 0.0 {
            continue;
        }
        let ret_pct = ((last_close - first_close) / first_close) * 100.0;
        let avg_volume = rows
            .iter()
            .map(|r| r.volume.to_string().parse::<f64>().unwrap_or(0.0))
            .sum::<f64>()
            / rows.len() as f64;
        let atr_pct = avg_true_range_pct(&rows);
        if avg_volume <= 0.0 && !matches!(asset_class, AssetClass::Forex) {
            continue;
        }
        if atr_pct < 0.05 {
            continue;
        }

        let event = evaluate_with_feeds(finnhub, bea, idx_snapshot, asset_class, ticker).await;
        out.push(ScoutCandidate {
            asset_class,
            ticker: ticker.to_string(),
            timeframe: "1h".into(),
            correlation_group: correlation_group(asset_class, ticker),
            last_close: last.close,
            ret_pct,
            avg_volume,
            atr_pct,
            event_risk_blocked: event.blocked_entry,
            event_risk_reasons: event.reasons,
        });
    }

    out.sort_by(|a, b| {
        b.ret_pct
            .abs()
            .partial_cmp(&a.ret_pct.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(limit as usize);
    Ok(out)
}

fn avg_true_range_pct(rows: &[CandleRow]) -> f64 {
    if rows.len() < 2 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut n = 0.0;
    for i in 1..rows.len() {
        let h = rows[i].high.to_string().parse::<f64>().unwrap_or(0.0);
        let l = rows[i].low.to_string().parse::<f64>().unwrap_or(0.0);
        let pc = rows[i - 1].close.to_string().parse::<f64>().unwrap_or(0.0);
        if pc <= 0.0 {
            continue;
        }
        let tr = (h - l).abs().max((h - pc).abs()).max((l - pc).abs());
        sum += (tr / pc) * 100.0;
        n += 1.0;
    }
    if n == 0.0 {
        0.0
    } else {
        sum / n
    }
}

#[derive(Debug, sqlx::FromRow)]
struct CandleRow {
    #[allow(dead_code)]
    open_time: chrono::DateTime<Utc>,
    #[allow(dead_code)]
    open: Decimal,
    high: Decimal,
    low: Decimal,
    close: Decimal,
    volume: Decimal,
}
