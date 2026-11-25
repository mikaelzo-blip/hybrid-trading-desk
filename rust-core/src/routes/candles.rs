use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::models::market::{AssetClass, Candle, Timeframe};
use crate::routes::AppState;

#[derive(Debug, Deserialize)]
pub struct CandlesQuery {
    pub asset_class: String,
    pub ticker: String,
    pub timeframe: String,
    pub limit: Option<usize>,
    pub start: Option<i64>,
    pub end: Option<i64>,
}

pub async fn get_candles(
    State(state): State<AppState>,
    Query(q): Query<CandlesQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let asset_class: AssetClass = q.asset_class.parse().map_err(bad_request)?;
    let timeframe: Timeframe = q.timeframe.parse().map_err(bad_request)?;
    let ticker = q.ticker.trim().to_uppercase();
    let limit = q.limit.unwrap_or(200).clamp(1, 1500);
    let start = parse_ms_opt(q.start).map_err(bad_request)?;
    let end = parse_ms_opt(q.end).map_err(bad_request)?;

    let candles = load_candles(&state, asset_class, &ticker, timeframe, start, end, limit)
        .await
        .map_err(internal_error)?;

    Ok(Json(json!({
        "asset_class": asset_class.as_str(),
        "ticker": ticker,
        "timeframe": timeframe.as_str(),
        "count": candles.len(),
        "candles": candles,
    })))
}

pub(super) async fn load_candles(
    state: &AppState,
    asset_class: AssetClass,
    ticker: &str,
    timeframe: Timeframe,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: usize,
) -> anyhow::Result<Vec<Candle>> {
    let crypto_window = (asset_class == AssetClass::Crypto && start.is_none() && end.is_none())
        .then(|| {
            let period = crypto_candle_period(timeframe);
            let now = Utc::now().timestamp();
            let closed_open = now - now.rem_euclid(period.num_seconds()) - period.num_seconds();
            (DateTime::from_timestamp(closed_open, 0).unwrap(), period)
        });
    let end = crypto_window.map(|(closed_open, _)| closed_open).or(end);
    let candles =
        fetch_candles_db(state, asset_class, ticker, timeframe, start, end, limit).await?;
    if candles.len() == limit
        && crypto_window
            .is_none_or(|(closed_open, period)| is_current_window(&candles, closed_open, period))
    {
        return Ok(candles);
    }

    let fetched =
        fetch_from_adapter(state, asset_class, ticker, timeframe, start, end, limit).await?;
    if let Some((closed_open, period)) = crypto_window {
        anyhow::ensure!(
            is_current_window(&fetched, closed_open, period),
            "Binance returned stale or incomplete candles for {ticker} {timeframe}"
        );
    }
    crate::adapters::binance::BinanceAdapter::persist_candles(&state.db, &fetched).await?;
    if crypto_window.is_some() {
        return Ok(fetched);
    }

    let candles =
        fetch_candles_db(state, asset_class, ticker, timeframe, start, end, limit).await?;
    Ok(if candles.is_empty() { fetched } else { candles })
}

fn crypto_candle_period(timeframe: Timeframe) -> TimeDelta {
    TimeDelta::minutes(match timeframe {
        Timeframe::M1 => 1,
        Timeframe::M5 => 5,
        Timeframe::M15 => 15,
        Timeframe::H1 => 60,
        Timeframe::H4 => 240,
        Timeframe::D1 => 1440,
    })
}

fn is_current_window(candles: &[Candle], closed_open: DateTime<Utc>, period: TimeDelta) -> bool {
    candles
        .last()
        .is_some_and(|last| last.open_time == closed_open)
        && candles
            .windows(2)
            .all(|pair| pair[1].open_time - pair[0].open_time == period)
}

