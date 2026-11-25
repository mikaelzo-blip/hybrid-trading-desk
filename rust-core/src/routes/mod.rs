pub mod analyze;
pub mod candles;
pub mod paper;
pub mod paper_audit;
pub mod regime;
pub mod scout;
pub mod ws;

use std::sync::Arc;

use sqlx::PgPool;

use crate::adapters::alpaca::AlpacaAdapter;
use crate::adapters::bea_calendar::BeaCalendar;
use crate::adapters::binance::BinanceAdapter;
use crate::adapters::finnhub::FinnhubAdapter;
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
    pub finnhub: Arc<FinnhubAdapter>,
    pub bea_calendar: Arc<BeaCalendar>,
    pub forex: Arc<ForexAdapter>,
    pub idx: Arc<IdxAdapter>,
}
