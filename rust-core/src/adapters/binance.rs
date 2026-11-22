use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use redis::AsyncCommands;
use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tracing::{debug, warn};

use crate::models::market::{AssetClass, Candle, Timeframe};

#[allow(dead_code)]
const DEFAULT_REST_BASE: &str = "https://data-api.binance.vision";
#[allow(dead_code)]
const DEFAULT_WS_BASE: &str = "wss://stream.binance.com:9443/ws";

#[derive(Debug, Clone)]
pub struct BinanceAdapter {
    rest_base_url: String,
    ws_base_url: String,
    #[allow(dead_code)]
    outbound_proxy_url: Option<String>,
    http: reqwest::Client,
}

impl BinanceAdapter {
    pub fn new(
        rest_base_url: impl Into<String>,
        ws_base_url: impl Into<String>,
        outbound_proxy_url: Option<String>,
    ) -> Result<Self> {
        let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(15));
        if let Some(proxy) = outbound_proxy_url
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let proxy = reqwest::Proxy::all(proxy).context("invalid OUTBOUND_PROXY_URL")?;
            builder = builder.proxy(proxy);
        }
        let http = builder.build().context("failed to build reqwest client")?;
        Ok(Self {
            rest_base_url: rest_base_url.into().trim_end_matches('/').to_owned(),
            ws_base_url: ws_base_url.into().trim_end_matches('/').to_owned(),
            outbound_proxy_url: outbound_proxy_url.filter(|p| !p.trim().is_empty()),
            http,
        })
    }

    #[allow(dead_code)]
    pub fn with_defaults() -> Result<Self> {
        Self::new(DEFAULT_REST_BASE, DEFAULT_WS_BASE, None)
    }

    #[allow(dead_code)]
    pub fn rest_base_url(&self) -> &str {
        &self.rest_base_url
    }

    pub fn ws_base_url(&self) -> &str {
        &self.ws_base_url
    }

    pub async fn fetch_klines(
        &self,
        ticker: &str,
        timeframe: Timeframe,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<Candle>> {
        let symbol = normalize_symbol(ticker);
        let interval = timeframe.as_str();
        let mut url = format!(
            "{}/api/v3/klines?symbol={}&interval={}&limit={}",
            self.rest_base_url,
            symbol,
            interval,
            limit.clamp(1, 1500)
        );
        if let Some(s) = start {
            url.push_str(&format!("&startTime={}", s.timestamp_millis()));
        }
        if let Some(e) = end {
            url.push_str(&format!("&endTime={}", e.timestamp_millis()));
        }

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("binance klines request failed: {symbol} {interval}"))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("binance klines HTTP {status}: {body}"));
        }
        let rows: Vec<Vec<serde_json::Value>> = resp
            .json()
            .await
            .context("failed to decode binance klines response")?;
        rows.into_iter()
            .map(|row| parse_kline_row(&symbol, timeframe, &row))
            .collect()
    }

    pub async fn persist_candles(pool: &PgPool, candles: &[Candle]) -> Result<()> {
        for c in candles {
            sqlx::query(
                r#"
                INSERT INTO candles (asset_class, ticker, timeframe, open_time, open, high, low, close, volume)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (asset_class, ticker, timeframe, open_time)
                DO UPDATE SET open = EXCLUDED.open, high = EXCLUDED.high, low = EXCLUDED.low,
                              close = EXCLUDED.close, volume = EXCLUDED.volume
                "#,
            )
            .bind(c.asset_class.as_str())
            .bind(&c.ticker)
            .bind(c.timeframe.as_str())
            .bind(c.open_time)
            .bind(c.open)
            .bind(c.high)
            .bind(c.low)
            .bind(c.close)
            .bind(c.volume)
            .execute(pool)
            .await
            .context("failed to upsert candle")?;
        }
        Ok(())
    }

    pub async fn cache_live_candle(
        redis: &redis::Client,
        candle: &Candle,
        closed: bool,
    ) -> Result<()> {
        let mut conn = redis
            .get_multiplexed_async_connection()
            .await
            .context("redis connect failed")?;
        let payload = serde_json::to_string(candle).context("serialize candle failed")?;
        let live_key = live_price_key(&candle.ticker);
        let active_key = active_candle_key(&candle.ticker, candle.timeframe);

        let _: () = conn
            .set(&live_key, candle.close.to_string())
            .await
            .context("redis set live price failed")?;
        let _: () = conn
            .set(&active_key, &payload)
            .await
            .context("redis set active candle failed")?;
        if closed {
            let closed_key = last_closed_key(&candle.ticker, candle.timeframe);
            let _: () = conn
                .set(&closed_key, &payload)
                .await
                .context("redis set last closed candle failed")?;
        }
        Ok(())
    }

    /// Connects to Binance kline WS and yields candles. Reconnects with exponential backoff.
    /// Intended for background tasks; caller controls lifetime via task cancellation.
    #[allow(dead_code)]
    pub async fn run_kline_stream<F>(
        &self,
        ticker: &str,
        timeframe: Timeframe,
        mut on_candle: F,
    ) -> Result<()>
    where
        F: FnMut(
                Candle,
                bool,
            )
                -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>
            + Send,
    {
        let symbol = normalize_symbol(ticker).to_lowercase();
        let interval = timeframe.as_str();
        let stream = format!("{symbol}@kline_{interval}");
        let ws_url = format!("{}/{}", self.ws_base_url, stream);
        let mut backoff = Backoff::new(Duration::from_millis(500), Duration::from_secs(30));

        loop {
            match self
                .connect_and_stream(&ws_url, ticker, timeframe, &mut on_candle)
                .await
            {
                Ok(()) => {
                    warn!(%ws_url, "binance ws stream ended; reconnecting");
                    backoff.reset();
                }
                Err(err) => {
                    warn!(%ws_url, error = %err, "binance ws stream error; reconnecting");
                }
            }
            let delay = backoff.next_delay();
            tokio::time::sleep(delay).await;
        }
    }

    #[allow(dead_code)]
    async fn connect_and_stream<F>(
        &self,
        ws_url: &str,
        ticker: &str,
        timeframe: Timeframe,
        on_candle: &mut F,
    ) -> Result<()>
    where
        F: FnMut(
                Candle,
                bool,
            )
                -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>
            + Send,
    {
        let mut request = ws_url.into_client_request().context("invalid ws url")?;
        // Proxy support for tungstenite is handled at connector level in production;
        // for Batch 1B we log if configured and rely on system/network proxy where applicable.
        if let Some(proxy) = &self.outbound_proxy_url {
            request.headers_mut().insert(
                "x-outbound-proxy-configured",
                HeaderValue::from_str("true").unwrap_or_else(|_| HeaderValue::from_static("true")),
            );
            debug!(%proxy, "OUTBOUND_PROXY_URL configured for binance WS");
        }

        let (mut ws, _) = tokio_tungstenite::connect_async(request)
            .await
            .with_context(|| format!("binance ws connect failed: {ws_url}"))?;

        while let Some(msg) = ws.next().await {
            let msg = msg.context("binance ws message error")?;
            if msg.is_ping() {
                ws.send(tokio_tungstenite::tungstenite::Message::Pong(
                    Vec::new().into(),
                ))
                .await
                .ok();
                continue;
            }
            if !msg.is_text() {
                continue;
            }
            let text = msg.into_text().context("binance ws non-utf8 text")?;
            match parse_ws_kline(&text, ticker, timeframe) {
                Ok(Some((candle, closed))) => on_candle(candle, closed).await?,
                Ok(None) => {}
                Err(err) => warn!(error = %err, "skipping malformed binance ws message"),
            }
        }
        Ok(())
    }
}

