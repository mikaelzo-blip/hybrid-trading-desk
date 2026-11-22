pub mod analyze;
pub mod candles;
pub mod regime;
pub mod ws;

use std::sync::Arc;

use sqlx::PgPool;

use crate::adapters::alpaca::AlpacaAdapter;
use crate::adapters::binance::BinanceAdapter;
use crate::adapters::forex::ForexAdapter;
use crate::adapters::idx::IdxAdapter;
use crate::config::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: PgPool,
    pub redis: redis::Client,
    pub binance: Arc<BinanceAdapter>,
    pub alpaca: Arc<AlpacaAdapter>,
    pub forex: Arc<ForexAdapter>,
    pub idx: Arc<IdxAdapter>,
}
