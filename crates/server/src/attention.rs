use std::collections::HashMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    auth::{ApiCredentialAuthenticator, ApiPrincipal},
    notification::health::{NotificationHealthState, NotificationQueueSnapshot, derive_state},
    web_api::{RequestId, error_response},
};

// Authentication plus a fixed repository statement sequence; neither budget depends on tenant cardinality.
pub const ORGANIZATION_ATTENTION_QUERY_BUDGET: usize = 9;
pub const APPLICATION_ATTENTION_QUERY_BUDGET: usize = 8;

#[derive(Clone)]
struct AttentionState {
    pool: PgPool,
    auth: ApiCredentialAuthenticator,
    delivery_enabled: bool,
}

pub fn router(pool: PgPool, delivery_enabled: bool) -> Router {
    let state = AttentionState {
        auth: ApiCredentialAuthenticator::new(pool.clone()),
        pool,
        delivery_enabled,
    };
    Router::new()
        .route("/api/v1/attention-summary", get(organization_summary))
        .route(
            "/api/v1/projects/{project_id}/applications/{application_id}/attention-summary",
            get(application_summary),
        )
        .with_state(state)
}

#[derive(Debug)]
enum AttentionError {
    Unauthorized,
    Invalid(String),
    NotFound,
    Database(sqlx::Error),
}
impl IntoResponse for AttentionError {
    fn into_response(self) -> Response {
        let request_id = RequestId("uncorrelated".into());
        match self {
            Self::Unauthorized => error_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "invalid or missing bearer credential",
                &request_id,
            ),
            Self::Invalid(message) => error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                message,
                &request_id,
            ),
            Self::NotFound => error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                "resource not found",
                &request_id,
            ),
            Self::Database(error) => {
                tracing::error!(%error, "attention API database error");
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "internal server error",
                    &request_id,
                )
            }
        }
    }
}
impl From<sqlx::Error> for AttentionError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
enum WindowKind {
    #[serde(rename = "24h")]
    #[default]
    Day,
    #[serde(rename = "7d")]
    Week,
}
impl WindowKind {
    fn duration(self) -> Duration {
        match self {
            Self::Day => Duration::hours(24),
            Self::Week => Duration::days(7),
        }
    }
}

#[derive(Debug, Deserialize)]
struct OrganizationQuery {
    #[serde(default)]
    window: WindowKind,
    limit: Option<i64>,
    changed_application_limit: Option<i64>,
    recommendation_limit: Option<i64>,
}
#[derive(Debug, Deserialize)]
struct ApplicationQuery {
    #[serde(default)]
    window: WindowKind,
    limit: Option<i64>,
    largest_change_limit: Option<i64>,
    recommendation_limit: Option<i64>,
}

fn bounded(value: Option<i64>, default: i64, max: i64, name: &str) -> Result<i64, AttentionError> {
    let value = value.unwrap_or(default);
    if (1..=max).contains(&value) {
        Ok(value)
    } else {
        Err(AttentionError::Invalid(format!(
            "{name} must be between 1 and {max}"
        )))
    }
}

