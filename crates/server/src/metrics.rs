use std::sync::atomic::{AtomicU64, Ordering};

use axum::{Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use sqlx::PgPool;

static GROUPING_COUNT: AtomicU64 = AtomicU64::new(0);
static GROUPING_MICROSECONDS: AtomicU64 = AtomicU64::new(0);
static GROUPS_CREATED: AtomicU64 = AtomicU64::new(0);
static DUPLICATE_EVENTS: AtomicU64 = AtomicU64::new(0);
static API_REQUESTS: AtomicU64 = AtomicU64::new(0);
static BACKFILL_SCANNED: AtomicU64 = AtomicU64::new(0);
static BACKFILL_GROUPED: AtomicU64 = AtomicU64::new(0);

pub fn record_grouping(elapsed_micros: u64, group_created: bool) {
    GROUPING_COUNT.fetch_add(1, Ordering::Relaxed);
    GROUPING_MICROSECONDS.fetch_add(elapsed_micros, Ordering::Relaxed);
    GROUPS_CREATED.fetch_add(u64::from(group_created), Ordering::Relaxed);
}

pub fn record_duplicate_event() {
    DUPLICATE_EVENTS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_api_request() {
    API_REQUESTS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_backfill(scanned: u64, grouped: u64) {
    BACKFILL_SCANNED.store(scanned, Ordering::Relaxed);
    BACKFILL_GROUPED.store(grouped, Ordering::Relaxed);
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/metrics", get(render))
        .with_state(pool)
}

async fn render(State(pool): State<PgPool>) -> impl IntoResponse {
    let outbox_depth = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM outbox_messages WHERE processed_at IS NULL",
    )
    .fetch_one(&pool)
    .await;
    let Ok(outbox_depth) = outbox_depth else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "database metrics unavailable\n".to_owned(),
        );
    };
    let body = format!(
        "# TYPE okoscope_grouping_operations_total counter\n\
okoscope_grouping_operations_total {}\n\
# TYPE okoscope_grouping_duration_microseconds_total counter\n\
okoscope_grouping_duration_microseconds_total {}\n\
# TYPE okoscope_runtime_groups_created_total counter\n\
okoscope_runtime_groups_created_total {}\n\
# TYPE okoscope_ingestion_duplicate_events_total counter\n\
okoscope_ingestion_duplicate_events_total {}\n\
# TYPE okoscope_api_requests_total counter\n\
okoscope_api_requests_total {}\n\
# TYPE okoscope_backfill_scanned gauge\n\
okoscope_backfill_scanned {}\n\
# TYPE okoscope_backfill_grouped gauge\n\
okoscope_backfill_grouped {}\n\
# TYPE okoscope_outbox_pending gauge\n\
okoscope_outbox_pending {outbox_depth}\n",
        GROUPING_COUNT.load(Ordering::Relaxed),
        GROUPING_MICROSECONDS.load(Ordering::Relaxed),
        GROUPS_CREATED.load(Ordering::Relaxed),
        DUPLICATE_EVENTS.load(Ordering::Relaxed),
        API_REQUESTS.load(Ordering::Relaxed),
        BACKFILL_SCANNED.load(Ordering::Relaxed),
        BACKFILL_GROUPED.load(Ordering::Relaxed),
    );
    (StatusCode::OK, body)
}
