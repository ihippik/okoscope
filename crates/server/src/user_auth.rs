use axum::{
    Extension, Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::{
        AuthenticatedUser, OrganizationRole, SESSION_COOKIE, SessionToken,
        UserSessionAuthenticator, hash_password, normalize_email, session_digest, session_token,
        validate_password, verify_password,
    },
    web_api::{RequestId, WebApiConfig},
};

#[derive(Clone, Debug)]
struct AuthState {
    pool: PgPool,
    authenticator: UserSessionAuthenticator,
    registration_enabled: bool,
    secure_cookie: bool,
    session_lifetime: std::time::Duration,
}

pub fn router(pool: PgPool, config: &WebApiConfig) -> Router {
    Router::new()
        .route("/api/v1/auth/register", post(register))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/auth/logout", post(logout))
        .with_state(AuthState {
            authenticator: UserSessionAuthenticator::new(pool.clone()),
            pool,
            registration_enabled: config.registration_enabled,
            secure_cookie: config.secure_session_cookie,
            session_lifetime: config.session_lifetime,
        })
}

pub async fn bootstrap_owner(
    pool: &PgPool,
    organization_id: Uuid,
    email: &str,
    password: &str,
) -> anyhow::Result<()> {
    let email = normalize_email(email).map_err(anyhow::Error::msg)?;
    validate_password(password).map_err(anyhow::Error::msg)?;
    let password_hash =
        hash_password(password).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let mut tx = pool.begin().await?;
    let existing_owner: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM organization_memberships WHERE organization_id=$1 AND role='owner')",
    )
    .bind(organization_id)
    .fetch_one(&mut *tx)
    .await?;
    if existing_owner {
        tx.commit().await?;
        return Ok(());
    }
    let organization_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM organizations WHERE id=$1)")
            .bind(organization_id)
            .fetch_one(&mut *tx)
            .await?;
    anyhow::ensure!(organization_exists, "organization does not exist");
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users(id,email,password_hash) VALUES($1,$2,$3) ON CONFLICT(email) DO UPDATE SET email=EXCLUDED.email RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(email)
    .bind(password_hash)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO organization_memberships(organization_id,user_id,role) VALUES($1,$2,'owner') ON CONFLICT(organization_id,user_id) DO UPDATE SET role='owner'")
        .bind(organization_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn verify_user_access(
    pool: &PgPool,
    registration_enabled: bool,
    setup_enabled: bool,
) -> anyhow::Result<()> {
    let owner_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM organization_memberships WHERE role='owner'")
            .fetch_one(pool)
            .await?;
    anyhow::ensure!(
        registration_enabled || setup_enabled || owner_count > 0,
        "no Organization owner exists; configure setup authorization, run bootstrap-owner, or explicitly enable registration"
    );
    Ok(())
}

#[derive(Debug)]
struct AuthError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    request_id: RequestId,
}

impl AuthError {
    fn new(
        status: StatusCode,
        code: &'static str,
        message: &'static str,
        request_id: &RequestId,
    ) -> Self {
        Self {
            status,
            code,
            message,
            request_id: request_id.clone(),
        }
    }

    fn internal(error: &impl std::fmt::Display, request_id: &RequestId) -> Self {
        tracing::error!(%error, request_id=%request_id.0, "user authentication database failure");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "internal server error",
            request_id,
        )
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct ErrorBody {
            error: &'static str,
            message: &'static str,
            request_id: String,
        }
        (
            self.status,
            Json(ErrorBody {
                error: self.code,
                message: self.message,
                request_id: self.request_id.0,
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterRequest {
    email: String,
    password: String,
    organization_slug: String,
    organization_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct AuthResponse {
    user: UserResponse,
    organization: OrganizationResponse,
    role: OrganizationRole,
}

#[derive(Debug, Serialize)]
struct UserResponse {
    id: Uuid,
    email: String,
}

#[derive(Debug, Serialize)]
struct OrganizationResponse {
    id: Uuid,
    slug: String,
    name: String,
}

pub(crate) fn valid_slug(value: &str) -> bool {
    (1..=63).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

pub(crate) fn valid_name(value: &str) -> bool {
    value.trim() == value && (1..=120).contains(&value.chars().count())
}

pub(crate) fn session_cookie(
    token: &str,
    secure: bool,
    max_age: std::time::Duration,
) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={token}; HttpOnly{secure}; SameSite=Lax; Path=/; Max-Age={}",
        max_age.as_secs()
    ))
    .expect("generated session cookie is a valid header")
}

fn expired_cookie(secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}=; HttpOnly{secure}; SameSite=Lax; Path=/; Max-Age=0"
    ))
    .expect("generated expired cookie is a valid header")
}

