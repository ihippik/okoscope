use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
    routing::get,
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
    last_seen_at: DateTime<Utc>,
    occurrence_count: i64,
    representative_event_id: Uuid,
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
}

#[derive(Debug, Serialize)]
struct GroupDetail {
    #[serde(flatten)]
    group: GroupSummary,
    representative_event: EventOccurrence,
    recent_occurrences: Vec<EventOccurrence>,
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
        .is_some_and(|status| status != "open")
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
        "SELECT id,project_id,application_id,cluster_id,namespace,workload_kind,workload_name,fingerprint_version,event_kind,semantic_summary,status,first_seen_at,last_seen_at,occurrence_count,representative_event_id FROM runtime_event_groups WHERE organization_id=$1 AND project_id=$2 AND application_id=$3 AND ($4::text IS NULL OR event_kind=$4) AND ($5::text IS NULL OR status=$5) AND ($6::text IS NULL OR namespace=$6) AND ($7::text IS NULL OR workload_kind=$7) AND ($8::text IS NULL OR workload_name=$8) AND ($9::timestamptz IS NULL OR last_seen_at >= $9) AND ($10::timestamptz IS NULL OR (last_seen_at,id) < ($10,$11)) ORDER BY last_seen_at DESC,id DESC LIMIT $12",
    )
    .bind(principal.organization_id).bind(query.project_id).bind(query.application_id)
    .bind(query.event_kind).bind(query.status).bind(query.namespace).bind(query.workload_kind).bind(query.workload_name)
    .bind(query.since).bind(cursor_time).bind(cursor_id).bind(limit + 1)
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
        "SELECT id,project_id,application_id,cluster_id,namespace,workload_kind,workload_name,fingerprint_version,event_kind,semantic_summary,status,first_seen_at,last_seen_at,occurrence_count,representative_event_id FROM runtime_event_groups WHERE organization_id=$1 AND id=$2",
    )
    .bind(principal.organization_id).bind(group_id).fetch_optional(&state.pool).await?.ok_or(ApiError::NotFound)?;
    let representative_event = event_by_id(
        &state.pool,
        principal.organization_id,
        group.representative_event_id,
    )
    .await?
    .ok_or(ApiError::NotFound)?;
    let recent_occurrences = sqlx::query_as::<_, EventOccurrence>(
        "SELECT e.id,e.event_id,e.observed_at,e.node_name,e.namespace,e.pod_name,e.container_name,e.process_command,e.event_kind,e.payload FROM runtime_event_group_memberships m JOIN runtime_events e ON e.id=m.event_id AND e.organization_id=m.organization_id WHERE m.organization_id=$1 AND m.group_id=$2 ORDER BY e.observed_at DESC,e.id DESC LIMIT 100",
    )
    .bind(principal.organization_id).bind(group_id).fetch_all(&state.pool).await?;
    Ok(Json(GroupDetail {
        group,
        representative_event,
        recent_occurrences,
    }))
}

async fn event_by_id(
    pool: &PgPool,
    organization_id: Uuid,
    event_id: Uuid,
) -> Result<Option<EventOccurrence>, sqlx::Error> {
    sqlx::query_as::<_, EventOccurrence>(
        "SELECT id,event_id,observed_at,node_name,namespace,pod_name,container_name,process_command,event_kind,payload FROM runtime_events WHERE organization_id=$1 AND id=$2",
    )
    .bind(organization_id).bind(event_id).fetch_optional(pool).await
}
