use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::auth::{ApiCredentialAuthenticator, ApiPrincipal};

#[derive(Clone, Debug)]
struct ApiState {
    pool: PgPool,
    authenticator: ApiCredentialAuthenticator,
}

pub fn router(pool: PgPool) -> Router {
    let state = ApiState {
        authenticator: ApiCredentialAuthenticator::new(pool.clone()),
        pool,
    };
    Router::new()
        .route("/api/v1/runtime-groups", get(list_groups))
        .route("/api/v1/runtime-groups/{group_id}", get(get_group))
        .route(
            "/api/v1/runtime-groups/{group_id}/occurrences",
            get(list_occurrences),
        )
        .route(
            "/api/v1/runtime-groups/{group_id}/acknowledge",
            post(acknowledge_group),
        )
        .route(
            "/api/v1/runtime-groups/{group_id}/resolve",
            post(resolve_group),
        )
        .route(
            "/api/v1/runtime-groups/{group_id}/reopen",
            post(reopen_group),
        )
        .with_state(state)
}

#[derive(Debug)]
enum ApiError {
    Unauthorized,
    Invalid(String),
    NotFound,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "invalid or missing bearer credential".to_owned(),
            ),
            Self::Invalid(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "runtime group not found".to_owned(),
            ),
            Self::Database(error) => {
                tracing::error!(error = %error, "runtime groups API database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "internal server error".to_owned(),
                )
            }
        };
        (
            status,
            Json(ErrorBody {
                error: code,
                message,
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

#[derive(Clone, Debug, Deserialize)]
struct ListQuery {
    project_id: Uuid,
    application_id: Uuid,
    event_kind: Option<String>,
    status: Option<String>,
    namespace: Option<String>,
    workload_kind: Option<String>,
    workload_name: Option<String>,
    since: Option<DateTime<Utc>>,
    first_seen_from: Option<DateTime<Utc>>,
    first_seen_to: Option<DateTime<Utc>>,
    last_seen_to: Option<DateTime<Utc>>,
    release_id: Option<Uuid>,
    cursor: Option<Uuid>,
    limit: Option<i64>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
struct GroupSummary {
    id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    cluster_id: Uuid,
    namespace: String,
    workload_kind: String,
    workload_name: String,
    fingerprint_version: i16,
    event_kind: String,
    semantic_summary: Value,
    status: String,
    first_seen_at: DateTime<Utc>,
    first_seen_event_id: Uuid,
    last_seen_at: DateTime<Utc>,
    occurrence_count: i64,
    representative_event_id: Uuid,
    status_changed_at: Option<DateTime<Utc>>,
    status_changed_by: Option<Uuid>,
}

#[derive(Debug, Serialize)]
struct GroupList {
    items: Vec<GroupSummary>,
    next_cursor: Option<Uuid>,
}

#[derive(Debug, FromRow, Serialize)]
struct EventOccurrence {
    id: Uuid,
    event_id: Uuid,
    observed_at: DateTime<Utc>,
    node_name: String,
    namespace: String,
    pod_name: String,
    container_name: String,
    process_command: String,
    event_kind: String,
    payload: Value,
    correlation: Value,
    release_id: Option<Uuid>,
    release_version: Option<String>,
}

#[derive(Debug, Serialize)]
struct OccurrencePage {
    items: Vec<EventOccurrence>,
    next_cursor: Option<Uuid>,
}

#[derive(Debug, FromRow, Serialize)]
struct NotificationSummary {
    state: String,
    delivery_count: i64,
    succeeded_count: i64,
    failed_count: i64,
}

#[derive(Debug, Serialize)]
struct GroupDetail {
    #[serde(flatten)]
    group: GroupSummary,
    representative_event: EventOccurrence,
    notification: NotificationSummary,
}

async fn principal(headers: &HeaderMap, state: &ApiState) -> Result<ApiPrincipal, ApiError> {
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;
    state
        .authenticator
        .authenticate(value)
        .await?
        .ok_or(ApiError::Unauthorized)
}

async fn list_groups(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<GroupList>, ApiError> {
    crate::metrics::record_api_request();
    let principal = principal(&headers, &state).await?;
    let limit = query.limit.unwrap_or(50);
    if !(1..=200).contains(&limit) {
        return Err(ApiError::Invalid("limit must be between 1 and 200".into()));
    }
    if query
        .status
        .as_deref()
        .is_some_and(|status| !matches!(status, "open" | "acknowledged" | "resolved"))
    {
        return Err(ApiError::Invalid("unsupported status".into()));
    }
    let cursor = if let Some(cursor) = query.cursor {
        let position = sqlx::query_as::<_, (DateTime<Utc>, Uuid)>(
            "SELECT last_seen_at,id FROM runtime_event_groups WHERE id=$1 AND organization_id=$2 AND project_id=$3 AND application_id=$4",
        )
        .bind(cursor)
        .bind(principal.organization_id)
        .bind(query.project_id)
        .bind(query.application_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::Invalid("cursor does not exist in this scope".into()))?;
        Some(position)
    } else {
        None
    };
    let (cursor_time, cursor_id) = cursor.unzip();
    let mut items = sqlx::query_as::<_, GroupSummary>(
        "SELECT id,project_id,application_id,cluster_id,namespace,workload_kind,workload_name,fingerprint_version,event_kind,semantic_summary,status,first_seen_at,first_seen_event_id,last_seen_at,occurrence_count,representative_event_id,status_changed_at,status_changed_by FROM runtime_event_groups g WHERE organization_id=$1 AND project_id=$2 AND application_id=$3 AND ($4::text IS NULL OR event_kind=$4) AND ($5::text IS NULL OR status=$5) AND ($6::text IS NULL OR namespace=$6) AND ($7::text IS NULL OR workload_kind=$7) AND ($8::text IS NULL OR workload_name=$8) AND ($9::timestamptz IS NULL OR last_seen_at >= $9) AND ($10::timestamptz IS NULL OR first_seen_at >= $10) AND ($11::timestamptz IS NULL OR first_seen_at <= $11) AND ($12::timestamptz IS NULL OR last_seen_at <= $12) AND ($13::uuid IS NULL OR EXISTS (SELECT 1 FROM runtime_event_group_releases gr WHERE gr.group_id=g.id AND gr.release_id=$13)) AND ($14::timestamptz IS NULL OR (last_seen_at,id) < ($14,$15)) ORDER BY last_seen_at DESC,id DESC LIMIT $16",
    )
    .bind(principal.organization_id).bind(query.project_id).bind(query.application_id)
    .bind(query.event_kind).bind(query.status).bind(query.namespace).bind(query.workload_kind).bind(query.workload_name)
    .bind(query.since).bind(query.first_seen_from).bind(query.first_seen_to).bind(query.last_seen_to)
    .bind(query.release_id).bind(cursor_time).bind(cursor_id).bind(limit + 1)
    .fetch_all(&state.pool).await?;
    let next_cursor = if i64::try_from(items.len()).unwrap_or(i64::MAX) > limit {
        items.pop();
        items.last().map(|group| group.id)
    } else {
        None
    };
    Ok(Json(GroupList { items, next_cursor }))
}

async fn get_group(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(group_id): Path<Uuid>,
) -> Result<Json<GroupDetail>, ApiError> {
    crate::metrics::record_api_request();
    let principal = principal(&headers, &state).await?;
    let group = sqlx::query_as::<_, GroupSummary>(
        "SELECT id,project_id,application_id,cluster_id,namespace,workload_kind,workload_name,fingerprint_version,event_kind,semantic_summary,status,first_seen_at,first_seen_event_id,last_seen_at,occurrence_count,representative_event_id,status_changed_at,status_changed_by FROM runtime_event_groups WHERE organization_id=$1 AND id=$2",
    )
    .bind(principal.organization_id).bind(group_id).fetch_optional(&state.pool).await?.ok_or(ApiError::NotFound)?;
    let representative_event = event_by_id(
        &state.pool,
        principal.organization_id,
        group.representative_event_id,
    )
    .await?
    .ok_or(ApiError::NotFound)?;
    let notification =
        notification_summary(&state.pool, principal.organization_id, group_id).await?;
    Ok(Json(GroupDetail {
        group,
        representative_event,
        notification,
    }))
}

#[derive(Debug, Deserialize)]
struct OccurrenceQuery {
    cursor: Option<Uuid>,
    limit: Option<i64>,
}

async fn list_occurrences(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(group_id): Path<Uuid>,
    Query(query): Query<OccurrenceQuery>,
) -> Result<Json<OccurrencePage>, ApiError> {
    crate::metrics::record_api_request();
    let principal = principal(&headers, &state).await?;
    let limit = query.limit.unwrap_or(50);
    if !(1..=200).contains(&limit) {
        return Err(ApiError::Invalid("limit must be between 1 and 200".into()));
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM runtime_event_groups WHERE organization_id=$1 AND id=$2)",
    )
    .bind(principal.organization_id)
    .bind(group_id)
    .fetch_one(&state.pool)
    .await?;
    if !exists {
        return Err(ApiError::NotFound);
    }
    let cursor = if let Some(cursor) = query.cursor {
        Some(
            sqlx::query_as::<_, (DateTime<Utc>, Uuid)>(
                "SELECT e.observed_at,e.id FROM runtime_event_group_memberships m JOIN runtime_events e ON e.id=m.event_id WHERE m.organization_id=$1 AND m.group_id=$2 AND e.id=$3",
            )
            .bind(principal.organization_id)
            .bind(group_id)
            .bind(cursor)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::Invalid("cursor does not exist in this scope".into()))?,
        )
    } else {
        None
    };
    let (cursor_time, cursor_id) = cursor.unzip();
    let mut items = sqlx::query_as::<_, EventOccurrence>(
        "SELECT e.id,e.event_id,e.observed_at,e.node_name,e.namespace,e.pod_name,e.container_name,e.process_command,e.event_kind,e.payload,COALESCE((SELECT jsonb_build_object('status',o.status,'candidate_count',o.candidate_count,'tolerance_seconds',o.tolerance_seconds,'related_event_ids',COALESCE((SELECT jsonb_agg(c.kernel_event_id) FROM runtime_event_correlations c WHERE c.lifecycle_event_id=e.id),'[]'::jsonb)) FROM runtime_event_correlation_outcomes o WHERE o.event_id=e.id),jsonb_build_object('status','absent','candidate_count',0,'related_event_ids','[]'::jsonb)) correlation,e.release_id,r.version release_version FROM runtime_event_group_memberships m JOIN runtime_events e ON e.id=m.event_id AND e.organization_id=m.organization_id LEFT JOIN releases r ON r.id=e.release_id WHERE m.organization_id=$1 AND m.group_id=$2 AND ($3::timestamptz IS NULL OR (e.observed_at,e.id)<($3,$4)) ORDER BY e.observed_at DESC,e.id DESC LIMIT $5",
    )
    .bind(principal.organization_id)
    .bind(group_id)
    .bind(cursor_time)
    .bind(cursor_id)
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await?;
    let next_cursor = if i64::try_from(items.len()).unwrap_or(i64::MAX) > limit {
        items.pop();
        items.last().map(|event| event.id)
    } else {
        None
    };
    Ok(Json(OccurrencePage { items, next_cursor }))
}

async fn acknowledge_group(
    state: State<ApiState>,
    headers: HeaderMap,
    path: Path<Uuid>,
) -> Result<Json<GroupSummary>, ApiError> {
    transition_group(state, headers, path, "acknowledged", &["open"]).await
}

async fn resolve_group(
    state: State<ApiState>,
    headers: HeaderMap,
    path: Path<Uuid>,
) -> Result<Json<GroupSummary>, ApiError> {
    transition_group(state, headers, path, "resolved", &["open", "acknowledged"]).await
}

async fn reopen_group(
    state: State<ApiState>,
    headers: HeaderMap,
    path: Path<Uuid>,
) -> Result<Json<GroupSummary>, ApiError> {
    transition_group(state, headers, path, "open", &["acknowledged", "resolved"]).await
}

async fn transition_group(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(group_id): Path<Uuid>,
    target: &'static str,
    allowed: &'static [&'static str],
) -> Result<Json<GroupSummary>, ApiError> {
    crate::metrics::record_api_request();
    let principal = principal(&headers, &state).await?;
    let group = sqlx::query_as::<_, GroupSummary>(
        "UPDATE runtime_event_groups SET status=$3,status_changed_at=CASE WHEN status=$3 THEN status_changed_at ELSE now() END,status_changed_by=CASE WHEN status=$3 THEN status_changed_by ELSE $4 END,updated_at=CASE WHEN status=$3 THEN updated_at ELSE now() END WHERE organization_id=$1 AND id=$2 AND (status=$3 OR status=ANY($5)) RETURNING id,project_id,application_id,cluster_id,namespace,workload_kind,workload_name,fingerprint_version,event_kind,semantic_summary,status,first_seen_at,first_seen_event_id,last_seen_at,occurrence_count,representative_event_id,status_changed_at,status_changed_by",
    )
    .bind(principal.organization_id)
    .bind(group_id)
    .bind(target)
    .bind(principal.credential_id)
    .bind(allowed)
    .fetch_optional(&state.pool)
    .await?;
    if let Some(group) = group {
        return Ok(Json(group));
    }
    let current: Option<String> = sqlx::query_scalar(
        "SELECT status FROM runtime_event_groups WHERE organization_id=$1 AND id=$2",
    )
    .bind(principal.organization_id)
    .bind(group_id)
    .fetch_optional(&state.pool)
    .await?;
    match current {
        None => Err(ApiError::NotFound),
        Some(current) => Err(ApiError::Invalid(format!(
            "cannot transition runtime group from {current} to {target}"
        ))),
    }
}

async fn notification_summary(
    pool: &PgPool,
    organization_id: Uuid,
    group_id: Uuid,
) -> Result<NotificationSummary, sqlx::Error> {
    sqlx::query_as::<_, NotificationSummary>(
        "SELECT CASE WHEN o.source='backfill' THEN 'backfill_suppressed' WHEN count(d.id)=0 AND o.processed_at IS NOT NULL THEN 'not_configured' WHEN count(d.id) FILTER (WHERE d.status IN ('pending','in_flight'))>0 THEN CASE WHEN count(d.id) FILTER (WHERE d.status='in_flight')>0 THEN 'delivering' ELSE 'pending' END WHEN count(d.id) FILTER (WHERE d.status='succeeded')>0 THEN 'delivered' WHEN count(d.id) FILTER (WHERE d.status IN ('failed','cancelled','suppressed'))>0 THEN 'terminally_failed' ELSE 'pending' END state,count(d.id)::bigint delivery_count,count(d.id) FILTER (WHERE d.status='succeeded')::bigint succeeded_count,count(d.id) FILTER (WHERE d.status IN ('failed','cancelled','suppressed'))::bigint failed_count FROM outbox_messages o LEFT JOIN notification_deliveries d ON d.outbox_message_id=o.id WHERE o.organization_id=$1 AND o.aggregate_id=$2 AND o.topic='runtime_group.first_seen' GROUP BY o.id,o.source,o.processed_at",
    )
    .bind(organization_id)
    .bind(group_id)
    .fetch_one(pool)
    .await
}

async fn event_by_id(
    pool: &PgPool,
    organization_id: Uuid,
    event_id: Uuid,
) -> Result<Option<EventOccurrence>, sqlx::Error> {
    sqlx::query_as::<_, EventOccurrence>(
        "SELECT e.id,e.event_id,e.observed_at,e.node_name,e.namespace,e.pod_name,e.container_name,e.process_command,e.event_kind,e.payload,COALESCE((SELECT jsonb_build_object('status',o.status,'candidate_count',o.candidate_count,'tolerance_seconds',o.tolerance_seconds,'related_event_ids',COALESCE((SELECT jsonb_agg(c.kernel_event_id) FROM runtime_event_correlations c WHERE c.lifecycle_event_id=e.id),'[]'::jsonb)) FROM runtime_event_correlation_outcomes o WHERE o.event_id=e.id),jsonb_build_object('status','absent','candidate_count',0,'related_event_ids','[]'::jsonb)) correlation,e.release_id,r.version release_version FROM runtime_events e LEFT JOIN releases r ON r.id=e.release_id WHERE e.organization_id=$1 AND e.id=$2",
    )
    .bind(organization_id).bind(event_id).fetch_optional(pool).await
}