pub(crate) async fn insert_session(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    organization_id: Uuid,
    lifetime: std::time::Duration,
) -> Result<(Uuid, SessionToken), sqlx::Error> {
    let session_id = Uuid::new_v4();
    let token = SessionToken::generate();
    let expires_at =
        Utc::now() + Duration::from_std(lifetime).unwrap_or_else(|_| Duration::hours(12));
    sqlx::query("INSERT INTO user_sessions(id,user_id,organization_id,token_hash,expires_at) VALUES($1,$2,$3,$4,$5)")
        .bind(session_id)
        .bind(user_id)
        .bind(organization_id)
        .bind(token.digest().to_vec())
        .bind(expires_at)
        .execute(&mut **tx)
        .await?;
    Ok((session_id, token))
}

#[allow(clippy::too_many_lines)]
async fn register(
    State(state): State<AuthState>,
    Extension(request_id): Extension<RequestId>,
    Json(input): Json<RegisterRequest>,
) -> Result<Response, AuthError> {
    if !state.registration_enabled {
        crate::metrics::record_authentication(false);
        return Err(AuthError::new(
            StatusCode::NOT_FOUND,
            "registration_disabled",
            "registration is disabled",
            &request_id,
        ));
    }
    let email = normalize_email(&input.email).map_err(|message| {
        AuthError::new(
            StatusCode::BAD_REQUEST,
            "validation_failed",
            message,
            &request_id,
        )
    })?;
    validate_password(&input.password).map_err(|message| {
        AuthError::new(
            StatusCode::BAD_REQUEST,
            "validation_failed",
            message,
            &request_id,
        )
    })?;
    if !valid_slug(&input.organization_slug) || !valid_name(&input.organization_name) {
        return Err(AuthError::new(
            StatusCode::BAD_REQUEST,
            "validation_failed",
            "organization slug or name is invalid",
            &request_id,
        ));
    }
    let password_hash =
        hash_password(&input.password).map_err(|error| AuthError::internal(&error, &request_id))?;
    let user_id = Uuid::new_v4();
    let organization_id = Uuid::new_v4();
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| AuthError::internal(&error, &request_id))?;
    let result = async {
        sqlx::query("INSERT INTO users(id,email,password_hash) VALUES($1,$2,$3)")
            .bind(user_id)
            .bind(&email)
            .bind(password_hash)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO organizations(id,slug,name) VALUES($1,$2,$3)")
            .bind(organization_id)
            .bind(&input.organization_slug)
            .bind(&input.organization_name)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO organization_memberships(organization_id,user_id,role) VALUES($1,$2,'owner')")
            .bind(organization_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        let (_, token) = insert_session(
            &mut tx,
            user_id,
            organization_id,
            state.session_lifetime,
        )
        .await?;
        Ok::<_, sqlx::Error>(token)
    }
    .await;
    let token = match result {
        Ok(token) => token,
        Err(error)
            if error
                .as_database_error()
                .is_some_and(sqlx::error::DatabaseError::is_unique_violation) =>
        {
            return Err(AuthError::new(
                StatusCode::CONFLICT,
                "registration_conflict",
                "registration identity already exists",
                &request_id,
            ));
        }
        Err(error) => return Err(AuthError::internal(&error, &request_id)),
    };
    tx.commit()
        .await
        .map_err(|error| AuthError::internal(&error, &request_id))?;
    crate::metrics::record_authentication(true);
    let mut response = (
        StatusCode::CREATED,
        Json(AuthResponse {
            user: UserResponse { id: user_id, email },
            organization: OrganizationResponse {
                id: organization_id,
                slug: input.organization_slug,
                name: input.organization_name,
            },
            role: OrganizationRole::Owner,
        }),
    )
        .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(token.expose(), state.secure_cookie, state.session_lifetime),
    );
    Ok(response)
}

async fn lookup_user(pool: &PgPool, email: &str) -> Result<Option<AuthenticatedUser>, sqlx::Error> {
    sqlx::query_as("SELECT u.id user_id,u.email,u.password_hash,m.organization_id,o.slug organization_slug,o.name organization_name,m.role,u.disabled_at FROM users u JOIN organization_memberships m ON m.user_id=u.id JOIN organizations o ON o.id=m.organization_id WHERE u.email=$1 ORDER BY m.created_at,m.organization_id LIMIT 1")
        .bind(email)
        .fetch_optional(pool)
        .await
}