#[derive(Clone, Debug, FromRow, Serialize)]
pub struct ProjectRef {
    id: Uuid,
    name: String,
    slug: String,
}
#[derive(Clone, Debug, FromRow, Serialize)]
pub struct ApplicationRef {
    id: Uuid,
    name: String,
    slug: String,
}
#[derive(Clone, Debug, FromRow, Serialize)]
pub struct ReleaseRef {
    id: Uuid,
    version: String,
    deployed_at: DateTime<Utc>,
}
#[derive(Clone, Debug, Serialize)]
struct AttentionWindow {
    kind: WindowKind,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Priority {
    Urgent,
    High,
    Normal,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ItemKind {
    NotificationDeliveryFailing,
    NotificationDeliveryBacklogged,
    NotificationDestinationMissing,
    ReleaseRuntimeChanged,
    NewDiscovery,
    OpenDiscovery,
    ContainerRestartLoop,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReasonCode {
    NotificationHealthFailing,
    NotificationHealthBacklogged,
    NotificationHealthRetrying,
    EnabledDestinationMissing,
    ReleaseRuntimeChanged,
    DiscoveryFirstSeenInWindow,
    DiscoveryOpen,
    ContainerRestartLoopObserved,
}
#[derive(Clone, Debug, Serialize)]
struct RestartLoopFacts {
    projection_version: i64,
    threshold: i64,
    observed_restart_count: i64,
    window_started_at: DateTime<Utc>,
    window_ended_at: DateTime<Utc>,
    container_name: String,
}
#[derive(Clone, Debug, Serialize)]
#[allow(clippy::struct_field_names)]
struct AttentionFacts {
    reason_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disappeared_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failed_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    occurrence_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    restart_loop: Option<RestartLoopFacts>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResourceRef {
    Project {
        project_id: Uuid,
    },
    Application {
        project_id: Uuid,
        application_id: Uuid,
    },
    RuntimeGroup {
        project_id: Uuid,
        application_id: Uuid,
        runtime_group_id: Uuid,
    },
    RuntimeDiff {
        project_id: Uuid,
        application_id: Uuid,
        target_release_id: Uuid,
        baseline_release_id: Uuid,
    },
}
#[derive(Clone, Debug, Serialize)]
struct PriorityItem {
    id: String,
    kind: ItemKind,
    priority: Priority,
    reason_code: ReasonCode,
    facts: AttentionFacts,
    occurred_at: DateTime<Utc>,
    project: ProjectRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    application: Option<ApplicationRef>,
    resource: ResourceRef,
    #[serde(skip)]
    stable_id: Uuid,
}

#[derive(Clone, Debug, FromRow)]
struct DiscoveryRow {
    group_id: Uuid,
    project_id: Uuid,
    project_name: String,
    project_slug: String,
    application_id: Uuid,
    application_name: String,
    application_slug: String,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    occurrence_count: i64,
    event_kind: String,
    semantic_summary: Value,
    is_new: bool,
}

#[derive(Clone, Debug, FromRow)]
struct ChangedRow {
    project_id: Uuid,
    project_name: String,
    project_slug: String,
    application_id: Uuid,
    application_name: String,
    application_slug: String,
    target_id: Uuid,
    target_version: String,
    target_deployed_at: DateTime<Utc>,
    baseline_id: Uuid,
    baseline_version: String,
    baseline_deployed_at: DateTime<Utc>,
    new_count: i64,
    disappeared_count: i64,
    unchanged_count: i64,
    total_item_count: i64,
    absolute_occurrence_delta_sum: i64,
    max_absolute_occurrence_delta: i64,
}

#[derive(Clone, Debug, Serialize)]
struct LargestChange {
    group_id: Uuid,
    classification: String,
    baseline_occurrence_count: i64,
    target_occurrence_count: i64,
    occurrence_delta: i64,
}
#[derive(Clone, Debug, Serialize)]
struct ReleaseComparison {
    target_release: ReleaseRef,
    baseline_release: Option<ReleaseRef>,
    new_count: i64,
    disappeared_count: i64,
    unchanged_count: i64,
    total_item_count: i64,
    absolute_occurrence_delta_sum: i64,
    max_absolute_occurrence_delta: i64,
    largest_changes: Vec<LargestChange>,
}
#[derive(Clone, Debug, Serialize)]
struct ChangedApplication {
    project: ProjectRef,
    application: ApplicationRef,
    #[serde(flatten)]
    comparison: ReleaseComparison,
    changed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow)]
struct ProblemRow {
    project_id: Uuid,
    project_name: String,
    project_slug: String,
    enabled_destination_count: i64,
    pending_count: i64,
    due_count: i64,
    retrying_count: i64,
    in_flight_count: i64,
    expired_lease_count: i64,
    failed_count: i64,
    oldest_due_age_seconds: Option<i64>,
    total_problem_count: i64,
}
#[derive(Clone, Debug, Serialize)]
struct NotificationProblem {
    project: ProjectRef,
    state: NotificationHealthState,
    delivery_enabled: bool,
    enabled_destination_count: i64,
    pending_count: i64,
    due_count: i64,
    retrying_count: i64,
    in_flight_count: i64,
    expired_lease_count: i64,
    failed_count: i64,
    oldest_due_age_seconds: Option<i64>,
    observed_at: DateTime<Utc>,
    priority: Priority,
    reason_code: ReasonCode,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum RecommendationKind {
    ReviewFailedDeliveries,
    ConfigureWebhookDestination,
    ReviewNotificationBacklog,
    ReviewReleaseChanges,
    ReviewNewDiscoveries,
}
#[derive(Clone, Debug, Serialize)]
struct Recommendation {
    id: String,
    kind: RecommendationKind,
    priority: Priority,
    reason_code: ReasonCode,
    facts: AttentionFacts,
    project: ProjectRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    application: Option<ApplicationRef>,
    resource: ResourceRef,
    created_from_snapshot_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow)]
struct NewDiscoveryScope {
    project_id: Uuid,
    project_name: String,
    project_slug: String,
    application_id: Uuid,
    application_name: String,
    application_slug: String,
    discovery_count: i64,
}

#[derive(Debug, FromRow, Serialize)]
struct OrganizationTotals {
    new_discoveries: i64,
    open_discoveries: i64,
    acknowledged_discoveries: i64,
    changed_applications: i64,
    projects_with_notification_problems: i64,
    failed_notification_deliveries: i64,
}
#[derive(Debug, Serialize)]
struct OrganizationSummary {
    generated_at: DateTime<Utc>,
    window: AttentionWindow,
    totals: OrganizationTotals,
    priority_items: Vec<PriorityItem>,
    changed_applications: Vec<ChangedApplication>,
    notification_problems: Vec<NotificationProblem>,
    recommendations: Vec<Recommendation>,
}
#[derive(Debug, Serialize)]
struct ApplicationTotals {
    new_discoveries: i64,
    open_discoveries: i64,
    acknowledged_discoveries: i64,
    new_runtime_items: i64,
    disappeared_runtime_items: i64,
    unchanged_runtime_items: i64,
    total_runtime_items: i64,
}
#[derive(Debug, Serialize)]
struct ApplicationSummary {
    generated_at: DateTime<Utc>,
    window: AttentionWindow,
    project: ProjectRef,
    application: ApplicationRef,
    totals: ApplicationTotals,
    release_comparison: Option<ReleaseComparison>,
    priority_items: Vec<PriorityItem>,
    recommendations: Vec<Recommendation>,
}

async fn principal(
    headers: &HeaderMap,
    state: &AttentionState,
) -> Result<ApiPrincipal, AttentionError> {
    let credential = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(AttentionError::Unauthorized)?;
    state
        .auth
        .authenticate(credential)
        .await?
        .ok_or(AttentionError::Unauthorized)
}
async fn snapshot(
    pool: &PgPool,
) -> Result<(Transaction<'_, Postgres>, DateTime<Utc>), AttentionError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let now = sqlx::query_scalar("SELECT transaction_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    Ok((tx, now))
}

const CHANGED_CTE: &str = "WITH ranked AS (SELECT r.*,row_number() OVER(PARTITION BY application_id ORDER BY deployed_at DESC,id DESC) rn FROM releases r WHERE organization_id=$1), pairs AS (SELECT t.organization_id,t.project_id,t.application_id,t.id target_id,t.version target_version,t.deployed_at target_deployed_at,b.id baseline_id,b.version baseline_version,b.deployed_at baseline_deployed_at FROM ranked t JOIN ranked b ON b.application_id=t.application_id AND b.rn=2 WHERE t.rn=1), diff AS (SELECT p.*,ids.group_id,CASE WHEN b.group_id IS NULL THEN 'new' WHEN t.group_id IS NULL THEN 'disappeared' ELSE 'unchanged' END classification,coalesce(t.occurrence_count,0) tc,coalesce(b.occurrence_count,0) bc FROM pairs p JOIN LATERAL (SELECT group_id FROM runtime_event_group_releases WHERE release_id=p.target_id UNION SELECT group_id FROM runtime_event_group_releases WHERE release_id=p.baseline_id) ids ON true LEFT JOIN runtime_event_group_releases t ON t.release_id=p.target_id AND t.group_id=ids.group_id LEFT JOIN runtime_event_group_releases b ON b.release_id=p.baseline_id AND b.group_id=ids.group_id), agg AS (SELECT project_id,application_id,target_id,target_version,target_deployed_at,baseline_id,baseline_version,baseline_deployed_at,count(*) FILTER(WHERE classification='new')::bigint new_count,count(*) FILTER(WHERE classification='disappeared')::bigint disappeared_count,count(*) FILTER(WHERE classification='unchanged')::bigint unchanged_count,count(*)::bigint total_item_count,coalesce(sum(abs(tc-bc)),0)::bigint absolute_occurrence_delta_sum,coalesce(max(abs(tc-bc)),0)::bigint max_absolute_occurrence_delta FROM diff GROUP BY project_id,application_id,target_id,target_version,target_deployed_at,baseline_id,baseline_version,baseline_deployed_at)";

#[derive(FromRow)]
struct LargestRow {
    application_id: Uuid,
    group_id: Uuid,
    classification: String,
    baseline_occurrence_count: i64,
    target_occurrence_count: i64,
    occurrence_delta: i64,
}

async fn load_largest(
    tx: &mut Transaction<'_, Postgres>,
    rows: &[ChangedRow],
    limit: i64,
) -> Result<HashMap<Uuid, Vec<LargestChange>>, sqlx::Error> {
    if rows.is_empty() {
        return Ok(HashMap::new());
    }
    let application_ids: Vec<_> = rows.iter().map(|r| r.application_id).collect();
    let target_ids: Vec<_> = rows.iter().map(|r| r.target_id).collect();
    let baseline_ids: Vec<_> = rows.iter().map(|r| r.baseline_id).collect();
    let values: Vec<LargestRow> = sqlx::query_as("WITH pairs AS (SELECT * FROM unnest($1::uuid[],$2::uuid[],$3::uuid[]) AS p(application_id,target_id,baseline_id)), changes AS (SELECT p.application_id,ids.group_id,CASE WHEN b.group_id IS NULL THEN 'new' WHEN t.group_id IS NULL THEN 'disappeared' ELSE 'unchanged' END classification,coalesce(b.occurrence_count,0)::bigint baseline_occurrence_count,coalesce(t.occurrence_count,0)::bigint target_occurrence_count,(coalesce(t.occurrence_count,0)-coalesce(b.occurrence_count,0))::bigint occurrence_delta,greatest(coalesce(t.last_seen_at,'epoch'),coalesce(b.last_seen_at,'epoch')) relevant_at FROM pairs p JOIN LATERAL (SELECT group_id FROM runtime_event_group_releases WHERE release_id=p.target_id UNION SELECT group_id FROM runtime_event_group_releases WHERE release_id=p.baseline_id) ids ON true LEFT JOIN runtime_event_group_releases t ON t.release_id=p.target_id AND t.group_id=ids.group_id LEFT JOIN runtime_event_group_releases b ON b.release_id=p.baseline_id AND b.group_id=ids.group_id), ranked AS (SELECT *,row_number() OVER(PARTITION BY application_id ORDER BY abs(occurrence_delta) DESC,relevant_at DESC,group_id) rn FROM changes) SELECT application_id,group_id,classification,baseline_occurrence_count,target_occurrence_count,occurrence_delta FROM ranked WHERE rn<=$4 ORDER BY application_id,rn")
        .bind(application_ids).bind(target_ids).bind(baseline_ids).bind(limit).fetch_all(&mut **tx).await?;
    let mut result: HashMap<Uuid, Vec<LargestChange>> = HashMap::new();
    for row in values {
        result
            .entry(row.application_id)
            .or_default()
            .push(LargestChange {
                group_id: row.group_id,
                classification: row.classification,
                baseline_occurrence_count: row.baseline_occurrence_count,
                target_occurrence_count: row.target_occurrence_count,
                occurrence_delta: row.occurrence_delta,
            });
    }
    Ok(result)
}

async fn load_changed(
    tx: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    limit: i64,
) -> Result<Vec<ChangedRow>, sqlx::Error> {
    let sql = format!(
        "{CHANGED_CTE} SELECT a.project_id,p.name project_name,p.slug project_slug,a.application_id,app.name application_name,app.slug application_slug,a.target_id,a.target_version,a.target_deployed_at,a.baseline_id,a.baseline_version,a.baseline_deployed_at,a.new_count,a.disappeared_count,a.unchanged_count,a.total_item_count,a.absolute_occurrence_delta_sum,a.max_absolute_occurrence_delta FROM agg a JOIN projects p ON p.organization_id=$1 AND p.id=a.project_id JOIN applications app ON app.organization_id=$1 AND app.id=a.application_id WHERE a.new_count+a.disappeared_count>0 ORDER BY a.new_count+a.disappeared_count DESC,a.target_deployed_at DESC,a.application_id LIMIT $2"
    );
    sqlx::query_as(&sql)
        .bind(organization_id)
        .bind(limit)
        .fetch_all(&mut **tx)
        .await
}

async fn load_application_changed(
    tx: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    application_id: Uuid,
) -> Result<Option<ChangedRow>, sqlx::Error> {
    let sql = format!(
        "{CHANGED_CTE} SELECT a.project_id,p.name project_name,p.slug project_slug,a.application_id,app.name application_name,app.slug application_slug,a.target_id,a.target_version,a.target_deployed_at,a.baseline_id,a.baseline_version,a.baseline_deployed_at,a.new_count,a.disappeared_count,a.unchanged_count,a.total_item_count,a.absolute_occurrence_delta_sum,a.max_absolute_occurrence_delta FROM agg a JOIN projects p ON p.organization_id=$1 AND p.id=a.project_id JOIN applications app ON app.organization_id=$1 AND app.id=a.application_id WHERE a.application_id=$2"
    );
    sqlx::query_as(&sql)
        .bind(organization_id)
        .bind(application_id)
        .fetch_optional(&mut **tx)
        .await
}

async fn load_discoveries(
    tx: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    application_id: Option<Uuid>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<DiscoveryRow>, sqlx::Error> {
    sqlx::query_as("SELECT g.id group_id,g.project_id,p.name project_name,p.slug project_slug,g.application_id,a.name application_name,a.slug application_slug,g.first_seen_at,g.last_seen_at,g.occurrence_count,g.event_kind,g.semantic_summary,(g.first_seen_at BETWEEN $3 AND $4) is_new FROM runtime_event_groups g JOIN projects p ON p.organization_id=g.organization_id AND p.id=g.project_id JOIN applications a ON a.organization_id=g.organization_id AND a.project_id=g.project_id AND a.id=g.application_id WHERE g.organization_id=$1 AND ($2::uuid IS NULL OR g.application_id=$2) AND g.status='open' ORDER BY CASE WHEN g.event_kind='container.restart_loop' THEN 0 WHEN g.first_seen_at BETWEEN $3 AND $4 THEN 1 ELSE 2 END,g.occurrence_count DESC,CASE WHEN g.first_seen_at BETWEEN $3 AND $4 THEN g.first_seen_at ELSE g.last_seen_at END DESC,g.id LIMIT $5")
        .bind(organization_id).bind(application_id).bind(from).bind(to).bind(limit).fetch_all(&mut **tx).await
}

async fn load_problems(
    tx: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    now: DateTime<Utc>,
    delivery_enabled: bool,
    limit: i64,
) -> Result<(Vec<NotificationProblem>, i64), sqlx::Error> {
    let rows: Vec<ProblemRow> = sqlx::query_as("WITH snapshots AS (SELECT p.id project_id,p.name project_name,p.slug project_slug,(SELECT count(*) FROM webhook_destinations w WHERE w.organization_id=p.organization_id AND w.project_id=p.id AND w.enabled) enabled_destination_count,count(d.id) FILTER(WHERE d.status='pending')::bigint pending_count,count(d.id) FILTER(WHERE d.status='pending' AND d.available_at<=$2)::bigint due_count,count(d.id) FILTER(WHERE d.status='pending' AND d.attempt_count>0)::bigint retrying_count,count(d.id) FILTER(WHERE d.status='in_flight')::bigint in_flight_count,count(d.id) FILTER(WHERE d.status='in_flight' AND d.lease_expires_at<=$2)::bigint expired_lease_count,count(d.id) FILTER(WHERE d.status='failed')::bigint failed_count,CASE WHEN count(d.id) FILTER(WHERE d.status='pending' AND d.available_at<=$2)=0 THEN NULL ELSE greatest(extract(epoch from ($2-min(d.available_at) FILTER(WHERE d.status='pending' AND d.available_at<=$2)))::bigint,0) END oldest_due_age_seconds FROM projects p LEFT JOIN notification_deliveries d ON d.organization_id=p.organization_id AND d.project_id=p.id WHERE p.organization_id=$1 GROUP BY p.id,p.name,p.slug,p.organization_id), problems AS (SELECT *,count(*) OVER()::bigint total_problem_count FROM snapshots WHERE ($3 AND enabled_destination_count=0) OR failed_count>0 OR expired_lease_count>0 OR retrying_count>0 OR due_count>0 OR pending_count>0) SELECT * FROM problems ORDER BY CASE WHEN failed_count>0 OR expired_lease_count>0 OR ($3 AND enabled_destination_count=0) THEN 0 ELSE 1 END,greatest(failed_count,due_count,retrying_count,expired_lease_count) DESC,project_id LIMIT $4")
        .bind(organization_id).bind(now).bind(delivery_enabled).bind(limit).fetch_all(&mut **tx).await?;
    let total = rows.first().map_or(0, |row| row.total_problem_count);
    Ok((
        rows.into_iter()
            .filter_map(|r| {
                let snap = NotificationQueueSnapshot {
                    enabled_destination_count: r.enabled_destination_count,
                    pending_count: r.pending_count,
                    due_count: r.due_count,
                    retrying_count: r.retrying_count,
                    in_flight_count: r.in_flight_count,
                    expired_lease_count: r.expired_lease_count,
                    failed_count: r.failed_count,
                    oldest_due_age_seconds: r.oldest_due_age_seconds,
                };
                let state = derive_state(delivery_enabled, false, &snap);
                let missing = delivery_enabled && r.enabled_destination_count == 0;
                let (priority, reason_code) = if missing {
                    (Priority::Urgent, ReasonCode::EnabledDestinationMissing)
                } else {
                    match state {
                        NotificationHealthState::Failing => {
                            (Priority::Urgent, ReasonCode::NotificationHealthFailing)
                        }
                        NotificationHealthState::Backlogged => {
                            (Priority::High, ReasonCode::NotificationHealthBacklogged)
                        }
                        NotificationHealthState::Retrying => {
                            (Priority::High, ReasonCode::NotificationHealthRetrying)
                        }
                        _ => return None,
                    }
                };
                Some(NotificationProblem {
                    project: ProjectRef {
                        id: r.project_id,
                        name: r.project_name,
                        slug: r.project_slug,
                    },
                    state,
                    delivery_enabled,
                    enabled_destination_count: r.enabled_destination_count,
                    pending_count: r.pending_count,
                    due_count: r.due_count,
                    retrying_count: r.retrying_count,
                    in_flight_count: r.in_flight_count,
                    expired_lease_count: r.expired_lease_count,
                    failed_count: r.failed_count,
                    oldest_due_age_seconds: r.oldest_due_age_seconds,
                    observed_at: now,
                    priority,
                    reason_code,
                })
            })
            .collect(),
        total,
    ))
}

async fn load_new_discovery_scopes(
    tx: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<NewDiscoveryScope>, sqlx::Error> {
    sqlx::query_as("SELECT g.project_id,p.name project_name,p.slug project_slug,g.application_id,a.name application_name,a.slug application_slug,count(*)::bigint discovery_count FROM runtime_event_groups g JOIN projects p ON p.organization_id=g.organization_id AND p.id=g.project_id JOIN applications a ON a.organization_id=g.organization_id AND a.project_id=g.project_id AND a.id=g.application_id WHERE g.organization_id=$1 AND g.status='open' AND g.first_seen_at BETWEEN $2 AND $3 GROUP BY g.project_id,p.name,p.slug,g.application_id,a.name,a.slug ORDER BY count(*) DESC,g.application_id LIMIT $4")
        .bind(organization_id).bind(from).bind(to).bind(limit).fetch_all(&mut **tx).await
}

fn recommendation_stable_id(value: &Recommendation) -> Uuid {
    match value.resource {
        ResourceRef::Project { project_id } => project_id,
        ResourceRef::Application { application_id, .. }
        | ResourceRef::RuntimeGroup { application_id, .. }
        | ResourceRef::RuntimeDiff { application_id, .. } => application_id,
    }
}

#[allow(clippy::too_many_lines)]
fn build_recommendations(
    problems: &[NotificationProblem],
    changed: &[ChangedRow],
    new_discoveries: &[NewDiscoveryScope],
    now: DateTime<Utc>,
    limit: usize,
) -> Vec<Recommendation> {
    let mut values = Vec::new();
    for p in problems {
        let missing = p.delivery_enabled && p.enabled_destination_count == 0;
        if p.failed_count > 0 || p.state == NotificationHealthState::Failing {
            values.push(Recommendation {
                id: format!("review_failed_deliveries:{}", p.project.id),
                kind: RecommendationKind::ReviewFailedDeliveries,
                priority: Priority::Urgent,
                reason_code: ReasonCode::NotificationHealthFailing,
                facts: AttentionFacts {
                    reason_count: p.failed_count.max(1),
                    new_count: None,
                    disappeared_count: None,
                    failed_count: Some(p.failed_count),
                    occurrence_count: None,
                    restart_loop: None,
                },
                project: p.project.clone(),
                application: None,
                resource: ResourceRef::Project {
                    project_id: p.project.id,
                },
                created_from_snapshot_at: now,
            });
        }
        if missing {
            values.push(Recommendation {
                id: format!("configure_webhook_destination:{}", p.project.id),
                kind: RecommendationKind::ConfigureWebhookDestination,
                priority: Priority::Urgent,
                reason_code: ReasonCode::EnabledDestinationMissing,
                facts: AttentionFacts {
                    reason_count: 1,
                    new_count: None,
                    disappeared_count: None,
                    failed_count: None,
                    occurrence_count: None,
                    restart_loop: None,
                },
                project: p.project.clone(),
                application: None,
                resource: ResourceRef::Project {
                    project_id: p.project.id,
                },
                created_from_snapshot_at: now,
            });
        }
        if p.state == NotificationHealthState::Backlogged || p.due_count > 0 || p.retrying_count > 0
        {
            values.push(Recommendation {
                id: format!("review_notification_backlog:{}", p.project.id),
                kind: RecommendationKind::ReviewNotificationBacklog,
                priority: Priority::High,
                reason_code: if p.retrying_count > 0 {
                    ReasonCode::NotificationHealthRetrying
                } else {
                    ReasonCode::NotificationHealthBacklogged
                },
                facts: AttentionFacts {
                    reason_count: p.due_count.max(p.retrying_count).max(p.expired_lease_count),
                    new_count: None,
                    disappeared_count: None,
                    failed_count: None,
                    occurrence_count: None,
                    restart_loop: None,
                },
                project: p.project.clone(),
                application: None,
                resource: ResourceRef::Project {
                    project_id: p.project.id,
                },
                created_from_snapshot_at: now,
            });
        }
    }
    for r in changed {
        values.push(Recommendation {
            id: format!("review_release_changes:{}", r.application_id),
            kind: RecommendationKind::ReviewReleaseChanges,
            priority: Priority::High,
            reason_code: ReasonCode::ReleaseRuntimeChanged,
            facts: AttentionFacts {
                reason_count: r.new_count + r.disappeared_count,
                new_count: Some(r.new_count),
                disappeared_count: Some(r.disappeared_count),
                failed_count: None,
                occurrence_count: None,
                restart_loop: None,
            },
            project: ProjectRef {
                id: r.project_id,
                name: r.project_name.clone(),
                slug: r.project_slug.clone(),
            },
            application: Some(ApplicationRef {
                id: r.application_id,
                name: r.application_name.clone(),
                slug: r.application_slug.clone(),
            }),
            resource: ResourceRef::Application {
                project_id: r.project_id,
                application_id: r.application_id,
            },
            created_from_snapshot_at: now,
        });
    }
    for r in new_discoveries {
        values.push(Recommendation {
            id: format!("review_new_discoveries:{}", r.application_id),
            kind: RecommendationKind::ReviewNewDiscoveries,
            priority: Priority::Normal,
            reason_code: ReasonCode::DiscoveryFirstSeenInWindow,
            facts: AttentionFacts {
                reason_count: r.discovery_count,
                new_count: None,
                disappeared_count: None,
                failed_count: None,
                occurrence_count: Some(r.discovery_count),
                restart_loop: None,
            },
            project: ProjectRef {
                id: r.project_id,
                name: r.project_name.clone(),
                slug: r.project_slug.clone(),
            },
            application: Some(ApplicationRef {
                id: r.application_id,
                name: r.application_name.clone(),
                slug: r.application_slug.clone(),
            }),
            resource: ResourceRef::Application {
                project_id: r.project_id,
                application_id: r.application_id,
            },
            created_from_snapshot_at: now,
        });
    }
    values.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| b.facts.reason_count.cmp(&a.facts.reason_count))
            .then_with(|| a.id.cmp(&b.id))
            .then_with(|| recommendation_stable_id(a).cmp(&recommendation_stable_id(b)))
    });
    values.truncate(limit);
    values
}

fn discovery_item(r: DiscoveryRow) -> PriorityItem {
    let relevant = if r.is_new {
        r.first_seen_at
    } else {
        r.last_seen_at
    };
    let restart_loop = (r.event_kind == "container.restart_loop").then(|| RestartLoopFacts {
        projection_version: r.semantic_summary["projection_version"]
            .as_i64()
            .unwrap_or(1),
        threshold: r.semantic_summary["threshold"].as_i64().unwrap_or(3),
        observed_restart_count: r.semantic_summary["observed_restart_count"]
            .as_i64()
            .unwrap_or(r.occurrence_count),
        window_started_at: serde_json::from_value(r.semantic_summary["window_started_at"].clone())
            .unwrap_or(r.first_seen_at),
        window_ended_at: serde_json::from_value(r.semantic_summary["window_ended_at"].clone())
            .unwrap_or(r.last_seen_at),
        container_name: r.semantic_summary["container_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned(),
    });
    let is_restart_loop = restart_loop.is_some();
    PriorityItem {
        id: format!(
            "{}:{}",
            if r.is_new {
                "new_discovery"
            } else {
                "open_discovery"
            },
            r.group_id
        ),
        kind: if is_restart_loop {
            ItemKind::ContainerRestartLoop
        } else if r.is_new {
            ItemKind::NewDiscovery
        } else {
            ItemKind::OpenDiscovery
        },
        priority: if is_restart_loop {
            Priority::High
        } else {
            Priority::Normal
        },
        reason_code: if is_restart_loop {
            ReasonCode::ContainerRestartLoopObserved
        } else if r.is_new {
            ReasonCode::DiscoveryFirstSeenInWindow
        } else {
            ReasonCode::DiscoveryOpen
        },
        facts: AttentionFacts {
            reason_count: r.occurrence_count,
            new_count: None,
            disappeared_count: None,
            failed_count: None,
            occurrence_count: Some(r.occurrence_count),
            restart_loop,
        },
        occurred_at: relevant,
        project: ProjectRef {
            id: r.project_id,
            name: r.project_name,
            slug: r.project_slug,
        },
        application: Some(ApplicationRef {
            id: r.application_id,
            name: r.application_name,
            slug: r.application_slug,
        }),
        resource: ResourceRef::RuntimeGroup {
            project_id: r.project_id,
            application_id: r.application_id,
            runtime_group_id: r.group_id,
        },
        stable_id: r.group_id,
    }
}
fn changed_item(r: &ChangedRow) -> PriorityItem {
    PriorityItem {
        id: format!("release_runtime_changed:{}:{}", r.target_id, r.baseline_id),
        kind: ItemKind::ReleaseRuntimeChanged,
        priority: Priority::High,
        reason_code: ReasonCode::ReleaseRuntimeChanged,
        facts: AttentionFacts {
            reason_count: r.new_count + r.disappeared_count,
            new_count: Some(r.new_count),
            disappeared_count: Some(r.disappeared_count),
            failed_count: None,
            occurrence_count: None,
            restart_loop: None,
        },
        occurred_at: r.target_deployed_at,
        project: ProjectRef {
            id: r.project_id,
            name: r.project_name.clone(),
            slug: r.project_slug.clone(),
        },
        application: Some(ApplicationRef {
            id: r.application_id,
            name: r.application_name.clone(),
            slug: r.application_slug.clone(),
        }),
        resource: ResourceRef::RuntimeDiff {
            project_id: r.project_id,
            application_id: r.application_id,
            target_release_id: r.target_id,
            baseline_release_id: r.baseline_id,
        },
        stable_id: r.application_id,
    }
}
fn changed_response(r: ChangedRow, largest_changes: Vec<LargestChange>) -> ChangedApplication {
    ChangedApplication {
        project: ProjectRef {
            id: r.project_id,
            name: r.project_name,
            slug: r.project_slug,
        },
        application: ApplicationRef {
            id: r.application_id,
            name: r.application_name,
            slug: r.application_slug,
        },
        comparison: ReleaseComparison {
            target_release: ReleaseRef {
                id: r.target_id,
                version: r.target_version,
                deployed_at: r.target_deployed_at,
            },
            baseline_release: Some(ReleaseRef {
                id: r.baseline_id,
                version: r.baseline_version,
                deployed_at: r.baseline_deployed_at,
            }),
            new_count: r.new_count,
            disappeared_count: r.disappeared_count,
            unchanged_count: r.unchanged_count,
            total_item_count: r.total_item_count,
            absolute_occurrence_delta_sum: r.absolute_occurrence_delta_sum,
            max_absolute_occurrence_delta: r.max_absolute_occurrence_delta,
            largest_changes,
        },
        changed_at: r.target_deployed_at,
    }
}
fn sort_items(items: &mut [PriorityItem]) {
    items.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| b.facts.reason_count.cmp(&a.facts.reason_count))
            .then_with(|| b.occurred_at.cmp(&a.occurred_at))
            .then_with(|| a.stable_id.cmp(&b.stable_id))
    });
}

