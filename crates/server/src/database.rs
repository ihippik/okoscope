use sqlx::{PgPool, Row};
use thiserror::Error;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
pub const REQUIRED_MIGRATION: i64 = 4;

#[derive(Debug, Error)]
pub enum ReadinessError {
    #[error("database query failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("database schema is not ready: expected migration {expected}, found {actual:?}")]
    MissingMigration { expected: i64, actual: Option<i64> },
}

pub async fn verify_schema(pool: &PgPool) -> Result<(), ReadinessError> {
    let row =
        sqlx::query("SELECT max(version) AS version FROM _sqlx_migrations WHERE success = true")
            .fetch_one(pool)
            .await?;
    let actual: Option<i64> = row.try_get("version")?;
    if actual != Some(REQUIRED_MIGRATION) {
        return Err(ReadinessError::MissingMigration {
            expected: REQUIRED_MIGRATION,
            actual,
        });
    }
    Ok(())
}
