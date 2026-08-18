use std::time::Instant;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{
    auth::{ApiCredentialAuthenticator, ApiPrincipal},
    inventory::CURRENT_INVENTORY_IDENTITY_VERSION,
};

#[derive(Clone, Debug)]
struct InventoryApiState {
    pool: PgPool,
    auth: ApiCredentialAuthenticator,
}

pub fn router(pool: PgPool) -> Router {
    let state = InventoryApiState {
        auth: ApiCredentialAuthenticator::new(pool.clone()),
        pool,
    };
    Router::new()
        .route(
            "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory",
            get(list_items),
        )
        .route(
            "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/summary",
            get(summary),
        )
        .route(
            "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/{item_id}",
            get(item_detail),
        )
        .route(
            "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/{item_id}/releases",
            get(item_releases),
        )
        .route(
            "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/{item_id}/sightings",
            get(item_sightings),
        )
        .route(
            "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/{item_id}/groups",
            get(item_groups),
        )
        .route(
            "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/{item_id}/occurrences",
            get(item_occurrences),
        )
        .with_state(state)
}

#[derive(Debug)]
enum InventoryApiError {
    Unauthorized,
    Invalid(String),
    NotFound,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for InventoryApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl IntoResponse for InventoryApiError {
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
                "runtime inventory resource not found".to_owned(),
            ),
            Self::Database(error) => {
                tracing::error!(error=%error, "runtime inventory API database error");
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
struct InventoryQuery {
    kind: Option<String>,
    release_id: Option<Uuid>,
    cluster_id: Option<Uuid>,
    namespace: Option<String>,
    workload_kind: Option<String>,
    workload_name: Option<String>,
    container_name: Option<String>,
    observed_from: Option<DateTime<Utc>>,
    observed_to: Option<DateTime<Utc>>,
    search: Option<String>,
    cursor: Option<Uuid>,
    limit: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
struct PageQuery {
    cursor: Option<Uuid>,
    limit: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
struct StringCursorPageQuery {
    cursor: Option<String>,
    limit: Option<i64>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
struct InventoryItem {
    id: Uuid,
    project_id: Uuid,
    application_id: Uuid,
    inventory_kind: String,
    identity_version: i16,
    semantic_summary: Value,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    occurrence_count: i64,
    release_count: i64,
    cluster_count: i64,
    namespace_count: i64,
    workload_count: i64,
    pod_count: i64,
    container_count: i64,
    group_count: i64,
}

#[derive(Debug, Serialize)]
struct InventoryItemPage {
    items: Vec<InventoryItem>,
    next_cursor: Option<Uuid>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
struct KindCount {
    kind: String,
    item_count: i64,
    occurrence_count: i64,
}

#[derive(Debug, Serialize)]
struct InventorySummary {
    identity_version: i16,
    item_count: i64,
    occurrence_count: i64,
    first_seen_at: Option<DateTime<Utc>>,
    last_seen_at: Option<DateTime<Utc>>,
    kinds: Vec<KindCount>,
}

#[derive(Debug, Serialize)]
struct InventoryItemDetail {
    #[serde(flatten)]
    item: InventoryItem,
    evidence: EvidenceLinks,
}

#[derive(Debug, Serialize)]
struct EvidenceLinks {
    releases: String,
    sightings: String,
    groups: String,
    occurrences: String,
}

#[derive(Clone, Debug, FromRow, Serialize)]
struct ReleasePresence {
    release_id: Uuid,
    version: String,
    deployed_at: DateTime<Utc>,
    presence: String,
    occurrence_count: Option<i64>,
    first_seen_at: Option<DateTime<Utc>>,
    last_seen_at: Option<DateTime<Utc>>,
    release_evidence_count: i64,
}

#[derive(Debug, Serialize)]
struct ReleasePresencePage {
    items: Vec<ReleasePresence>,
    next_cursor: Option<Uuid>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
struct InventorySighting {
    cluster_id: Uuid,
    namespace: String,
    workload_kind: String,
    workload_name: String,
    pod_uid: String,
    pod_name: String,
    container_name: String,
    occurrence_count: i64,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SightingCursor {
    last_seen_at: DateTime<Utc>,
    cluster_id: Uuid,
    namespace: String,
    workload_kind: String,
    workload_name: String,
    pod_uid: String,
    container_name: String,
}

#[derive(Debug, Serialize)]
struct SightingPage {
    items: Vec<InventorySighting>,
    next_cursor: Option<String>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
struct InventoryGroup {
    id: Uuid,
    cluster_id: Uuid,
    namespace: String,
    workload_kind: String,
    workload_name: String,
    event_kind: String,
    status: String,
    first_seen_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    occurrence_count: i64,
}

#[derive(Debug, Serialize)]
struct GroupPage {
    items: Vec<InventoryGroup>,
    next_cursor: Option<Uuid>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
struct InventoryOccurrence {
    id: Uuid,
    event_id: Uuid,
    observed_at: DateTime<Utc>,
    cluster_id: Uuid,
    node_name: String,
    namespace: String,
    pod_uid: String,
    pod_name: String,
    container_name: String,
    process_command: String,
    event_kind: String,
    payload: Value,
    release_id: Option<Uuid>,
    release_version: Option<String>,
}

#[derive(Debug, Serialize)]
struct OccurrencePage {
    items: Vec<InventoryOccurrence>,
    next_cursor: Option<Uuid>,
}

async fn principal(
    headers: &HeaderMap,
    state: &InventoryApiState,
) -> Result<ApiPrincipal, InventoryApiError> {
    let credential = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(InventoryApiError::Unauthorized)?;
    state
        .auth
        .authenticate(credential)
        .await?
        .ok_or(InventoryApiError::Unauthorized)
}

fn limit(value: Option<i64>) -> Result<i64, InventoryApiError> {
    let value = value.unwrap_or(50);
    if (1..=200).contains(&value) {
        Ok(value)
    } else {
        Err(InventoryApiError::Invalid(
            "limit must be between 1 and 200".into(),
        ))
    }
}

fn validate_kind(kind: Option<&str>) -> Result<(), InventoryApiError> {
    if kind.is_none_or(|value| matches!(value, "process" | "destination" | "domain" | "syscall")) {
        Ok(())
    } else {
        Err(InventoryApiError::Invalid(
            "kind must be process, destination, domain, or syscall".into(),
        ))
    }
}

fn validate_search(search: Option<&str>) -> Result<(), InventoryApiError> {
    if search.is_none_or(|value| !value.is_empty() && value.chars().count() <= 200) {
        Ok(())
    } else {
        Err(InventoryApiError::Invalid(
            "search must contain between 1 and 200 characters".into(),
        ))
    }
}

async fn ensure_application(
    pool: &PgPool,
    principal: ApiPrincipal,
    project_id: Uuid,
    application_id: Uuid,
) -> Result<(), InventoryApiError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM applications WHERE organization_id=$1 AND project_id=$2 AND id=$3)")
        .bind(principal.organization_id)
        .bind(project_id)
        .bind(application_id)
        .fetch_one(pool)
        .await?;
    exists.then_some(()).ok_or(InventoryApiError::NotFound)
}

async fn ensure_item(
    pool: &PgPool,
    principal: ApiPrincipal,
    project_id: Uuid,
    application_id: Uuid,
    item_id: Uuid,
) -> Result<(), InventoryApiError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runtime_inventory_items WHERE organization_id=$1 AND project_id=$2 AND application_id=$3 AND id=$4 AND identity_version=$5)")
        .bind(principal.organization_id)
        .bind(project_id)
        .bind(application_id)
        .bind(item_id)
        .bind(CURRENT_INVENTORY_IDENTITY_VERSION.get())
        .fetch_one(pool)
        .await?;
    exists.then_some(()).ok_or(InventoryApiError::NotFound)
}

async fn summary(
    State(state): State<InventoryApiState>,
    headers: HeaderMap,
    Path((project_id, application_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<InventorySummary>, InventoryApiError> {
    let started = Instant::now();
    let principal = principal(&headers, &state).await?;
    ensure_application(&state.pool, principal, project_id, application_id).await?;
    let version = CURRENT_INVENTORY_IDENTITY_VERSION.get();
    let aggregate: (i64, i64, Option<DateTime<Utc>>, Option<DateTime<Utc>>) =
        sqlx::query_as("SELECT count(*)::bigint,COALESCE(sum(occurrence_count),0)::bigint,min(first_seen_at),max(last_seen_at) FROM runtime_inventory_items WHERE organization_id=$1 AND project_id=$2 AND application_id=$3 AND identity_version=$4")
            .bind(principal.organization_id).bind(project_id).bind(application_id).bind(version)
            .fetch_one(&state.pool).await?;
    let kinds = sqlx::query_as::<_, KindCount>("SELECT inventory_kind kind,count(*)::bigint item_count,sum(occurrence_count)::bigint occurrence_count FROM runtime_inventory_items WHERE organization_id=$1 AND project_id=$2 AND application_id=$3 AND identity_version=$4 GROUP BY inventory_kind ORDER BY inventory_kind")
        .bind(principal.organization_id).bind(project_id).bind(application_id).bind(version)
        .fetch_all(&state.pool).await?;
    crate::metrics::record_inventory_query(
        u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    Ok(Json(InventorySummary {
        identity_version: version,
        item_count: aggregate.0,
        occurrence_count: aggregate.1,
        first_seen_at: aggregate.2,
        last_seen_at: aggregate.3,
        kinds,
    }))
}

#[allow(clippy::too_many_lines)]
async fn list_items(
    State(state): State<InventoryApiState>,
    headers: HeaderMap,
    Path((project_id, application_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<InventoryQuery>,
) -> Result<Json<InventoryItemPage>, InventoryApiError> {
    let started = Instant::now();
    let principal = principal(&headers, &state).await?;
    ensure_application(&state.pool, principal, project_id, application_id).await?;
    validate_kind(query.kind.as_deref())?;
    validate_search(query.search.as_deref())?;
    if query
        .observed_from
        .zip(query.observed_to)
        .is_some_and(|(from, to)| from > to)
    {
        return Err(InventoryApiError::Invalid(
            "observed_from must not be after observed_to".into(),
        ));
    }
    let limit = limit(query.limit)?;
    let cursor = if let Some(cursor) = query.cursor {
        Some(sqlx::query_as::<_, (DateTime<Utc>, Uuid)>("SELECT last_seen_at,id FROM runtime_inventory_items WHERE organization_id=$1 AND project_id=$2 AND application_id=$3 AND id=$4 AND identity_version=$5")
            .bind(principal.organization_id).bind(project_id).bind(application_id).bind(cursor).bind(CURRENT_INVENTORY_IDENTITY_VERSION.get())
            .fetch_optional(&state.pool).await?.ok_or_else(|| InventoryApiError::Invalid("cursor is invalid for this application".into()))?)
    } else {
        None
    };
    let (cursor_time, cursor_id) = cursor.map_or((None, None), |(time, id)| (Some(time), Some(id)));
    let search = query.search.as_ref().map(|value| format!("%{value}%"));
    let mut items = sqlx::query_as::<_, InventoryItem>(
        "SELECT i.id,i.project_id,i.application_id,i.inventory_kind,i.identity_version,i.semantic_summary,i.first_seen_at,i.last_seen_at,i.occurrence_count,(SELECT count(*) FROM runtime_inventory_releases r WHERE r.item_id=i.id) release_count,(SELECT count(DISTINCT s.cluster_id) FROM runtime_inventory_sightings s WHERE s.item_id=i.id) cluster_count,(SELECT count(DISTINCT (s.cluster_id,s.namespace)) FROM runtime_inventory_sightings s WHERE s.item_id=i.id) namespace_count,(SELECT count(DISTINCT (s.cluster_id,s.namespace,s.workload_kind,s.workload_name)) FROM runtime_inventory_sightings s WHERE s.item_id=i.id) workload_count,(SELECT count(DISTINCT (s.cluster_id,s.pod_uid)) FROM runtime_inventory_sightings s WHERE s.item_id=i.id) pod_count,(SELECT count(DISTINCT s.container_name) FROM runtime_inventory_sightings s WHERE s.item_id=i.id) container_count,(SELECT count(*) FROM runtime_inventory_group_links gl WHERE gl.item_id=i.id) group_count FROM runtime_inventory_items i WHERE i.organization_id=$1 AND i.project_id=$2 AND i.application_id=$3 AND i.identity_version=$4 AND ($5::text IS NULL OR i.inventory_kind=$5) AND ($6::uuid IS NULL OR EXISTS(SELECT 1 FROM runtime_inventory_releases r WHERE r.item_id=i.id AND r.release_id=$6)) AND ($7::uuid IS NULL OR EXISTS(SELECT 1 FROM runtime_inventory_sightings s WHERE s.item_id=i.id AND s.cluster_id=$7)) AND ($8::text IS NULL OR EXISTS(SELECT 1 FROM runtime_inventory_sightings s WHERE s.item_id=i.id AND s.namespace=$8)) AND ($9::text IS NULL OR EXISTS(SELECT 1 FROM runtime_inventory_sightings s WHERE s.item_id=i.id AND s.workload_kind=$9)) AND ($10::text IS NULL OR EXISTS(SELECT 1 FROM runtime_inventory_sightings s WHERE s.item_id=i.id AND s.workload_name=$10)) AND ($11::text IS NULL OR EXISTS(SELECT 1 FROM runtime_inventory_sightings s WHERE s.item_id=i.id AND s.container_name=$11)) AND ($12::timestamptz IS NULL OR i.last_seen_at >= $12) AND ($13::timestamptz IS NULL OR i.first_seen_at <= $13) AND ($14::text IS NULL OR concat_ws(' ',i.semantic_summary->>'executable',i.semantic_summary->>'process_command',i.semantic_summary->>'destination_address',i.semantic_summary->>'destination_port',i.semantic_summary->>'name',i.semantic_summary->>'query_type',i.semantic_summary->>'syscall') ILIKE $14) AND ($15::timestamptz IS NULL OR (i.last_seen_at,i.id)<($15,$16)) ORDER BY i.last_seen_at DESC,i.id DESC LIMIT $17",
    )
    .bind(principal.organization_id).bind(project_id).bind(application_id)
    .bind(CURRENT_INVENTORY_IDENTITY_VERSION.get()).bind(query.kind).bind(query.release_id).bind(query.cluster_id)
    .bind(query.namespace).bind(query.workload_kind).bind(query.workload_name).bind(query.container_name)
    .bind(query.observed_from).bind(query.observed_to).bind(search).bind(cursor_time).bind(cursor_id).bind(limit + 1)
    .fetch_all(&state.pool).await?;
    let next_cursor = if items.len() > usize::try_from(limit).unwrap_or(usize::MAX) {
        items.pop();
        items.last().map(|item| item.id)
    } else {
        None
    };
    crate::metrics::record_inventory_query(
        u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    Ok(Json(InventoryItemPage { items, next_cursor }))
}

async fn fetch_item(
    state: &InventoryApiState,
    principal: ApiPrincipal,
    project_id: Uuid,
    application_id: Uuid,
    item_id: Uuid,
) -> Result<InventoryItem, InventoryApiError> {
    sqlx::query_as::<_, InventoryItem>("SELECT i.id,i.project_id,i.application_id,i.inventory_kind,i.identity_version,i.semantic_summary,i.first_seen_at,i.last_seen_at,i.occurrence_count,(SELECT count(*) FROM runtime_inventory_releases r WHERE r.item_id=i.id) release_count,(SELECT count(DISTINCT s.cluster_id) FROM runtime_inventory_sightings s WHERE s.item_id=i.id) cluster_count,(SELECT count(DISTINCT (s.cluster_id,s.namespace)) FROM runtime_inventory_sightings s WHERE s.item_id=i.id) namespace_count,(SELECT count(DISTINCT (s.cluster_id,s.namespace,s.workload_kind,s.workload_name)) FROM runtime_inventory_sightings s WHERE s.item_id=i.id) workload_count,(SELECT count(DISTINCT (s.cluster_id,s.pod_uid)) FROM runtime_inventory_sightings s WHERE s.item_id=i.id) pod_count,(SELECT count(DISTINCT s.container_name) FROM runtime_inventory_sightings s WHERE s.item_id=i.id) container_count,(SELECT count(*) FROM runtime_inventory_group_links gl WHERE gl.item_id=i.id) group_count FROM runtime_inventory_items i WHERE i.organization_id=$1 AND i.project_id=$2 AND i.application_id=$3 AND i.id=$4 AND i.identity_version=$5")
        .bind(principal.organization_id).bind(project_id).bind(application_id).bind(item_id).bind(CURRENT_INVENTORY_IDENTITY_VERSION.get())
        .fetch_optional(&state.pool).await?.ok_or(InventoryApiError::NotFound)
}

async fn item_detail(
    State(state): State<InventoryApiState>,
    headers: HeaderMap,
    Path((project_id, application_id, item_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<InventoryItemDetail>, InventoryApiError> {
    let started = Instant::now();
    let principal = principal(&headers, &state).await?;
    let item = fetch_item(&state, principal, project_id, application_id, item_id).await?;
    let base = format!(
        "/api/v1/projects/{project_id}/applications/{application_id}/runtime-inventory/{item_id}"
    );
    crate::metrics::record_inventory_query(
        u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    Ok(Json(InventoryItemDetail {
        item,
        evidence: EvidenceLinks {
            releases: format!("{base}/releases"),
            sightings: format!("{base}/sightings"),
            groups: format!("{base}/groups"),
            occurrences: format!("{base}/occurrences"),
        },
    }))
}

async fn item_releases(
    State(state): State<InventoryApiState>,
    headers: HeaderMap,
    Path((project_id, application_id, item_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(query): Query<PageQuery>,
) -> Result<Json<ReleasePresencePage>, InventoryApiError> {
    let principal = principal(&headers, &state).await?;
    ensure_item(&state.pool, principal, project_id, application_id, item_id).await?;
    let limit = limit(query.limit)?;
    let cursor = if let Some(cursor) = query.cursor {
        Some(sqlx::query_as::<_, (DateTime<Utc>, Uuid)>("SELECT deployed_at,id FROM releases WHERE organization_id=$1 AND project_id=$2 AND application_id=$3 AND id=$4")
            .bind(principal.organization_id).bind(project_id).bind(application_id).bind(cursor)
            .fetch_optional(&state.pool).await?.ok_or_else(|| InventoryApiError::Invalid("release cursor is invalid".into()))?)
    } else {
        None
    };
    let (cursor_time, cursor_id) = cursor.map_or((None, None), |(time, id)| (Some(time), Some(id)));
    let mut items = sqlx::query_as::<_, ReleasePresence>("SELECT r.id release_id,r.version,r.deployed_at,CASE WHEN ir.release_id IS NOT NULL THEN 'observed' WHEN EXISTS(SELECT 1 FROM runtime_events e WHERE e.organization_id=r.organization_id AND e.project_id=r.project_id AND e.application_id=r.application_id AND e.release_id=r.id) THEN 'not_observed' ELSE 'unknown' END presence,ir.occurrence_count,ir.first_seen_at,ir.last_seen_at,(SELECT count(*) FROM runtime_events e WHERE e.organization_id=r.organization_id AND e.project_id=r.project_id AND e.application_id=r.application_id AND e.release_id=r.id) release_evidence_count FROM releases r LEFT JOIN runtime_inventory_releases ir ON ir.release_id=r.id AND ir.item_id=$4 WHERE r.organization_id=$1 AND r.project_id=$2 AND r.application_id=$3 AND ($5::timestamptz IS NULL OR (r.deployed_at,r.id)<($5,$6)) ORDER BY r.deployed_at DESC,r.id DESC LIMIT $7")
        .bind(principal.organization_id).bind(project_id).bind(application_id).bind(item_id).bind(cursor_time).bind(cursor_id).bind(limit + 1)
        .fetch_all(&state.pool).await?;
    let next_cursor = if items.len() > usize::try_from(limit).unwrap_or(usize::MAX) {
        items.pop();
        items.last().map(|item| item.release_id)
    } else {
        None
    };
    Ok(Json(ReleasePresencePage { items, next_cursor }))
}

fn encode_cursor<T: Serialize>(cursor: &T) -> Result<String, InventoryApiError> {
    serde_json::to_vec(cursor)
        .map(hex::encode)
        .map_err(|_| InventoryApiError::Invalid("cursor cannot be encoded".into()))
}

fn decode_cursor<T: DeserializeOwned>(cursor: &str) -> Result<T, InventoryApiError> {
    let bytes =
        hex::decode(cursor).map_err(|_| InventoryApiError::Invalid("cursor is invalid".into()))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| InventoryApiError::Invalid("cursor is invalid".into()))
}

async fn item_sightings(
    State(state): State<InventoryApiState>,
    headers: HeaderMap,
    Path((project_id, application_id, item_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(query): Query<StringCursorPageQuery>,
) -> Result<Json<SightingPage>, InventoryApiError> {
    let principal = principal(&headers, &state).await?;
    ensure_item(&state.pool, principal, project_id, application_id, item_id).await?;
    let limit = limit(query.limit)?;
    let cursor: Option<SightingCursor> = query.cursor.as_deref().map(decode_cursor).transpose()?;
    let mut items = sqlx::query_as::<_, InventorySighting>("SELECT cluster_id,namespace,workload_kind,workload_name,pod_uid,pod_name,container_name,occurrence_count,first_seen_at,last_seen_at FROM runtime_inventory_sightings WHERE organization_id=$1 AND project_id=$2 AND application_id=$3 AND item_id=$4 AND ($5::timestamptz IS NULL OR (last_seen_at,cluster_id,namespace,workload_kind,workload_name,pod_uid,container_name)<($5,$6,$7,$8,$9,$10,$11)) ORDER BY last_seen_at DESC,cluster_id DESC,namespace DESC,workload_kind DESC,workload_name DESC,pod_uid DESC,container_name DESC LIMIT $12")
        .bind(principal.organization_id).bind(project_id).bind(application_id).bind(item_id)
        .bind(cursor.as_ref().map(|value| value.last_seen_at)).bind(cursor.as_ref().map(|value| value.cluster_id))
        .bind(cursor.as_ref().map(|value| value.namespace.as_str())).bind(cursor.as_ref().map(|value| value.workload_kind.as_str()))
        .bind(cursor.as_ref().map(|value| value.workload_name.as_str())).bind(cursor.as_ref().map(|value| value.pod_uid.as_str()))
        .bind(cursor.as_ref().map(|value| value.container_name.as_str())).bind(limit + 1).fetch_all(&state.pool).await?;
    let next_cursor = if items.len() > usize::try_from(limit).unwrap_or(usize::MAX) {
        items.pop();
        items
            .last()
            .map(|item| {
                encode_cursor(&SightingCursor {
                    last_seen_at: item.last_seen_at,
                    cluster_id: item.cluster_id,
                    namespace: item.namespace.clone(),
                    workload_kind: item.workload_kind.clone(),
                    workload_name: item.workload_name.clone(),
                    pod_uid: item.pod_uid.clone(),
                    container_name: item.container_name.clone(),
                })
            })
            .transpose()?
    } else {
        None
    };
    Ok(Json(SightingPage { items, next_cursor }))
}

async fn item_groups(
    State(state): State<InventoryApiState>,
    headers: HeaderMap,
    Path((project_id, application_id, item_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(query): Query<PageQuery>,
) -> Result<Json<GroupPage>, InventoryApiError> {
    let principal = principal(&headers, &state).await?;
    ensure_item(&state.pool, principal, project_id, application_id, item_id).await?;
    let limit = limit(query.limit)?;
    let mut items = sqlx::query_as::<_, InventoryGroup>("SELECT g.id,g.cluster_id,g.namespace,g.workload_kind,g.workload_name,g.event_kind,g.status,g.first_seen_at,g.last_seen_at,g.occurrence_count FROM runtime_inventory_group_links l JOIN runtime_event_groups g ON g.id=l.group_id WHERE l.organization_id=$1 AND l.project_id=$2 AND l.application_id=$3 AND l.item_id=$4 AND ($5::uuid IS NULL OR g.id<$5) ORDER BY g.id DESC LIMIT $6")
        .bind(principal.organization_id).bind(project_id).bind(application_id).bind(item_id).bind(query.cursor).bind(limit + 1)
        .fetch_all(&state.pool).await?;
    let next_cursor = if items.len() > usize::try_from(limit).unwrap_or(usize::MAX) {
        items.pop();
        items.last().map(|item| item.id)
    } else {
        None
    };
    Ok(Json(GroupPage { items, next_cursor }))
}

async fn item_occurrences(
    State(state): State<InventoryApiState>,
    headers: HeaderMap,
    Path((project_id, application_id, item_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(query): Query<PageQuery>,
) -> Result<Json<OccurrencePage>, InventoryApiError> {
    let principal = principal(&headers, &state).await?;
    ensure_item(&state.pool, principal, project_id, application_id, item_id).await?;
    let limit = limit(query.limit)?;
    let cursor = if let Some(cursor) = query.cursor {
        Some(sqlx::query_as::<_, (DateTime<Utc>, Uuid)>("SELECT e.observed_at,e.id FROM runtime_inventory_event_memberships m JOIN runtime_events e ON e.id=m.event_id WHERE m.organization_id=$1 AND m.project_id=$2 AND m.application_id=$3 AND m.item_id=$4 AND e.id=$5")
            .bind(principal.organization_id).bind(project_id).bind(application_id).bind(item_id).bind(cursor)
            .fetch_optional(&state.pool).await?.ok_or_else(|| InventoryApiError::Invalid("occurrence cursor is invalid".into()))?)
    } else {
        None
    };
    let (cursor_time, cursor_id) = cursor.map_or((None, None), |(time, id)| (Some(time), Some(id)));
    let mut items = sqlx::query_as::<_, InventoryOccurrence>("SELECT e.id,e.event_id,e.observed_at,e.cluster_id,e.node_name,e.namespace,e.pod_uid,e.pod_name,e.container_name,e.process_command,e.event_kind,e.payload,e.release_id,r.version release_version FROM runtime_inventory_event_memberships m JOIN runtime_events e ON e.id=m.event_id LEFT JOIN releases r ON r.id=e.release_id WHERE m.organization_id=$1 AND m.project_id=$2 AND m.application_id=$3 AND m.item_id=$4 AND m.identity_version=$5 AND ($6::timestamptz IS NULL OR (e.observed_at,e.id)<($6,$7)) ORDER BY e.observed_at DESC,e.id DESC LIMIT $8")
        .bind(principal.organization_id).bind(project_id).bind(application_id).bind(item_id).bind(CURRENT_INVENTORY_IDENTITY_VERSION.get()).bind(cursor_time).bind(cursor_id).bind(limit + 1)
        .fetch_all(&state.pool).await?;
    let next_cursor = if items.len() > usize::try_from(limit).unwrap_or(usize::MAX) {
        items.pop();
        items.last().map(|item| item.id)
    } else {
        None
    };
    Ok(Json(OccurrencePage { items, next_cursor }))
}
