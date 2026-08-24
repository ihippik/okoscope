use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use server::{admin_auth::AdminAuthenticator, health, web_api::WebApiConfig};
use tower::ServiceExt;
use uuid::Uuid;

const ADMIN: &str = "test-admin-credential-with-at-least-32-bytes";

fn app(pool: sqlx::PgPool) -> axum::Router {
    health::router(
        pool,
        true,
        None,
        &WebApiConfig::new(vec!["https://ui.example.com".into()])
            .unwrap()
            .with_admin_authenticator(AdminAuthenticator::new(ADMIN).unwrap()),
    )
}

fn request(method: &str, uri: &str, credential: Option<&str>, body: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-request-id", "provisioning-test")
        .header(header::ORIGIN, "https://ui.example.com");
    if let Some(credential) = credential {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {credential}"));
    }
    builder.body(Body::from(body.to_owned())).unwrap()
}

fn idempotent_request(
    method: &str,
    uri: &str,
    credential: Option<&str>,
    key: Uuid,
    body: &str,
) -> Request<Body> {
    let mut request = request(method, uri, credential, body);
    request
        .headers_mut()
        .insert("idempotency-key", key.to_string().parse().unwrap());
    request
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

#[sqlx::test(migrator = "server::database::MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn admin_provisions_hierarchy_and_rotates_secret_safely(pool: sqlx::PgPool) {
    let app = app(pool.clone());
    let unauthorized = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/organizations",
            None,
            r#"{"slug":"acme","name":"Acme"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(unauthorized.headers()["x-request-id"], "provisioning-test");
    assert_eq!(unauthorized.headers()[header::CACHE_CONTROL], "no-store");

    let organization = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/organizations",
            Some(ADMIN),
            r#"{"slug":"acme","name":"Acme"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(organization.status(), StatusCode::CREATED);
    let organization = json(organization).await;
    let organization_id = organization["id"].as_str().unwrap();

    let conflict = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/organizations",
            Some(ADMIN),
            r#"{"slug":"acme","name":"Duplicate"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(conflict.status(), StatusCode::CONFLICT);

    let project = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/api/v1/organizations/{organization_id}/projects"),
            Some(ADMIN),
            r#"{"slug":"payments","name":"Payments"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(project.status(), StatusCode::CREATED);
    let project = json(project).await;
    let project_id = project["id"].as_str().unwrap();

    let application = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/api/v1/projects/{project_id}/applications"),
            Some(ADMIN),
            r#"{"slug":"payment-api","name":"Payment API"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(application.status(), StatusCode::CREATED);
    let application = json(application).await;
    let application_id = application["application"]["id"].as_str().unwrap();
    let first_id = application["credential"]["id"].as_str().unwrap();
    let first_token = application["credential"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(first_token.starts_with("oko_app_v1_"));
    assert_eq!(application["credential"]["shown_once"], true);

    let listed = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/api/v1/projects/{project_id}/applications/{application_id}/credentials"),
            Some(ADMIN),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let listed = json(listed).await;
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    assert!(!listed.to_string().contains(&first_token));

    let rotated = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/api/v1/projects/{project_id}/applications/{application_id}/credentials"),
            Some(ADMIN),
            r#"{"name":"rotation"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(rotated.status(), StatusCode::CREATED);
    let rotated = json(rotated).await;
    assert_ne!(rotated["token"], first_token);

    for _ in 0..2 {
        let revoked = app
            .clone()
            .oneshot(request(
                "DELETE",
                &format!(
                    "/api/v1/projects/{project_id}/applications/{application_id}/credentials/{first_id}"
                ),
                Some(ADMIN),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
    }

    let foreign = app
        .clone()
        .oneshot(request(
            "GET",
            &format!(
                "/api/v1/projects/{}/applications/{application_id}/credentials",
                Uuid::new_v4()
            ),
            Some(ADMIN),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);

    let digest_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM application_ingestion_credentials WHERE octet_length(credential_hash)=32",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(digest_count, 2);
}

#[sqlx::test(migrator = "server::database::MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn failed_application_creation_is_atomic(pool: sqlx::PgPool) {
    let app = app(pool.clone());
    let missing_project = Uuid::new_v4();
    let response = app
        .oneshot(request(
            "POST",
            &format!("/api/v1/projects/{missing_project}/applications"),
            Some(ADMIN),
            r#"{"slug":"missing","name":"Missing"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let application_count: i64 = sqlx::query_scalar("SELECT count(*) FROM applications")
        .fetch_one(&pool)
        .await
        .unwrap();
    let credential_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM application_ingestion_credentials")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((application_count, credential_count), (0, 0));
}

#[sqlx::test(migrator = "server::database::MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn admin_reads_hierarchy_and_idempotency_never_replays_secrets(pool: sqlx::PgPool) {
    let app = app(pool);
    let organization_key = Uuid::new_v4();
    let create_organization = || {
        idempotent_request(
            "POST",
            "/api/v1/organizations",
            Some(ADMIN),
            organization_key,
            r#"{"slug":"acme","name":"Acme"}"#,
        )
    };
    let first = app.clone().oneshot(create_organization()).await.unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);
    let first = json(first).await;
    let organization_id = first["id"].as_str().unwrap();
    let replay = app.clone().oneshot(create_organization()).await.unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(json(replay).await["id"], first["id"]);

    let organizations = app
        .clone()
        .oneshot(request(
            "GET",
            "/api/v1/admin/organizations",
            Some(ADMIN),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(
        json(organizations).await["items"].as_array().unwrap().len(),
        1
    );

    let project = app
        .clone()
        .oneshot(idempotent_request(
            "POST",
            &format!("/api/v1/organizations/{organization_id}/projects"),
            Some(ADMIN),
            Uuid::new_v4(),
            r#"{"slug":"payments","name":"Payments"}"#,
        ))
        .await
        .unwrap();
    let project = json(project).await;
    let project_id = project["id"].as_str().unwrap();
    let application_key = Uuid::new_v4();
    let application_uri = format!("/api/v1/projects/{project_id}/applications");
    let application_body = r#"{"slug":"api","name":"API"}"#;
    let application = app
        .clone()
        .oneshot(idempotent_request(
            "POST",
            &application_uri,
            Some(ADMIN),
            application_key,
            application_body,
        ))
        .await
        .unwrap();
    assert_eq!(application.status(), StatusCode::CREATED);
    let application = json(application).await;
    assert!(application["credential"]["token"].as_str().is_some());
    let replay = app
        .clone()
        .oneshot(idempotent_request(
            "POST",
            &application_uri,
            Some(ADMIN),
            application_key,
            application_body,
        ))
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::CONFLICT);
    let replay = json(replay).await;
    assert_eq!(replay["error"], "operation_already_completed");
    assert!(!replay.to_string().contains("oko_app_v1_"));

    let applications = app
        .oneshot(request(
            "GET",
            &format!("/api/v1/admin/projects/{project_id}/applications"),
            Some(ADMIN),
            "",
        ))
        .await
        .unwrap();
    assert_eq!(
        json(applications).await["items"].as_array().unwrap().len(),
        1
    );
}
