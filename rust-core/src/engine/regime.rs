#![allow(dead_code)]

use anyhow::{Context, Result};
use chrono::{Timelike, Utc};
use redis::AsyncCommands;
use serde::Deserialize;
use serde_json::json;

use crate::adapters::alpaca::AlpacaAdapter;
use crate::adapters::binance::BinanceAdapter;
use crate::adapters::idx::IdxAdapter;
use crate::models::features::MarketRegime;
use crate::models::market::{AssetClass, Candle, Timeframe};

const DEFAULT_FAPI_BASE: &str = "https://fapi.binance.com";

#[derive(Debug, Clone)]
pub struct RegimeEngine {
    fapi_base_url: String,
    http: reqwest::Client,
}

impl RegimeEngine {
    pub fn new(fapi_base_url: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .context("failed to build regime http client")?;
        Ok(Self {
            fapi_base_url: fapi_base_url.into().trim_end_matches('/').to_owned(),
            http,
        })
    }

    pub fn with_default() -> Result<Self> {
        Self::new(DEFAULT_FAPI_BASE)
    }

    pub async fn get_regime(
        &self,
        asset_class: AssetClass,
        binance: &BinanceAdapter,
        alpaca: &AlpacaAdapter,
        idx: &IdxAdapter,
        redis: &redis::Client,
    ) -> Result<MarketRegime> {
        match asset_class {
            AssetClass::Crypto => self.crypto_regime(binance, redis).await,
            AssetClass::UsStock => self.us_regime(alpaca, redis).await,
            AssetClass::Forex => self.forex_regime(redis).await,
            AssetClass::IdxStock => self.idx_regime(idx, redis).await,
        }
    }

    async fn crypto_regime(
        &self,
        binance: &BinanceAdapter,
        redis: &redis::Client,
    ) -> Result<MarketRegime> {
        let cache_key = "regime:crypto:BTCUSDT";
        if let Some(regime) = read_cached_regime(redis, cache_key).await {
            return Ok(regime);
        }

        let candles_4h = binance
            .fetch_klines("BTCUSDT", Timeframe::H4, None, None, 120)
            .await
            .unwrap_or_default();
        let candles_1d = binance
            .fetch_klines("BTCUSDT", Timeframe::D1, None, None, 60)
            .await
            .unwrap_or_default();
        let trend_4h = trend_bias_from_candles(&candles_4h);
        let trend_1d = trend_bias_from_candles(&candles_1d);
        let funding = self.fetch_funding_rate("BTCUSDT").await.ok();
        let oi = self.fetch_open_interest("BTCUSDT").await.ok();
        let (bias, regime_label) = derive_crypto_bias(&trend_4h, &trend_1d, funding);
        let volatility_regime = derive_volatility(&candles_4h);
        let regime = MarketRegime {
            asset_class: AssetClass::Crypto,
            regime_label,
            volatility_regime,
            bias,
            summary: format!(
                "BTC 4H {trend_4h}, 1D {trend_1d}; funding={}; OI={}",
                funding
                    .map(|f| format!("{:.4}%", f * 100.0))
                    .unwrap_or_else(|| "n/a".into()),
                oi.map(|v| format!("{v:.0}"))
                    .unwrap_or_else(|| "n/a".into())
            ),
            details: json!({"btc_trend_4h":trend_4h,"btc_trend_1d":trend_1d,"funding_rate":funding,"open_interest":oi,"generated_at":Utc::now().to_rfc3339()}),
        };
        let _ = write_cache(redis, cache_key, &serde_json::to_string(&regime)?, 60).await;
        Ok(regime)
    }