fn normalize_symbol(ticker: &str) -> String {
    ticker.trim().to_uppercase()
}

pub fn live_price_key(ticker: &str) -> String {
    format!("live_price:crypto:{}", normalize_symbol(ticker))
}

pub fn active_candle_key(ticker: &str, timeframe: Timeframe) -> String {
    format!(
        "active_candle:crypto:{}:{}",
        normalize_symbol(ticker),
        timeframe.as_str()
    )
}

pub fn last_closed_key(ticker: &str, timeframe: Timeframe) -> String {
    format!(
        "last_closed_candle:crypto:{}:{}",
        normalize_symbol(ticker),
        timeframe.as_str()
    )
}

fn parse_decimal(value: &serde_json::Value, field: &str) -> Result<Decimal> {
    match value {
        serde_json::Value::String(s) => s
            .parse::<Decimal>()
            .with_context(|| format!("invalid decimal for {field}: {s}")),
        serde_json::Value::Number(n) => n
            .to_string()
            .parse::<Decimal>()
            .with_context(|| format!("invalid numeric decimal for {field}: {n}")),
        other => Err(anyhow!("invalid type for {field}: {other}")),
    }
}

fn parse_ms(ms: i64) -> Result<DateTime<Utc>> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .ok_or_else(|| anyhow!("invalid timestamp millis: {ms}"))
}

fn parse_kline_row(
    symbol: &str,
    timeframe: Timeframe,
    row: &[serde_json::Value],
) -> Result<Candle> {
    if row.len() < 6 {
        return Err(anyhow!("kline row too short: {}", row.len()));
    }
    let open_time_ms = row[0]
        .as_i64()
        .ok_or_else(|| anyhow!("kline open_time missing"))?;
    let open = parse_decimal(&row[1], "open")?;
    let high = parse_decimal(&row[2], "high")?;
    let low = parse_decimal(&row[3], "low")?;
    let close = parse_decimal(&row[4], "close")?;
    let volume = parse_decimal(&row[5], "volume")?;
    Ok(Candle {
        asset_class: AssetClass::Crypto,
        ticker: normalize_symbol(symbol),
        timeframe,
        open_time: parse_ms(open_time_ms)?,
        open,
        high,
        low,
        close,
        volume,
    })
}

#[derive(Debug, Deserialize)]
struct WsEnvelope {
    #[serde(rename = "k")]
    kline: Option<WsKline>,
}

