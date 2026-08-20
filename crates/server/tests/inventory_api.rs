use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::AUTHORIZATION},
};
use chrono::{Duration, Utc};
use event_model::{
    DnsDirection, DnsName, DnsQueryType, DnsTransport, EVENT_SCHEMA_VERSION, EventPayload,
    KubernetesAttribution, NetworkAddressFamily, NetworkConnect, NetworkConnectOutcome,
    NetworkDnsQuery, ProcessExec, ProcessIdentity, RuntimeEvent, SyscallEvent,
};
use server::{
    auth::SessionScope,
    bootstrap::{BootstrapConfig, BootstrapIds, bootstrap},
    ingestion::{IngestionContext, persist_batch},
    inventory_api,
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
        application_name: "Application".into(),
        cluster_credential: format!("cluster-{name}"),
        api_credential: format!("api-{name}"),
    }
}

fn event(ids: &BootstrapIds, payload: EventPayload, release: Option<&str>) -> RuntimeEvent {
    RuntimeEvent {
        id: Uuid::new_v4(),
        observed_at: Utc::now(),
        schema_version: EVENT_SCHEMA_VERSION,
        attribution: KubernetesAttribution {
            project_id: ids.project_id,
            application_id: ids.application_id,
            node_name: "node-a".into(),
            namespace: "production".into(),
            pod_uid: Uuid::new_v4().to_string(),
            pod_name: "app-1".into(),
            container_id: Uuid::new_v4().to_string(),
            container_name: "app".into(),
            workload_uid: "workload-a".into(),
            workload_kind: "Deployment".into(),
            workload_name: "app".into(),
            release: release.map(str::to_owned),
        },
        process: ProcessIdentity {
            cgroup_id: 1,
            pid: 10,
            tgid: 10,
            command: "app".into(),
        },
        payload,
    }
}

fn request(uri: &str, credential: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap()
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

async fn release(pool: &sqlx::PgPool, ids: &BootstrapIds, version: &str, age_days: i64) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO releases(id,organization_id,project_id,application_id,version,deployed_at) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(id).bind(ids.organization_id).bind(ids.project_id).bind(ids.application_id).bind(version)
        .bind(Utc::now() - Duration::days(age_days)).execute(pool).await.unwrap();
    id
}

