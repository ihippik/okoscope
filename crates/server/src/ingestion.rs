use event_model::{EVENT_SCHEMA_VERSION, RuntimeEvent};
use serde_json::to_value;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::auth::SessionScope;
use crate::grouping::{GroupingSource, TrustedGroupingScope, assign_event};

#[derive(Clone, Copy, Debug)]
pub struct IngestionContext {
    pub scope: SessionScope,
    pub agent_id: Uuid,
}

#[derive(Debug, Error)]
pub enum IngestionError {
    #[error("unsupported event schema version {0}")]
    UnsupportedSchema(u32),
    #[error("event project/application is outside the authenticated tenant")]
    InvalidOwnership,
    #[error("event cgroup ID exceeds PostgreSQL signed integer range")]
    CgroupOverflow,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("event payload serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub async fn persist_batch(
    pool: &PgPool,
    context: IngestionContext,
    events: &[RuntimeEvent],
) -> Result<u32, IngestionError> {
    let mut tx = pool.begin().await?;
    let mut accepted = 0_u32;
    for event in events {
        accepted = accepted.saturating_add(persist_event(&mut tx, context, event).await?);
    }
    tx.commit().await?;
    Ok(accepted)
}

async fn persist_event(
    tx: &mut Transaction<'_, Postgres>,
    context: IngestionContext,
    event: &RuntimeEvent,
) -> Result<u32, IngestionError> {
    if event.schema_version != EVENT_SCHEMA_VERSION {
        return Err(IngestionError::UnsupportedSchema(event.schema_version));
    }
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM applications WHERE id = $1 AND project_id = $2 AND organization_id = $3)",
    )
    .bind(event.attribution.application_id)
    .bind(event.attribution.project_id)
    .bind(context.scope.organization_id)
    .fetch_one(&mut **tx)
    .await?;
    if !owned {
        return Err(IngestionError::InvalidOwnership);
    }
    let cgroup_id =
        i64::try_from(event.process.cgroup_id).map_err(|_| IngestionError::CgroupOverflow)?;
    let raw_event_id = Uuid::new_v4();
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO runtime_events (id, event_id, organization_id, project_id, cluster_id, application_id, agent_id, observed_at, node_name, namespace, pod_uid, pod_name, container_id, container_name, workload_uid, workload_kind, workload_name, cgroup_id, pid, tgid, process_command, event_kind, event_schema_version, payload) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24) ON CONFLICT (agent_id, event_id) DO NOTHING RETURNING id",
    )
    .bind(raw_event_id).bind(event.id).bind(context.scope.organization_id).bind(event.attribution.project_id)
    .bind(context.scope.cluster_id).bind(event.attribution.application_id).bind(context.agent_id).bind(event.observed_at)
    .bind(&event.attribution.node_name).bind(&event.attribution.namespace).bind(&event.attribution.pod_uid).bind(&event.attribution.pod_name)
    .bind(&event.attribution.container_id).bind(&event.attribution.container_name).bind(&event.attribution.workload_uid)
    .bind(&event.attribution.workload_kind).bind(&event.attribution.workload_name).bind(cgroup_id).bind(i64::from(event.process.pid))
    .bind(i64::from(event.process.tgid)).bind(&event.process.command).bind(event.kind()).bind(i32::try_from(event.schema_version).unwrap_or(i32::MAX))
    .bind(to_value(&event.payload)?).fetch_optional(&mut **tx).await?;
    let Some(raw_event_id) = inserted else {
        crate::metrics::record_duplicate_event();
        return Ok(0);
    };
    let scope = TrustedGroupingScope {
        organization_id: context.scope.organization_id,
        project_id: event.attribution.project_id,
        application_id: event.attribution.application_id,
        cluster_id: context.scope.cluster_id,
        namespace: &event.attribution.namespace,
        workload_kind: &event.attribution.workload_kind,
        workload_name: &event.attribution.workload_name,
    };
    assign_event(tx, raw_event_id, &scope, event, GroupingSource::Live).await?;
    Ok(1)
}
