pub mod candles;
pub mod regime;
pub mod ws;

use std::sync::Arc;

use sqlx::PgPool;

use crate::adapters::binance::BinanceAdapter;
use crate::config::AppConfig;

#[derive(Clone)]
pub struct AppState {
    #[allow(dead_code)]
    pub config: Arc<AppConfig>,
    pub db: PgPool,
    pub redis: redis::Client,
    pub binance: Arc<BinanceAdapter>,
}
