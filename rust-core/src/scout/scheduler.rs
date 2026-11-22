#![allow(dead_code)]

use anyhow::Result;
use chrono_tz::Asia::Jakarta;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};

/// Start scout scheduler in Asia/Jakarta timezone.
/// Default: 08:30, 12:30, 16:00 WIB weekdays (Mon-Fri).
pub async fn start_scout_scheduler<F, Fut>(on_trigger: F) -> Result<JobScheduler>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let sched = JobScheduler::new().await?;
    let on_trigger = Arc::new(on_trigger);
    for cron in [
        "0 30 8 * * Mon-Fri",
        "0 30 12 * * Mon-Fri",
        "0 0 16 * * Mon-Fri",
    ] {
        let cb = on_trigger.clone();
        let job = Job::new_async_tz(cron, Jakarta, move |_uuid, _l| {
            let cb = cb.clone();
            Box::pin(async move {
                cb().await;
            })
        })?;
        sched.add(job).await?;
    }
    sched.start().await?;
    tracing::info!("scout scheduler started (Asia/Jakarta)");
    Ok(sched)
}
