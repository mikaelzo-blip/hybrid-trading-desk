mod adapters;
mod config;
mod models;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Batch 1A scaffold: config + models only.
    // Batch 1B wires HTTP/WS routes and market adapters.
    let _config = config::AppConfig::from_env();
    tracing::info!("rust-core scaffold initialized");
    Ok(())
}