    async fn us_regime(
        &self,
        alpaca: &AlpacaAdapter,
        redis: &redis::Client,
    ) -> Result<MarketRegime> {
        let cache_key = "regime:us_stock";
        if let Some(regime) = read_cached_regime(redis, cache_key).await {
            return Ok(regime);
        }

        let spy = alpaca
            .fetch_candles("SPY", Timeframe::D1, 60)
            .await
            .unwrap_or_default();
        let qqq = alpaca
            .fetch_candles("QQQ", Timeframe::D1, 60)
            .await
            .unwrap_or_default();
        let vix = alpaca
            .fetch_candles("VIXY", Timeframe::D1, 30)
            .await
            .unwrap_or_default(); // proxy via ETF if VIX not available
        let spy_trend = trend_bias_from_candles(&spy);
        let qqq_trend = trend_bias_from_candles(&qqq);
        let vix_close = vix.last().map(|c| c.close);
        let vol = derive_volatility(&spy);
        let bias = if spy_trend == "bullish" && qqq_trend != "bearish" {
            "bullish"
        } else if spy_trend == "bearish" {
            "bearish"
        } else {
            "neutral"
        };
        let regime_label = if bias == "bullish" {
            "risk_on"
        } else if bias == "bearish" {
            "risk_off"
        } else {
            "mixed"
        };
        let regime = MarketRegime {
            asset_class: AssetClass::UsStock,
            regime_label: regime_label.into(),
            volatility_regime: vol,
            bias: bias.into(),
            summary: format!(
                "SPY {spy_trend}, QQQ {qqq_trend}, VIXY proxy {}",
                vix_close
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "n/a".into())
            ),
            details: json!({"spy_trend":spy_trend,"qqq_trend":qqq_trend,"vix_proxy_close":vix_close.map(|v| v.to_string()),"generated_at":Utc::now().to_rfc3339()}),
        };
        let _ = write_cache(redis, cache_key, &serde_json::to_string(&regime)?, 120).await;
        Ok(regime)
    }

    async fn idx_regime(&self, idx: &IdxAdapter, redis: &redis::Client) -> Result<MarketRegime> {
        let cache_key = "regime:idx_stock";
        if let Some(regime) = read_cached_regime(redis, cache_key).await {
            return Ok(regime);
        }

        let jkse = idx
            .fetch_candles("^JKSE", Timeframe::D1, 60)
            .await
            .unwrap_or_default();
        let usdidr = idx
            .fetch_candles("USDIDR=X", Timeframe::D1, 30)
            .await
            .unwrap_or_default();
        let ihsg_trend = trend_bias_from_candles(&jkse);
        let usdidr_close = usdidr.last().map(|c| c.close);
        let vol = derive_volatility(&jkse);
        let bias = if ihsg_trend == "bullish" {
            "bullish"
        } else if ihsg_trend == "bearish" {
            "bearish"
        } else {
            "neutral"
        };
        let regime = MarketRegime {
            asset_class: AssetClass::IdxStock,
            regime_label: if bias == "bullish" {
                "risk_on".into()
            } else if bias == "bearish" {
                "risk_off".into()
            } else {
                "mixed".into()
            },
            volatility_regime: vol,
            bias: bias.into(),
            summary: format!(
                "IHSG {ihsg_trend}, USDIDR {}",
                usdidr_close
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "n/a".into())
            ),
            details: json!({"ihsg_trend":ihsg_trend,"usdidr_close":usdidr_close.map(|v| v.to_string()),"generated_at":Utc::now().to_rfc3339()}),
        };
        let _ = write_cache(redis, cache_key, &serde_json::to_string(&regime)?, 120).await;
        Ok(regime)
    }

    async fn forex_regime(&self, redis: &redis::Client) -> Result<MarketRegime> {
        let cache_key = "regime:forex";
        if let Some(regime) = read_cached_regime(redis, cache_key).await {
            return Ok(regime);
        }

        let session = active_session_utc(Utc::now().hour());
        // DXY/US10Y best-effort via yahoo public endpoint through reqwest
        let dxy = self.fetch_yahoo_close("DX-Y.NYB").await.ok();
        let us10y = self.fetch_yahoo_close("^TNX").await.ok();
        let regime = MarketRegime {
            asset_class: AssetClass::Forex,
            regime_label: "mixed".into(),
            volatility_regime: "normal".into(),
            bias: "neutral".into(),
            summary: format!(
                "Session {session}, DXY {}, US10Y {}",
                dxy.map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "n/a".into()),
                us10y
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "n/a".into())
            ),
            details: json!({"active_session":session,"dxy":dxy,"us10y":us10y,"generated_at":Utc::now().to_rfc3339()}),
        };
        let _ = write_cache(redis, cache_key, &serde_json::to_string(&regime)?, 120).await;
        Ok(regime)
    }

    async fn fetch_yahoo_close(&self, symbol: &str) -> Result<f64> {
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{symbol}?interval=1d&range=5d"
        );
        let resp = self.http.get(&url).send().await?.error_for_status()?;
        let body: serde_json::Value = resp.json().await?;
        let close = body["chart"]["result"][0]["indicators"]["quote"][0]["close"]
            .as_array()
            .and_then(|arr| arr.iter().rev().find_map(|v| v.as_f64()));
        close.context("missing close")
    }

    async fn fetch_funding_rate(&self, symbol: &str) -> Result<f64> {
        let url = format!(
            "{}/fapi/v1/premiumIndex?symbol={}",
            self.fapi_base_url, symbol
        );
        let resp = self.http.get(&url).send().await?.error_for_status()?;
        let body: PremiumIndex = resp.json().await?;
        body.last_funding_rate
            .parse::<f64>()
            .context("invalid funding rate")
    }

    async fn fetch_open_interest(&self, symbol: &str) -> Result<f64> {
        let url = format!(
            "{}/fapi/v1/openInterest?symbol={}",
            self.fapi_base_url, symbol
        );
        let resp = self.http.get(&url).send().await?.error_for_status()?;
        let body: OpenInterest = resp.json().await?;
        body.open_interest
            .parse::<f64>()
            .context("invalid open interest")
    }
}