async fn login(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
    Json(input): Json<LoginRequest>,
) -> Result<Response, AuthError> {
    let email = normalize_email(&input.email).unwrap_or_default();
    let found = lookup_user(&state.pool, &email)
        .await
        .map_err(|error| AuthError::internal(&error, &request_id))?;
    let dummy_hash = hash_password("okoscope enumeration resistance")
        .map_err(|error| AuthError::internal(&error, &request_id))?;
    let verified = found.as_ref().is_some_and(|user| {
        user.disabled_at.is_none() && verify_password(&input.password, &user.password_hash)
    });
    if found.is_none() {
        let _ = verify_password(&input.password, &dummy_hash);
    }
    let Some(user) = found.filter(|_| verified) else {
        crate::metrics::record_authentication(false);
        return Err(AuthError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "invalid email or password",
            &request_id,
        ));
    };
    let role = user.role.parse().map_err(|()| {
        AuthError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "invalid email or password",
            &request_id,
        )
    })?;
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| AuthError::internal(&error, &request_id))?;
    if let Some(old) = session_token(&headers).and_then(session_digest) {
        sqlx::query(
            "UPDATE user_sessions SET revoked_at=coalesce(revoked_at,now()) WHERE token_hash=$1",
        )
        .bind(old.to_vec())
        .execute(&mut *tx)
        .await
        .map_err(|error| AuthError::internal(&error, &request_id))?;
    }
    let (_, token) = insert_session(
        &mut tx,
        user.user_id,
        user.organization_id,
        state.session_lifetime,
    )
    .await
    .map_err(|error| AuthError::internal(&error, &request_id))?;
    tx.commit()
        .await
        .map_err(|error| AuthError::internal(&error, &request_id))?;
    crate::metrics::record_authentication(true);
    let mut response = Json(AuthResponse {
        user: UserResponse {
            id: user.user_id,
            email: user.email,
        },
        organization: OrganizationResponse {
            id: user.organization_id,
            slug: user.organization_slug,
            name: user.organization_name,
        },
        role,
    })
    .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(token.expose(), state.secure_cookie, state.session_lifetime),
    );
    Ok(response)
}

async fn me(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<AuthResponse>, AuthError> {
    let principal = state
        .authenticator
        .authenticate(session_token(&headers).unwrap_or_default())
        .await
        .map_err(|error| AuthError::internal(&error, &request_id))?
        .ok_or_else(|| {
            AuthError::new(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "authentication required",
                &request_id,
            )
        })?;
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT u.email,o.slug,o.name FROM users u JOIN organizations o ON o.id=$2 WHERE u.id=$1",
    )
    .bind(principal.user_id)
    .bind(principal.organization_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|error| AuthError::internal(&error, &request_id))?;
    let (email, slug, name) = row.ok_or_else(|| {
        AuthError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "authentication required",
            &request_id,
        )
    })?;
    Ok(Json(AuthResponse {
        user: UserResponse {
            id: principal.user_id,
            email,
        },
        organization: OrganizationResponse {
            id: principal.organization_id,
            slug,
            name,
        },
        role: principal.role,
    }))
}

async fn logout(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
) -> Result<Response, AuthError> {
    if let Some(digest) = session_token(&headers).and_then(session_digest) {
        sqlx::query(
            "UPDATE user_sessions SET revoked_at=coalesce(revoked_at,now()) WHERE token_hash=$1",
        )
        .bind(digest.to_vec())
        .execute(&state.pool)
        .await
        .map_err(|error| AuthError::internal(&error, &request_id))?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, expired_cookie(state.secure_cookie));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_and_development_cookie_attributes_are_explicit() {
        let production = session_cookie("opaque", true, std::time::Duration::from_secs(600));
        let production = production.to_str().unwrap();
        assert!(production.contains("HttpOnly"));
        assert!(production.contains("Secure"));
        assert!(production.contains("SameSite=Lax"));
        assert!(production.contains("Max-Age=600"));

        let development = session_cookie("opaque", false, std::time::Duration::from_secs(600));
        assert!(!development.to_str().unwrap().contains("Secure"));
        assert!(expired_cookie(true).to_str().unwrap().contains("Max-Age=0"));
    }
}
