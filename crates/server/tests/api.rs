use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::AUTHORIZATION},
};
use chrono::Utc;
use event_model::{
    EVENT_SCHEMA_VERSION, EventPayload, KubernetesAttribution, ProcessExec, ProcessIdentity,
    RuntimeEvent,
};
use server::{
    api,
    auth::SessionScope,
    bootstrap::{BootstrapConfig, bootstrap},
    ingestion::{IngestionContext, persist_batch},
};
use tower::ServiceExt;
use uuid::Uuid;

fn config(name: &str) -> BootstrapConfig {
    BootstrapConfig {
        organization_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        cluster_id: Uuid::new_v4(),
        application_id: Uuid::new_v4(),
        organization_slug: name.into(),
        organization_name: name.into(),
        project_slug: "payments".into(),
        project_name: "Payments".into(),
        cluster_external_id: "test".into(),
        cluster_name: "Test".into(),
        application_slug: "payment-api".into(),
        application_name: "Payment API".into(),
        cluster_credential: format!("cluster-{name}"),
        api_credential: format!("api-{name}"),
    }
}

fn event(project_id: Uuid, application_id: Uuid) -> RuntimeEvent {
    RuntimeEvent {
        id: Uuid::new_v4(),
        observed_at: Utc::now(),
        schema_version: EVENT_SCHEMA_VERSION,
        attribution: KubernetesAttribution {
            project_id,
            application_id,
            node_name: "node-1".into(),
            namespace: "production".into(),
            pod_uid: "pod-uid".into(),
            pod_name: "payment-api-1".into(),
            container_id: "container-id".into(),
            container_name: "payment-api".into(),
            workload_uid: "workload-uid".into(),
            workload_kind: "Deployment".into(),
            workload_name: "payment-api".into(),
        },
        process: ProcessIdentity {
            cgroup_id: 42,
            pid: 10,
            tgid: 10,
            command: "sh".into(),
        },
        payload: EventPayload::ProcessExec(ProcessExec {
            executable: "/bin/sh".into(),
            parent_command: None,
        }),
    }
}

#[sqlx::test(migrator = "server::database::MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn list_and_detail_are_authenticated_paginated_and_tenant_safe(pool: sqlx::PgPool) {
    let first_config = config("first-api");
    let first = bootstrap(&pool, &first_config).await.unwrap();
    let second_config = config("second-api");
    let second = bootstrap(&pool, &second_config).await.unwrap();
    let agent_id = Uuid::new_v4();
    sqlx::query("INSERT INTO agents (id,organization_id,cluster_id,node_name,agent_version) VALUES ($1,$2,$3,'node-1','test')")
        .bind(agent_id).bind(first.organization_id).bind(first.cluster_id).execute(&pool).await.unwrap();
    persist_batch(
        &pool,
        IngestionContext {
            scope: SessionScope {
                organization_id: first.organization_id,
                cluster_id: first.cluster_id,
            },
            agent_id,
        },
        &[event(first.project_id, first.application_id)],
    )
    .await
    .unwrap();
    let group_id: Uuid = sqlx::query_scalar("SELECT id FROM runtime_event_groups")
        .fetch_one(&pool)
        .await
        .unwrap();
    let app = api::router(pool);

    let unauthorized = app.clone().oneshot(Request::builder().uri("/api/v1/runtime-groups?project_id=00000000-0000-0000-0000-000000000000&application_id=00000000-0000-0000-0000-000000000000").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let list_uri = format!(
        "/api/v1/runtime-groups?project_id={}&application_id={}&event_kind=process.exec&limit=1",
        first.project_id, first.application_id
    );
    let listed = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(list_uri)
                .header(
                    AUTHORIZATION,
                    format!("Bearer {}", first_config.api_credential),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body: serde_json::Value =
        serde_json::from_slice(&to_bytes(listed.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(listed_body["items"].as_array().unwrap().len(), 1);

    let detail = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/runtime-groups/{group_id}"))
                .header(
                    AUTHORIZATION,
                    format!("Bearer {}", first_config.api_credential),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(detail.status(), StatusCode::OK);

    let hidden = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/runtime-groups/{group_id}"))
                .header(
                    AUTHORIZATION,
                    format!("Bearer {}", second_config.api_credential),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
    assert_ne!(first.organization_id, second.organization_id);
}
