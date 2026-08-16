use std::sync::Arc;

use axum::{Router, http::StatusCode, routing::get};
use sqlx::PgPool;

use crate::{api, database::verify_schema, metrics};

pub fn router(pool: PgPool) -> Router {
    let pool = Arc::new(pool);
    Router::new()
        .route("/healthz", get(|| async { StatusCode::OK }))
        .route(
            "/readyz",
            get({
                let pool = pool.clone();
                move || {
                    let pool = pool.clone();
                    async move {
                        if verify_schema(&pool).await.is_ok() {
                            StatusCode::OK
                        } else {
                            StatusCode::SERVICE_UNAVAILABLE
                        }
                    }
                }
            }),
        )
        .merge(api::router((*pool).clone()))
        .merge(metrics::router((*pool).clone()))
}
