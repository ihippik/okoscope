//! Transport-independent runtime event domain model.

use std::net::IpAddr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
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
            EventPayload::NetworkConnect(_) => "network.connect",
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<String>,
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
    NetworkConnect(NetworkConnect),
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

pub const LINUX_EINPROGRESS: u16 = 115;
pub const MAX_LINUX_ERRNO: u16 = 4095;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAddressFamily {
    Ipv4,
    Ipv6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkConnectOutcome {
    Succeeded,
    InProgress,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkConnect {
    pub address_family: NetworkAddressFamily,
    pub destination_address: IpAddr,
    pub destination_port: u16,
    pub outcome: NetworkConnectOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errno: Option<u16>,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum NetworkConnectError {
    #[error("destination port must be non-zero")]
    ZeroDestinationPort,
    #[error("address family does not match destination address")]
    AddressFamilyMismatch,
    #[error("connect outcome and errno are inconsistent")]
    InconsistentOutcome,
    #[error("errno must be in 1..={MAX_LINUX_ERRNO}")]
    InvalidErrno,
}

impl NetworkConnect {
    /// Constructs a validated outbound connection attempt.
    ///
    /// # Errors
    ///
    /// Returns an error for a zero port, a mismatched address family, or an
    /// inconsistent outcome/errno pair.
    pub fn new(
        address_family: NetworkAddressFamily,
        destination_address: IpAddr,
        destination_port: u16,
        outcome: NetworkConnectOutcome,
        errno: Option<u16>,
    ) -> Result<Self, NetworkConnectError> {
        if destination_port == 0 {
            return Err(NetworkConnectError::ZeroDestinationPort);
        }
        if !matches!(
            (address_family, destination_address),
            (NetworkAddressFamily::Ipv4, IpAddr::V4(_))
                | (NetworkAddressFamily::Ipv6, IpAddr::V6(_))
        ) {
            return Err(NetworkConnectError::AddressFamilyMismatch);
        }
        if errno.is_some_and(|value| value == 0 || value > MAX_LINUX_ERRNO) {
            return Err(NetworkConnectError::InvalidErrno);
        }
        let consistent = matches!(
            (outcome, errno),
            (NetworkConnectOutcome::Succeeded, None)
                | (NetworkConnectOutcome::InProgress, Some(LINUX_EINPROGRESS))
                | (NetworkConnectOutcome::Failed, Some(1..=MAX_LINUX_ERRNO))
        ) && !(outcome == NetworkConnectOutcome::Failed
            && errno == Some(LINUX_EINPROGRESS));
        if !consistent {
            return Err(NetworkConnectError::InconsistentOutcome);
        }
        Ok(Self {
            address_family,
            destination_address,
            destination_port,
            outcome,
            errno,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn network_connect_validates_family_port_and_outcome() {
        let ipv4 = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));
        let event = NetworkConnect::new(
            NetworkAddressFamily::Ipv4,
            ipv4,
            443,
            NetworkConnectOutcome::Succeeded,
            None,
        )
        .unwrap();
        assert_eq!(event.destination_address, ipv4);
        assert_eq!(
            NetworkConnect::new(
                NetworkAddressFamily::Ipv6,
                ipv4,
                443,
                NetworkConnectOutcome::Succeeded,
                None,
            ),
            Err(NetworkConnectError::AddressFamilyMismatch)
        );
        assert_eq!(
            NetworkConnect::new(
                NetworkAddressFamily::Ipv4,
                ipv4,
                0,
                NetworkConnectOutcome::Succeeded,
                None,
            ),
            Err(NetworkConnectError::ZeroDestinationPort)
        );
        assert_eq!(
            NetworkConnect::new(
                NetworkAddressFamily::Ipv4,
                ipv4,
                443,
                NetworkConnectOutcome::Failed,
                None,
            ),
            Err(NetworkConnectError::InconsistentOutcome)
        );
        assert!(
            NetworkConnect::new(
                NetworkAddressFamily::Ipv6,
                IpAddr::V6(Ipv6Addr::LOCALHOST),
                443,
                NetworkConnectOutcome::InProgress,
                Some(LINUX_EINPROGRESS),
            )
            .is_ok()
        );
    }

    #[test]
    fn network_connect_serializes_canonical_safe_fields() {
        let event = NetworkConnect::new(
            NetworkAddressFamily::Ipv6,
            "2001:0db8:0:0:0:0:0:1".parse().unwrap(),
            8443,
            NetworkConnectOutcome::Failed,
            Some(111),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::json!({
                "address_family": "ipv6",
                "destination_address": "2001:db8::1",
                "destination_port": 8443,
                "outcome": "failed",
                "errno": 111
            })
        );
    }
}
