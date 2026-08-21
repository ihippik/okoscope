use chrono::{Duration, Utc};
use event_model::{
    DnsAddressAnswer, DnsContext, DnsDirection, DnsName, DnsQueryType, DnsResponseCode,
    DnsTransport, EVENT_SCHEMA_VERSION, EventPayload, FileActivityPath, FileModify,
    KubernetesAttribution, NetworkAddressFamily, NetworkConnect, NetworkConnectOutcome,
    NetworkDnsResponse, ProcessExec, ProcessIdentity, RuntimeEvent, SyscallEvent,
};
use server::{
    auth::SessionScope,
    backfill::{BackfillOptions, run as run_backfill},
    bootstrap::{BootstrapConfig, bootstrap},
    ingestion::{IngestionContext, IngestionError, persist_batch},
    inventory_operations::{
        InventoryBackfillOptions, InventoryBackfillStats, backfill as backfill_inventory,
        reconcile as reconcile_inventory,
    },
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
async fn file_activity_is_persisted_grouped_inventoried_and_replay_safe(pool: sqlx::PgPool) {
    let ids = bootstrap(&pool, &config("file-activity")).await.unwrap();
    let agent_id = Uuid::new_v4();
    sqlx::query("INSERT INTO agents(id,organization_id,cluster_id,node_name,agent_version) VALUES($1,$2,$3,'node-file','test')")
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
    let mut file = event(ids.project_id, ids.application_id);
    file.payload = EventPayload::FileModify(FileModify {
        path: FileActivityPath::new("/app/data/report").unwrap(),
    });
    assert_eq!(
        persist_batch(&pool, context, &[file.clone()])
            .await
            .unwrap(),
        1
    );
    assert_eq!(persist_batch(&pool, context, &[file]).await.unwrap(), 0);
    let (event_kind, inventory_kind, event_count, group_count, occurrence_count):
        (String, String, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT event_kind FROM runtime_events),(SELECT inventory_kind FROM runtime_inventory_items),(SELECT count(*) FROM runtime_events),(SELECT count(*) FROM runtime_event_groups),(SELECT occurrence_count FROM runtime_inventory_items)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(event_kind, "file.modify");
    assert_eq!(inventory_kind, "file_activity");
    assert_eq!((event_count, group_count, occurrence_count), (1, 1, 1));
}

#[sqlx::test(migrator = "server::database::MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn inventory_projection_is_concurrent_idempotent_scoped_and_transactional(
    pool: sqlx::PgPool,
) {
    let ids = bootstrap(&pool, &config("inventory-projection"))
        .await
        .unwrap();
    let agent_id = Uuid::new_v4();
    sqlx::query("INSERT INTO agents(id,organization_id,cluster_id,node_name,agent_version) VALUES($1,$2,$3,'node-inventory','test')")
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
    let now = Utc::now();
    let release_id = Uuid::new_v4();
    sqlx::query("INSERT INTO releases(id,organization_id,project_id,application_id,version,deployed_at) VALUES($1,$2,$3,$4,'inventory-v1',$5)")
        .bind(release_id)
        .bind(ids.organization_id)
        .bind(ids.project_id)
        .bind(ids.application_id)
        .bind(now - Duration::days(1))
        .execute(&pool)
        .await
        .unwrap();
    let mut first = event(ids.project_id, ids.application_id);
    first.observed_at = now;
    first.attribution.release = Some("inventory-v1".into());
    first.attribution.pod_uid = "inventory-pod-a".into();
    let mut second = first.clone();
    second.id = Uuid::new_v4();
    second.observed_at = now + Duration::seconds(1);
    second.attribution.namespace = "canary".into();
    second.attribution.workload_name = "payment-api-canary".into();
    second.attribution.pod_uid = "inventory-pod-b".into();
    second.attribution.pod_name = "payment-api-canary-1".into();

    let (first_result, second_result) = tokio::join!(
        persist_batch(&pool, context, std::slice::from_ref(&first)),
        persist_batch(&pool, context, std::slice::from_ref(&second))
    );
    assert_eq!(first_result.unwrap(), 1);
    assert_eq!(second_result.unwrap(), 1);

    let (item_count, membership_count, group_count, group_link_count, sighting_count, occurrence_count): (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM runtime_inventory_items),(SELECT count(*) FROM runtime_inventory_event_memberships),(SELECT count(*) FROM runtime_event_groups),(SELECT count(*) FROM runtime_inventory_group_links),(SELECT count(*) FROM runtime_inventory_sightings),(SELECT occurrence_count FROM runtime_inventory_items)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (
            item_count,
            membership_count,
            group_count,
            group_link_count,
            sighting_count,
            occurrence_count
        ),
        (1, 2, 2, 2, 2, 2)
    );
    let release_occurrences: i64 = sqlx::query_scalar(
        "SELECT occurrence_count FROM runtime_inventory_releases WHERE release_id=$1",
    )
    .bind(release_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(release_occurrences, 2);

    assert_eq!(
        persist_batch(&pool, context, std::slice::from_ref(&first))
            .await
            .unwrap(),
        0
    );
    let unchanged: i64 = sqlx::query_scalar("SELECT occurrence_count FROM runtime_inventory_items")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(unchanged, 2);

    let mut delayed = first.clone();
    delayed.id = Uuid::new_v4();
    delayed.observed_at = now - Duration::hours(1);
    delayed.attribution.pod_uid = "inventory-pod-c".into();
    delayed.attribution.pod_name = "payment-api-old".into();
    persist_batch(&pool, context, &[delayed]).await.unwrap();
    let (count, first_seen, last_seen): (i64, chrono::DateTime<Utc>, chrono::DateTime<Utc>) =
        sqlx::query_as(
            "SELECT occurrence_count,first_seen_at,last_seen_at FROM runtime_inventory_items",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 3);
    assert_eq!(first_seen, now - Duration::hours(1));
    assert_eq!(last_seen, now + Duration::seconds(1));

    let mut rejected = first.clone();
    rejected.id = Uuid::new_v4();
    rejected.attribution.application_id = Uuid::new_v4();
    let mut valid_before_rejection = first.clone();
    valid_before_rejection.id = Uuid::new_v4();
    valid_before_rejection.attribution.pod_uid = "rolled-back-pod".into();
    let before_events: i64 = sqlx::query_scalar("SELECT count(*) FROM runtime_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        persist_batch(&pool, context, &[valid_before_rejection, rejected])
            .await
            .is_err()
    );
    let (after_events, after_inventory): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM runtime_events),(SELECT occurrence_count FROM runtime_inventory_items)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after_events, before_events);
    assert_eq!(after_inventory, 3);

    sqlx::query("DELETE FROM runtime_inventory_items WHERE application_id=$1")
        .bind(ids.application_id)
        .execute(&pool)
        .await
        .unwrap();
    let backfill_options = InventoryBackfillOptions {
        organization_id: ids.organization_id,
        project_id: ids.project_id,
        application_id: Some(ids.application_id),
        identity_version: 1,
        batch_size: 2,
        throttle: std::time::Duration::ZERO,
    };
    let backfilled = backfill_inventory(&pool, backfill_options).await.unwrap();
    assert_eq!(backfilled.scanned, 3);
    assert_eq!(backfilled.projected, 3);
    assert_eq!(backfilled.skipped, 0);
    assert_eq!(backfilled.items_created, 1);
    let resumed = backfill_inventory(&pool, backfill_options).await.unwrap();
    assert_eq!(resumed, InventoryBackfillStats::default());
    let reconciliation = reconcile_inventory(
        &pool,
        ids.organization_id,
        ids.project_id,
        ids.application_id,
        1,
    )
    .await
    .unwrap();
    assert!(reconciliation.is_consistent());
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
    let (group_id, representative_event_id, first_seen_event_id, occurrence_count, first_seen_at, last_seen_at): (
        Uuid,
        Uuid,
        Uuid,
        i64,
        chrono::DateTime<Utc>,
        chrono::DateTime<Utc>,
    ) = sqlx::query_as(
        "SELECT id, representative_event_id, first_seen_event_id, occurrence_count, first_seen_at, last_seen_at FROM runtime_event_groups WHERE organization_id=$1",
    )
    .bind(first.organization_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(occurrence_count, 1);
    let valid_storage_id: Uuid =
        sqlx::query_scalar("SELECT id FROM runtime_events WHERE agent_id=$1 AND event_id=$2")
            .bind(agent_id)
            .bind(valid.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(first_seen_event_id, valid_storage_id);
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
    let (new_representative, new_first_seen_event, count, first_seen, last_seen): (
        Uuid,
        Uuid,
        i64,
        chrono::DateTime<Utc>,
        chrono::DateTime<Utc>,
    ) = sqlx::query_as(
        "SELECT representative_event_id, first_seen_event_id, occurrence_count, first_seen_at, last_seen_at FROM runtime_event_groups WHERE id=$1",
    )
    .bind(group_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(new_representative, representative_event_id);
    let delayed_storage_id: Uuid =
        sqlx::query_scalar("SELECT id FROM runtime_events WHERE agent_id=$1 AND event_id=$2")
            .bind(agent_id)
            .bind(delayed.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(new_first_seen_event, delayed_storage_id);
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
async fn network_events_persist_canonically_replay_and_share_safe_group(pool: sqlx::PgPool) {
    let ids = bootstrap(&pool, &config("network-storage")).await.unwrap();
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
    let mut succeeded = event(ids.project_id, ids.application_id);
    let dns_context = DnsContext::new(
        vec![DnsName::new("api.example.com").unwrap()],
        succeeded.observed_at - Duration::seconds(1),
        succeeded.observed_at + Duration::seconds(59),
    )
    .unwrap();
    succeeded.payload = EventPayload::NetworkConnect(
        NetworkConnect::new(
            NetworkAddressFamily::Ipv6,
            "2001:0db8::7".parse().unwrap(),
            443,
            NetworkConnectOutcome::Succeeded,
            None,
        )
        .unwrap()
        .with_dns_context(dns_context.clone()),
    );
    assert_eq!(
        persist_batch(&pool, context, std::slice::from_ref(&succeeded))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        persist_batch(&pool, context, std::slice::from_ref(&succeeded))
            .await
            .unwrap(),
        0
    );
    let mut failed = succeeded.clone();
    failed.id = Uuid::new_v4();
    failed.payload = EventPayload::NetworkConnect(
        NetworkConnect::new(
            NetworkAddressFamily::Ipv6,
            "2001:db8::7".parse().unwrap(),
            443,
            NetworkConnectOutcome::Failed,
            Some(111),
        )
        .unwrap(),
    );
    assert_eq!(persist_batch(&pool, context, &[failed]).await.unwrap(), 1);

    let (event_kind, payload): (String, serde_json::Value) = sqlx::query_as(
        "SELECT event_kind,payload FROM runtime_events WHERE agent_id=$1 AND event_id=$2",
    )
    .bind(agent_id)
    .bind(succeeded.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(event_kind, "network.connect");
    assert_eq!(payload["data"]["destination_address"], "2001:db8::7");
    let (groups, occurrences, outbox): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM runtime_event_groups), (SELECT occurrence_count FROM runtime_event_groups), (SELECT count(*) FROM outbox_messages)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((groups, occurrences, outbox), (1, 2, 1));
    let semantic: serde_json::Value = sqlx::query_scalar(
        "SELECT payload->'semantic' FROM outbox_messages WHERE topic='runtime_group.first_seen'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        semantic,
        serde_json::json!({
            "process_command": "sh",
            "address_family": "ipv6",
            "destination_address": "2001:db8::7",
            "destination_port": 443,
            "dns_context": dns_context
        })
    );
}

#[sqlx::test(migrator = "server::database::MIGRATOR")]
#[ignore = "requires a PostgreSQL server with DATABASE_URL"]
async fn dns_events_persist_replay_and_group_without_answer_identity(pool: sqlx::PgPool) {
    let ids = bootstrap(&pool, &config("dns-storage")).await.unwrap();
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
    let name = DnsName::new("api.example.com").unwrap();
    let mut first = event(ids.project_id, ids.application_id);
    first.payload = EventPayload::NetworkDnsResponse(NetworkDnsResponse {
        transaction_id: 1,
        direction: DnsDirection::Ingress,
        transport: DnsTransport::Udp,
        resolver_address: "10.96.0.10".parse().unwrap(),
        name: name.clone(),
        query_type: DnsQueryType::A,
        response_code: DnsResponseCode::NoError,
        truncated: false,
        answers: vec![
            DnsAddressAnswer::new(name.clone(), "203.0.113.7".parse().unwrap(), 60).unwrap(),
        ],
        cname_chain: vec![],
        effective_ttl_seconds: Some(60),
    });
    assert_eq!(
        persist_batch(&pool, context, std::slice::from_ref(&first))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        persist_batch(&pool, context, std::slice::from_ref(&first))
            .await
            .unwrap(),
        0
    );
    let mut varied = first.clone();
    varied.id = Uuid::new_v4();
    let EventPayload::NetworkDnsResponse(response) = &mut varied.payload else {
        unreachable!()
    };
    response.transaction_id = 2;
    response.answers[0].address = "203.0.113.8".parse().unwrap();
    assert_eq!(persist_batch(&pool, context, &[varied]).await.unwrap(), 1);

    let (events, groups, occurrences, outbox): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM runtime_events), (SELECT count(*) FROM runtime_event_groups), (SELECT occurrence_count FROM runtime_event_groups), (SELECT count(*) FROM outbox_messages)",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!((events, groups, occurrences, outbox), (2, 1, 2, 1));
    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM runtime_events WHERE event_id=$1")
            .bind(first.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(payload["data"]["name"], "api.example.com");
    assert_eq!(payload["data"]["answers"][0]["address"], "203.0.113.7");
    assert!(payload.get("packet").is_none());
    let semantic: serde_json::Value =
        sqlx::query_scalar("SELECT payload->'semantic' FROM outbox_messages")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(semantic["name"], "api.example.com");
    assert!(semantic.get("answers").is_none());
    assert!(semantic.get("transaction_id").is_none());
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
