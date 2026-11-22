use anyhow::{anyhow, Context, Result};
use chrono::{TimeZone, Utc};
use serde::Deserialize;

use crate::models::market::{AssetClass, Candle, Timeframe};

/// IDX adapter (MVP): uses Yahoo Finance compatible chart endpoint for .JK tickers and benchmarks (^JKSE, USDIDR=X).
#[derive(Debug, Clone)]
pub struct IdxAdapter {
    base_url: String,
    http: reqwest::Client,
}

impl IdxAdapter {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            http,
        })
    }

    pub async fn fetch_candles(
        &self,
        ticker: &str,
        timeframe: Timeframe,
        limit: usize,
    ) -> Result<Vec<Candle>> {
        let symbol = normalize_idx_symbol(ticker);
        let interval = map_timeframe(timeframe)?;
        let range = map_range(timeframe, limit);
        let url = format!(
            "{}/v8/finance/chart/{}?interval={}&range={}",
            self.base_url, symbol, interval, range
        );
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        let body: YahooChartResponse = resp.json().await.context("invalid yahoo chart response")?;
        if !status.is_success() {
            return Err(anyhow!("yahoo chart HTTP {status}"));
        }
        let result = body.chart.result.unwrap_or_default();
        let Some(r) = result.into_iter().next() else {
            return Ok(vec![]);
        };
        let ts = r.timestamp.unwrap_or_default();
        let quote = r.indicators.quote.into_iter().next().unwrap_or_default();
        let mut candles = Vec::new();
        for (i, ts_val) in ts.iter().enumerate() {
            let Some(open) = quote.open.get(i).and_then(|v| *v) else {
                continue;
            };
            let Some(high) = quote.high.get(i).and_then(|v| *v) else {
                continue;
            };
            let Some(low) = quote.low.get(i).and_then(|v| *v) else {
                continue;
            };
            let Some(close) = quote.close.get(i).and_then(|v| *v) else {
                continue;
            };
            let volume = quote.volume.get(i).and_then(|v| *v).unwrap_or(0.0);
            let open_time = Utc
                .timestamp_opt(*ts_val, 0)
                .single()
                .context("invalid ts")?;
            candles.push(Candle {
                asset_class: AssetClass::IdxStock,
                ticker: symbol.clone(),
                timeframe,
                open_time,
                open: decimal_from_f64(open)?,
                high: decimal_from_f64(high)?,
                low: decimal_from_f64(low)?,
                close: decimal_from_f64(close)?,
                volume: decimal_from_f64(volume)?,
            });
        }
        if candles.len() > limit {
            candles = candles.split_off(candles.len() - limit);
        }
        Ok(candles)
    }
}

fn normalize_idx_symbol(ticker: &str) -> String {
    let t = ticker.trim().to_uppercase();
    if t == "IHSG" {
        "^JKSE".to_owned()
    } else {
        t
    }
}

fn map_timeframe(tf: Timeframe) -> Result<&'static str> {
    Ok(match tf {
        Timeframe::M1 => "1m",
        Timeframe::M5 => "5m",
        Timeframe::M15 => "15m",
        Timeframe::H1 => "60m",
        Timeframe::H4 => "60m", // yahoo has no 4h; caller can resample later
        Timeframe::D1 => "1d",
    })
}

fn map_range(tf: Timeframe, limit: usize) -> String {
    match tf {
        Timeframe::M1 | Timeframe::M5 | Timeframe::M15 => "5d".to_owned(),
        Timeframe::H1 | Timeframe::H4 => {
            if limit > 300 {
                "6mo".to_owned()
            } else {
                "1mo".to_owned()
            }
        }
        Timeframe::D1 => "1y".to_owned(),
    }
}

fn decimal_from_f64(v: f64) -> Result<rust_decimal::Decimal> {
    rust_decimal::Decimal::from_f64_retain(v)
        .ok_or_else(|| anyhow!("invalid decimal from f64: {v}"))
}

#[derive(Debug, Deserialize)]
struct YahooChartResponse {
    chart: YahooChart,
}
#[derive(Debug, Deserialize)]
struct YahooChart {
    result: Option<Vec<YahooResult>>,
}
#[derive(Debug, Deserialize)]
struct YahooResult {
    timestamp: Option<Vec<i64>>,
    indicators: YahooIndicators,
}
#[derive(Debug, Deserialize)]
struct YahooIndicators {
    quote: Vec<YahooQuote>,
}
#[derive(Debug, Default, Deserialize)]
struct YahooQuote {
    open: Vec<Option<f64>>,
    high: Vec<Option<f64>>,
    low: Vec<Option<f64>>,
    close: Vec<Option<f64>>,
    volume: Vec<Option<f64>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetches_idx_chart() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v8/finance/chart/BBCA.JK"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "chart":{"result":[{"timestamp":[1704067200],"indicators":{"quote":[{"open":[9000.0],"high":[9100.0],"low":[8900.0],"close":[9050.0],"volume":[1000000.0]}]}}]}
            })))
            .mount(&server)
            .await;
        let adapter = IdxAdapter::new(server.uri()).unwrap();
        let candles = adapter
            .fetch_candles("BBCA.JK", Timeframe::D1, 10)
            .await
            .unwrap();
        assert_eq!(candles.len(), 1);
        assert_eq!(candles[0].asset_class, AssetClass::IdxStock);
    }
}
