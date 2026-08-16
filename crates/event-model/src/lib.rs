//! Transport-independent runtime event domain model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const EVENT_SCHEMA_VERSION: u32 = 1;
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub id: Uuid,
    pub observed_at: DateTime<Utc>,
    pub schema_version: u32,
    pub attribution: KubernetesAttribution,
    pub process: ProcessIdentity,
    pub payload: EventPayload,
}

impl RuntimeEvent {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self.payload {
            EventPayload::ProcessExec(_) => "process.exec",
            EventPayload::Syscall(_) => "syscall",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KubernetesAttribution {
    pub project_id: Uuid,
    pub application_id: Uuid,
    pub node_name: String,
    pub namespace: String,
    pub pod_uid: String,
    pub pod_name: String,
    pub container_id: String,
    pub container_name: String,
    pub workload_uid: String,
    pub workload_kind: String,
    pub workload_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub cgroup_id: u64,
    pub pid: u32,
    pub tgid: u32,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EventPayload {
    ProcessExec(ProcessExec),
    Syscall(SyscallEvent),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessExec {
    pub executable: String,
    pub parent_command: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyscallEvent {
    pub name: String,
}
