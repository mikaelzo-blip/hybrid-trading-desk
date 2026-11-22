#![allow(dead_code)]

use anyhow::{Context, Result};
use chrono::Utc;
use redis::AsyncCommands;
use serde::Deserialize;
use serde_json::json;

use crate::adapters::binance::BinanceAdapter;
use crate::models::features::MarketRegime;
use crate::models::market::{AssetClass, Timeframe};

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
        redis: &redis::Client,
    ) -> Result<MarketRegime> {
        match asset_class {
            AssetClass::Crypto => self.crypto_regime(binance, redis).await,
            _ => Ok(placeholder_regime(asset_class)),
        }
    }

    async fn crypto_regime(
        &self,
        binance: &BinanceAdapter,
        redis: &redis::Client,
    ) -> Result<MarketRegime> {
        let cache_key = "regime:crypto:BTCUSDT";
        if let Ok(Some(cached)) = read_cache(redis, cache_key).await {
            if let Ok(regime) = serde_json::from_str::<MarketRegime>(&cached) {
                return Ok(regime);
            }
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

        let summary = format!(
            "BTC 4H {}, 1D {}; funding={}; OI={}",
            trend_4h,
            trend_1d,
            funding
                .map(|f| format!("{:.4}%", f * 100.0))
                .unwrap_or_else(|| "n/a".to_owned()),
            oi.map(|v| format!("{:.0}", v))
                .unwrap_or_else(|| "n/a".to_owned())
        );

        let regime = MarketRegime {
            asset_class: AssetClass::Crypto,
            regime_label,
            volatility_regime,
            bias,
            summary,
            details: json!({
                "btc_trend_4h": trend_4h,
                "btc_trend_1d": trend_1d,
                "funding_rate": funding,
                "open_interest": oi,
                "generated_at": Utc::now().to_rfc3339(),
            }),
        };

        let _ = write_cache(redis, cache_key, &serde_json::to_string(&regime)?, 60).await;
        Ok(regime)
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

fn trend_bias_from_candles(candles: &[crate::models::market::Candle]) -> String {
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
        } // crowded longs
        if f < -0.0005 {
            score += 1;
        } // crowded shorts
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

fn derive_volatility(candles: &[crate::models::market::Candle]) -> String {
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
        "extreme".to_owned()
    } else if pct >= rust_decimal::Decimal::from(3u64) {
        "high".to_owned()
    } else if pct >= rust_decimal::Decimal::from(1u64) {
        "normal".to_owned()
    } else {
        "low".to_owned()
    }
}

fn placeholder_regime(asset_class: AssetClass) -> MarketRegime {
    MarketRegime {
        asset_class,
        regime_label: "unknown".to_owned(),
        volatility_regime: "unknown".to_owned(),
        bias: "neutral".to_owned(),
        summary: "Regime engine belum diimplementasikan penuh untuk asset class ini".to_owned(),
        details: json!({ "status": "placeholder" }),
    }
}

async fn read_cache(redis: &redis::Client, key: &str) -> Result<Option<String>> {
    let mut conn = redis.get_multiplexed_async_connection().await?;
    let val: Option<String> = conn.get(key).await?;
    Ok(val)
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
        let (bias, _) = derive_crypto_bias("bearish", "bearish", Some(0.001));
        assert_eq!(bias, "bearish");
    }
}
