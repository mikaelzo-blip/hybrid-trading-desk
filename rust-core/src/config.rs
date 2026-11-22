use std::env;
use std::net::SocketAddr;

/// Runtime configuration parsed from environment variables.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub redis_url: String,
    pub rust_core_bind: SocketAddr,
    pub ai_worker_base_url: String,
    pub binance_rest_base_url: String,
    pub binance_ws_base_url: String,
    pub outbound_proxy_url: Option<String>,
}

impl AppConfig {
    /// Loads configuration from process environment.
    ///
    /// Required vars: `DATABASE_URL`, `REDIS_URL`.
    /// Others fall back to Batch 1A-safe defaults.
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = required("DATABASE_URL")?;
        let redis_url = required("REDIS_URL")?;

        let host = env_or("RUST_CORE_HOST", "0.0.0.0");
        let port: u16 = env_or("RUST_CORE_PORT", "8080")
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid RUST_CORE_PORT: {e}"))?;
        let rust_core_bind: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid rust-core bind address: {e}"))?;

        Ok(Self {
            database_url,
            redis_url,
            rust_core_bind,
            ai_worker_base_url: env_or("AI_WORKER_BASE_URL", "http://localhost:8000"),
            binance_rest_base_url: env_or(
                "BINANCE_REST_BASE_URL",
                "https://data-api.binance.vision",
            ),
            binance_ws_base_url: env_or("BINANCE_WS_BASE_URL", "wss://stream.binance.com:9443/ws"),
            outbound_proxy_url: optional("OUTBOUND_PROXY_URL"),
        })
    }
}

fn required(key: &str) -> anyhow::Result<String> {
    env::var(key).map_err(|_| anyhow::anyhow!("missing required env var: {key}"))
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn optional(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.trim().is_empty())
}
