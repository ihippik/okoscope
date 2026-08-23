use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderName, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use tower_http::cors::{AllowOrigin, CorsLayer};
use url::Url;
use uuid::Uuid;

use crate::database::REQUIRED_MIGRATION;

pub const API_VERSION: &str = "v1";
pub const REQUEST_ID_HEADER: &str = "x-request-id";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WebApiConfig {
    pub cors_origins: Vec<String>,
}

impl WebApiConfig {
    pub fn new(origins: Vec<String>) -> Result<Self, String> {
        if origins.len() > 32 {
            return Err("at most 32 CORS origins are allowed".into());
        }
        let mut validated = Vec::with_capacity(origins.len());
        for value in origins {
            let value = value.trim();
            let url = Url::parse(value).map_err(|_| format!("invalid CORS origin {value:?}"))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
                || value.contains('*')
                || value == "null"
            {
                return Err(format!("unsafe CORS origin {value:?}"));
            }
            let normalized = url.origin().ascii_serialization();
            if !validated.contains(&normalized) {
                validated.push(normalized);
            }
        }
        Ok(Self {
            cors_origins: validated,
        })
    }
}

#[derive(Clone, Debug)]
pub struct RequestId(pub String);

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub error: &'static str,
    pub message: String,
    pub request_id: String,
}

pub fn error_response(
    status: StatusCode,
    code: &'static str,
    message: impl Into<String>,
    request_id: &RequestId,
) -> Response {
    (
        status,
        Json(ErrorEnvelope {
            error: code,
            message: message.into(),
            request_id: request_id.0.clone(),
        }),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
pub struct BuildInfo {
    service_version: &'static str,
    git_commit: &'static str,
    api_version: &'static str,
    required_database_migration: i64,
}

async fn build_info() -> Json<BuildInfo> {
    Json(build_info_value(option_env!("OKOSCOPE_GIT_COMMIT")))
}

fn build_info_value(git_commit: Option<&'static str>) -> BuildInfo {
    BuildInfo {
        service_version: env!("CARGO_PKG_VERSION"),
        git_commit: git_commit.unwrap_or("unknown"),
        api_version: API_VERSION,
        required_database_migration: REQUIRED_MIGRATION,
    }
}

async fn conventions(
    State(config): State<WebApiConfig>,
    mut request: Request,
    next: Next,
) -> Response {
    let started = std::time::Instant::now();
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| valid_request_id(value))
        .map_or_else(|| Uuid::new_v4().to_string(), str::to_owned);
    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));
    if let Some(origin) = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        && !config.cors_origins.iter().any(|allowed| allowed == origin)
    {
        crate::metrics::record_cors_denial();
        tracing::warn!(request_id=%request_id, "cross-origin request denied");
    }
    let mut response = next.run(request).await;
    crate::metrics::record_web_api(
        response.status().as_u16(),
        u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(REQUEST_ID_HEADER), value);
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    correlate_error(response, &RequestId(request_id)).await
}