#[sqlx::test(migrator = "server::database::MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn inventory_api_covers_kinds_filters_evidence_pagination_and_tenant_isolation(
    pool: sqlx::PgPool,
) {
    let first_config = config("inventory-api-first");
    let first = bootstrap(&pool, &first_config).await.unwrap();
    let second_config = config("inventory-api-second");
    let second = bootstrap(&pool, &second_config).await.unwrap();
    let observed_release = release(&pool, &first, "v1", 3).await;
    let other_evidence_release = release(&pool, &first, "v2", 2).await;
    let unknown_release = release(&pool, &first, "v3", 1).await;
    let foreign_release = release(&pool, &second, "foreign", 1).await;
    let agent_id = Uuid::new_v4();
    sqlx::query("INSERT INTO agents(id,organization_id,cluster_id,node_name,agent_version) VALUES($1,$2,$3,'node-a','test')")
        .bind(agent_id).bind(first.organization_id).bind(first.cluster_id).execute(&pool).await.unwrap();
    let context = IngestionContext {
        scope: SessionScope {
            organization_id: first.organization_id,
            cluster_id: first.cluster_id,
        },
        agent_id,
    };
    let dns_name = DnsName::new("api.example.com").unwrap();
    let events = vec![
        event(
            &first,
            EventPayload::ProcessExec(ProcessExec {
                executable: "/app/server".into(),
                parent_command: None,
            }),
            Some("v1"),
        ),
        event(
            &first,
            EventPayload::NetworkConnect(
                NetworkConnect::new(
                    NetworkAddressFamily::Ipv4,
                    "203.0.113.7".parse().unwrap(),
                    443,
                    NetworkConnectOutcome::Succeeded,
                    None,
                )
                .unwrap(),
            ),
            Some("v1"),
        ),
        event(
            &first,
            EventPayload::NetworkDnsQuery(NetworkDnsQuery {
                transaction_id: 1,
                direction: DnsDirection::Egress,
                transport: DnsTransport::Udp,
                resolver_address: "10.96.0.10".parse().unwrap(),
                name: dns_name,
                query_type: DnsQueryType::A,
            }),
            Some("v1"),
        ),
        event(
            &first,
            EventPayload::Syscall(SyscallEvent {
                name: "epoll_wait".into(),
            }),
            Some("v2"),
        ),
        event(
            &first,
            EventPayload::ProcessExec(ProcessExec {
                executable: "/app/worker".into(),
                parent_command: None,
            }),
            Some("v1"),
        ),
        event(
            &first,
            EventPayload::ProcessExec(ProcessExec {
                executable: "<script>alert(1)</script>".into(),
                parent_command: None,
            }),
            Some("v1"),
        ),
    ];
    persist_batch(&pool, context, &events).await.unwrap();
    let process_item: Uuid =
        sqlx::query_scalar("SELECT id FROM runtime_inventory_items WHERE inventory_kind='process'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let app = inventory_api::router(pool.clone());
    let base = format!(
        "/api/v1/projects/{}/applications/{}/runtime-inventory",
        first.project_id, first.application_id
    );

    let summary_response = app
        .clone()
        .oneshot(request(
            &format!("{base}/summary"),
            &first_config.api_credential,
        ))
        .await
        .unwrap();
    assert_eq!(summary_response.status(), StatusCode::OK);
    let summary = json(summary_response).await;
    assert_eq!(summary["item_count"], 6);
    assert_eq!(summary["kinds"].as_array().unwrap().len(), 4);

    for kind in ["process", "destination", "domain", "syscall"] {
        let response = app
            .clone()
            .oneshot(request(
                &format!("{base}?kind={kind}&limit=1"),
                &first_config.api_credential,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json(response).await;
        assert_eq!(body["items"][0]["inventory_kind"], kind);
        let distribution = app
            .clone()
            .oneshot(request(
                &format!("{base}/distribution?kind={kind}&limit=10"),
                &first_config.api_credential,
            ))
            .await
            .unwrap();
        assert_eq!(distribution.status(), StatusCode::OK);
        assert_eq!(json(distribution).await["kind"], kind);
    }

    let distribution_response = app
        .clone()
        .oneshot(request(
            &format!("{base}/distribution?kind=process&limit=1"),
            &first_config.api_credential,
        ))
        .await
        .unwrap();
    assert_eq!(distribution_response.status(), StatusCode::OK);
    let distribution = json(distribution_response).await;
    assert_eq!(distribution["total_item_count"], 3);
    assert_eq!(distribution["entries"].as_array().unwrap().len(), 1);
    assert_eq!(distribution["other"]["item_count"], 2);
    assert_eq!(
        distribution["entries"][0]["occurrence_count"]
            .as_i64()
            .unwrap()
            + distribution["other"]["occurrence_count"].as_i64().unwrap(),
        distribution["total_occurrence_count"].as_i64().unwrap()
    );
    let all_processes = app
        .clone()
        .oneshot(request(
            &format!("{base}/distribution?kind=process&limit=10"),
            &first_config.api_credential,
        ))
        .await
        .unwrap();
    let all_processes = json(all_processes).await;
    assert!(all_processes["other"].is_null());
    let tokens: Vec<_> = all_processes["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["identity_token"].as_str().unwrap())
        .collect();
    assert!(tokens.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        all_processes["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["semantic_summary"]["executable"] == "<script>alert(1)</script>"
            })
    );

    let scoped_distribution = app
        .clone()
        .oneshot(request(
            &format!(
                "{base}/distribution?kind=process&release_id={observed_release}&cluster_id={}&namespace=production&workload_kind=Deployment&workload_name=app&container_name=app&search=worker",
                first.cluster_id
            ),
            &first_config.api_credential,
        ))
        .await
        .unwrap();
    assert_eq!(scoped_distribution.status(), StatusCode::OK);
    assert_eq!(json(scoped_distribution).await["total_item_count"], 1);

    let empty_distribution = app
        .clone()
        .oneshot(request(
            &format!("{base}/distribution?kind=process&search=does-not-exist"),
            &first_config.api_credential,
        ))
        .await
        .unwrap();
    let empty_distribution = json(empty_distribution).await;
    assert_eq!(empty_distribution["total_item_count"], 0);
    assert!(empty_distribution["entries"].as_array().unwrap().is_empty());
    assert!(empty_distribution["other"].is_null());
    let identity_token = distribution["entries"][0]["identity_token"]
        .as_str()
        .unwrap();
    let identity_filtered = app
        .clone()
        .oneshot(request(
            &format!("{base}?kind=process&identity_token={identity_token}"),
            &first_config.api_credential,
        ))
        .await
        .unwrap();
    assert_eq!(identity_filtered.status(), StatusCode::OK);
    assert_eq!(
        json(identity_filtered).await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let tampered = format!("{identity_token}0");
    let invalid_token = app
        .clone()
        .oneshot(request(
            &format!("{base}?kind=process&identity_token={tampered}"),
            &first_config.api_credential,
        ))
        .await
        .unwrap();
    assert_eq!(invalid_token.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json(invalid_token).await["error"], "invalid_identity_token");

    for invalid_limit in [0, 11] {
        let response = app
            .clone()
            .oneshot(request(
                &format!("{base}/distribution?kind=process&limit={invalid_limit}"),
                &first_config.api_credential,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    let filtered = app
        .clone()
        .oneshot(request(
            &format!(
                "{base}?release_id={observed_release}&cluster_id={}&namespace=production&workload_kind=Deployment&workload_name=app&container_name=app&search=server",
                first.cluster_id
            ),
            &first_config.api_credential,
        ))
        .await
        .unwrap();
    assert_eq!(filtered.status(), StatusCode::OK);
    assert_eq!(json(filtered).await["items"].as_array().unwrap().len(), 1);

    let page = app
        .clone()
        .oneshot(request(
            &format!("{base}?limit=1"),
            &first_config.api_credential,
        ))
        .await
        .unwrap();
    let page = json(page).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert!(page["next_cursor"].is_string());

    let detail_base = format!("{base}/{process_item}");
    for suffix in ["", "/sightings", "/groups", "/occurrences"] {
        let response = app
            .clone()
            .oneshot(request(
                &format!("{detail_base}{suffix}?limit=200"),
                &first_config.api_credential,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let releases_response = app
        .clone()
        .oneshot(request(
            &format!("{detail_base}/releases"),
            &first_config.api_credential,
        ))
        .await
        .unwrap();
    let releases = json(releases_response).await;
    let states: std::collections::HashMap<Uuid, &str> = releases["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                Uuid::parse_str(item["release_id"].as_str().unwrap()).unwrap(),
                item["presence"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(states[&observed_release], "observed");
    assert_eq!(states[&other_evidence_release], "not_observed");
    assert_eq!(states[&unknown_release], "unknown");

    let invalid = app
        .clone()
        .oneshot(request(
            &format!("{base}?kind=payload&limit=201"),
            &first_config.api_credential,
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let foreign_filter = app
        .clone()
        .oneshot(request(
            &format!("{base}?release_id={foreign_release}"),
            &first_config.api_credential,
        ))
        .await
        .unwrap();
    assert_eq!(foreign_filter.status(), StatusCode::BAD_REQUEST);

    let foreign_item = app
        .clone()
        .oneshot(request(&detail_base, &second_config.api_credential))
        .await
        .unwrap();
    assert_eq!(foreign_item.status(), StatusCode::NOT_FOUND);
    let foreign_application = app
        .clone()
        .oneshot(request(&base, &second_config.api_credential))
        .await
        .unwrap();
    assert_eq!(foreign_application.status(), StatusCode::NOT_FOUND);
    let foreign_distribution = app
        .oneshot(request(
            &format!("{base}/distribution?kind=process"),
            &second_config.api_credential,
        ))
        .await
        .unwrap();
    assert_eq!(foreign_distribution.status(), StatusCode::NOT_FOUND);
}
