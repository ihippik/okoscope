use std::time::{Duration, Instant};

use axum::{
    body::{Body, to_bytes},
    http::{Request, header::AUTHORIZATION},
};
use chrono::{Duration as ChronoDuration, Utc};
use event_model::{
    EVENT_SCHEMA_VERSION, EventPayload, KubernetesAttribution, ProcessExec, ProcessIdentity,
    RuntimeEvent,
};
use server::{
    auth::SessionScope,
    bootstrap::{BootstrapConfig, bootstrap},
    ingestion::{IngestionContext, persist_batch},
    inventory_api, releases,
};
use tower::ServiceExt;
use uuid::Uuid;

const EVENT_COUNT: usize = 10_000;
const ITEM_COUNT: usize = 1_000;
const POD_COUNT: usize = 200;
const MAX_PROJECTION_DURATION: Duration = Duration::from_secs(60);
const MAX_LIST_QUERY_DURATION: Duration = Duration::from_secs(2);
const MAX_DETAIL_QUERY_DURATION: Duration = Duration::from_secs(2);
const SAMPLE_COUNT: usize = 30;

fn config() -> BootstrapConfig {
    BootstrapConfig {
        organization_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        cluster_id: Uuid::new_v4(),
        application_id: Uuid::new_v4(),
        organization_slug: "inventory-benchmark".into(),
        organization_name: "Inventory benchmark".into(),
        project_slug: "benchmark".into(),
        project_name: "Benchmark".into(),
        cluster_external_id: "benchmark".into(),
        cluster_name: "Benchmark".into(),
        application_slug: "benchmark".into(),
        application_name: "Benchmark".into(),
        cluster_credential: "cluster-inventory-benchmark".into(),
        api_credential: "api-inventory-benchmark".into(),
    }
}

