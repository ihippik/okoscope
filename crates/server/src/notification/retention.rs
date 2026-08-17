use std::time::Duration;

use serde::Serialize;
use sqlx::PgPool;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetentionConfig {
    pub enabled: bool,
    pub terminal_window: Duration,
    pub recovery_window: Duration,
    pub batch_size: i64,
    pub poll_interval: Duration,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RetentionStats {
    pub recovery_operations_deleted: u64,
    pub terminal_deliveries_deleted: u64,
    pub duration_micros: u64,
}

pub async fn delete_once(
    pool: &PgPool,
    config: RetentionConfig,
) -> Result<RetentionStats, sqlx::Error> {
    if !config.enabled {
        return Ok(RetentionStats::default());
    }
    let started_at = std::time::Instant::now();
    let terminal_seconds = i64::try_from(config.terminal_window.as_secs()).unwrap_or(i64::MAX);
    let recovery_seconds = i64::try_from(config.recovery_window.as_secs()).unwrap_or(i64::MAX);
    let mut tx = pool.begin().await?;
    let operations = sqlx::query("WITH candidates AS (SELECT id FROM notification_recovery_operations WHERE completed_at < now()-make_interval(secs=>$1) ORDER BY completed_at,id FOR UPDATE SKIP LOCKED LIMIT $2) DELETE FROM notification_recovery_operations o USING candidates c WHERE o.id=c.id")
        .bind(recovery_seconds).bind(config.batch_size).execute(&mut *tx).await?;
    let deliveries = sqlx::query("WITH candidates AS (SELECT id FROM notification_deliveries WHERE status IN ('succeeded','failed','suppressed','cancelled') AND terminal_at < now()-make_interval(secs=>$1) AND NOT EXISTS (SELECT 1 FROM notification_recovery_operations o WHERE o.target_delivery_id=notification_deliveries.id) ORDER BY terminal_at,id FOR UPDATE SKIP LOCKED LIMIT $2) DELETE FROM notification_deliveries d USING candidates c WHERE d.id=c.id")
        .bind(terminal_seconds).bind(config.batch_size).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(RetentionStats {
        recovery_operations_deleted: operations.rows_affected(),
        terminal_deliveries_deleted: deliveries.rows_affected(),
        duration_micros: u64::try_from(started_at.elapsed().as_micros()).unwrap_or(u64::MAX),
    })
}

pub async fn run(pool: PgPool, config: RetentionConfig, mut shutdown: watch::Receiver<bool>) {
    if !config.enabled {
        tracing::info!("notification retention disabled");
        return;
    }
    tracing::info!(
        terminal_days = config.terminal_window.as_secs() / 86_400,
        recovery_days = config.recovery_window.as_secs() / 86_400,
        batch_size = config.batch_size,
        "notification retention active"
    );
    let mut ticker = tokio::time::interval(config.poll_interval);
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() { break; }
            }
            _ = ticker.tick() => match delete_once(&pool, config).await {
                Ok(stats) => {
                    crate::metrics::record_notification_retention_success(stats);
                    tracing::info!(?stats, "notification retention batch complete");
                }
                Err(error) => {
                    crate::metrics::record_notification_retention_failure();
                    tracing::error!(error=%error, "notification retention batch failed");
                }
            }
        }
    }
}
