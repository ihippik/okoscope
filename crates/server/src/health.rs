use std::sync::Arc;

use axum::{Router, http::StatusCode, routing::get};
use sqlx::PgPool;

use crate::{api, database::verify_schema, metrics, notification::NotificationService};

pub fn router(
    pool: PgPool,
    notification_ready: bool,
    notifications: Option<NotificationService>,
) -> Router {
    let pool = Arc::new(pool);
    let router = Router::new()
        .route("/healthz", get(|| async { StatusCode::OK }))
        .route(
            "/readyz",
            get({
                let pool = pool.clone();
                move || {
                    let pool = pool.clone();
                    async move {
                        if notification_ready && verify_schema(&pool).await.is_ok() {
                            StatusCode::OK
                        } else {
                            StatusCode::SERVICE_UNAVAILABLE
                        }
                    }
                }
            }),
        )
        .merge(api::router((*pool).clone()))
        .merge(crate::releases::router((*pool).clone()))
        .merge(metrics::router((*pool).clone()));
    if let Some(notifications) = notifications {
        router.merge(crate::notification::api::router(
            (*pool).clone(),
            notifications,
        ))
    } else {
        router
    }
}
