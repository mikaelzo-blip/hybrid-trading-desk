use anyhow::{anyhow, Context, Result};
use chrono::{NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;

use crate::models::market::{AssetClass, Candle, Timeframe};

#[derive(Debug, Clone)]
pub struct ForexAdapter {
    base_url: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

impl ForexAdapter {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key,
            http,
        })
    }

    pub async fn fetch_candles(
        &self,
        ticker: &str,
        timeframe: Timeframe,
        limit: usize,
    ) -> Result<Vec<Candle>> {
        let symbol = ticker.trim().to_ascii_uppercase();
        let upstream_symbol = if symbol.len() == 6 && symbol.bytes().all(|c| c.is_ascii_uppercase())
        {
            let (base, quote) = symbol.split_at(3);
            format!("{base}/{quote}")
        } else {
            symbol.clone()
        };
        let interval = map_timeframe(timeframe)?;
        let outputsize = limit.clamp(1, 5000).to_string();
        let mut request = self
            .http
            .get(format!("{}/time_series", self.base_url))
            .query(&[
                ("symbol", upstream_symbol.as_str()),
                ("interval", interval),
                ("outputsize", outputsize.as_str()),
                ("format", "JSON"),
            ]);
        if let Some(key) = &self.api_key {
            request = request.query(&[("apikey", key)]);
        }
        let resp = request.send().await.map_err(reqwest::Error::without_url)?;
        let status = resp.status();
        let body: TwelveResponse = resp
            .json()
            .await
            .map_err(reqwest::Error::without_url)
            .context("invalid twelve data response")?;
        if !status.is_success() || body.status.as_deref() == Some("error") {
            return Err(anyhow!("twelve data error: {:?}", body.message));
        }
        let mut candles: Vec<Candle> = body
            .values
            .unwrap_or_default()
            .into_iter()
            .map(|v| v.into_candle(&symbol, timeframe))
            .collect::<Result<Vec<_>>>()?;
        candles.sort_by_key(|c| c.open_time);
        Ok(candles)
    }

    #[allow(dead_code)]
    pub fn volume_reliability(&self) -> &'static str {
        "low_tick_only"
    }
}

fn map_timeframe(tf: Timeframe) -> Result<&'static str> {
    Ok(match tf {
        Timeframe::M1 => "1min",
        Timeframe::M5 => "5min",
        Timeframe::M15 => "15min",
        Timeframe::H1 => "1h",
        Timeframe::H4 => "4h",
        Timeframe::D1 => "1day",
    })
}

#[derive(Debug, Deserialize)]
struct TwelveResponse {
    status: Option<String>,
    message: Option<String>,
    values: Option<Vec<TwelveValue>>,
}

#[derive(Debug, Deserialize)]
struct TwelveValue {
    datetime: String,
    open: String,
    high: String,
    low: String,
    close: String,
    #[serde(default)]
    volume: Option<String>,
}

impl TwelveValue {
    fn into_candle(self, ticker: &str, timeframe: Timeframe) -> Result<Candle> {
        let naive = NaiveDateTime::parse_from_str(&self.datetime, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| {
                NaiveDateTime::parse_from_str(
                    &format!("{} 00:00:00", self.datetime),
                    "%Y-%m-%d %H:%M:%S",
                )
            })
            .context("invalid twelve datetime")?;
        let open_time = Utc.from_utc_datetime(&naive);
        Ok(Candle {
            asset_class: AssetClass::Forex,
            ticker: ticker.to_string(),
            timeframe,
            open_time,
            open: self.open.parse().context("invalid open")?,
            high: self.high.parse().context("invalid high")?,
            low: self.low.parse().context("invalid low")?,
            close: self.close.parse().context("invalid close")?,
            volume: self
                .volume
                .unwrap_or_else(|| "0".into())
                .parse()
                .unwrap_or(rust_decimal::Decimal::ZERO),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetches_twelve_data_series() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/time_series"))
            .and(query_param("symbol", "EUR/USD"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status":"ok",
                "values":[{"datetime":"2026-01-01 00:00:00","open":"1.1","high":"1.2","low":"1.0","close":"1.15","volume":"123"}]
            })))
            .mount(&server)
            .await;
        let adapter = ForexAdapter::new(server.uri(), Some("k".into())).unwrap();
        for ticker in ["EUR/USD", "EURUSD"] {
            let candles = adapter
                .fetch_candles(ticker, Timeframe::H1, 10)
                .await
                .unwrap();
            assert_eq!(candles.len(), 1);
            assert_eq!(candles[0].asset_class, AssetClass::Forex);
            assert_eq!(candles[0].ticker, ticker);
        }
        assert_eq!(adapter.volume_reliability(), "low_tick_only");
    }
}
