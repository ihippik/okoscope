use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::AUTHORIZATION},
};
use chrono::{Duration, Utc};
use event_model::{
    EVENT_SCHEMA_VERSION, EventPayload, KubernetesAttribution, ProcessExec, ProcessIdentity,
    RuntimeEvent,
};
use server::{
    auth::SessionScope,
    bootstrap::{BootstrapConfig, BootstrapIds, bootstrap},
    ingestion::{IngestionContext, persist_batch},
    releases,
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
        project_slug: "project".into(),
        project_name: "Project".into(),
        cluster_external_id: "cluster".into(),
        cluster_name: "Cluster".into(),
        application_slug: "app".into(),
        application_name: "App".into(),
        cluster_credential: format!("cluster-{name}"),
        api_credential: format!("api-{name}"),
    }
}

fn request(
    method: &str,
    uri: &str,
    credential: &str,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {credential}"));
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    builder
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .unwrap()
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

async fn create_release(
    app: &axum::Router,
    ids: &BootstrapIds,
    credential: &str,
    version: &str,
    deployed_at: chrono::DateTime<Utc>,
) -> Uuid {
    let uri = format!(
        "/api/v1/projects/{}/applications/{}/releases",
        ids.project_id, ids.application_id
    );
    let response = app
        .clone()
        .clone()
        .oneshot(request(
            "POST",
            &uri,
            credential,
            Some(serde_json::json!({"version":version,"deployed_at":deployed_at})),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    Uuid::parse_str(json(response).await["id"].as_str().unwrap()).unwrap()
}

fn event(
    ids: &BootstrapIds,
    release: Option<&str>,
    executable: &str,
    observed_at: chrono::DateTime<Utc>,
) -> RuntimeEvent {
    RuntimeEvent {
        id: Uuid::new_v4(),
        observed_at,
        schema_version: EVENT_SCHEMA_VERSION,
        attribution: KubernetesAttribution {
            project_id: ids.project_id,
            application_id: ids.application_id,
            node_name: "node".into(),
            namespace: "default".into(),
            pod_uid: Uuid::new_v4().to_string(),
            pod_name: "app-1".into(),
            container_id: Uuid::new_v4().to_string(),
            container_name: "app".into(),
            workload_uid: "workload".into(),
            workload_kind: "Deployment".into(),
            workload_name: "app".into(),
            release: release.map(str::to_owned),
        },
        process: ProcessIdentity {
            cgroup_id: 1,
            pid: 1,
            tgid: 1,
            command: "app".into(),
        },
        payload: EventPayload::ProcessExec(ProcessExec {
            executable: executable.into(),
            parent_command: None,
        }),
    }
}

async fn agent(pool: &sqlx::PgPool, ids: &BootstrapIds) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO agents(id,organization_id,cluster_id,node_name,agent_version) VALUES($1,$2,$3,'node','test')")
        .bind(id).bind(ids.organization_id).bind(ids.cluster_id).execute(pool).await.unwrap();
    id
}

#[sqlx::test(migrator = "server::database::MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn release_api_is_paginated_conflict_safe_and_tenant_scoped(pool: sqlx::PgPool) {
    let first_config = config("release-first");
    let first = bootstrap(&pool, &first_config).await.unwrap();
    let second_config = config("release-second");
    let second = bootstrap(&pool, &second_config).await.unwrap();
    let app = releases::router(pool.clone());
    let now = Utc::now();
    let first_id = create_release(&app, &first, &first_config.api_credential, "1.0.0", now).await;
    let first_diff = app
        .clone()
        .oneshot(request(
            "GET",
            &format!(
                "/api/v1/projects/{}/applications/{}/releases/{first_id}/runtime-diff",
                first.project_id, first.application_id
            ),
            &first_config.api_credential,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(first_diff.status(), StatusCode::OK);
    assert!(json(first_diff).await["baseline"].is_null());
    let first_summary = app
        .clone()
        .oneshot(request(
            "GET",
            &format!(
                "/api/v1/projects/{}/applications/{}/releases/{first_id}/runtime-diff/summary",
                first.project_id, first.application_id
            ),
            &first_config.api_credential,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(first_summary.status(), StatusCode::OK);
    let first_summary = json(first_summary).await;
    assert!(first_summary["baseline"].is_null());
    assert_eq!(first_summary["total_item_count"], 0);
    assert!(
        first_summary["largest_changes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let duplicate_uri = format!(
        "/api/v1/projects/{}/applications/{}/releases",
        first.project_id, first.application_id
    );
    let duplicate = app
        .clone()
        .oneshot(request(
            "POST",
            &duplicate_uri,
            &first_config.api_credential,
            Some(serde_json::json!({"version":"1.0.0","deployed_at":now})),
        ))
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    create_release(&app, &first, &first_config.api_credential, "1.0.1", now).await;
    let foreign_release_id =
        create_release(&app, &second, &second_config.api_credential, "1.0.0", now).await;
    let foreign_baseline = app
        .clone()
        .oneshot(request(
            "GET",
            &format!(
                "/api/v1/projects/{}/applications/{}/releases/{first_id}/runtime-diff/summary?baseline_id={foreign_release_id}",
                first.project_id, first.application_id
            ),
            &first_config.api_credential,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(foreign_baseline.status(), StatusCode::NOT_FOUND);
    let list = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("{duplicate_uri}?limit=1"),
            &first_config.api_credential,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list = json(list).await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert!(list["next_cursor"].is_string());
    let foreign = app
        .oneshot(request(
            "GET",
            &format!(
                "/api/v1/projects/{}/applications/{}/releases/{first_id}",
                first.project_id, first.application_id
            ),
            &second_config.api_credential,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    assert_eq!(server::database::REQUIRED_MIGRATION, 13);
    let columns: i64 = sqlx::query_scalar("SELECT count(*) FROM information_schema.columns WHERE table_schema='public' AND ((table_name='runtime_events' AND column_name='release_id') OR (table_name='runtime_event_group_memberships' AND column_name='release_id'))").fetch_one(&pool).await.unwrap();
    assert_eq!(columns, 2);
}

#[sqlx::test(migrator = "server::database::MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn ingestion_builds_idempotent_release_summaries_and_complete_diff(pool: sqlx::PgPool) {
    let cfg = config("release-diff");
    let ids = bootstrap(&pool, &cfg).await.unwrap();
    let app = releases::router(pool.clone());
    let baseline_id = create_release(
        &app,
        &ids,
        &cfg.api_credential,
        "1.0.0",
        Utc::now() - Duration::hours(1),
    )
    .await;
    let target_id = create_release(&app, &ids, &cfg.api_credential, "1.1.0", Utc::now()).await;
    let agent_id = agent(&pool, &ids).await;
    let context = IngestionContext {
        scope: SessionScope {
            organization_id: ids.organization_id,
            cluster_id: ids.cluster_id,
        },
        agent_id,
    };
    let shared_baseline = event(
        &ids,
        Some("1.0.0"),
        "/bin/shared",
        Utc::now() - Duration::minutes(30),
    );
    let events = vec![
        shared_baseline.clone(),
        event(
            &ids,
            Some("1.0.0"),
            "/bin/old",
            Utc::now() - Duration::minutes(20),
        ),
        event(&ids, Some("1.1.0"), "/bin/shared", Utc::now()),
        event(&ids, Some("1.1.0"), "/bin/new", Utc::now()),
        event(
            &ids,
            Some("1.0.0"),
            "/bin/equal",
            Utc::now() - Duration::minutes(10),
        ),
        event(&ids, Some("1.1.0"), "/bin/equal", Utc::now()),
        event(&ids, Some("missing"), "/bin/unattributed", Utc::now()),
    ];
    assert_eq!(persist_batch(&pool, context, &events).await.unwrap(), 7);
    let concurrent_one = event(
        &ids,
        Some("1.1.0"),
        "/bin/shared",
        Utc::now() - Duration::minutes(5),
    );
    let concurrent_two = event(
        &ids,
        Some("1.1.0"),
        "/bin/shared",
        Utc::now() + Duration::minutes(5),
    );
    let concurrent_one = [concurrent_one];
    let concurrent_two = [concurrent_two];
    let (one, two) = tokio::join!(
        persist_batch(&pool, context, &concurrent_one),
        persist_batch(&pool, context, &concurrent_two)
    );
    assert_eq!(one.unwrap(), 1);
    assert_eq!(two.unwrap(), 1);
    assert_eq!(
        persist_batch(&pool, context, &[shared_baseline])
            .await
            .unwrap(),
        0
    );
    let counts: Vec<(Uuid, i64)> = sqlx::query_as("SELECT release_id,sum(occurrence_count)::bigint FROM runtime_event_group_releases GROUP BY release_id ORDER BY release_id").fetch_all(&pool).await.unwrap();
    assert_eq!(counts.iter().map(|(_, count)| *count).sum::<i64>(), 8);
    assert!(
        counts
            .iter()
            .any(|(id, count)| *id == baseline_id && *count == 3)
    );
    assert!(
        counts
            .iter()
            .any(|(id, count)| *id == target_id && *count == 5)
    );
    let unattributed: i64 =
        sqlx::query_scalar("SELECT count(*) FROM runtime_events WHERE release_id IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(unattributed, 1);
    let uri = format!(
        "/api/v1/projects/{}/applications/{}/releases/{target_id}/runtime-diff",
        ids.project_id, ids.application_id
    );
    let response = app
        .clone()
        .oneshot(request("GET", &uri, &cfg.api_credential, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    assert_eq!(body["baseline"]["id"], baseline_id.to_string());
    let mut classes: Vec<_> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["classification"].as_str().unwrap())
        .collect();
    classes.sort_unstable();
    assert_eq!(
        classes,
        vec!["disappeared", "new", "unchanged", "unchanged"]
    );
    let summary_response = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("{uri}/summary?baseline_id={baseline_id}&limit=10"),
            &cfg.api_credential,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(summary_response.status(), StatusCode::OK);
    let summary = json(summary_response).await;
    assert_eq!(summary["baseline"]["id"], baseline_id.to_string());
    assert_eq!(summary["target"]["id"], target_id.to_string());
    assert_eq!(summary["total_item_count"], 4);
    assert_eq!(summary["classifications"].as_array().unwrap().len(), 3);
    assert_eq!(summary["largest_changes"].as_array().unwrap().len(), 4);
    assert!(
        summary["largest_changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|change| change["occurrence_delta"] == 0)
    );
    for change in summary["largest_changes"].as_array().unwrap() {
        assert_eq!(
            change["occurrence_delta"].as_i64().unwrap(),
            change["target_occurrence_count"].as_i64().unwrap()
                - change["baseline_occurrence_count"].as_i64().unwrap()
        );
    }
    let first_page = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("{uri}?baseline_id={baseline_id}&limit=1"),
            &cfg.api_credential,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(first_page.status(), StatusCode::OK);
    let first_page = json(first_page).await;
    let cursor = first_page["next_cursor"].as_str().unwrap();
    let second_page = app
        .oneshot(request(
            "GET",
            &format!("{uri}?baseline_id={baseline_id}&limit=1&cursor={cursor}"),
            &cfg.api_credential,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(second_page.status(), StatusCode::OK);
    assert_eq!(
        json(second_page).await["items"].as_array().unwrap().len(),
        1
    );
}