async fn correlate_error(response: Response, request_id: &RequestId) -> Response {
    if !response.status().is_client_error() && !response.status().is_server_error() {
        return response;
    }
    let status = response.status();
    let (mut parts, body) = response.into_parts();
    let bytes = to_bytes(body, 1_048_576).await.unwrap_or_default();
    let mut value = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_else(|_| {
        serde_json::json!({
            "error": status.canonical_reason().unwrap_or("request_failed").to_ascii_lowercase().replace(' ', "_"),
            "message": status.canonical_reason().unwrap_or("request failed")
        })
    });
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "request_id".into(),
            serde_json::Value::String(request_id.0.clone()),
        );
    }
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(
        parts,
        Body::from(serde_json::to_vec(&value).unwrap_or_default()),
    )
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub fn router(api: Router, config: &WebApiConfig) -> Router {
    let origins = config
        .cors_origins
        .iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect::<Vec<_>>();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::OPTIONS])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static(REQUEST_ID_HEADER),
        ])
        .expose_headers([HeaderName::from_static(REQUEST_ID_HEADER)]);
    Router::new()
        .route("/api/v1/build-info", get(build_info))
        .merge(api)
        .layer(cors)
        .layer(middleware::from_fn_with_state(config.clone(), conventions))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, http::Request, routing::get};
    use tower::ServiceExt;

    #[test]
    fn validates_exact_origins() {
        assert_eq!(WebApiConfig::default().cors_origins, Vec::<String>::new());
        assert_eq!(
            WebApiConfig::new(vec!["https://ui.example.com".into()])
                .unwrap()
                .cors_origins,
            vec!["https://ui.example.com"]
        );
        for invalid in [
            "*",
            "null",
            "ftp://example.com",
            "https://user@example.com",
            "https://example.com/path",
            "https://example.com?q=1",
        ] {
            assert!(WebApiConfig::new(vec![invalid.into()]).is_err());
        }
    }

    #[test]
    fn validates_request_identifiers() {
        assert!(valid_request_id("request-123.test"));
        assert!(!valid_request_id(""));
        assert!(!valid_request_id("bad request"));
        assert!(!valid_request_id(&"x".repeat(129)));
    }

    #[test]
    fn serializes_common_types_without_secrets() {
        let request_id = RequestId("test-1".into());
        let response = error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "bad",
            &request_id,
        );
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let info = BuildInfo {
            service_version: "1",
            git_commit: "unknown",
            api_version: "v1",
            required_database_migration: 13,
        };
        let value = serde_json::to_value(info).unwrap();
        assert_eq!(value["git_commit"], "unknown");
        assert_eq!(value.as_object().unwrap().len(), 4);
        assert_eq!(build_info_value(None).git_commit, "unknown");
        assert_eq!(build_info_value(Some("abc123")).git_commit, "abc123");
    }

    #[tokio::test]
    async fn request_ids_are_supplied_generated_and_isolated() {
        let app = router(
            Router::new().route(
                "/api/v1/test",
                get(|| async { Json(serde_json::json!({"ok": true})) }),
            ),
            &WebApiConfig::default(),
        );
        let supplied = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/test")
                    .header(REQUEST_ID_HEADER, "client-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(supplied.headers()[REQUEST_ID_HEADER], "client-1");
        assert_eq!(supplied.headers()[header::CACHE_CONTROL], "no-store");
        let malformed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/missing")
                    .header(REQUEST_ID_HEADER, "bad id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let generated = malformed.headers()[REQUEST_ID_HEADER]
            .to_str()
            .unwrap()
            .to_owned();
        assert_ne!(generated, "bad id");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &to_bytes(malformed.into_body(), usize::MAX).await.unwrap()
            )
            .unwrap()["request_id"],
            generated
        );
        let (one, two) = tokio::join!(
            app.clone().oneshot(
                Request::builder()
                    .uri("/api/v1/test")
                    .body(Body::empty())
                    .unwrap()
            ),
            app.oneshot(
                Request::builder()
                    .uri("/api/v1/test")
                    .body(Body::empty())
                    .unwrap()
            )
        );
        assert_ne!(
            one.unwrap().headers()[REQUEST_ID_HEADER],
            two.unwrap().headers()[REQUEST_ID_HEADER]
        );
    }

    #[tokio::test]
    async fn cors_is_exact_bounded_and_does_not_authenticate() {
        let config = WebApiConfig::new(vec!["https://ui.example.com".into()]).unwrap();
        let app = router(
            Router::new().route("/api/v1/test", get(|| async { StatusCode::UNAUTHORIZED })),
            &config,
        );
        let allowed = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/v1/test")
                    .header(header::ORIGIN, "https://ui.example.com")
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .header(
                        header::ACCESS_CONTROL_REQUEST_HEADERS,
                        "authorization,x-request-id",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            allowed.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            "https://ui.example.com"
        );
        assert!(
            allowed
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                .is_none()
        );
        for (method, request_headers, forbidden) in [
            ("DELETE", None, "DELETE"),
            ("GET", Some("cookie"), "cookie"),
        ] {
            let mut request = Request::builder()
                .method(Method::OPTIONS)
                .uri("/api/v1/test")
                .header(header::ORIGIN, "https://ui.example.com")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, method);
            if let Some(value) = request_headers {
                request = request.header(header::ACCESS_CONTROL_REQUEST_HEADERS, value);
            }
            let denied = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            let permission_headers = denied
                .headers()
                .values()
                .filter_map(|value| value.to_str().ok())
                .collect::<Vec<_>>()
                .join(",");
            assert!(
                !permission_headers
                    .to_ascii_lowercase()
                    .contains(&forbidden.to_ascii_lowercase())
            );
        }
        let unauthenticated = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/test")
                    .header(header::ORIGIN, "https://ui.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    }
}
