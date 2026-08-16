use chrono::{Duration, Utc};
use event_model::{
    EVENT_SCHEMA_VERSION, EventPayload, KubernetesAttribution, ProcessExec, ProcessIdentity,
    RuntimeEvent,
};
use server::{
    auth::SessionScope,
    bootstrap::{BootstrapConfig, bootstrap},
    ingestion::{IngestionContext, IngestionError, persist_batch},
};
use uuid::Uuid;

fn config(organization: &str) -> BootstrapConfig {
    BootstrapConfig {
        organization_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        cluster_id: Uuid::new_v4(),
        application_id: Uuid::new_v4(),
        organization_slug: organization.into(),
        organization_name: organization.into(),
        project_slug: "payments".into(),
        project_name: "Payments".into(),
        cluster_external_id: "local".into(),
        cluster_name: "Local".into(),
        application_slug: "payment-api".into(),
        application_name: "Payment API".into(),
        cluster_credential: format!("credential-{organization}"),
    }
}

fn event(project_id: Uuid, application_id: Uuid) -> RuntimeEvent {
    RuntimeEvent {
        id: Uuid::new_v4(),
        observed_at: Utc::now() - Duration::seconds(2),
        schema_version: EVENT_SCHEMA_VERSION,
        attribution: KubernetesAttribution {
            project_id,
            application_id,
            node_name: "node-1".into(),
            namespace: "production".into(),
            pod_uid: "pod-uid".into(),
            pod_name: "payment-api-1".into(),
            container_id: "abc".into(),
            container_name: "payment-api".into(),
            workload_uid: "deployment-uid".into(),
            workload_kind: "Deployment".into(),
            workload_name: "payment-api".into(),
        },
        process: ProcessIdentity {
            cgroup_id: 42,
            pid: 100,
            tgid: 100,
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
async fn batch_is_tenant_safe_idempotent_and_preserves_timestamps(pool: sqlx::PgPool) {
    let first = bootstrap(&pool, &config("first")).await.unwrap();
    let second = bootstrap(&pool, &config("second")).await.unwrap();
    let agent_id = Uuid::new_v4();
    sqlx::query("INSERT INTO agents (id, organization_id, cluster_id, node_name, agent_version) VALUES ($1,$2,$3,'node-1','test')")
        .bind(agent_id)
        .bind(first.organization_id)
        .bind(first.cluster_id)
        .execute(&pool)
        .await
        .unwrap();
    let context = IngestionContext {
        scope: SessionScope {
            organization_id: first.organization_id,
            cluster_id: first.cluster_id,
        },
        agent_id,
    };
    let valid = event(first.project_id, first.application_id);
    assert_eq!(
        persist_batch(&pool, context, std::slice::from_ref(&valid))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        persist_batch(&pool, context, std::slice::from_ref(&valid))
            .await
            .unwrap(),
        0
    );
    let received_at: chrono::DateTime<Utc> = sqlx::query_scalar(
        "SELECT received_at FROM runtime_events WHERE agent_id=$1 AND event_id=$2",
    )
    .bind(agent_id)
    .bind(valid.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(received_at > valid.observed_at);
    let foreign = event(second.project_id, second.application_id);
    assert!(matches!(
        persist_batch(&pool, context, &[foreign]).await,
        Err(IngestionError::InvalidOwnership)
    ));
}
