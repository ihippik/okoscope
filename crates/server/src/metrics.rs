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
static NOTIFICATION_MATERIALIZED: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_SUPPRESSED: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_NO_DESTINATIONS: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_CLAIMS: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_RETRIES: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_TERMINAL_FAILURES: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_DURATION_MICROSECONDS: AtomicU64 = AtomicU64::new(0);
static RELEASE_ATTRIBUTED: AtomicU64 = AtomicU64::new(0);
static RELEASE_ABSENT: AtomicU64 = AtomicU64::new(0);
static RELEASE_UNKNOWN: AtomicU64 = AtomicU64::new(0);
static RELEASE_SUMMARY_UPDATES: AtomicU64 = AtomicU64::new(0);
static RELEASE_DIFF_REQUESTS: AtomicU64 = AtomicU64::new(0);

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

pub fn record_notification_materialization(deliveries: u64, suppressed: u64, no_destinations: u64) {
    NOTIFICATION_MATERIALIZED.fetch_add(deliveries, Ordering::Relaxed);
    NOTIFICATION_SUPPRESSED.fetch_add(suppressed, Ordering::Relaxed);
    NOTIFICATION_NO_DESTINATIONS.fetch_add(no_destinations, Ordering::Relaxed);
}

pub fn record_notification_claims(count: usize) {
    NOTIFICATION_CLAIMS.fetch_add(u64::try_from(count).unwrap_or(u64::MAX), Ordering::Relaxed);
}

pub fn record_notification_attempt(duration_micros: u64, retry: bool, terminal_failure: bool) {
    NOTIFICATION_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    NOTIFICATION_DURATION_MICROSECONDS.fetch_add(duration_micros, Ordering::Relaxed);
    NOTIFICATION_RETRIES.fetch_add(u64::from(retry), Ordering::Relaxed);
    NOTIFICATION_TERMINAL_FAILURES.fetch_add(u64::from(terminal_failure), Ordering::Relaxed);
}

pub fn record_release_attribution(provided: bool, resolved: bool) {
    match (provided, resolved) {
        (_, true) => &RELEASE_ATTRIBUTED,
        (true, false) => &RELEASE_UNKNOWN,
        (false, false) => &RELEASE_ABSENT,
    }
    .fetch_add(1, Ordering::Relaxed);
}

pub fn record_release_summary() {
    RELEASE_SUMMARY_UPDATES.fetch_add(1, Ordering::Relaxed);
}

pub fn record_release_diff() {
    RELEASE_DIFF_REQUESTS.fetch_add(1, Ordering::Relaxed);
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/metrics", get(render))
        .with_state(pool)
}

#[allow(clippy::too_many_lines)]
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
    let delivery_depth = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT count(*) FILTER (WHERE status='pending'), count(*) FILTER (WHERE status='in_flight'), count(*) FILTER (WHERE status='in_flight' AND lease_expires_at<=now()) FROM notification_deliveries",
    )
    .fetch_one(&pool)
    .await;
    let Ok((pending_deliveries, in_flight_deliveries, expired_leases)) = delivery_depth else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "database metrics unavailable\n".to_owned(),
        );
    };
    let release_summary_count =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM runtime_event_group_releases")
            .fetch_one(&pool)
            .await;
    let Ok(release_summary_count) = release_summary_count else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "database metrics unavailable\n".to_owned(),
        );
    };
    let metrics = [
        (
            "okoscope_grouping_operations_total",
            GROUPING_COUNT.load(Ordering::Relaxed),
        ),
        (
            "okoscope_grouping_duration_microseconds_total",
            GROUPING_MICROSECONDS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_runtime_groups_created_total",
            GROUPS_CREATED.load(Ordering::Relaxed),
        ),
        (
            "okoscope_ingestion_duplicate_events_total",
            DUPLICATE_EVENTS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_api_requests_total",
            API_REQUESTS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_backfill_scanned",
            BACKFILL_SCANNED.load(Ordering::Relaxed),
        ),
        (
            "okoscope_backfill_grouped",
            BACKFILL_GROUPED.load(Ordering::Relaxed),
        ),
        (
            "okoscope_outbox_pending",
            u64::try_from(outbox_depth).unwrap_or_default(),
        ),
        (
            "okoscope_notification_materialized_total",
            NOTIFICATION_MATERIALIZED.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_suppressed_total",
            NOTIFICATION_SUPPRESSED.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_no_destinations_total",
            NOTIFICATION_NO_DESTINATIONS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_claims_total",
            NOTIFICATION_CLAIMS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_attempts_total",
            NOTIFICATION_ATTEMPTS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_retries_total",
            NOTIFICATION_RETRIES.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_terminal_failures_total",
            NOTIFICATION_TERMINAL_FAILURES.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_duration_microseconds_total",
            NOTIFICATION_DURATION_MICROSECONDS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_pending",
            u64::try_from(pending_deliveries).unwrap_or_default(),
        ),
        (
            "okoscope_notification_in_flight",
            u64::try_from(in_flight_deliveries).unwrap_or_default(),
        ),
        (
            "okoscope_notification_expired_leases",
            u64::try_from(expired_leases).unwrap_or_default(),
        ),
        (
            "okoscope_release_attributed_total",
            RELEASE_ATTRIBUTED.load(Ordering::Relaxed),
        ),
        (
            "okoscope_release_absent_total",
            RELEASE_ABSENT.load(Ordering::Relaxed),
        ),
        (
            "okoscope_release_unknown_total",
            RELEASE_UNKNOWN.load(Ordering::Relaxed),
        ),
        (
            "okoscope_release_summary_updates_total",
            RELEASE_SUMMARY_UPDATES.load(Ordering::Relaxed),
        ),
        (
            "okoscope_release_summaries",
            u64::try_from(release_summary_count).unwrap_or_default(),
        ),
        (
            "okoscope_release_diff_requests_total",
            RELEASE_DIFF_REQUESTS.load(Ordering::Relaxed),
        ),
    ];
    let mut body = String::new();
    for (name, value) in metrics {
        body.push_str("# TYPE ");
        body.push_str(name);
        body.push_str(" gauge\n");
        body.push_str(name);
        body.push(' ');
        body.push_str(&value.to_string());
        body.push('\n');
    }
    (StatusCode::OK, body)
}