#[allow(clippy::too_many_lines)]
async fn organization_summary(
    State(state): State<AttentionState>,
    headers: HeaderMap,
    Extension(_request_id): Extension<RequestId>,
    Query(q): Query<OrganizationQuery>,
) -> Result<Json<OrganizationSummary>, AttentionError> {
    let principal = principal(&headers, &state).await?;
    let limit = bounded(q.limit, 20, 50, "limit")?;
    let changed_limit = bounded(
        q.changed_application_limit,
        5,
        10,
        "changed_application_limit",
    )?;
    let rec_limit = bounded(q.recommendation_limit, 5, 10, "recommendation_limit")?;
    let (mut tx, now) = snapshot(&state.pool).await?;
    let from = now - q.window.duration();
    let changed =
        load_changed(&mut tx, principal.organization_id, limit.max(changed_limit)).await?;
    let selected_changed: Vec<_> = changed
        .iter()
        .take(usize::try_from(changed_limit).unwrap_or_default())
        .cloned()
        .collect();
    let mut largest_by_application = load_largest(&mut tx, &selected_changed, 5).await?;
    let discoveries =
        load_discoveries(&mut tx, principal.organization_id, None, from, now, limit).await?;
    let (problems, total_problem_count) = load_problems(
        &mut tx,
        principal.organization_id,
        now,
        state.delivery_enabled,
        limit,
    )
    .await?;
    let new_discovery_scopes =
        load_new_discovery_scopes(&mut tx, principal.organization_id, from, now, rec_limit).await?;
    let mut totals:OrganizationTotals=sqlx::query_as(&format!("{CHANGED_CTE} SELECT (SELECT count(*) FROM runtime_event_groups WHERE organization_id=$1 AND first_seen_at BETWEEN $2 AND $3)::bigint new_discoveries,(SELECT count(*) FROM runtime_event_groups WHERE organization_id=$1 AND status='open')::bigint open_discoveries,(SELECT count(*) FROM runtime_event_groups WHERE organization_id=$1 AND status='acknowledged')::bigint acknowledged_discoveries,(SELECT count(*) FROM agg WHERE new_count+disappeared_count>0)::bigint changed_applications,0::bigint projects_with_notification_problems,(SELECT count(*) FROM notification_deliveries WHERE organization_id=$1 AND status='failed' AND terminal_at BETWEEN $2 AND $3)::bigint failed_notification_deliveries" )).bind(principal.organization_id).bind(from).bind(now).fetch_one(&mut *tx).await?;
    totals.projects_with_notification_problems = total_problem_count;
    let mut items: Vec<_> = changed
        .iter()
        .map(changed_item)
        .chain(discoveries.into_iter().map(discovery_item))
        .collect();
    for p in &problems {
        let count = if p.failed_count > 0 {
            p.failed_count
        } else {
            p.due_count
                .max(p.retrying_count)
                .max(p.expired_lease_count)
                .max(1)
        };
        items.push(PriorityItem {
            id: format!("notification:{:?}:{}", p.reason_code, p.project.id),
            kind: match p.reason_code {
                ReasonCode::EnabledDestinationMissing => ItemKind::NotificationDestinationMissing,
                ReasonCode::NotificationHealthBacklogged
                | ReasonCode::NotificationHealthRetrying => {
                    ItemKind::NotificationDeliveryBacklogged
                }
                _ => ItemKind::NotificationDeliveryFailing,
            },
            priority: p.priority,
            reason_code: p.reason_code.clone(),
            facts: AttentionFacts {
                reason_count: count,
                new_count: None,
                disappeared_count: None,
                failed_count: Some(p.failed_count),
                occurrence_count: None,
                restart_loop: None,
            },
            occurred_at: now,
            project: p.project.clone(),
            application: None,
            resource: ResourceRef::Project {
                project_id: p.project.id,
            },
            stable_id: p.project.id,
        });
    }
    sort_items(&mut items);
    items.truncate(usize::try_from(limit).unwrap_or_default());
    let recommendations = build_recommendations(
        &problems,
        &changed,
        &new_discovery_scopes,
        now,
        usize::try_from(rec_limit).unwrap_or_default(),
    );
    let changed_applications = selected_changed
        .into_iter()
        .map(|r| {
            let largest = largest_by_application
                .remove(&r.application_id)
                .unwrap_or_default();
            changed_response(r, largest)
        })
        .collect();
    tx.commit().await?;
    Ok(Json(OrganizationSummary {
        generated_at: now,
        window: AttentionWindow {
            kind: q.window,
            from,
            to: now,
        },
        totals,
        priority_items: items,
        changed_applications,
        notification_problems: problems,
        recommendations,
    }))
}

