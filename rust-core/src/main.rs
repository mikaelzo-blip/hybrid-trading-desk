mod adapters;
mod config;
mod engine;
mod models;
mod routes;

use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::EnvFilter;

use crate::adapters::binance::BinanceAdapter;
use crate::routes::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Arc::new(config::AppConfig::from_env()?);

    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&db).await?;

    let redis = redis::Client::open(config.redis_url.clone())?;

    let binance = Arc::new(BinanceAdapter::new(
        config.binance_rest_base_url.clone(),
        config.binance_ws_base_url.clone(),
        config.outbound_proxy_url.clone(),
    )?);

    let state = AppState {
        config: config.clone(),
        db,
        redis,
        binance,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/candles", get(routes::candles::get_candles))
        .route("/api/regime/{asset_class}", get(routes::regime::get_regime))
        .route(
            "/ws/candles/{asset_class}/{ticker}/{tf}",
            get(routes::ws::ws_candles),
        )
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(config.rust_core_bind).await?;
    tracing::info!(bind = %config.rust_core_bind, "rust-core listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> &'static str {
    "ok"
}
