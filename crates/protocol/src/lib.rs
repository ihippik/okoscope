//! Versioned agent/server wire protocol.

#[allow(clippy::all, clippy::pedantic)]
pub mod v1 {
    tonic::include_proto!("okoscope.v1");
}

use chrono::{DateTime, Utc};
use event_model::{
    EVENT_SCHEMA_VERSION, EventPayload, KubernetesAttribution, PROTOCOL_VERSION, ProcessExec,
    ProcessIdentity, RuntimeEvent, SyscallEvent,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("unsupported protocol version {0}")]
    UnsupportedProtocol(u32),
    #[error("unsupported event schema version {0}")]
    UnsupportedEventSchema(u32),
    #[error("missing field: {0}")]
    Missing(&'static str),
    #[error("invalid UUID in {field}: {value}")]
    InvalidUuid { field: &'static str, value: String },
    #[error("invalid observation timestamp")]
    InvalidTimestamp,
}

/// Validates the wire protocol version.
///
/// # Errors
///
/// Returns [`ProtocolError::UnsupportedProtocol`] when the peer version is not supported.
pub fn validate_protocol(version: u32) -> Result<(), ProtocolError> {
    if version == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ProtocolError::UnsupportedProtocol(version))
    }
}

impl From<RuntimeEvent> for v1::RuntimeEvent {
    fn from(event: RuntimeEvent) -> Self {
        let payload = match event.payload {
            EventPayload::ProcessExec(exec) => {
                v1::runtime_event::Payload::ProcessExec(v1::ProcessExec {
                    executable: exec.executable,
                    parent_command: exec.parent_command.unwrap_or_default(),
                })
            }
            EventPayload::Syscall(syscall) => {
                v1::runtime_event::Payload::Syscall(v1::Syscall { name: syscall.name })
            }
        };
        let a = event.attribution;
        let p = event.process;
        Self {
            event_id: event.id.to_string(),
            observed_at_unix_nanos: event.observed_at.timestamp_nanos_opt().unwrap_or_default(),
            schema_version: event.schema_version,
            attribution: Some(v1::KubernetesAttribution {
                project_id: a.project_id.to_string(),
                application_id: a.application_id.to_string(),
                node_name: a.node_name,
                namespace: a.namespace,
                pod_uid: a.pod_uid,
                pod_name: a.pod_name,
                container_id: a.container_id,
                container_name: a.container_name,
                workload_uid: a.workload_uid,
                workload_kind: a.workload_kind,
                workload_name: a.workload_name,
            }),
            process: Some(v1::ProcessIdentity {
                cgroup_id: p.cgroup_id,
                pid: p.pid,
                tgid: p.tgid,
                command: p.command,
            }),
            payload: Some(payload),
        }
    }
}

impl TryFrom<v1::RuntimeEvent> for RuntimeEvent {
    type Error = ProtocolError;

    fn try_from(event: v1::RuntimeEvent) -> Result<Self, Self::Error> {
        if event.schema_version != EVENT_SCHEMA_VERSION {
            return Err(ProtocolError::UnsupportedEventSchema(event.schema_version));
        }
        let id = parse_uuid("event_id", &event.event_id)?;
        let secs = event.observed_at_unix_nanos.div_euclid(1_000_000_000);
        let nanos = u32::try_from(event.observed_at_unix_nanos.rem_euclid(1_000_000_000))
            .map_err(|_| ProtocolError::InvalidTimestamp)?;
        let observed_at =
            DateTime::<Utc>::from_timestamp(secs, nanos).ok_or(ProtocolError::InvalidTimestamp)?;
        let a = event
            .attribution
            .ok_or(ProtocolError::Missing("attribution"))?;
        require("node_name", &a.node_name)?;
        require("namespace", &a.namespace)?;
        require("pod_uid", &a.pod_uid)?;
        require("container_id", &a.container_id)?;
        require("workload_uid", &a.workload_uid)?;
        let attribution = KubernetesAttribution {
            project_id: parse_uuid("project_id", &a.project_id)?,
            application_id: parse_uuid("application_id", &a.application_id)?,
            node_name: a.node_name,
            namespace: a.namespace,
            pod_uid: a.pod_uid,
            pod_name: a.pod_name,
            container_id: a.container_id,
            container_name: a.container_name,
            workload_uid: a.workload_uid,
            workload_kind: a.workload_kind,
            workload_name: a.workload_name,
        };
        let p = event.process.ok_or(ProtocolError::Missing("process"))?;
        let process = ProcessIdentity {
            cgroup_id: p.cgroup_id,
            pid: p.pid,
            tgid: p.tgid,
            command: p.command,
        };
        let payload = match event.payload.ok_or(ProtocolError::Missing("payload"))? {
            v1::runtime_event::Payload::ProcessExec(exec) => {
                require("executable", &exec.executable)?;
                EventPayload::ProcessExec(ProcessExec {
                    executable: exec.executable,
                    parent_command: (!exec.parent_command.is_empty())
                        .then_some(exec.parent_command),
                })
            }
            v1::runtime_event::Payload::Syscall(syscall) => {
                require("syscall.name", &syscall.name)?;
                EventPayload::Syscall(SyscallEvent { name: syscall.name })
            }
        };
        Ok(Self {
            id,
            observed_at,
            schema_version: event.schema_version,
            attribution,
            process,
            payload,
        })
    }
}

fn require(field: &'static str, value: &str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() {
        Err(ProtocolError::Missing(field))
    } else {
        Ok(())
    }
}

fn parse_uuid(field: &'static str, value: &str) -> Result<Uuid, ProtocolError> {
    Uuid::parse_str(value).map_err(|_| ProtocolError::InvalidUuid {
        field,
        value: value.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> RuntimeEvent {
        RuntimeEvent {
            id: Uuid::new_v4(),
            observed_at: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION,
            attribution: KubernetesAttribution {
                project_id: Uuid::new_v4(),
                application_id: Uuid::new_v4(),
                node_name: "node-1".into(),
                namespace: "production".into(),
                pod_uid: "pod-uid".into(),
                pod_name: "payment-api-1".into(),
                container_id: "containerd://abc".into(),
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
                parent_command: Some("payment-api".into()),
            }),
        }
    }

    #[test]
    fn event_round_trip() {
        let original = event();
        let decoded = RuntimeEvent::try_from(v1::RuntimeEvent::from(original.clone())).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn rejects_unknown_schema_and_missing_payload() {
        let mut wire = v1::RuntimeEvent::from(event());
        wire.schema_version = 999;
        assert_eq!(
            RuntimeEvent::try_from(wire).unwrap_err(),
            ProtocolError::UnsupportedEventSchema(999)
        );
        let mut wire = v1::RuntimeEvent::from(event());
        wire.payload = None;
        assert_eq!(
            RuntimeEvent::try_from(wire).unwrap_err(),
            ProtocolError::Missing("payload")
        );
    }

    #[test]
    fn rejects_unknown_protocol() {
        assert_eq!(
            validate_protocol(999),
            Err(ProtocolError::UnsupportedProtocol(999))
        );
    }
}