#[allow(clippy::too_many_lines)]
async fn application_summary(
    State(state): State<AttentionState>,
    headers: HeaderMap,
    Extension(_request_id): Extension<RequestId>,
    Path((project_id, application_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<ApplicationQuery>,
) -> Result<Json<ApplicationSummary>, AttentionError> {
    let principal = principal(&headers, &state).await?;
    let limit = bounded(q.limit, 20, 50, "limit")?;
    let largest = bounded(q.largest_change_limit, 5, 10, "largest_change_limit")?;
    let rec_limit = bounded(q.recommendation_limit, 5, 10, "recommendation_limit")?;
    let (mut tx, now) = snapshot(&state.pool).await?;
    let from = now - q.window.duration();
    let identity:Option<(Uuid,String,String,String,String)>=sqlx::query_as("SELECT p.id,p.name,p.slug,a.name,a.slug FROM projects p JOIN applications a ON a.organization_id=p.organization_id AND a.project_id=p.id WHERE p.organization_id=$1 AND p.id=$2 AND a.id=$3").bind(principal.organization_id).bind(project_id).bind(application_id).fetch_optional(&mut *tx).await?;
    let (_, pn, ps, an, aslug) = identity.ok_or(AttentionError::NotFound)?;
    let project = ProjectRef {
        id: project_id,
        name: pn,
        slug: ps,
    };
    let application = ApplicationRef {
        id: application_id,
        name: an,
        slug: aslug,
    };
    let discoveries = load_discoveries(
        &mut tx,
        principal.organization_id,
        Some(application_id),
        from,
        now,
        limit,
    )
    .await?;
    let mut items: Vec<_> = discoveries.into_iter().map(discovery_item).collect();
    let changed =
        load_application_changed(&mut tx, principal.organization_id, application_id).await?;
    if let Some(ref r) = changed
        && r.new_count + r.disappeared_count > 0
    {
        items.push(changed_item(r));
    }
    sort_items(&mut items);
    items.truncate(usize::try_from(limit).unwrap_or_default());
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT count(*) FILTER(WHERE first_seen_at BETWEEN $3 AND $4)::bigint,count(*) FILTER(WHERE status='open')::bigint,count(*) FILTER(WHERE status='acknowledged')::bigint FROM runtime_event_groups WHERE organization_id=$1 AND application_id=$2").bind(principal.organization_id).bind(application_id).bind(from).bind(now).fetch_one(&mut *tx).await?;
    let largest_changes = if let Some(ref row) = changed {
        load_largest(&mut tx, std::slice::from_ref(row), largest)
            .await?
            .remove(&application_id)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let comparison = changed
        .map(|r| changed_response(r, largest_changes))
        .map(|x| x.comparison);
    let (
        new_runtime_items,
        disappeared_runtime_items,
        unchanged_runtime_items,
        total_runtime_items,
    ) = comparison.as_ref().map_or((0, 0, 0, 0), |c| {
        (
            c.new_count,
            c.disappeared_count,
            c.unchanged_count,
            c.total_item_count,
        )
    });
    let mut recommendations = Vec::new();
    if let Some(c) = &comparison
        && c.new_count + c.disappeared_count > 0
    {
        recommendations.push(Recommendation {
            id: format!("recommendation:release:{application_id}"),
            kind: RecommendationKind::ReviewReleaseChanges,
            priority: Priority::High,
            reason_code: ReasonCode::ReleaseRuntimeChanged,
            facts: AttentionFacts {
                reason_count: c.new_count + c.disappeared_count,
                new_count: Some(c.new_count),
                disappeared_count: Some(c.disappeared_count),
                failed_count: None,
                occurrence_count: None,
                restart_loop: None,
            },
            project: project.clone(),
            application: Some(application.clone()),
            resource: ResourceRef::Application {
                project_id,
                application_id,
            },
            created_from_snapshot_at: now,
        });
    }
    if counts.0 > 0 {
        recommendations.push(Recommendation {
            id: format!("recommendation:new:{application_id}"),
            kind: RecommendationKind::ReviewNewDiscoveries,
            priority: Priority::Normal,
            reason_code: ReasonCode::DiscoveryFirstSeenInWindow,
            facts: AttentionFacts {
                reason_count: counts.0,
                new_count: None,
                disappeared_count: None,
                failed_count: None,
                occurrence_count: Some(counts.0),
                restart_loop: None,
            },
            project: project.clone(),
            application: Some(application.clone()),
            resource: ResourceRef::Application {
                project_id,
                application_id,
            },
            created_from_snapshot_at: now,
        });
    }
    recommendations.truncate(usize::try_from(rec_limit).unwrap_or_default());
    tx.commit().await?;
    Ok(Json(ApplicationSummary {
        generated_at: now,
        window: AttentionWindow {
            kind: q.window,
            from,
            to: now,
        },
        project,
        application,
        totals: ApplicationTotals {
            new_discoveries: counts.0,
            open_discoveries: counts.1,
            acknowledged_discoveries: counts.2,
            new_runtime_items,
            disappeared_runtime_items,
            unchanged_runtime_items,
            total_runtime_items,
        },
        release_comparison: comparison,
        priority_items: items,
        recommendations,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    #[test]
    fn windows_and_limits() {
        let now = Utc::now();
        assert_eq!(now - WindowKind::Day.duration(), now - Duration::hours(24));
        assert_eq!(now - WindowKind::Week.duration(), now - Duration::days(7));
        for max in [10, 50] {
            assert!(bounded(Some(0), 5, max, "limit").is_err());
            assert!(bounded(Some(max + 1), 5, max, "limit").is_err());
            assert_eq!(bounded(Some(1), 5, max, "limit").unwrap(), 1);
            assert_eq!(bounded(Some(max), 5, max, "limit").unwrap(), max);
        }
        assert_eq!(bounded(None, 20, 50, "limit").unwrap(), 20);
        assert_eq!(serde_json::to_string(&WindowKind::Day).unwrap(), "\"24h\"");
        assert!(serde_json::from_str::<WindowKind>("\"30d\"").is_err());
        assert_eq!(ORGANIZATION_ATTENTION_QUERY_BUDGET, 9);
        assert_eq!(APPLICATION_ATTENTION_QUERY_BUDGET, 8);
    }
    #[test]
    fn tuple_order_is_stable() {
        let id1 = Uuid::from_u128(1);
        let id2 = Uuid::from_u128(2);
        let p = ProjectRef {
            id: id1,
            name: "p".into(),
            slug: "p".into(),
        };
        let occurred_at = Utc::now();
        let make = |id: Uuid, count: i64, priority: Priority, at: DateTime<Utc>| PriorityItem {
            id: id.to_string(),
            kind: ItemKind::OpenDiscovery,
            priority,
            reason_code: ReasonCode::DiscoveryOpen,
            facts: AttentionFacts {
                reason_count: count,
                new_count: None,
                disappeared_count: None,
                failed_count: None,
                occurrence_count: Some(count),
                restart_loop: None,
            },
            occurred_at: at,
            project: p.clone(),
            application: None,
            resource: ResourceRef::Project { project_id: id },
            stable_id: id,
        };
        let mut v = vec![
            make(id2, 100, Priority::Normal, occurred_at),
            make(id2, 2, Priority::High, occurred_at),
            make(id1, 2, Priority::High, occurred_at),
            make(id1, 1, Priority::Urgent, occurred_at - Duration::days(1)),
        ];
        sort_items(&mut v);
        assert_eq!(v[0].stable_id, id1);
        assert_eq!(v[1].stable_id, id1);
        assert_eq!(v[2].stable_id, id2);
        assert_eq!(v[3].priority, Priority::Normal);
    }

    #[test]
    fn serialized_item_is_typed_and_secret_free() {
        let id = Uuid::from_u128(1);
        let item = PriorityItem {
            id: format!("open_discovery:{id}"),
            kind: ItemKind::OpenDiscovery,
            priority: Priority::Normal,
            reason_code: ReasonCode::DiscoveryOpen,
            facts: AttentionFacts {
                reason_count: 4,
                new_count: None,
                disappeared_count: None,
                failed_count: None,
                occurrence_count: Some(4),
                restart_loop: None,
            },
            occurred_at: Utc::now(),
            project: ProjectRef {
                id,
                name: "project".into(),
                slug: "project".into(),
            },
            application: None,
            resource: ResourceRef::RuntimeGroup {
                project_id: id,
                application_id: id,
                runtime_group_id: id,
            },
            stable_id: id,
        };
        let value = serde_json::to_value(item).unwrap();
        assert_eq!(value["resource"]["type"], "runtime_group");
        let text = value.to_string();
        for forbidden in [
            "webhook",
            "secret",
            "credential",
            "response_excerpt",
            "frontend_url",
        ] {
            assert!(!text.contains(forbidden));
        }
    }

    #[test]
    fn restart_loop_discovery_has_dedicated_attention_variant_and_facts() {
        let now = Utc::now();
        let item = discovery_item(DiscoveryRow {
            group_id: Uuid::from_u128(1),
            project_id: Uuid::from_u128(2),
            project_name: "project".into(),
            project_slug: "project".into(),
            application_id: Uuid::from_u128(3),
            application_name: "app".into(),
            application_slug: "app".into(),
            first_seen_at: now - Duration::minutes(5),
            last_seen_at: now,
            occurrence_count: 1,
            event_kind: "container.restart_loop".into(),
            semantic_summary: serde_json::json!({
                "projection_version": 1,
                "threshold": 3,
                "observed_restart_count": 4,
                "window_started_at": now - Duration::minutes(10),
                "window_ended_at": now,
                "container_name": "worker"
            }),
            is_new: true,
        });
        let value = serde_json::to_value(item).unwrap();
        assert_eq!(value["kind"], "container_restart_loop");
        assert_eq!(value["reason_code"], "container_restart_loop_observed");
        assert_eq!(value["priority"], "high");
        assert_eq!(value["facts"]["restart_loop"]["threshold"], 3);
        assert_eq!(value["facts"]["restart_loop"]["observed_restart_count"], 4);
    }

    #[test]
    fn recommendation_rules_are_complete_deduplicated_and_bounded() {
        let id = Uuid::from_u128(7);
        let now = Utc::now();
        let project = ProjectRef {
            id,
            name: "p".into(),
            slug: "p".into(),
        };
        let problem = NotificationProblem {
            project: project.clone(),
            state: NotificationHealthState::Failing,
            delivery_enabled: true,
            enabled_destination_count: 0,
            pending_count: 2,
            due_count: 2,
            retrying_count: 1,
            in_flight_count: 0,
            expired_lease_count: 0,
            failed_count: 3,
            oldest_due_age_seconds: Some(10),
            observed_at: now,
            priority: Priority::Urgent,
            reason_code: ReasonCode::EnabledDestinationMissing,
        };
        let changed = ChangedRow {
            project_id: id,
            project_name: "p".into(),
            project_slug: "p".into(),
            application_id: id,
            application_name: "a".into(),
            application_slug: "a".into(),
            target_id: Uuid::from_u128(8),
            target_version: "2".into(),
            target_deployed_at: now,
            baseline_id: Uuid::from_u128(9),
            baseline_version: "1".into(),
            baseline_deployed_at: now - Duration::hours(1),
            new_count: 2,
            disappeared_count: 1,
            unchanged_count: 0,
            total_item_count: 3,
            absolute_occurrence_delta_sum: 3,
            max_absolute_occurrence_delta: 2,
        };
        let discovery = NewDiscoveryScope {
            project_id: id,
            project_name: "p".into(),
            project_slug: "p".into(),
            application_id: id,
            application_name: "a".into(),
            application_slug: "a".into(),
            discovery_count: 4,
        };
        let values = build_recommendations(&[problem], &[changed], &[discovery], now, 10);
        assert_eq!(values.len(), 5);
        let serialized = serde_json::to_value(&values).unwrap().to_string();
        for kind in [
            "review_failed_deliveries",
            "configure_webhook_destination",
            "review_notification_backlog",
            "review_release_changes",
            "review_new_discoveries",
        ] {
            assert!(serialized.contains(kind));
        }
        assert_eq!(build_recommendations(&[], &[], &[], now, 10).len(), 0);
        assert_eq!(values[0].priority, Priority::Urgent);
        assert_eq!(
            build_recommendations(
                &[],
                &[ChangedRow {
                    project_id: id,
                    project_name: "p".into(),
                    project_slug: "p".into(),
                    application_id: id,
                    application_name: "a".into(),
                    application_slug: "a".into(),
                    target_id: Uuid::from_u128(8),
                    target_version: "2".into(),
                    target_deployed_at: now,
                    baseline_id: Uuid::from_u128(9),
                    baseline_version: "1".into(),
                    baseline_deployed_at: now,
                    new_count: 1,
                    disappeared_count: 0,
                    unchanged_count: 0,
                    total_item_count: 1,
                    absolute_occurrence_delta_sum: 1,
                    max_absolute_occurrence_delta: 1
                }],
                &[],
                now,
                0
            )
            .len(),
            0
        );
    }

    #[tokio::test]
    async fn routes_reject_missing_credentials_with_correlated_no_store_error() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/invalid")
            .unwrap();
        let app = crate::web_api::router(
            router(pool, false),
            &crate::web_api::WebApiConfig::default(),
        );
        for path in [
            "/api/v1/attention-summary",
            "/api/v1/projects/00000000-0000-0000-0000-000000000001/applications/00000000-0000-0000-0000-000000000002/attention-summary",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(response.headers()["cache-control"], "no-store");
            assert!(response.headers().contains_key("x-request-id"));
        }
    }
}
