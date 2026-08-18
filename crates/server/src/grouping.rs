use std::fmt;

use event_model::{EventPayload, NetworkAddressFamily, RuntimeEvent};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

pub const CURRENT_FINGERPRINT_VERSION: FingerprintVersion = FingerprintVersion::V1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i16)]
pub enum FingerprintVersion {
    V1 = 1,
}

impl From<FingerprintVersion> for i16 {
    fn from(value: FingerprintVersion) -> Self {
        value as Self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeGroupStatus {
    #[default]
    Open,
    Acknowledged,
    Resolved,
}

impl fmt::Display for RuntimeGroupStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Open => "open",
            Self::Acknowledged => "acknowledged",
            Self::Resolved => "resolved",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OccurrenceSummary {
    pub event_kind: String,
    pub semantic: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedGroupingScope<'a> {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub application_id: Uuid,
    pub cluster_id: Uuid,
    pub namespace: &'a str,
    pub workload_kind: &'a str,
    pub workload_name: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventFingerprint {
    pub version: FingerprintVersion,
    pub digest: [u8; 32],
    pub summary: OccurrenceSummary,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FingerprintError {
    #[error("fingerprint field {0} must not be empty")]
    EmptyField(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupingSource {
    Live,
    Backfill,
}

impl GroupingSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Backfill => "backfill",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupingOutcome {
    pub group_id: Uuid,
    pub group_created: bool,
    pub membership_created: bool,
}

pub async fn assign_event(
    tx: &mut Transaction<'_, Postgres>,
    raw_event_id: Uuid,
    release_id: Option<Uuid>,
    scope: &TrustedGroupingScope<'_>,
    event: &RuntimeEvent,
    source: GroupingSource,
) -> Result<GroupingOutcome, sqlx::Error> {
    let started_at = std::time::Instant::now();
    let fingerprint =
        fingerprint_v1(scope, event).map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
    let version = i16::from(fingerprint.version);
    let candidate_group_id = Uuid::new_v4();
    let created_group_id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO runtime_event_groups (id, organization_id, project_id, cluster_id, application_id, namespace, workload_kind, workload_name, fingerprint_version, fingerprint_digest, event_kind, semantic_summary, first_seen_at, last_seen_at, occurrence_count, representative_event_id, first_seen_event_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$13,1,$14,$14) ON CONFLICT (organization_id, project_id, application_id, cluster_id, namespace, workload_kind, workload_name, fingerprint_version, fingerprint_digest) DO NOTHING RETURNING id",
    )
    .bind(candidate_group_id)
    .bind(scope.organization_id)
    .bind(scope.project_id)
    .bind(scope.cluster_id)
    .bind(scope.application_id)
    .bind(scope.namespace)
    .bind(scope.workload_kind)
    .bind(scope.workload_name)
    .bind(version)
    .bind(fingerprint.digest.as_slice())
    .bind(&fingerprint.summary.event_kind)
    .bind(&fingerprint.summary.semantic)
    .bind(event.observed_at)
    .bind(raw_event_id)
    .fetch_optional(&mut **tx)
    .await?;

    let group_created = created_group_id.is_some();
    let group_id = if let Some(group_id) = created_group_id {
        group_id
    } else {
        sqlx::query_scalar(
            "SELECT id FROM runtime_event_groups WHERE organization_id=$1 AND project_id=$2 AND application_id=$3 AND cluster_id=$4 AND namespace=$5 AND workload_kind=$6 AND workload_name=$7 AND fingerprint_version=$8 AND fingerprint_digest=$9 FOR UPDATE",
        )
        .bind(scope.organization_id)
        .bind(scope.project_id)
        .bind(scope.application_id)
        .bind(scope.cluster_id)
        .bind(scope.namespace)
        .bind(scope.workload_kind)
        .bind(scope.workload_name)
        .bind(version)
        .bind(fingerprint.digest.as_slice())
        .fetch_one(&mut **tx)
        .await?
    };

    let membership_created = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO runtime_event_group_memberships (organization_id, project_id, application_id, event_id, group_id, fingerprint_version, release_id) VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (event_id, fingerprint_version) DO NOTHING RETURNING event_id",
    )
    .bind(scope.organization_id)
    .bind(scope.project_id)
    .bind(scope.application_id)
    .bind(raw_event_id)
    .bind(group_id)
    .bind(version)
    .bind(release_id)
    .fetch_optional(&mut **tx)
    .await?
    .is_some();

    if membership_created && !group_created {
        update_group_occurrence(tx, group_id, raw_event_id, event).await?;
    }

    if membership_created && let Some(release_id) = release_id {
        update_release_summary(tx, scope, release_id, group_id, raw_event_id, event).await?;
    }

    if group_created {
        sqlx::query(
            "INSERT INTO outbox_messages (id, organization_id, project_id, topic, aggregate_id, schema_version, source, payload) VALUES ($1,$2,$3,'runtime_group.first_seen',$4,1,$5,$6) ON CONFLICT (topic, aggregate_id, schema_version) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(scope.organization_id)
        .bind(scope.project_id)
        .bind(group_id)
        .bind(source.as_str())
        .bind(json!({
            "group_id": group_id,
            "application_id": scope.application_id,
            "event_kind": fingerprint.summary.event_kind,
            "semantic": fingerprint.summary.semantic,
            "fingerprint_version": version,
        }))
        .execute(&mut **tx)
        .await?;
    }

    let outcome = GroupingOutcome {
        group_id,
        group_created,
        membership_created,
    };
    crate::metrics::record_grouping(
        u64::try_from(started_at.elapsed().as_micros()).unwrap_or(u64::MAX),
        group_created,
    );
    tracing::debug!(group_id=%group_id, group_created, membership_created, source=source.as_str(), "runtime event grouped");
    Ok(outcome)
}

async fn update_group_occurrence(
    tx: &mut Transaction<'_, Postgres>,
    group_id: Uuid,
    raw_event_id: Uuid,
    event: &RuntimeEvent,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE runtime_event_groups SET first_seen_event_id=CASE WHEN ($2,$3) < (first_seen_at,first_seen_event_id) THEN $3 ELSE first_seen_event_id END, first_seen_at=LEAST(first_seen_at,$2), last_seen_at=GREATEST(last_seen_at,$2), occurrence_count=occurrence_count+1, updated_at=now() WHERE id=$1",
    )
    .bind(group_id)
    .bind(event.observed_at)
    .bind(raw_event_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn update_release_summary(
    tx: &mut Transaction<'_, Postgres>,
    scope: &TrustedGroupingScope<'_>,
    release_id: Uuid,
    group_id: Uuid,
    raw_event_id: Uuid,
    event: &RuntimeEvent,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO runtime_event_group_releases (organization_id,project_id,application_id,release_id,group_id,occurrence_count,first_seen_at,last_seen_at,representative_event_id) VALUES ($1,$2,$3,$4,$5,1,$6,$6,$7) ON CONFLICT (release_id,group_id) DO UPDATE SET occurrence_count=runtime_event_group_releases.occurrence_count+1,first_seen_at=LEAST(runtime_event_group_releases.first_seen_at,EXCLUDED.first_seen_at),last_seen_at=GREATEST(runtime_event_group_releases.last_seen_at,EXCLUDED.last_seen_at),updated_at=now()",
    )
    .bind(scope.organization_id).bind(scope.project_id).bind(scope.application_id)
    .bind(release_id).bind(group_id).bind(event.observed_at).bind(raw_event_id)
    .execute(&mut **tx).await?;
    crate::metrics::record_release_summary();
    Ok(())
}

pub fn fingerprint_v1(
    scope: &TrustedGroupingScope<'_>,
    event: &RuntimeEvent,
) -> Result<EventFingerprint, FingerprintError> {
    let namespace = required("namespace", scope.namespace)?;
    let workload_kind = required("workload_kind", scope.workload_kind)?;
    let workload_name = required("workload_name", scope.workload_name)?;
    let event_kind = event.kind();
    let mut encoder = CanonicalEncoder::new();
    encoder.field(b"okoscope.runtime-event-group");
    encoder.field(&i16::from(FingerprintVersion::V1).to_be_bytes());
    encoder.field(scope.organization_id.as_bytes());
    encoder.field(scope.project_id.as_bytes());
    encoder.field(scope.application_id.as_bytes());
    encoder.field(scope.cluster_id.as_bytes());
    encoder.field(namespace.as_bytes());
    encoder.field(workload_kind.as_bytes());
    encoder.field(workload_name.as_bytes());
    encoder.field(event_kind.as_bytes());

    let semantic = match &event.payload {
        EventPayload::ProcessExec(process) => {
            let executable = required("executable", &process.executable)?;
            encoder.field(executable.as_bytes());
            json!({"executable": executable})
        }
        EventPayload::Syscall(syscall) => {
            let command = required("process_command", &event.process.command)?;
            let syscall_name = required("syscall_name", &syscall.name)?.to_ascii_lowercase();
            encoder.field(command.as_bytes());
            encoder.field(syscall_name.as_bytes());
            json!({"process_command": command, "syscall": syscall_name})
        }
        EventPayload::NetworkConnect(network) => {
            let command = required("process_command", &event.process.command)?;
            let family = match network.address_family {
                NetworkAddressFamily::Ipv4 => "ipv4",
                NetworkAddressFamily::Ipv6 => "ipv6",
            };
            encoder.field(command.as_bytes());
            encoder.field(family.as_bytes());
            match network.destination_address {
                std::net::IpAddr::V4(address) => encoder.field(&address.octets()),
                std::net::IpAddr::V6(address) => encoder.field(&address.octets()),
            }
            encoder.field(&network.destination_port.to_be_bytes());
            let mut semantic = json!({
                "process_command": command,
                "address_family": family,
                "destination_address": network.destination_address,
                "destination_port": network.destination_port
            });
            if let Some(context) = &network.dns_context {
                semantic["dns_context"] = json!(context);
            }
            semantic
        }
        EventPayload::NetworkDnsQuery(query) => {
            let command = required("process_command", &event.process.command)?;
            encoder.field(command.as_bytes());
            encoder.field(query.name.as_str().as_bytes());
            encoder.field(format!("{:?}", query.query_type).as_bytes());
            json!({
                "process_command": command,
                "name": query.name,
                "query_type": query.query_type,
                "transport": query.transport,
                "direction": query.direction
            })
        }
        EventPayload::NetworkDnsResponse(response) => {
            let command = required("process_command", &event.process.command)?;
            encoder.field(command.as_bytes());
            encoder.field(response.name.as_str().as_bytes());
            encoder.field(format!("{:?}", response.query_type).as_bytes());
            encoder.field(format!("{:?}", response.response_code).as_bytes());
            json!({
                "process_command": command,
                "name": response.name,
                "query_type": response.query_type,
                "response_code": response.response_code,
                "transport": response.transport,
                "direction": response.direction
            })
        }
    };

    Ok(EventFingerprint {
        version: FingerprintVersion::V1,
        digest: encoder.finish(),
        summary: OccurrenceSummary {
            event_kind: event_kind.to_owned(),
            semantic,
        },
    })
}

fn required<'a>(name: &'static str, value: &'a str) -> Result<&'a str, FingerprintError> {
    let normalized =
        value.trim_matches(|character: char| character == '\0' || character.is_whitespace());
    if normalized.is_empty() {
        Err(FingerprintError::EmptyField(name))
    } else {
        Ok(normalized)
    }
}

#[derive(Debug)]
struct CanonicalEncoder(Sha256);

impl CanonicalEncoder {
    fn new() -> Self {
        Self(Sha256::new())
    }

    fn field(&mut self, bytes: &[u8]) {
        self.0
            .update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
        self.0.update(bytes);
    }

    fn finish(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use event_model::{
        DnsAddressAnswer, DnsContext, DnsDirection, DnsName, DnsQueryType, DnsResponseCode,
        DnsTransport, EventPayload, KubernetesAttribution, NetworkAddressFamily, NetworkConnect,
        NetworkConnectOutcome, NetworkDnsQuery, NetworkDnsResponse, ProcessExec, ProcessIdentity,
        RuntimeEvent, SyscallEvent,
    };

    use super::*;

    fn event(payload: EventPayload) -> RuntimeEvent {
        RuntimeEvent {
            id: Uuid::from_u128(50),
            observed_at: Utc::now(),
            schema_version: event_model::EVENT_SCHEMA_VERSION,
            attribution: KubernetesAttribution {
                project_id: Uuid::from_u128(2),
                application_id: Uuid::from_u128(3),
                node_name: "node-a".into(),
                namespace: "payments".into(),
                pod_uid: "pod-uid-a".into(),
                pod_name: "payment-api-abc".into(),
                container_id: "container-a".into(),
                container_name: "api".into(),
                workload_uid: "deployment-uid-a".into(),
                workload_kind: "Deployment".into(),
                workload_name: "payment-api".into(),
                release: None,
            },
            process: ProcessIdentity {
                cgroup_id: 10,
                pid: 100,
                tgid: 100,
                command: "curl".into(),
            },
            payload,
        }
    }

    fn scope(event: &RuntimeEvent) -> TrustedGroupingScope<'_> {
        TrustedGroupingScope {
            organization_id: Uuid::from_u128(1),
            project_id: event.attribution.project_id,
            application_id: event.attribution.application_id,
            cluster_id: Uuid::from_u128(4),
            namespace: &event.attribution.namespace,
            workload_kind: &event.attribution.workload_kind,
            workload_name: &event.attribution.workload_name,
        }
    }

    #[test]
    fn fingerprint_is_stable_across_rollout_identity() {
        let first = event(EventPayload::ProcessExec(ProcessExec {
            executable: "/bin/sh".into(),
            parent_command: None,
        }));
        let mut rolled = first.clone();
        rolled.id = Uuid::from_u128(51);
        rolled.attribution.pod_uid = "pod-uid-b".into();
        rolled.attribution.pod_name = "payment-api-def".into();
        rolled.attribution.container_id = "container-b".into();
        rolled.attribution.workload_uid = "deployment-uid-b".into();
        rolled.process.pid = 200;
        assert_eq!(
            fingerprint_v1(&scope(&first), &first).unwrap().digest,
            fingerprint_v1(&scope(&rolled), &rolled).unwrap().digest
        );
    }

    #[test]
    fn tenant_and_workload_are_part_of_fingerprint() {
        let event = event(EventPayload::Syscall(SyscallEvent {
            name: "PTRACE".into(),
        }));
        let first = fingerprint_v1(&scope(&event), &event).unwrap();
        let mut other_tenant = scope(&event);
        other_tenant.organization_id = Uuid::from_u128(99);
        let mut other_workload = scope(&event);
        other_workload.workload_name = "worker";
        assert_ne!(
            first.digest,
            fingerprint_v1(&other_tenant, &event).unwrap().digest
        );
        assert_ne!(
            first.digest,
            fingerprint_v1(&other_workload, &event).unwrap().digest
        );
    }

    #[test]
    fn semantic_identity_is_normalized_and_validated() {
        let spaced = event(EventPayload::ProcessExec(ProcessExec {
            executable: "  /bin/sh\0 ".into(),
            parent_command: None,
        }));
        let clean = event(EventPayload::ProcessExec(ProcessExec {
            executable: "/bin/sh".into(),
            parent_command: None,
        }));
        assert_eq!(
            fingerprint_v1(&scope(&spaced), &spaced).unwrap().digest,
            fingerprint_v1(&scope(&clean), &clean).unwrap().digest
        );

        let invalid = event(EventPayload::Syscall(SyscallEvent { name: " \0".into() }));
        assert_eq!(
            fingerprint_v1(&scope(&invalid), &invalid),
            Err(FingerprintError::EmptyField("syscall_name"))
        );
    }

    fn network(address: &str, port: u16, outcome: NetworkConnectOutcome) -> RuntimeEvent {
        let errno = match outcome {
            NetworkConnectOutcome::Succeeded => None,
            NetworkConnectOutcome::InProgress => Some(event_model::LINUX_EINPROGRESS),
            NetworkConnectOutcome::Failed => Some(111),
        };
        let address: std::net::IpAddr = address.parse().unwrap();
        let family = if address.is_ipv4() {
            NetworkAddressFamily::Ipv4
        } else {
            NetworkAddressFamily::Ipv6
        };
        event(EventPayload::NetworkConnect(
            NetworkConnect::new(family, address, port, outcome, errno).unwrap(),
        ))
    }

    #[test]
    fn network_fingerprint_uses_endpoint_but_not_outcome() {
        let succeeded = network("203.0.113.7", 443, NetworkConnectOutcome::Succeeded);
        let failed = network("203.0.113.7", 443, NetworkConnectOutcome::Failed);
        let other_address = network("203.0.113.8", 443, NetworkConnectOutcome::Succeeded);
        let other_port = network("203.0.113.7", 8443, NetworkConnectOutcome::Succeeded);
        let mut other_process = succeeded.clone();
        other_process.process.command = "wget".into();

        let fingerprint = fingerprint_v1(&scope(&succeeded), &succeeded).unwrap();
        assert_eq!(
            fingerprint.digest,
            fingerprint_v1(&scope(&failed), &failed).unwrap().digest
        );
        assert_ne!(
            fingerprint.digest,
            fingerprint_v1(&scope(&other_address), &other_address)
                .unwrap()
                .digest
        );
        assert_ne!(
            fingerprint.digest,
            fingerprint_v1(&scope(&other_port), &other_port)
                .unwrap()
                .digest
        );
        assert_ne!(
            fingerprint.digest,
            fingerprint_v1(&scope(&other_process), &other_process)
                .unwrap()
                .digest
        );
        assert_eq!(
            fingerprint.summary.semantic,
            serde_json::json!({
                "process_command": "curl",
                "address_family": "ipv4",
                "destination_address": "203.0.113.7",
                "destination_port": 443
            })
        );
    }

    #[test]
    fn dns_fingerprint_excludes_transaction_and_answers() {
        let name = DnsName::new("api.example.com").unwrap();
        let query = event(EventPayload::NetworkDnsQuery(NetworkDnsQuery {
            transaction_id: 1,
            direction: DnsDirection::Egress,
            transport: DnsTransport::Udp,
            resolver_address: "10.96.0.10".parse().unwrap(),
            name: name.clone(),
            query_type: DnsQueryType::A,
        }));
        let mut repeated = query.clone();
        let EventPayload::NetworkDnsQuery(value) = &mut repeated.payload else {
            unreachable!()
        };
        value.transaction_id = 99;
        assert_eq!(
            fingerprint_v1(&scope(&query), &query).unwrap().digest,
            fingerprint_v1(&scope(&repeated), &repeated).unwrap().digest
        );

        let response = event(EventPayload::NetworkDnsResponse(NetworkDnsResponse {
            transaction_id: 1,
            direction: DnsDirection::Ingress,
            transport: DnsTransport::Udp,
            resolver_address: "10.96.0.10".parse().unwrap(),
            name: name.clone(),
            query_type: DnsQueryType::A,
            response_code: DnsResponseCode::NoError,
            truncated: false,
            answers: vec![DnsAddressAnswer::new(name, "203.0.113.7".parse().unwrap(), 60).unwrap()],
            cname_chain: vec![],
            effective_ttl_seconds: Some(60),
        }));
        let mut changed_answer = response.clone();
        let EventPayload::NetworkDnsResponse(value) = &mut changed_answer.payload else {
            unreachable!()
        };
        value.transaction_id = 2;
        value.answers[0].address = "203.0.113.8".parse().unwrap();
        assert_eq!(
            fingerprint_v1(&scope(&response), &response).unwrap().digest,
            fingerprint_v1(&scope(&changed_answer), &changed_answer)
                .unwrap()
                .digest
        );
    }

    #[test]
    fn connection_context_changes_summary_but_not_ip_first_fingerprint() {
        let plain = network("203.0.113.7", 443, NetworkConnectOutcome::Succeeded);
        let mut qualified = plain.clone();
        let observed_at = Utc::now();
        let EventPayload::NetworkConnect(connect) = &mut qualified.payload else {
            unreachable!()
        };
        connect.dns_context = Some(
            DnsContext::new(
                vec![DnsName::new("api.example.com").unwrap()],
                observed_at,
                observed_at + chrono::Duration::seconds(60),
            )
            .unwrap(),
        );
        let plain_fingerprint = fingerprint_v1(&scope(&plain), &plain).unwrap();
        let qualified_fingerprint = fingerprint_v1(&scope(&qualified), &qualified).unwrap();
        assert_eq!(plain_fingerprint.digest, qualified_fingerprint.digest);
        assert!(
            plain_fingerprint
                .summary
                .semantic
                .get("dns_context")
                .is_none()
        );
        assert!(
            qualified_fingerprint
                .summary
                .semantic
                .get("dns_context")
                .is_some()
        );
    }
}
