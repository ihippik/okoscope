use chrono::{Duration, Utc};
use event_model::{
    EVENT_SCHEMA_VERSION, EventPayload, KubernetesAttribution, ProcessExec, ProcessIdentity,
    RuntimeEvent, SyscallEvent,
};
use server::{
    auth::SessionScope,
    backfill::{BackfillOptions, run as run_backfill},
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
        api_credential: format!("api-credential-{organization}"),
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
            release: None,
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
    let (group_id, representative_event_id, occurrence_count, first_seen_at, last_seen_at): (
        Uuid,
        Uuid,
        i64,
        chrono::DateTime<Utc>,
        chrono::DateTime<Utc>,
    ) = sqlx::query_as(
        "SELECT id, representative_event_id, occurrence_count, first_seen_at, last_seen_at FROM runtime_event_groups WHERE organization_id=$1",
    )
    .bind(first.organization_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(occurrence_count, 1);
    assert_eq!(first_seen_at, valid.observed_at);
    assert_eq!(last_seen_at, valid.observed_at);
    let membership_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM runtime_event_group_memberships WHERE group_id=$1",
    )
    .bind(group_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(membership_count, 1);
    let outbox_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outbox_messages WHERE aggregate_id=$1 AND topic='runtime_group.first_seen'",
    )
    .bind(group_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(outbox_count, 1);

    let mut delayed = valid.clone();
    delayed.id = Uuid::new_v4();
    delayed.observed_at -= Duration::minutes(5);
    assert_eq!(
        persist_batch(&pool, context, &[delayed.clone()])
            .await
            .unwrap(),
        1
    );
    let (new_representative, count, first_seen, last_seen): (
        Uuid,
        i64,
        chrono::DateTime<Utc>,
        chrono::DateTime<Utc>,
    ) = sqlx::query_as(
        "SELECT representative_event_id, occurrence_count, first_seen_at, last_seen_at FROM runtime_event_groups WHERE id=$1",
    )
    .bind(group_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(new_representative, representative_event_id);
    assert_eq!(count, 2);
    assert_eq!(first_seen, delayed.observed_at);
    assert_eq!(last_seen, valid.observed_at);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox_messages WHERE aggregate_id=$1")
            .bind(group_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
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

    let constraint_error = sqlx::query(
        "INSERT INTO runtime_event_group_memberships (organization_id, project_id, application_id, event_id, group_id, fingerprint_version) VALUES ($1,$2,$3,$4,$5,1)",
    )
    .bind(first.organization_id)
    .bind(first.project_id)
    .bind(first.application_id)
    .bind(Uuid::new_v4())
    .bind(group_id)
    .execute(&pool)
    .await;
    assert!(constraint_error.is_err());
}

#[sqlx::test(migrator = "server::database::MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn concurrent_first_occurrences_share_one_group(pool: sqlx::PgPool) {
    let ids = bootstrap(&pool, &config("concurrent")).await.unwrap();
    let agent_id = Uuid::new_v4();
    sqlx::query("INSERT INTO agents (id, organization_id, cluster_id, node_name, agent_version) VALUES ($1,$2,$3,'node-1','test')")
        .bind(agent_id)
        .bind(ids.organization_id)
        .bind(ids.cluster_id)
        .execute(&pool)
        .await
        .unwrap();
    let context = IngestionContext {
        scope: SessionScope {
            organization_id: ids.organization_id,
            cluster_id: ids.cluster_id,
        },
        agent_id,
    };
    let first = event(ids.project_id, ids.application_id);
    let mut second = first.clone();
    second.id = Uuid::new_v4();
    let first_batch = [first];
    let second_batch = [second];
    let (first_result, second_result) = tokio::join!(
        persist_batch(&pool, context, &first_batch),
        persist_batch(&pool, context, &second_batch)
    );
    assert_eq!(first_result.unwrap(), 1);
    assert_eq!(second_result.unwrap(), 1);
    let (groups, occurrences, memberships, outbox): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM runtime_event_groups), (SELECT coalesce(sum(occurrence_count),0)::bigint FROM runtime_event_groups), (SELECT count(*) FROM runtime_event_group_memberships), (SELECT count(*) FROM outbox_messages)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((groups, occurrences, memberships, outbox), (1, 2, 2, 1));

    let mut distinct = event(ids.project_id, ids.application_id);
    distinct.payload = EventPayload::Syscall(SyscallEvent {
        name: "ptrace".into(),
    });
    assert_eq!(persist_batch(&pool, context, &[distinct]).await.unwrap(), 1);
    let totals: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM runtime_event_groups), (SELECT coalesce(sum(occurrence_count),0)::bigint FROM runtime_event_groups), (SELECT count(*) FROM runtime_event_group_memberships), (SELECT count(*) FROM outbox_messages)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(totals, (2, 3, 3, 2));
}

#[sqlx::test(migrator = "server::database::MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn backfill_is_restartable_and_marks_historical_outbox(pool: sqlx::PgPool) {
    let ids = bootstrap(&pool, &config("backfill")).await.unwrap();
    let agent_id = Uuid::new_v4();
    sqlx::query("INSERT INTO agents (id,organization_id,cluster_id,node_name,agent_version) VALUES ($1,$2,$3,'node-1','test')")
        .bind(agent_id).bind(ids.organization_id).bind(ids.cluster_id).execute(&pool).await.unwrap();
    let context = IngestionContext {
        scope: SessionScope {
            organization_id: ids.organization_id,
            cluster_id: ids.cluster_id,
        },
        agent_id,
    };
    let first = event(ids.project_id, ids.application_id);
    let mut second = first.clone();
    second.id = Uuid::new_v4();
    persist_batch(&pool, context, &[first, second])
        .await
        .unwrap();
    sqlx::query("DELETE FROM outbox_messages")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM runtime_event_group_memberships")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM runtime_event_groups")
        .execute(&pool)
        .await
        .unwrap();

    let options = BackfillOptions {
        organization_id: ids.organization_id,
        project_id: ids.project_id,
        fingerprint_version: 1,
        batch_size: 1,
        throttle: std::time::Duration::ZERO,
    };
    let first_run = run_backfill(&pool, options).await.unwrap();
    assert_eq!(first_run.scanned, 2);
    assert_eq!(first_run.grouped, 2);
    assert_eq!(first_run.groups_created, 1);
    assert_eq!(run_backfill(&pool, options).await.unwrap().scanned, 0);
    let (groups, occurrences, memberships, historical_outbox): (i64, i64, i64, i64) =
        sqlx::query_as("SELECT (SELECT count(*) FROM runtime_event_groups), (SELECT coalesce(sum(occurrence_count),0)::bigint FROM runtime_event_groups), (SELECT count(*) FROM runtime_event_group_memberships), (SELECT count(*) FROM outbox_messages WHERE source='backfill' AND processed_at IS NULL)")
            .fetch_one(&pool).await.unwrap();
    assert_eq!(
        (groups, occurrences, memberships, historical_outbox),
        (1, 2, 2, 1)
    );
}