async fn fetch_candles_db(
    state: &AppState,
    asset_class: AssetClass,
    ticker: &str,
    timeframe: Timeframe,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: usize,
) -> anyhow::Result<Vec<Candle>> {
    let mut sql = String::from(
        "SELECT asset_class, ticker, timeframe, open_time, open, high, low, close, volume \
         FROM candles WHERE asset_class = $1 AND ticker = $2 AND timeframe = $3",
    );
    let mut param_idx = 4;
    if start.is_some() {
        sql.push_str(&format!(" AND open_time >= ${param_idx}"));
        param_idx += 1;
    }
    if end.is_some() {
        sql.push_str(&format!(" AND open_time <= ${param_idx}"));
        param_idx += 1;
    }
    let order = if start.is_some() { "ASC" } else { "DESC" };
    sql.push_str(&format!(" ORDER BY open_time {order} LIMIT ${param_idx}"));

    let mut q = sqlx::query_as::<_, CandleRow>(&sql)
        .bind(asset_class.as_str())
        .bind(ticker)
        .bind(timeframe.as_str());
    if let Some(s) = start {
        q = q.bind(s);
    }
    if let Some(e) = end {
        q = q.bind(e);
    }
    q = q.bind(limit as i64);

    let mut rows = q.fetch_all(&state.db).await?;
    if start.is_none() {
        rows.reverse();
    }
    rows.into_iter().map(CandleRow::into_candle).collect()
}

#[derive(Debug, sqlx::FromRow)]
struct CandleRow {
    asset_class: String,
    ticker: String,
    timeframe: String,
    open_time: DateTime<Utc>,
    open: rust_decimal::Decimal,
    high: rust_decimal::Decimal,
    low: rust_decimal::Decimal,
    close: rust_decimal::Decimal,
    volume: rust_decimal::Decimal,
}

impl CandleRow {
    fn into_candle(self) -> anyhow::Result<Candle> {
        Ok(Candle {
            asset_class: self
                .asset_class
                .parse()
                .map_err(|e: String| anyhow::anyhow!(e))?,
            ticker: self.ticker,
            timeframe: self
                .timeframe
                .parse()
                .map_err(|e: String| anyhow::anyhow!(e))?,
            open_time: self.open_time,
            open: self.open,
            high: self.high,
            low: self.low,
            close: self.close,
            volume: self.volume,
        })
    }
}

fn parse_ms_opt(ms: Option<i64>) -> Result<Option<DateTime<Utc>>, String> {
    match ms {
        None => Ok(None),
        Some(ms) => chrono::Utc
            .timestamp_millis_opt(ms)
            .single()
            .map(Some)
            .ok_or_else(|| format!("invalid timestamp millis: {ms}")),
    }
}

async fn fetch_from_adapter(
    state: &AppState,
    asset_class: AssetClass,
    ticker: &str,
    timeframe: Timeframe,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: usize,
) -> anyhow::Result<Vec<Candle>> {
    match asset_class {
        AssetClass::Crypto => {
            state
                .binance
                .fetch_klines(ticker, timeframe, start, end, limit)
                .await
        }
        AssetClass::UsStock => state.alpaca.fetch_candles(ticker, timeframe, limit).await,
        AssetClass::Forex => state.forex.fetch_candles(ticker, timeframe, limit).await,
        AssetClass::IdxStock => state.idx.fetch_candles(ticker, timeframe, limit).await,
    }
}

fn bad_request(msg: String) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg })))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, Json<Value>) {
    tracing::error!(error = %err, "internal error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "internal server error" })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn latest_crypto_window_rejects_stale_candles_gaps_and_open_candle() {
        let closed_open = DateTime::from_timestamp(1_800, 0).unwrap();
        let period = crypto_candle_period(Timeframe::M15);
        let candle = |offset: i32| Candle {
            asset_class: AssetClass::Crypto,
            ticker: "BTCUSDT".into(),
            timeframe: Timeframe::M15,
            open_time: closed_open + period * offset,
            open: Decimal::ONE,
            high: Decimal::ONE,
            low: Decimal::ONE,
            close: Decimal::ONE,
            volume: Decimal::ONE,
        };

        assert!(is_current_window(
            &[candle(-1), candle(0)],
            closed_open,
            period
        ));
        for window in [
            [candle(-2), candle(-1)],
            [candle(-2), candle(0)],
            [candle(0), candle(1)],
        ] {
            assert!(!is_current_window(&window, closed_open, period));
        }
    }
}