#[sqlx::test(migrator = "server::database::MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL; run explicitly for acceptance"]
async fn inventory_projection_and_read_queries_meet_documented_acceptance_limits(
    pool: sqlx::PgPool,
) {
    let ids = bootstrap(&pool, &config()).await.unwrap();
    let baseline_id = Uuid::new_v4();
    let target_id = Uuid::new_v4();
    sqlx::query("INSERT INTO releases(id,organization_id,project_id,application_id,version,deployed_at) VALUES($1,$2,$3,$4,'benchmark-baseline',now()-interval '1 hour'),($5,$2,$3,$4,'benchmark-target',now())")
        .bind(baseline_id).bind(ids.organization_id).bind(ids.project_id).bind(ids.application_id).bind(target_id)
        .execute(&pool).await.unwrap();
    let agent_id = Uuid::new_v4();
    sqlx::query("INSERT INTO agents(id,organization_id,cluster_id,node_name,agent_version) VALUES($1,$2,$3,'benchmark-node','benchmark')")
        .bind(agent_id).bind(ids.organization_id).bind(ids.cluster_id).execute(&pool).await.unwrap();
    let events: Vec<_> = (0..EVENT_COUNT)
        .map(|index| RuntimeEvent {
            id: Uuid::new_v4(),
            observed_at: Utc::now()
                - ChronoDuration::minutes(i64::try_from(index).unwrap_or(i64::MAX)),
            schema_version: EVENT_SCHEMA_VERSION,
            attribution: KubernetesAttribution {
                project_id: ids.project_id,
                application_id: ids.application_id,
                node_name: "benchmark-node".into(),
                namespace: if index % 2 == 0 {
                    "production"
                } else {
                    "canary"
                }
                .into(),
                pod_uid: format!("pod-{}", index % POD_COUNT),
                pod_name: format!("benchmark-{}", index % POD_COUNT),
                container_id: format!("container-{index}"),
                container_name: "benchmark".into(),
                workload_uid: "benchmark-workload".into(),
                workload_kind: "Deployment".into(),
                workload_name: "benchmark".into(),
                release: Some(
                    if index % 2 == 0 {
                        "benchmark-baseline"
                    } else {
                        "benchmark-target"
                    }
                    .into(),
                ),
            },
            process: ProcessIdentity {
                cgroup_id: u64::try_from(index + 1).unwrap(),
                pid: u32::try_from(index + 1).unwrap(),
                tgid: u32::try_from(index + 1).unwrap(),
                command: "benchmark".into(),
            },
            payload: EventPayload::ProcessExec(ProcessExec {
                executable: format!("/app/bin/{}", index % ITEM_COUNT),
                parent_command: None,
            }),
        })
        .collect();

    let projection_started = Instant::now();
    let accepted = persist_batch(
        &pool,
        IngestionContext {
            scope: SessionScope {
                organization_id: ids.organization_id,
                cluster_id: ids.cluster_id,
            },
            agent_id,
        },
        &events,
    )
    .await
    .unwrap();
    let projection_duration = projection_started.elapsed();
    assert_eq!(usize::try_from(accepted).unwrap(), EVENT_COUNT);
    assert!(projection_duration <= MAX_PROJECTION_DURATION);

    let list_started = Instant::now();
    let listed: Vec<(Uuid, i64, i64)> = sqlx::query_as(
        "SELECT i.id,i.occurrence_count,(SELECT count(*) FROM runtime_inventory_sightings s WHERE s.item_id=i.id) sighting_count FROM runtime_inventory_items i WHERE i.organization_id=$1 AND i.project_id=$2 AND i.application_id=$3 AND i.identity_version=1 ORDER BY i.last_seen_at DESC,i.id DESC LIMIT 200",
    )
    .bind(ids.organization_id).bind(ids.project_id).bind(ids.application_id)
    .fetch_all(&pool).await.unwrap();
    let list_duration = list_started.elapsed();
    assert_eq!(listed.len(), ITEM_COUNT.min(200));
    assert!(list_duration <= MAX_LIST_QUERY_DURATION);

    let detail_started = Instant::now();
    let detail_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM runtime_inventory_event_memberships WHERE item_id=$1",
    )
    .bind(listed[0].0)
    .fetch_one(&pool)
    .await
    .unwrap();
    let detail_duration = detail_started.elapsed();
    assert_eq!(
        detail_count,
        i64::try_from(EVENT_COUNT / ITEM_COUNT).unwrap()
    );
    assert!(detail_duration <= MAX_DETAIL_QUERY_DURATION);

    let credential = config().api_credential;
    let inventory = inventory_api::router(pool.clone());
    let release_api = releases::router(pool.clone());
    let distribution_uri = format!(
        "/api/v1/projects/{}/applications/{}/runtime-inventory/distribution?kind=process&release_id={baseline_id}&namespace=production&workload_kind=Deployment&limit=10",
        ids.project_id, ids.application_id
    );
    let diff_uri = format!(
        "/api/v1/projects/{}/applications/{}/releases/{target_id}/runtime-diff/summary?baseline_id={baseline_id}&limit=10",
        ids.project_id, ids.application_id
    );
    sqlx::query("ANALYZE runtime_inventory_items")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("ANALYZE runtime_event_group_releases")
        .execute(&pool)
        .await
        .unwrap();
    let mut distribution_samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut diff_samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut max_response_bytes = 0usize;
    for _ in 0..SAMPLE_COUNT {
        let started = Instant::now();
        let response = inventory
            .clone()
            .oneshot(authenticated_request(&distribution_uri, &credential))
            .await
            .unwrap();
        assert!(response.status().is_success());
        distribution_samples.push(started.elapsed());
        max_response_bytes = max_response_bytes.max(
            to_bytes(response.into_body(), 1_048_576)
                .await
                .unwrap()
                .len(),
        );

        let started = Instant::now();
        let response = release_api
            .clone()
            .oneshot(authenticated_request(&diff_uri, &credential))
            .await
            .unwrap();
        assert!(response.status().is_success());
        diff_samples.push(started.elapsed());
        max_response_bytes = max_response_bytes.max(
            to_bytes(response.into_body(), 1_048_576)
                .await
                .unwrap()
                .len(),
        );
    }
    distribution_samples.sort_unstable();
    diff_samples.sort_unstable();
    let mut plans = Vec::new();
    for kind in ["process", "destination", "domain", "syscall"] {
        let plan: Vec<String> = sqlx::query_scalar("EXPLAIN SELECT identity_digest,occurrence_count FROM runtime_inventory_items WHERE organization_id=$1 AND project_id=$2 AND application_id=$3 AND identity_version=1 AND inventory_kind=$4 ORDER BY occurrence_count DESC,identity_digest ASC LIMIT 10")
            .bind(ids.organization_id).bind(ids.project_id).bind(ids.application_id).bind(kind).fetch_all(&pool).await.unwrap();
        plans.push((kind, plan));
    }
    let diff_plan: Vec<String> = sqlx::query_scalar("EXPLAIN WITH b AS (SELECT group_id,occurrence_count FROM runtime_event_group_releases WHERE release_id=$1),t AS (SELECT group_id,occurrence_count FROM runtime_event_group_releases WHERE release_id=$2) SELECT COALESCE(t.group_id,b.group_id) FROM b FULL OUTER JOIN t ON t.group_id=b.group_id ORDER BY ABS(COALESCE(t.occurrence_count,0)-COALESCE(b.occurrence_count,0)) DESC LIMIT 10")
        .bind(baseline_id).bind(target_id).fetch_all(&pool).await.unwrap();

    eprintln!(
        "runtime inventory benchmark: events={EVENT_COUNT} items={ITEM_COUNT} pods={POD_COUNT} projection_ms={} list_ms={} detail_ms={} distribution_p50_ms={} distribution_p95_ms={} distribution_p99_ms={} diff_p50_ms={} diff_p95_ms={} diff_p99_ms={} max_response_bytes={} inventory_plans={:?} diff_plan={:?}",
        projection_duration.as_millis(),
        list_duration.as_millis(),
        detail_duration.as_millis(),
        percentile(&distribution_samples, 50).as_millis(),
        percentile(&distribution_samples, 95).as_millis(),
        percentile(&distribution_samples, 99).as_millis(),
        percentile(&diff_samples, 50).as_millis(),
        percentile(&diff_samples, 95).as_millis(),
        percentile(&diff_samples, 99).as_millis(),
        max_response_bytes,
        plans,
        diff_plan,
    );
}

fn authenticated_request(uri: &str, credential: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::empty())
        .unwrap()
}

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    let index = ((samples.len() - 1) * percentile).div_ceil(100);
    samples[index]
}
