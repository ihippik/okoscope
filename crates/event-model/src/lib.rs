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
            EventPayload::NetworkDnsQuery(_) => "network.dns.query",
            EventPayload::NetworkDnsResponse(_) => "network.dns.response",
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
    NetworkDnsQuery(NetworkDnsQuery),
    NetworkDnsResponse(NetworkDnsResponse),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns_context: Option<DnsContext>,
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
            dns_context: None,
        })
    }

    #[must_use]
    pub fn with_dns_context(mut self, context: DnsContext) -> Self {
        self.dns_context = Some(context);
        self
    }
}

pub const MAX_DNS_NAME_LEN: usize = 253;
pub const MAX_DNS_CONTEXT_NAMES: usize = 8;
pub const MAX_DNS_ANSWERS: usize = 16;
pub const MAX_DNS_CNAME_CHAIN: usize = 8;
pub const MAX_DNS_TTL_SECONDS: u32 = 86_400;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DnsName(String);

impl DnsName {
    pub fn new(value: impl Into<String>) -> Result<Self, DnsValidationError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_DNS_NAME_LEN || value.ends_with('.') {
            return Err(DnsValidationError::InvalidName);
        }
        if value
            .bytes()
            .any(|byte| byte.is_ascii_uppercase() || !byte.is_ascii())
        {
            return Err(DnsValidationError::InvalidName);
        }
        let valid = value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        });
        valid
            .then_some(Self(value))
            .ok_or(DnsValidationError::InvalidName)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DnsName {
    type Error = DnsValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DnsName> for String {
    fn from(value: DnsName) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DnsQueryType {
    A,
    Aaaa,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsTransport {
    Udp,
    Tcp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsDirection {
    Egress,
    Ingress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsResponseCode {
    NoError,
    FormErr,
    ServFail,
    NxDomain,
    NotImp,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsConfidence {
    ObservedRecently,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsAddressAnswer {
    pub name: DnsName,
    pub address: IpAddr,
    pub ttl_seconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsCname {
    pub alias: DnsName,
    pub canonical: DnsName,
    pub ttl_seconds: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkDnsQuery {
    pub transaction_id: u16,
    pub direction: DnsDirection,
    pub transport: DnsTransport,
    pub resolver_address: IpAddr,
    pub name: DnsName,
    pub query_type: DnsQueryType,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkDnsResponse {
    pub transaction_id: u16,
    pub direction: DnsDirection,
    pub transport: DnsTransport,
    pub resolver_address: IpAddr,
    pub name: DnsName,
    pub query_type: DnsQueryType,
    pub response_code: DnsResponseCode,
    pub truncated: bool,
    pub answers: Vec<DnsAddressAnswer>,
    pub cname_chain: Vec<DnsCname>,
    pub effective_ttl_seconds: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsContext {
    pub names: Vec<DnsName>,
    pub observed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub confidence: DnsConfidence,
    pub ambiguous: bool,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum DnsValidationError {
    #[error("DNS name is not canonical or exceeds bounds")]
    InvalidName,
    #[error("DNS collection exceeds bounds")]
    TooManyValues,
    #[error("DNS TTL is zero or exceeds the platform clamp")]
    InvalidTtl,
    #[error("DNS context timestamps or ambiguity are inconsistent")]
    InvalidContext,
}

impl DnsAddressAnswer {
    pub fn new(
        name: DnsName,
        address: IpAddr,
        ttl_seconds: u32,
    ) -> Result<Self, DnsValidationError> {
        validate_ttl(ttl_seconds)?;
        Ok(Self {
            name,
            address,
            ttl_seconds,
        })
    }
}

impl DnsCname {
    pub fn new(
        alias: DnsName,
        canonical: DnsName,
        ttl_seconds: u32,
    ) -> Result<Self, DnsValidationError> {
        validate_ttl(ttl_seconds)?;
        Ok(Self {
            alias,
            canonical,
            ttl_seconds,
        })
    }
}

impl NetworkDnsResponse {
    pub fn validate(&self) -> Result<(), DnsValidationError> {
        if self.answers.len() > MAX_DNS_ANSWERS || self.cname_chain.len() > MAX_DNS_CNAME_CHAIN {
            return Err(DnsValidationError::TooManyValues);
        }
        for answer in &self.answers {
            validate_ttl(answer.ttl_seconds)?;
        }
        for cname in &self.cname_chain {
            validate_ttl(cname.ttl_seconds)?;
        }
        if let Some(ttl) = self.effective_ttl_seconds {
            validate_ttl(ttl)?;
        }
        Ok(())
    }
}

impl DnsContext {
    pub fn new(
        names: Vec<DnsName>,
        observed_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, DnsValidationError> {
        if names.is_empty() || names.len() > MAX_DNS_CONTEXT_NAMES || expires_at <= observed_at {
            return Err(DnsValidationError::InvalidContext);
        }
        let ambiguous = names.len() > 1;
        Ok(Self {
            names,
            observed_at,
            expires_at,
            confidence: DnsConfidence::ObservedRecently,
            ambiguous,
        })
    }

    pub fn validate(&self) -> Result<(), DnsValidationError> {
        if self.names.is_empty()
            || self.names.len() > MAX_DNS_CONTEXT_NAMES
            || self.expires_at <= self.observed_at
            || self.ambiguous != (self.names.len() > 1)
            || self.confidence != DnsConfidence::ObservedRecently
        {
            return Err(DnsValidationError::InvalidContext);
        }
        Ok(())
    }
}

fn validate_ttl(ttl_seconds: u32) -> Result<(), DnsValidationError> {
    (1..=MAX_DNS_TTL_SECONDS)
        .contains(&ttl_seconds)
        .then_some(())
        .ok_or(DnsValidationError::InvalidTtl)
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

    #[test]
    fn dns_name_requires_canonical_bounded_ascii_labels() {
        assert_eq!(
            DnsName::new("api.example.com").unwrap().as_str(),
            "api.example.com"
        );
        for invalid in [
            "",
            "API.example.com",
            "api.example.com.",
            "-api.example",
            "api..example",
            "пример.рф",
        ] {
            assert_eq!(DnsName::new(invalid), Err(DnsValidationError::InvalidName));
        }
        assert_eq!(
            DnsName::new(format!("{}.example", "a".repeat(64))),
            Err(DnsValidationError::InvalidName)
        );
    }

    #[test]
    fn dns_ttl_and_context_are_strictly_bounded() {
        let name = DnsName::new("api.example.com").unwrap();
        assert_eq!(
            DnsAddressAnswer::new(name.clone(), "203.0.113.4".parse().unwrap(), 0),
            Err(DnsValidationError::InvalidTtl)
        );
        let observed_at = Utc::now();
        let context = DnsContext::new(
            vec![name.clone(), DnsName::new("cdn.example.com").unwrap()],
            observed_at,
            observed_at + chrono::Duration::seconds(60),
        )
        .unwrap();
        assert!(context.ambiguous);
        assert_eq!(context.confidence, DnsConfidence::ObservedRecently);
        assert_eq!(
            DnsContext::new(vec![name], observed_at, observed_at),
            Err(DnsValidationError::InvalidContext)
        );
    }

    #[test]
    fn dns_payload_kinds_are_typed() {
        let query = NetworkDnsQuery {
            transaction_id: 7,
            direction: DnsDirection::Egress,
            transport: DnsTransport::Udp,
            resolver_address: "10.96.0.10".parse().unwrap(),
            name: DnsName::new("api.example.com").unwrap(),
            query_type: DnsQueryType::A,
        };
        let mut event = RuntimeEvent {
            id: Uuid::new_v4(),
            observed_at: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION,
            attribution: KubernetesAttribution {
                project_id: Uuid::new_v4(),
                application_id: Uuid::new_v4(),
                node_name: "node".into(),
                namespace: "default".into(),
                pod_uid: "pod".into(),
                pod_name: "pod".into(),
                container_id: "container".into(),
                container_name: "container".into(),
                workload_uid: "workload".into(),
                workload_kind: "Deployment".into(),
                workload_name: "api".into(),
                release: None,
            },
            process: ProcessIdentity {
                cgroup_id: 1,
                pid: 2,
                tgid: 2,
                command: "api".into(),
            },
            payload: EventPayload::NetworkDnsQuery(query.clone()),
        };
        assert_eq!(event.kind(), "network.dns.query");
        event.payload = EventPayload::NetworkDnsResponse(NetworkDnsResponse {
            transaction_id: query.transaction_id,
            direction: DnsDirection::Ingress,
            transport: query.transport,
            resolver_address: query.resolver_address,
            name: query.name,
            query_type: query.query_type,
            response_code: DnsResponseCode::NxDomain,
            truncated: false,
            answers: vec![],
            cname_chain: vec![],
            effective_ttl_seconds: None,
        });
        assert_eq!(event.kind(), "network.dns.response");
    }
}