#[derive(Debug, Deserialize)]
struct WsKline {
    #[serde(rename = "t")]
    open_time: i64,
    #[serde(rename = "o")]
    open: String,
    #[serde(rename = "h")]
    high: String,
    #[serde(rename = "l")]
    low: String,
    #[serde(rename = "c")]
    close: String,
    #[serde(rename = "v")]
    volume: String,
    #[serde(rename = "x")]
    closed: bool,
}

pub fn parse_ws_kline(
    text: &str,
    ticker: &str,
    timeframe: Timeframe,
) -> Result<Option<(Candle, bool)>> {
    let envelope: WsEnvelope = serde_json::from_str(text).context("invalid ws json")?;
    let Some(k) = envelope.kline else {
        return Ok(None);
    };
    let candle = Candle {
        asset_class: AssetClass::Crypto,
        ticker: normalize_symbol(ticker),
        timeframe,
        open_time: parse_ms(k.open_time)?,
        open: k.open.parse().context("invalid ws open")?,
        high: k.high.parse().context("invalid ws high")?,
        low: k.low.parse().context("invalid ws low")?,
        close: k.close.parse().context("invalid ws close")?,
        volume: k.volume.parse().context("invalid ws volume")?,
    };
    Ok(Some((candle, k.closed)))
}

#[derive(Debug, Clone)]
pub struct Backoff {
    current: Duration,
    initial: Duration,
    max: Duration,
}

impl Backoff {
    pub fn new(initial: Duration, max: Duration) -> Self {
        Self {
            current: initial,
            initial,
            max,
        }
    }

    pub fn next_delay(&mut self) -> Duration {
        let delay = self.current;
        self.current = (self.current.saturating_mul(2)).min(self.max);
        delay
    }

    pub fn reset(&mut self) {
        self.current = self.initial;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn parses_binance_kline_row() {
        let row = vec![
            serde_json::json!(1735689600000_i64),
            serde_json::json!("42000.10"),
            serde_json::json!("42100.20"),
            serde_json::json!("41900.30"),
            serde_json::json!("42050.40"),
            serde_json::json!("123.456"),
        ];
        let candle = parse_kline_row("btcusdt", Timeframe::M15, &row).expect("parse");
        assert_eq!(candle.asset_class, AssetClass::Crypto);
        assert_eq!(candle.ticker, "BTCUSDT");
        assert_eq!(candle.timeframe, Timeframe::M15);
        assert_eq!(candle.open, dec!(42000.10));
        assert_eq!(candle.close, dec!(42050.40));
        assert_eq!(candle.volume, dec!(123.456));
    }

    #[test]
    fn parses_ws_kline_message() {
        let msg = r#"{"e":"kline","E":1735689600000,"s":"BTCUSDT","k":{"t":1735689600000,"o":"1.0","h":"2.0","l":"0.5","c":"1.5","v":"10.0","x":true}}"#;
        let (candle, closed) = parse_ws_kline(msg, "BTCUSDT", Timeframe::M1)
            .expect("parse")
            .expect("kline");
        assert!(closed);
        assert_eq!(candle.close, dec!(1.5));
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let mut b = Backoff::new(Duration::from_millis(100), Duration::from_millis(350));
        assert_eq!(b.next_delay(), Duration::from_millis(100));
        assert_eq!(b.next_delay(), Duration::from_millis(200));
        assert_eq!(b.next_delay(), Duration::from_millis(350));
        assert_eq!(b.next_delay(), Duration::from_millis(350));
        b.reset();
        assert_eq!(b.next_delay(), Duration::from_millis(100));
    }

    #[test]
    fn cache_keys_are_stable() {
        assert_eq!(live_price_key("btcusdt"), "live_price:crypto:BTCUSDT");
        assert_eq!(
            active_candle_key("BTCUSDT", Timeframe::M15),
            "active_candle:crypto:BTCUSDT:15m"
        );
    }

    #[tokio::test]
    async fn fetch_klines_uses_rest_and_parses_decimals() {
        let server = wiremock::MockServer::start().await;
        let body = serde_json::json!([[
            1735689600000_i64,
            "42000.10",
            "42100.20",
            "41900.30",
            "42050.40",
            "123.456",
            1735689699999_i64,
            "0",
            10,
            "0",
            "0",
            "0"
        ]]);
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/api/v3/klines"))
            .and(wiremock::matchers::query_param("symbol", "BTCUSDT"))
            .and(wiremock::matchers::query_param("interval", "15m"))
            .and(wiremock::matchers::query_param("limit", "1"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let adapter = BinanceAdapter::new(server.uri(), DEFAULT_WS_BASE, None).expect("adapter");
        let candles = adapter
            .fetch_klines("BTCUSDT", Timeframe::M15, None, None, 1)
            .await
            .expect("candles");
        assert_eq!(candles.len(), 1);
        assert_eq!(candles[0].open, dec!(42000.10));
        assert_eq!(candles[0].ticker, "BTCUSDT");
    }
}
