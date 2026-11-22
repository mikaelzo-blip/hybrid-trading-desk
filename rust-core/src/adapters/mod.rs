pub mod binance;

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::future::Future;

use crate::models::market::{AssetClass, Candle, Timeframe};

/// Unified market data adapter contract for all supported exchanges/providers.
///
/// Implementations normalize venue-specific payloads into the shared `Candle`
/// model. Real-time streaming and persistence are wired by higher layers.
#[allow(dead_code)]
pub trait MarketAdapter: Send + Sync {
    /// Asset class served by this adapter.
    fn asset_class(&self) -> AssetClass;

    /// Fetch historical candles in ascending open_time order.
    fn fetch_candles(
        &self,
        ticker: &str,
        timeframe: Timeframe,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<Candle>>> + Send;
}
