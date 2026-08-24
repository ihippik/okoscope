use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
    routing::post,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    admin_auth::AdminAuthenticator,
    application_credentials::{
        ApplicationCredentialSummary, issue, list as list_credentials, revoke,
    },
    web_api::{RequestId, error_response},
};

#[derive(Clone, Debug)]
struct ProvisioningState {
    pool: PgPool,
    admin: Option<AdminAuthenticator>,
}

pub fn router(pool: PgPool, admin: Option<AdminAuthenticator>) -> Router {
    Router::new()
        .route("/api/v1/organizations", post(create_organization))
        .route(
            "/api/v1/organizations/{organization_id}/projects",
            post(create_project),
        )
        .route(
            "/api/v1/projects/{project_id}/applications",
            post(create_application),
        )
        .route(
            "/api/v1/projects/{project_id}/applications/{application_id}/credentials",
            axum::routing::get(list_application_credentials).post(issue_application_credential),
        )
        .route(
            "/api/v1/projects/{project_id}/applications/{application_id}/credentials/{credential_id}",
            axum::routing::delete(revoke_application_credential),
        )
        .with_state(ProvisioningState { pool, admin })
}

#[derive(Debug)]
struct ProvisioningError {
    status: StatusCode,
    code: &'static str,
    message: String,
    request_id: RequestId,
}

impl ProvisioningError {
    fn unauthorized(request_id: &RequestId) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "invalid or missing admin bearer credential".into(),
            request_id: request_id.clone(),
        }
    }

    fn invalid(message: impl Into<String>, request_id: &RequestId) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_request",
            message: message.into(),
            request_id: request_id.clone(),
        }
    }

    fn not_found(request_id: &RequestId) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: "resource not found".into(),
            request_id: request_id.clone(),
        }
    }

    fn conflict(request_id: &RequestId) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "conflict",
            message: "resource already exists".into(),
            request_id: request_id.clone(),
        }
    }

    fn database(error: &sqlx::Error, request_id: &RequestId) -> Self {
        if error
            .as_database_error()
            .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
        {
            return Self::conflict(request_id);
        }
        tracing::error!(error=%error, request_id=%request_id.0, "provisioning database error");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: "internal server error".into(),
            request_id: request_id.clone(),
        }
    }
}