#[derive(Debug, Deserialize)]
struct PremiumIndex {
    #[serde(rename = "lastFundingRate")]
    last_funding_rate: String,
}
#[derive(Debug, Deserialize)]
struct OpenInterest {
    #[serde(rename = "openInterest")]
    open_interest: String,
}

fn trend_bias_from_candles(candles: &[Candle]) -> String {
    if candles.len() < 55 {
        return "unknown".to_owned();
    }
    let closes: Vec<_> = candles.iter().map(|c| c.close).collect();
    let ema20 = crate::engine::indicators::ema(&closes, 20);
    let ema50 = crate::engine::indicators::ema(&closes, 50);
    let last = closes.len() - 1;
    match (ema20[last], ema50[last]) {
        (Some(fast), Some(slow)) if fast > slow && closes[last] > fast => "bullish".to_owned(),
        (Some(fast), Some(slow)) if fast < slow && closes[last] < fast => "bearish".to_owned(),
        _ => "neutral".to_owned(),
    }
}

fn derive_crypto_bias(trend_4h: &str, trend_1d: &str, funding: Option<f64>) -> (String, String) {
    let mut score: i32 = 0;
    score += match trend_4h {
        "bullish" => 1,
        "bearish" => -1,
        _ => 0,
    };
    score += match trend_1d {
        "bullish" => 1,
        "bearish" => -1,
        _ => 0,
    };
    if let Some(f) = funding {
        if f > 0.0005 {
            score -= 1;
        }
        if f < -0.0005 {
            score += 1;
        }
    }
    let bias = if score >= 2 {
        "bullish"
    } else if score <= -2 {
        "bearish"
    } else {
        "neutral"
    };
    let regime = if score >= 2 {
        "risk_on"
    } else if score <= -2 {
        "risk_off"
    } else {
        "mixed"
    };
    (bias.to_owned(), regime.to_owned())
}

fn derive_volatility(candles: &[Candle]) -> String {
    let atrs = crate::engine::indicators::atr(candles, 14);
    let Some(last_atr) = atrs.last().copied().flatten() else {
        return "unknown".to_owned();
    };
    let Some(last_close) = candles.last().map(|c| c.close) else {
        return "unknown".to_owned();
    };
    if last_close.is_zero() {
        return "unknown".to_owned();
    }
    let pct = (last_atr / last_close) * rust_decimal::Decimal::from(100u64);
    if pct >= rust_decimal::Decimal::from(5u64) {
        "extreme".into()
    } else if pct >= rust_decimal::Decimal::from(3u64) {
        "high".into()
    } else if pct >= rust_decimal::Decimal::from(1u64) {
        "normal".into()
    } else {
        "low".into()
    }
}

fn active_session_utc(hour: u32) -> String {
    // Rough UTC sessions
    match hour {
        0..=6 => "asia".into(),
        7..=12 => "london".into(),
        13..=20 => "new_york".into(),
        _ => "off_hours".into(),
    }
}

async fn read_cached_regime(redis: &redis::Client, key: &str) -> Option<MarketRegime> {
    let mut conn = redis.get_multiplexed_async_connection().await.ok()?;
    let val: Option<String> = conn.get(key).await.ok()?;
    val.and_then(|v| serde_json::from_str(&v).ok())
}

async fn write_cache(redis: &redis::Client, key: &str, value: &str, ttl_secs: u64) -> Result<()> {
    let mut conn = redis.get_multiplexed_async_connection().await?;
    let _: () = conn.set_ex(key, value, ttl_secs).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_crypto_bias_from_trends_and_funding() {
        let (bias, regime) = derive_crypto_bias("bullish", "bullish", Some(0.0));
        assert_eq!(bias, "bullish");
        assert_eq!(regime, "risk_on");
    }

    #[test]
    fn detects_active_session() {
        assert_eq!(active_session_utc(3), "asia");
        assert_eq!(active_session_utc(10), "london");
        assert_eq!(active_session_utc(15), "new_york");
    }
}
