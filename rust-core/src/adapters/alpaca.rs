use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::models::market::{AssetClass, Candle, Timeframe};

#[derive(Debug, Clone)]
pub struct AlpacaAdapter {
    data_base_url: String,
    #[allow(dead_code)]
    paper_base_url: String,
    api_key: Option<String>,
    api_secret: Option<String>,
    http: reqwest::Client,
}

impl AlpacaAdapter {
    pub fn new(
        data_base_url: impl Into<String>,
        paper_base_url: impl Into<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        Ok(Self {
            data_base_url: data_base_url.into().trim_end_matches('/').to_owned(),
            paper_base_url: paper_base_url.into().trim_end_matches('/').to_owned(),
            api_key,
            api_secret,
            http,
        })
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut r = req;
        if let Some(k) = &self.api_key {
            r = r.header("APCA-API-KEY-ID", k);
        }
        if let Some(s) = &self.api_secret {
            r = r.header("APCA-API-SECRET-KEY", s);
        }
        r
    }

    pub async fn fetch_candles(
        &self,
        ticker: &str,
        timeframe: Timeframe,
        limit: usize,
    ) -> Result<Vec<Candle>> {
        let symbol = ticker.trim().to_uppercase();
        let tf = map_timeframe(timeframe)?;
        let url = format!(
            "{}/v2/stocks/{}/bars?timeframe={}&limit={}&feed=iex",
            self.data_base_url,
            symbol,
            tf,
            limit.clamp(1, 1000)
        );
        let resp = self.auth(self.http.get(&url)).send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(anyhow!(
                "alpaca bars HTTP {status}: {}",
                resp.text().await.unwrap_or_default()
            ));
        }
        let body: AlpacaBarsResponse = resp.json().await.context("invalid alpaca bars response")?;
        let bars = body.bars.unwrap_or_default();
        bars.into_iter()
            .map(|b| b.into_candle(&symbol, timeframe))
            .collect()
    }

    #[allow(dead_code)]
    pub async fn place_paper_order(&self, order: &serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/v2/orders", self.paper_base_url);
        let resp = self.auth(self.http.post(&url).json(order)).send().await?;
        let status = resp.status();
        let val: serde_json::Value = resp.json().await.unwrap_or_else(|_| serde_json::json!({}));
        if !status.is_success() {
            return Err(anyhow!("alpaca paper order HTTP {status}: {val}"));
        }
        Ok(val)
    }
}

fn map_timeframe(tf: Timeframe) -> Result<&'static str> {
    Ok(match tf {
        Timeframe::M1 => "1Min",
        Timeframe::M5 => "5Min",
        Timeframe::M15 => "15Min",
        Timeframe::H1 => "1Hour",
        Timeframe::H4 => "4Hour",
        Timeframe::D1 => "1Day",
    })
}

#[derive(Debug, Deserialize)]
struct AlpacaBarsResponse {
    bars: Option<Vec<AlpacaBar>>,
}

#[derive(Debug, Deserialize)]
struct AlpacaBar {
    t: String,
    o: String,
    h: String,
    l: String,
    c: String,
    v: String,
}

impl AlpacaBar {
    fn into_candle(self, ticker: &str, timeframe: Timeframe) -> Result<Candle> {
        Ok(Candle {
            asset_class: AssetClass::UsStock,
            ticker: ticker.to_string(),
            timeframe,
            open_time: DateTime::parse_from_rfc3339(&self.t)
                .context("invalid alpaca time")?
                .with_timezone(&Utc),
            open: self.o.parse().context("invalid open")?,
            high: self.h.parse().context("invalid high")?,
            low: self.l.parse().context("invalid low")?,
            close: self.c.parse().context("invalid close")?,
            volume: self.v.parse().context("invalid volume")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetches_alpaca_bars() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/stocks/NVDA/bars"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "bars": [{"t":"2026-01-01T00:00:00Z","o":"100","h":"110","l":"90","c":"105","v":"1000"}]
            })))
            .mount(&server)
            .await;
        let adapter = AlpacaAdapter::new(
            server.uri(),
            server.uri(),
            Some("k".into()),
            Some("s".into()),
        )
        .unwrap();
        let candles = adapter
            .fetch_candles("NVDA", Timeframe::H1, 10)
            .await
            .unwrap();
        assert_eq!(candles.len(), 1);
        assert_eq!(candles[0].close, dec!(105));
        assert_eq!(candles[0].asset_class, AssetClass::UsStock);
    }
}