impl IntoResponse for ProvisioningError {
    fn into_response(self) -> Response {
        error_response(self.status, self.code, self.message, &self.request_id)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateNamedResource {
    slug: String,
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueCredentialRequest {
    name: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct OrganizationResponse {
    id: Uuid,
    slug: String,
    name: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct ProjectResponse {
    id: Uuid,
    organization_id: Uuid,
    slug: String,
    name: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct ApplicationResponse {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    slug: String,
    name: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct IssuedCredentialResponse {
    id: Uuid,
    name: String,
    token: String,
    token_hint: String,
    created_at: DateTime<Utc>,
    shown_once: bool,
}

#[derive(Debug, Serialize)]
struct CreatedApplicationResponse {
    application: ApplicationResponse,
    credential: IssuedCredentialResponse,
}

#[derive(Debug, Serialize)]
struct CredentialPage {
    items: Vec<ApplicationCredentialSummary>,
}

fn authorize(
    state: &ProvisioningState,
    headers: &HeaderMap,
    request_id: &RequestId,
) -> Result<(), ProvisioningError> {
    let presented = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if state
        .admin
        .as_ref()
        .zip(presented)
        .is_some_and(|(admin, credential)| admin.authenticate(credential))
    {
        Ok(())
    } else {
        Err(ProvisioningError::unauthorized(request_id))
    }
}

fn validate_slug(value: &str, request_id: &RequestId) -> Result<(), ProvisioningError> {
    let valid = (1..=63).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--");
    if valid {
        Ok(())
    } else {
        Err(ProvisioningError::invalid(
            "slug must contain 1-63 lowercase letters, digits, or single hyphens",
            request_id,
        ))
    }
}

fn validate_name(value: &str, request_id: &RequestId) -> Result<(), ProvisioningError> {
    if value.trim() == value && (1..=120).contains(&value.chars().count()) {
        Ok(())
    } else {
        Err(ProvisioningError::invalid(
            "name must contain 1-120 characters without surrounding whitespace",
            request_id,
        ))
    }
}

fn validate_credential_name(value: &str, request_id: &RequestId) -> Result<(), ProvisioningError> {
    if value.trim() == value && (1..=64).contains(&value.chars().count()) {
        Ok(())
    } else {
        Err(ProvisioningError::invalid(
            "credential name must contain 1-64 characters without surrounding whitespace",
            request_id,
        ))
    }
}

async fn create_organization(
    State(state): State<ProvisioningState>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
    Json(input): Json<CreateNamedResource>,
) -> Result<(StatusCode, Json<OrganizationResponse>), ProvisioningError> {
    authorize(&state, &headers, &request_id)?;
    validate_slug(&input.slug, &request_id)?;
    validate_name(&input.name, &request_id)?;
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    let organization = sqlx::query_as(
        "INSERT INTO organizations(id,slug,name) VALUES($1,$2,$3) RETURNING id,slug,name,created_at",
    )
    .bind(Uuid::new_v4())
    .bind(input.slug)
    .bind(input.name)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    tx.commit()
        .await
        .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    Ok((StatusCode::CREATED, Json(organization)))
}

async fn create_project(
    State(state): State<ProvisioningState>,
    Path(organization_id): Path<Uuid>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
    Json(input): Json<CreateNamedResource>,
) -> Result<(StatusCode, Json<ProjectResponse>), ProvisioningError> {
    authorize(&state, &headers, &request_id)?;
    validate_slug(&input.slug, &request_id)?;
    validate_name(&input.name, &request_id)?;
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM organizations WHERE id=$1)")
        .bind(organization_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    if !exists {
        return Err(ProvisioningError::not_found(&request_id));
    }
    let project = sqlx::query_as("INSERT INTO projects(id,organization_id,slug,name) VALUES($1,$2,$3,$4) RETURNING id,organization_id,slug,name,created_at")
        .bind(Uuid::new_v4()).bind(organization_id).bind(input.slug).bind(input.name)
        .fetch_one(&mut *tx).await.map_err(|error| ProvisioningError::database(&error,&request_id))?;
    tx.commit()
        .await
        .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    Ok((StatusCode::CREATED, Json(project)))
}

async fn create_application(
    State(state): State<ProvisioningState>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
    Json(input): Json<CreateNamedResource>,
) -> Result<(StatusCode, Json<CreatedApplicationResponse>), ProvisioningError> {
    authorize(&state, &headers, &request_id)?;
    validate_slug(&input.slug, &request_id)?;
    validate_name(&input.name, &request_id)?;
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    let organization_id: Uuid =
        sqlx::query_scalar("SELECT organization_id FROM projects WHERE id=$1")
            .bind(project_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| ProvisioningError::database(&error, &request_id))?
            .ok_or_else(|| ProvisioningError::not_found(&request_id))?;
    let application: ApplicationResponse = sqlx::query_as("INSERT INTO applications(id,organization_id,project_id,slug,name) VALUES($1,$2,$3,$4,$5) RETURNING id,organization_id,project_id,slug,name,created_at")
        .bind(Uuid::new_v4()).bind(organization_id).bind(project_id).bind(input.slug).bind(input.name)
        .fetch_one(&mut *tx).await.map_err(|error| ProvisioningError::database(&error,&request_id))?;
    let credential = issue(
        &mut tx,
        organization_id,
        project_id,
        application.id,
        "default",
    )
    .await
    .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    let response = CreatedApplicationResponse {
        application,
        credential: issued_response(&credential),
    };
    tx.commit()
        .await
        .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn owned_application(
    state: &ProvisioningState,
    project_id: Uuid,
    application_id: Uuid,
    request_id: &RequestId,
) -> Result<Uuid, ProvisioningError> {
    sqlx::query_scalar("SELECT organization_id FROM applications WHERE project_id=$1 AND id=$2")
        .bind(project_id)
        .bind(application_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|error| ProvisioningError::database(&error, request_id))?
        .ok_or_else(|| ProvisioningError::not_found(request_id))
}

async fn list_application_credentials(
    State(state): State<ProvisioningState>,
    Path((project_id, application_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<CredentialPage>, ProvisioningError> {
    authorize(&state, &headers, &request_id)?;
    let organization_id =
        owned_application(&state, project_id, application_id, &request_id).await?;
    let items = list_credentials(&state.pool, organization_id, project_id, application_id)
        .await
        .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    Ok(Json(CredentialPage { items }))
}

async fn issue_application_credential(
    State(state): State<ProvisioningState>,
    Path((project_id, application_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
    Json(input): Json<IssueCredentialRequest>,
) -> Result<(StatusCode, Json<IssuedCredentialResponse>), ProvisioningError> {
    authorize(&state, &headers, &request_id)?;
    validate_credential_name(&input.name, &request_id)?;
    let organization_id =
        owned_application(&state, project_id, application_id, &request_id).await?;
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    let credential = issue(
        &mut tx,
        organization_id,
        project_id,
        application_id,
        &input.name,
    )
    .await
    .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    let response = issued_response(&credential);
    tx.commit()
        .await
        .map_err(|error| ProvisioningError::database(&error, &request_id))?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn revoke_application_credential(
    State(state): State<ProvisioningState>,
    Path((project_id, application_id, credential_id)): Path<(Uuid, Uuid, Uuid)>,
    headers: HeaderMap,
    Extension(request_id): Extension<RequestId>,
) -> Result<StatusCode, ProvisioningError> {
    authorize(&state, &headers, &request_id)?;
    let organization_id =
        owned_application(&state, project_id, application_id, &request_id).await?;
    revoke(
        &state.pool,
        organization_id,
        project_id,
        application_id,
        credential_id,
    )
    .await
    .map_err(|error| ProvisioningError::database(&error, &request_id))?
    .ok_or_else(|| ProvisioningError::not_found(&request_id))?;
    Ok(StatusCode::NO_CONTENT)
}

fn issued_response(
    credential: &crate::application_credentials::IssuedApplicationCredential,
) -> IssuedCredentialResponse {
    IssuedCredentialResponse {
        id: credential.summary.id,
        name: credential.summary.name.clone(),
        token: credential.token().to_owned(),
        token_hint: credential.summary.token_hint.clone(),
        created_at: credential.summary.created_at,
        shown_once: true,
    }
}
