#![allow(dead_code)]

use anyhow::Result;
use sqlx::PgPool;

/// P4-04: scaffold scheduler; real cron wiring added in Batch 4B.
pub async fn start_scout_scheduler(_db: PgPool) -> Result<()> {
    // tokio-cron-scheduler integration lands in P4-04.
    Ok(())
}
