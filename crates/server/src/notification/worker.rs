use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use rand::Rng;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, Postgres, Transaction};
use thiserror::Error;
use tokio::sync::watch;
use uuid::Uuid;

use super::{
    NotificationService,
    crypto::SecretVaultError,
    webhook::{WebhookEnvelope, WebhookError, WebhookResponse, send},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaterializeStats {
    pub outbox_messages: u64,
    pub deliveries: u64,
    pub suppressed: u64,
    pub no_destinations: u64,
}

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("stored notification payload is invalid: {0}")]
    InvalidPayload(#[from] serde_json::Error),
    #[error("stored webhook secret is invalid: {0}")]
    Secret(#[from] SecretVaultError),
}

#[derive(Debug, FromRow)]
struct OutboxRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    source: String,
    payload: Value,
    created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct DestinationSnapshot {
    id: Uuid,
    deliver_backfill: bool,
}

#[derive(Clone, Debug, FromRow)]
pub struct DeliveryClaim {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub destination_id: Uuid,
    pub outbox_message_id: Option<Uuid>,
    pub source: String,
    pub event_name: String,
    pub payload: Value,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub lease_owner: Uuid,
    pub url: String,
    pub encrypted_secret: Vec<u8>,
    pub secret_nonce: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeliveryFilter {
    pub destination_id: Option<Uuid>,
    pub status: Option<String>,
    pub source: Option<String>,
    pub origin: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub cursor: Option<Uuid>,
    pub limit: Option<i64>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct DeliverySummary {
    pub id: Uuid,
    pub project_id: Uuid,
    pub destination_id: Uuid,
    pub outbox_message_id: Option<Uuid>,
    pub origin: String,
    pub source: String,
    pub event_name: String,
    pub status: String,
    pub available_at: DateTime<Utc>,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub last_error_class: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub terminal_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct DeliveryAttempt {
    pub id: Uuid,
    pub attempt_number: i32,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub duration_ms: i64,
    pub outcome: String,
    pub http_status: Option<i32>,
    pub error_class: Option<String>,
    pub response_excerpt: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeliveryDetail {
    #[serde(flatten)]
    pub delivery: DeliverySummary,
    pub attempts: Vec<DeliveryAttempt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AttemptDisposition {
    Succeeded,
    Retryable,
    Failed,
}

#[derive(Debug)]
struct AttemptResult {
    disposition: AttemptDisposition,
    http_status: Option<u16>,
    error_class: Option<&'static str>,
    response_excerpt: Option<String>,
    retry_after: Option<Duration>,
    duration: Duration,
}

pub async fn materialize_once(
    service: &NotificationService,
) -> Result<MaterializeStats, WorkerError> {
    let mut tx = service.pool.begin().await?;
    let rows = sqlx::query_as::<_, OutboxRow>(
        "SELECT id,organization_id,project_id,source,payload,created_at FROM outbox_messages WHERE topic='runtime_group.first_seen' AND processed_at IS NULL AND materialized_at IS NULL ORDER BY created_at,id FOR UPDATE SKIP LOCKED LIMIT $1",
    )
    .bind(i64::from(service.config.claim_size))
    .fetch_all(&mut *tx)
    .await?;
    let mut stats = MaterializeStats::default();
    for row in rows {
        stats.outbox_messages = stats.outbox_messages.saturating_add(1);
        materialize_message(&mut tx, service, &row, &mut stats).await?;
    }
    tx.commit().await?;
    crate::metrics::record_notification_materialization(
        stats.deliveries,
        stats.suppressed,
        stats.no_destinations,
    );
    Ok(stats)
}

async fn materialize_message(
    tx: &mut Transaction<'_, Postgres>,
    service: &NotificationService,
    outbox: &OutboxRow,
    stats: &mut MaterializeStats,
) -> Result<(), WorkerError> {
    let destinations = sqlx::query_as::<_, DestinationSnapshot>(
        "SELECT id,deliver_backfill FROM webhook_destinations WHERE organization_id=$1 AND project_id=$2 AND enabled=true ORDER BY id",
    )
    .bind(outbox.organization_id)
    .bind(outbox.project_id)
    .fetch_all(&mut **tx)
    .await?;
    if destinations.is_empty() {
        stats.no_destinations = stats.no_destinations.saturating_add(1);
        sqlx::query("UPDATE outbox_messages SET materialized_at=now(),processed_at=now(),completion_reason='no_destinations' WHERE id=$1")
            .bind(outbox.id).execute(&mut **tx).await?;
        return Ok(());
    }
    let mut pending = 0_u64;
    for destination in destinations {
        let suppressed = outbox.source == "backfill" && !destination.deliver_backfill;
        let delivery_id = Uuid::new_v4();
        let envelope = envelope(outbox, delivery_id);
        let delivery_status = if suppressed { "suppressed" } else { "pending" };
        let terminal_at = suppressed.then(Utc::now);
        let inserted = sqlx::query("INSERT INTO notification_deliveries (id,organization_id,project_id,destination_id,outbox_message_id,origin,source,event_name,payload,status,max_attempts,terminal_at,last_error_class,last_error) VALUES ($1,$2,$3,$4,$5,'outbox',$6,'runtime_group.first_seen',$7,$8,$9,$10,$11,$12) ON CONFLICT (outbox_message_id,destination_id) WHERE outbox_message_id IS NOT NULL DO NOTHING")
            .bind(delivery_id).bind(outbox.organization_id).bind(outbox.project_id).bind(destination.id).bind(outbox.id)
            .bind(&outbox.source).bind(serde_json::to_value(envelope)?).bind(delivery_status)
            .bind(i32::try_from(service.config.max_attempts).unwrap_or(i32::MAX)).bind(terminal_at)
            .bind(suppressed.then_some("backfill_suppressed")).bind(suppressed.then_some("historical delivery is disabled"))
            .execute(&mut **tx).await?;
        if inserted.rows_affected() == 1 {
            stats.deliveries = stats.deliveries.saturating_add(1);
            if suppressed {
                stats.suppressed = stats.suppressed.saturating_add(1);
            } else {
                pending = pending.saturating_add(1);
            }
        }
    }
    if pending == 0 {
        sqlx::query("UPDATE outbox_messages SET materialized_at=now(),processed_at=now(),completion_reason='suppressed' WHERE id=$1")
            .bind(outbox.id).execute(&mut **tx).await?;
    } else {
        sqlx::query("UPDATE outbox_messages SET materialized_at=now() WHERE id=$1")
            .bind(outbox.id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

fn envelope(outbox: &OutboxRow, delivery_id: Uuid) -> WebhookEnvelope {
    let value = &outbox.payload;
    WebhookEnvelope {
        schema_version: 1,
        delivery_id,
        event: "runtime_group.first_seen".into(),
        created_at: outbox.created_at,
        source: outbox.source.clone(),
        organization_id: outbox.organization_id,
        project_id: outbox.project_id,
        application_id: uuid_field(value, "application_id"),
        group_id: uuid_field(value, "group_id"),
        event_kind: value
            .get("event_kind")
            .and_then(Value::as_str)
            .map(str::to_owned),
        semantic_summary: value.get("semantic").cloned(),
    }
}

fn uuid_field(value: &Value, name: &str) -> Option<Uuid> {
    value
        .get(name)
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
}

pub async fn claim_due(service: &NotificationService) -> Result<Vec<DeliveryClaim>, sqlx::Error> {
    let lease_owner = Uuid::new_v4();
    let lease_seconds = i64::try_from(service.config.lease_duration.as_secs()).unwrap_or(i64::MAX);
    let claims = sqlx::query_as::<_, DeliveryClaim>(
        "WITH candidates AS (SELECT id FROM notification_deliveries WHERE (status='pending' AND available_at<=now()) OR (status='in_flight' AND lease_expires_at<=now()) ORDER BY available_at,created_at FOR UPDATE SKIP LOCKED LIMIT $1), claimed AS (UPDATE notification_deliveries d SET status='in_flight',lease_owner=$2,lease_expires_at=now()+make_interval(secs=>$3),updated_at=now() FROM candidates c WHERE d.id=c.id RETURNING d.*) SELECT c.id,c.organization_id,c.project_id,c.destination_id,c.outbox_message_id,c.source,c.event_name,c.payload,c.attempt_count,c.max_attempts,c.lease_owner,w.url,w.encrypted_secret,w.secret_nonce FROM claimed c JOIN webhook_destinations w ON w.id=c.destination_id AND w.organization_id=c.organization_id AND w.project_id=c.project_id WHERE w.enabled=true",
    )
    .bind(i64::from(service.config.claim_size))
    .bind(lease_owner)
    .bind(lease_seconds)
    .fetch_all(&service.pool)
    .await?;
    crate::metrics::record_notification_claims(claims.len());
    Ok(claims)
}

pub async fn process_claim(
    service: &NotificationService,
    claim: DeliveryClaim,
) -> Result<(), WorkerError> {
    let started_at = Utc::now();
    let started = Instant::now();
    let result = match service
        .vault
        .decrypt(&claim.encrypted_secret, &claim.secret_nonce)
    {
        Ok(secret) => match serde_json::from_value::<WebhookEnvelope>(claim.payload.clone()) {
            Ok(envelope) => classify(
                send(&claim.url, &service.policy, &secret, &envelope).await,
                started.elapsed(),
            ),
            Err(_) => AttemptResult::failed("invalid_payload", started.elapsed()),
        },
        Err(_) => AttemptResult::failed("secret_decryption", started.elapsed()),
    };
    persist_attempt(service, &claim, started_at, result).await
}

pub async fn test_destination(
    service: &NotificationService,
    organization_id: Uuid,
    project_id: Uuid,
    destination_id: Uuid,
) -> Result<DeliverySummary, WorkerError> {
    let target = service
        .destinations
        .target(organization_id, project_id, destination_id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    if !target.enabled {
        return Err(WorkerError::Database(sqlx::Error::Protocol(
            "destination is disabled".into(),
        )));
    }
    let delivery_id = Uuid::new_v4();
    let lease_owner = Uuid::new_v4();
    let payload = serde_json::to_value(WebhookEnvelope {
        schema_version: 1,
        delivery_id,
        event: "okoscope.test".into(),
        created_at: Utc::now(),
        source: "test".into(),
        organization_id,
        project_id,
        application_id: None,
        group_id: None,
        event_kind: None,
        semantic_summary: None,
    })?;
    sqlx::query("INSERT INTO notification_deliveries (id,organization_id,project_id,destination_id,origin,source,event_name,payload,status,lease_owner,lease_expires_at,max_attempts) VALUES ($1,$2,$3,$4,'test','test','okoscope.test',$5,'in_flight',$6,now()+make_interval(secs=>$7),1)")
        .bind(delivery_id).bind(organization_id).bind(project_id).bind(destination_id).bind(&payload).bind(lease_owner)
        .bind(i64::try_from(service.config.lease_duration.as_secs()).unwrap_or(i64::MAX)).execute(&service.pool).await?;
    process_claim(
        service,
        DeliveryClaim {
            id: delivery_id,
            organization_id,
            project_id,
            destination_id,
            outbox_message_id: None,
            source: "test".into(),
            event_name: "okoscope.test".into(),
            payload,
            attempt_count: 0,
            max_attempts: 1,
            lease_owner,
            url: target.url,
            encrypted_secret: target.encrypted_secret,
            secret_nonce: target.secret_nonce,
        },
    )
    .await?;
    delivery_by_id(&service.pool, organization_id, project_id, delivery_id)
        .await?
        .ok_or(WorkerError::Database(sqlx::Error::RowNotFound))
}

pub async fn list_deliveries(
    pool: &sqlx::PgPool,
    organization_id: Uuid,
    project_id: Uuid,
    filter: &DeliveryFilter,
) -> Result<(Vec<DeliverySummary>, Option<Uuid>), sqlx::Error> {
    let limit = filter.limit.unwrap_or(50).clamp(1, 200);
    let cursor = if let Some(cursor) = filter.cursor {
        sqlx::query_as::<_, (DateTime<Utc>, Uuid)>("SELECT created_at,id FROM notification_deliveries WHERE organization_id=$1 AND project_id=$2 AND id=$3")
            .bind(organization_id).bind(project_id).bind(cursor).fetch_optional(pool).await?
    } else {
        None
    };
    let (cursor_time, cursor_id) = cursor.unzip();
    let mut rows = sqlx::query_as::<_, DeliverySummary>("SELECT id,project_id,destination_id,outbox_message_id,origin,source,event_name,status,available_at,attempt_count,max_attempts,last_error_class,created_at,updated_at,terminal_at FROM notification_deliveries WHERE organization_id=$1 AND project_id=$2 AND ($3::uuid IS NULL OR destination_id=$3) AND ($4::text IS NULL OR status=$4) AND ($5::text IS NULL OR source=$5) AND ($6::text IS NULL OR origin=$6) AND ($7::timestamptz IS NULL OR created_at >= $7) AND ($8::timestamptz IS NULL OR (created_at,id)<($8,$9)) ORDER BY created_at DESC,id DESC LIMIT $10")
        .bind(organization_id).bind(project_id).bind(filter.destination_id).bind(&filter.status).bind(&filter.source).bind(&filter.origin)
        .bind(filter.since).bind(cursor_time).bind(cursor_id).bind(limit + 1).fetch_all(pool).await?;
    let next_cursor = if i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit {
        rows.pop();
        rows.last().map(|delivery| delivery.id)
    } else {
        None
    };
    Ok((rows, next_cursor))
}

pub async fn delivery_detail(
    pool: &sqlx::PgPool,
    organization_id: Uuid,
    project_id: Uuid,
    delivery_id: Uuid,
) -> Result<Option<DeliveryDetail>, sqlx::Error> {
    let Some(delivery) = delivery_by_id(pool, organization_id, project_id, delivery_id).await?
    else {
        return Ok(None);
    };
    let attempts = sqlx::query_as::<_, DeliveryAttempt>("SELECT id,attempt_number,started_at,finished_at,duration_ms,outcome,http_status,error_class,response_excerpt FROM notification_delivery_attempts WHERE organization_id=$1 AND project_id=$2 AND delivery_id=$3 ORDER BY attempt_number DESC LIMIT 100")
        .bind(organization_id).bind(project_id).bind(delivery_id).fetch_all(pool).await?;
    Ok(Some(DeliveryDetail { delivery, attempts }))
}

async fn delivery_by_id(
    pool: &sqlx::PgPool,
    organization_id: Uuid,
    project_id: Uuid,
    id: Uuid,
) -> Result<Option<DeliverySummary>, sqlx::Error> {
    sqlx::query_as("SELECT id,project_id,destination_id,outbox_message_id,origin,source,event_name,status,available_at,attempt_count,max_attempts,last_error_class,created_at,updated_at,terminal_at FROM notification_deliveries WHERE organization_id=$1 AND project_id=$2 AND id=$3")
        .bind(organization_id).bind(project_id).bind(id).fetch_optional(pool).await
}

fn classify(result: Result<WebhookResponse, WebhookError>, elapsed: Duration) -> AttemptResult {
    match result {
        Ok(response) if response.status.is_success() => AttemptResult {
            disposition: AttemptDisposition::Succeeded,
            http_status: Some(response.status.as_u16()),
            error_class: None,
            response_excerpt: Some(response.response_excerpt),
            retry_after: None,
            duration: response.duration,
        },
        Ok(response) => {
            let retryable = retryable_status(response.status);
            AttemptResult {
                disposition: if retryable {
                    AttemptDisposition::Retryable
                } else {
                    AttemptDisposition::Failed
                },
                http_status: Some(response.status.as_u16()),
                error_class: Some(if retryable {
                    "http_retryable"
                } else {
                    "http_rejected"
                }),
                response_excerpt: Some(response.response_excerpt),
                retry_after: response.retry_after,
                duration: response.duration,
            }
        }
        Err(error) => {
            let (disposition, class) = match error {
                WebhookError::Request(ref request) if request.is_timeout() => {
                    (AttemptDisposition::Retryable, "timeout")
                }
                WebhookError::Request(_)
                | WebhookError::Resolution(_)
                | WebhookError::Response(_) => (AttemptDisposition::Retryable, "network"),
                WebhookError::UnsafeAddress | WebhookError::InvalidUrl(_) => {
                    (AttemptDisposition::Failed, "unsafe_target")
                }
                WebhookError::Client(_) => (AttemptDisposition::Retryable, "client"),
                WebhookError::Serialization(_) | WebhookError::SigningKey => {
                    (AttemptDisposition::Failed, "invalid_payload")
                }
            };
            AttemptResult {
                disposition,
                http_status: None,
                error_class: Some(class),
                response_excerpt: None,
                retry_after: None,
                duration: elapsed,
            }
        }
    }
}

fn retryable_status(status: StatusCode) -> bool {
    status.is_server_error() || matches!(status.as_u16(), 408 | 425 | 429)
}

impl AttemptResult {
    fn failed(class: &'static str, duration: Duration) -> Self {
        Self {
            disposition: AttemptDisposition::Failed,
            http_status: None,
            error_class: Some(class),
            response_excerpt: None,
            retry_after: None,
            duration,
        }
    }
}

async fn persist_attempt(
    service: &NotificationService,
    claim: &DeliveryClaim,
    started_at: DateTime<Utc>,
    result: AttemptResult,
) -> Result<(), WorkerError> {
    let mut tx = service.pool.begin().await?;
    let attempt_number = claim.attempt_count.saturating_add(1);
    let exhausted = attempt_number >= claim.max_attempts;
    let disposition = if result.disposition == AttemptDisposition::Retryable && exhausted {
        AttemptDisposition::Failed
    } else {
        result.disposition
    };
    crate::metrics::record_notification_attempt(
        u64::try_from(result.duration.as_micros()).unwrap_or(u64::MAX),
        disposition == AttemptDisposition::Retryable,
        disposition == AttemptDisposition::Failed,
    );
    sqlx::query("INSERT INTO notification_delivery_attempts (id,organization_id,project_id,delivery_id,attempt_number,started_at,finished_at,duration_ms,outcome,http_status,error_class,response_excerpt) VALUES ($1,$2,$3,$4,$5,$6,now(),$7,$8,$9,$10,$11)")
        .bind(Uuid::new_v4()).bind(claim.organization_id).bind(claim.project_id).bind(claim.id).bind(attempt_number).bind(started_at)
        .bind(i64::try_from(result.duration.as_millis()).unwrap_or(i64::MAX))
        .bind(match disposition { AttemptDisposition::Succeeded => "succeeded", AttemptDisposition::Retryable => "retryable", AttemptDisposition::Failed => "failed" })
        .bind(result.http_status.map(i32::from)).bind(result.error_class).bind(result.response_excerpt)
        .execute(&mut *tx).await?;
    match disposition {
        AttemptDisposition::Succeeded => {
            update_terminal(&mut tx, claim, attempt_number, "succeeded", None).await?;
        }
        AttemptDisposition::Failed => {
            update_terminal(&mut tx, claim, attempt_number, "failed", result.error_class).await?;
        }
        AttemptDisposition::Retryable => {
            let delay = retry_delay(service, attempt_number, result.retry_after);
            let delay_seconds = i64::try_from(delay.as_secs()).unwrap_or(i64::MAX);
            sqlx::query("UPDATE notification_deliveries SET status='pending',attempt_count=$3,available_at=now()+make_interval(secs=>$4),lease_owner=NULL,lease_expires_at=NULL,updated_at=now(),last_error_class=$5,last_error=$5 WHERE id=$1 AND lease_owner=$2")
                .bind(claim.id).bind(claim.lease_owner).bind(attempt_number).bind(delay_seconds).bind(result.error_class)
                .execute(&mut *tx).await?;
        }
    }
    complete_outbox_if_terminal(&mut tx, claim.outbox_message_id).await?;
    tx.commit().await?;
    Ok(())
}

async fn update_terminal(
    tx: &mut Transaction<'_, Postgres>,
    claim: &DeliveryClaim,
    attempt_number: i32,
    status: &str,
    error_class: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE notification_deliveries SET status=$3,attempt_count=$4,lease_owner=NULL,lease_expires_at=NULL,updated_at=now(),terminal_at=now(),last_error_class=$5,last_error=$5 WHERE id=$1 AND lease_owner=$2")
        .bind(claim.id).bind(claim.lease_owner).bind(status).bind(attempt_number).bind(error_class).execute(&mut **tx).await?;
    Ok(())
}

fn retry_delay(
    service: &NotificationService,
    attempt_number: i32,
    retry_after: Option<Duration>,
) -> Duration {
    let exponent = u32::try_from(attempt_number.saturating_sub(1))
        .unwrap_or(31)
        .min(31);
    let base = service
        .config
        .backoff_min
        .as_secs()
        .saturating_mul(1_u64 << exponent)
        .min(service.config.backoff_max.as_secs());
    let jitter_max = base / 4;
    let jitter = if jitter_max == 0 {
        0
    } else {
        rand::rng().random_range(0..=jitter_max)
    };
    let computed = Duration::from_secs(
        base.saturating_add(jitter)
            .min(service.config.backoff_max.as_secs()),
    );
    retry_after.map_or(computed, |retry_after| {
        retry_after
            .min(service.config.backoff_max)
            .max(service.config.backoff_min)
    })
}

async fn complete_outbox_if_terminal(
    tx: &mut Transaction<'_, Postgres>,
    outbox_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    if let Some(outbox_id) = outbox_id {
        sqlx::query("UPDATE outbox_messages o SET processed_at=now(),completion_reason='deliveries_terminal' WHERE o.id=$1 AND o.materialized_at IS NOT NULL AND NOT EXISTS (SELECT 1 FROM notification_deliveries d WHERE d.outbox_message_id=o.id AND d.status NOT IN ('succeeded','failed','suppressed','cancelled'))")
            .bind(outbox_id).execute(&mut **tx).await?;
    }
    Ok(())
}

pub async fn run(service: NotificationService, mut shutdown: watch::Receiver<bool>) {
    tracing::info!("notification worker active");
    let mut ticker = tokio::time::interval(service.config.poll_interval);
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() { break; }
            }
            _ = ticker.tick() => {
                match run_cycle(&service).await {
                    Ok(()) => crate::metrics::record_notification_cycle_success(),
                    Err(error) => {
                        crate::metrics::record_notification_cycle_failure();
                        tracing::error!(error=%error, "notification worker cycle failed");
                    }
                }
            }
        }
    }
    tracing::info!("notification worker stopped claiming work");
}

async fn run_cycle(service: &NotificationService) -> Result<(), WorkerError> {
    let stats = materialize_once(service).await?;
    if stats != MaterializeStats::default() {
        tracing::info!(?stats, "notification outbox materialized");
    }
    let claims = claim_due(service).await?;
    stream::iter(claims.into_iter().map(|claim| {
        let service = service.clone();
        async move {
            if let Err(error) = process_claim(&service, claim).await {
                tracing::error!(error=%error, "notification delivery attempt failed to persist");
            }
        }
    }))
    .buffer_unordered(service.config.concurrency)
    .collect::<Vec<_>>()
    .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_classification_matches_contract() {
        for status in [408, 425, 429, 500, 503] {
            assert!(retryable_status(StatusCode::from_u16(status).unwrap()));
        }
        for status in [301, 400, 401, 404, 422] {
            assert!(!retryable_status(StatusCode::from_u16(status).unwrap()));
        }
    }
}
