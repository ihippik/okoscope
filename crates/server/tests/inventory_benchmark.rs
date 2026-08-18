use std::time::{Duration, Instant};

use chrono::Utc;
use event_model::{
    EVENT_SCHEMA_VERSION, EventPayload, KubernetesAttribution, ProcessExec, ProcessIdentity,
    RuntimeEvent,
};
use server::{
    auth::SessionScope,
    bootstrap::{BootstrapConfig, bootstrap},
    ingestion::{IngestionContext, persist_batch},
};
use uuid::Uuid;

const EVENT_COUNT: usize = 1_000;
const ITEM_COUNT: usize = 100;
const POD_COUNT: usize = 20;
const MAX_PROJECTION_DURATION: Duration = Duration::from_secs(60);
const MAX_LIST_QUERY_DURATION: Duration = Duration::from_secs(2);
const MAX_DETAIL_QUERY_DURATION: Duration = Duration::from_secs(2);

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
    let agent_id = Uuid::new_v4();
    sqlx::query("INSERT INTO agents(id,organization_id,cluster_id,node_name,agent_version) VALUES($1,$2,$3,'benchmark-node','benchmark')")
        .bind(agent_id).bind(ids.organization_id).bind(ids.cluster_id).execute(&pool).await.unwrap();
    let events: Vec<_> = (0..EVENT_COUNT)
        .map(|index| RuntimeEvent {
            id: Uuid::new_v4(),
            observed_at: Utc::now(),
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
                release: None,
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
    assert_eq!(listed.len(), ITEM_COUNT);
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

    eprintln!(
        "runtime inventory benchmark: events={EVENT_COUNT} items={ITEM_COUNT} pods={POD_COUNT} projection_ms={} list_ms={} detail_ms={}",
        projection_duration.as_millis(),
        list_duration.as_millis(),
        detail_duration.as_millis()
    );
}
