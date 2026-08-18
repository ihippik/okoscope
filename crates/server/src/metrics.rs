use std::sync::atomic::{AtomicU64, Ordering};

use axum::{Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use sqlx::PgPool;

use crate::notification::health::{derive_state, load_global_snapshot};

static GROUPING_COUNT: AtomicU64 = AtomicU64::new(0);
static GROUPING_MICROSECONDS: AtomicU64 = AtomicU64::new(0);
static GROUPS_CREATED: AtomicU64 = AtomicU64::new(0);
static DUPLICATE_EVENTS: AtomicU64 = AtomicU64::new(0);
static NETWORK_EVENTS_ACCEPTED: AtomicU64 = AtomicU64::new(0);
static NETWORK_GROUPS_CREATED: AtomicU64 = AtomicU64::new(0);
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
static NOTIFICATION_CYCLE_FAILURES: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_DRAINS: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_DRAIN_TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_RETENTION_ENABLED: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_RETENTION_SUCCESSES: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_RETENTION_FAILURES: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_RETENTION_OPERATIONS_DELETED: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_RETENTION_DELIVERIES_DELETED: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_RETENTION_LAST_SUCCESS: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_RETENTION_DURATION_MICROSECONDS: AtomicU64 = AtomicU64::new(0);
static NOTIFICATION_WORKER_RUNTIME_STATE: AtomicU64 = AtomicU64::new(1);
static RELEASE_ATTRIBUTED: AtomicU64 = AtomicU64::new(0);
static RELEASE_ABSENT: AtomicU64 = AtomicU64::new(0);
static RELEASE_UNKNOWN: AtomicU64 = AtomicU64::new(0);
static RELEASE_SUMMARY_UPDATES: AtomicU64 = AtomicU64::new(0);
static RELEASE_DIFF_REQUESTS: AtomicU64 = AtomicU64::new(0);
static NAVIGATION_REQUESTS: AtomicU64 = AtomicU64::new(0);
static API_ERRORS: AtomicU64 = AtomicU64::new(0);
static API_CLIENT_ERRORS: AtomicU64 = AtomicU64::new(0);
static API_SERVER_ERRORS: AtomicU64 = AtomicU64::new(0);
static CORS_DENIALS: AtomicU64 = AtomicU64::new(0);
static WEB_API_DURATION_MICROSECONDS: AtomicU64 = AtomicU64::new(0);

pub fn record_grouping(elapsed_micros: u64, group_created: bool) {
    GROUPING_COUNT.fetch_add(1, Ordering::Relaxed);
    GROUPING_MICROSECONDS.fetch_add(elapsed_micros, Ordering::Relaxed);
    GROUPS_CREATED.fetch_add(u64::from(group_created), Ordering::Relaxed);
}

pub fn record_duplicate_event() {
    DUPLICATE_EVENTS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_network_event(group_created: bool) {
    NETWORK_EVENTS_ACCEPTED.fetch_add(1, Ordering::Relaxed);
    NETWORK_GROUPS_CREATED.fetch_add(u64::from(group_created), Ordering::Relaxed);
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

pub fn record_notification_cycle_failure() {
    NOTIFICATION_CYCLE_FAILURES.fetch_add(1, Ordering::Relaxed);
    NOTIFICATION_WORKER_RUNTIME_STATE.store(4, Ordering::Relaxed);
}

pub fn record_notification_cycle_success() {
    NOTIFICATION_WORKER_RUNTIME_STATE.store(1, Ordering::Relaxed);
}

pub fn record_notification_drain_started() {
    NOTIFICATION_WORKER_RUNTIME_STATE.store(5, Ordering::Relaxed);
}

pub fn record_notification_drain(completed: bool) {
    NOTIFICATION_DRAINS.fetch_add(1, Ordering::Relaxed);
    NOTIFICATION_DRAIN_TIMEOUTS.fetch_add(u64::from(!completed), Ordering::Relaxed);
}

pub fn notification_worker_is_draining() -> bool {
    NOTIFICATION_WORKER_RUNTIME_STATE.load(Ordering::Relaxed) == 5
}

pub fn configure_notification_retention(enabled: bool) {
    NOTIFICATION_RETENTION_ENABLED.store(u64::from(enabled), Ordering::Relaxed);
}

pub fn record_notification_retention_success(
    stats: crate::notification::retention::RetentionStats,
) {
    NOTIFICATION_RETENTION_SUCCESSES.fetch_add(1, Ordering::Relaxed);
    NOTIFICATION_RETENTION_OPERATIONS_DELETED
        .fetch_add(stats.recovery_operations_deleted, Ordering::Relaxed);
    NOTIFICATION_RETENTION_DELIVERIES_DELETED
        .fetch_add(stats.terminal_deliveries_deleted, Ordering::Relaxed);
    NOTIFICATION_RETENTION_DURATION_MICROSECONDS
        .fetch_add(stats.duration_micros, Ordering::Relaxed);
    NOTIFICATION_RETENTION_LAST_SUCCESS.store(
        u64::try_from(chrono::Utc::now().timestamp()).unwrap_or_default(),
        Ordering::Relaxed,
    );
}

pub fn record_notification_retention_failure() {
    NOTIFICATION_RETENTION_FAILURES.fetch_add(1, Ordering::Relaxed);
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

pub fn record_navigation(_success: bool) {
    NAVIGATION_REQUESTS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_web_api(status: u16, elapsed_micros: u64) {
    API_ERRORS.fetch_add(u64::from(status >= 400), Ordering::Relaxed);
    API_CLIENT_ERRORS.fetch_add(u64::from((400..500).contains(&status)), Ordering::Relaxed);
    API_SERVER_ERRORS.fetch_add(u64::from(status >= 500), Ordering::Relaxed);
    WEB_API_DURATION_MICROSECONDS.fetch_add(elapsed_micros, Ordering::Relaxed);
}

pub fn record_cors_denial() {
    CORS_DENIALS.fetch_add(1, Ordering::Relaxed);
}

#[derive(Clone)]
struct MetricsState {
    pool: PgPool,
    notification_enabled: bool,
}

pub fn router(pool: PgPool, notification_enabled: bool) -> Router {
    Router::new()
        .route("/metrics", get(render))
        .with_state(MetricsState {
            pool,
            notification_enabled,
        })
}

#[allow(clippy::too_many_lines)]
async fn render(State(state): State<MetricsState>) -> impl IntoResponse {
    let pool = &state.pool;
    let outbox_depth = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM outbox_messages WHERE processed_at IS NULL",
    )
    .fetch_one(pool)
    .await;
    let Ok(outbox_depth) = outbox_depth else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "database metrics unavailable\n".to_owned(),
        );
    };
    let Ok(notification) = load_global_snapshot(pool).await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "database metrics unavailable\n".to_owned(),
        );
    };
    let runtime_state = NOTIFICATION_WORKER_RUNTIME_STATE.load(Ordering::Relaxed);
    let mut worker_state = derive_state(
        state.notification_enabled,
        runtime_state == 5,
        &notification,
    );
    if runtime_state == 4 && state.notification_enabled {
        worker_state = crate::notification::health::NotificationHealthState::Failing;
    }
    let release_summary_count =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM runtime_event_group_releases")
            .fetch_one(pool)
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
            "okoscope_network_connect_events_accepted_total",
            NETWORK_EVENTS_ACCEPTED.load(Ordering::Relaxed),
        ),
        (
            "okoscope_network_connect_groups_created_total",
            NETWORK_GROUPS_CREATED.load(Ordering::Relaxed),
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
            "okoscope_notification_worker_enabled",
            u64::from(state.notification_enabled),
        ),
        (
            "okoscope_notification_worker_state",
            worker_state.metric_code(),
        ),
        (
            "okoscope_notification_enabled_destinations",
            u64::try_from(notification.enabled_destination_count).unwrap_or_default(),
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
            u64::try_from(notification.pending_count).unwrap_or_default(),
        ),
        (
            "okoscope_notification_due",
            u64::try_from(notification.due_count).unwrap_or_default(),
        ),
        (
            "okoscope_notification_retrying",
            u64::try_from(notification.retrying_count).unwrap_or_default(),
        ),
        (
            "okoscope_notification_oldest_due_seconds",
            u64::try_from(notification.oldest_due_age_seconds.unwrap_or_default())
                .unwrap_or_default(),
        ),
        (
            "okoscope_notification_in_flight",
            u64::try_from(notification.in_flight_count).unwrap_or_default(),
        ),
        (
            "okoscope_notification_expired_leases",
            u64::try_from(notification.expired_lease_count).unwrap_or_default(),
        ),
        (
            "okoscope_notification_failed",
            u64::try_from(notification.failed_count).unwrap_or_default(),
        ),
        (
            "okoscope_notification_cycle_failures_total",
            NOTIFICATION_CYCLE_FAILURES.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_drains_total",
            NOTIFICATION_DRAINS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_drain_timeouts_total",
            NOTIFICATION_DRAIN_TIMEOUTS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_retention_enabled",
            NOTIFICATION_RETENTION_ENABLED.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_retention_successes_total",
            NOTIFICATION_RETENTION_SUCCESSES.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_retention_failures_total",
            NOTIFICATION_RETENTION_FAILURES.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_retention_operations_deleted_total",
            NOTIFICATION_RETENTION_OPERATIONS_DELETED.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_retention_deliveries_deleted_total",
            NOTIFICATION_RETENTION_DELIVERIES_DELETED.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_retention_last_success_timestamp_seconds",
            NOTIFICATION_RETENTION_LAST_SUCCESS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_notification_retention_duration_microseconds_total",
            NOTIFICATION_RETENTION_DURATION_MICROSECONDS.load(Ordering::Relaxed),
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
        (
            "okoscope_navigation_requests_total",
            NAVIGATION_REQUESTS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_api_errors_total",
            API_ERRORS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_api_client_errors_total",
            API_CLIENT_ERRORS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_api_server_errors_total",
            API_SERVER_ERRORS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_cors_denials_total",
            CORS_DENIALS.load(Ordering::Relaxed),
        ),
        (
            "okoscope_web_api_duration_microseconds_total",
            WEB_API_DURATION_MICROSECONDS.load(Ordering::Relaxed),
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
