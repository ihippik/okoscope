//! Versioned agent/server wire protocol.

#[allow(clippy::all, clippy::pedantic)]
pub mod v1 {
    tonic::include_proto!("okoscope.v1");
}

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use chrono::{DateTime, Utc};
use event_model::{
    DnsAddressAnswer, DnsCname, DnsContext, DnsDirection, DnsName, DnsQueryType, DnsResponseCode,
    DnsTransport, EVENT_SCHEMA_VERSION, EventPayload, KubernetesAttribution, NetworkAddressFamily,
    NetworkConnect, NetworkConnectOutcome, NetworkDnsQuery, NetworkDnsResponse, PROTOCOL_VERSION,
    ProcessExec, ProcessIdentity, RuntimeEvent, SyscallEvent,
};
use thiserror::Error;
use uuid::Uuid;

pub const NETWORK_CONNECT_CAPABILITY: &str = "network.connect/v1";
pub const NETWORK_DNS_UDP_CAPABILITY: &str = "network.dns.udp/v1";
pub const NETWORK_DNS_TCP_CAPABILITY: &str = "network.dns.tcp/v1";

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
    #[error("invalid network field: {0}")]
    InvalidNetwork(&'static str),
    #[error("invalid DNS field: {0}")]
    InvalidDns(&'static str),
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
            EventPayload::NetworkConnect(network) => {
                let (address_family, destination_address) = match network.destination_address {
                    IpAddr::V4(address) => {
                        (v1::NetworkAddressFamily::Ipv4, address.octets().to_vec())
                    }
                    IpAddr::V6(address) => {
                        (v1::NetworkAddressFamily::Ipv6, address.octets().to_vec())
                    }
                };
                let outcome = match network.outcome {
                    NetworkConnectOutcome::Succeeded => v1::NetworkConnectOutcome::Succeeded,
                    NetworkConnectOutcome::InProgress => v1::NetworkConnectOutcome::InProgress,
                    NetworkConnectOutcome::Failed => v1::NetworkConnectOutcome::Failed,
                };
                v1::runtime_event::Payload::NetworkConnect(v1::NetworkConnect {
                    address_family: address_family.into(),
                    destination_address,
                    destination_port: u32::from(network.destination_port),
                    outcome: outcome.into(),
                    errno: network.errno.map(u32::from),
                    dns_context: network.dns_context.map(encode_dns_context),
                })
            }
            EventPayload::NetworkDnsQuery(query) => {
                v1::runtime_event::Payload::NetworkDnsQuery(encode_dns_query(query))
            }
            EventPayload::NetworkDnsResponse(response) => {
                v1::runtime_event::Payload::NetworkDnsResponse(encode_dns_response(response))
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
                release: a.release,
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
            release: a.release.and_then(|value| {
                let value = value.trim().to_owned();
                (!value.is_empty() && value.len() <= 200).then_some(value)
            }),
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
            v1::runtime_event::Payload::NetworkConnect(network) => decode_network(&network)?,
            v1::runtime_event::Payload::NetworkDnsQuery(query) => decode_dns_query(&query)?,
            v1::runtime_event::Payload::NetworkDnsResponse(response) => {
                decode_dns_response(&response)?
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

fn decode_network(network: &v1::NetworkConnect) -> Result<EventPayload, ProtocolError> {
    let (address_family, destination_address) =
        match v1::NetworkAddressFamily::try_from(network.address_family)
            .map_err(|_| ProtocolError::InvalidNetwork("address_family"))?
        {
            v1::NetworkAddressFamily::Ipv4 => (
                NetworkAddressFamily::Ipv4,
                IpAddr::V4(Ipv4Addr::from(
                    <[u8; 4]>::try_from(network.destination_address.as_slice())
                        .map_err(|_| ProtocolError::InvalidNetwork("destination_address"))?,
                )),
            ),
            v1::NetworkAddressFamily::Ipv6 => (
                NetworkAddressFamily::Ipv6,
                IpAddr::V6(Ipv6Addr::from(
                    <[u8; 16]>::try_from(network.destination_address.as_slice())
                        .map_err(|_| ProtocolError::InvalidNetwork("destination_address"))?,
                )),
            ),
            v1::NetworkAddressFamily::Unspecified => {
                return Err(ProtocolError::InvalidNetwork("address_family"));
            }
        };
    let destination_port = u16::try_from(network.destination_port)
        .map_err(|_| ProtocolError::InvalidNetwork("destination_port"))?;
    let outcome = match v1::NetworkConnectOutcome::try_from(network.outcome)
        .map_err(|_| ProtocolError::InvalidNetwork("outcome"))?
    {
        v1::NetworkConnectOutcome::Succeeded => NetworkConnectOutcome::Succeeded,
        v1::NetworkConnectOutcome::InProgress => NetworkConnectOutcome::InProgress,
        v1::NetworkConnectOutcome::Failed => NetworkConnectOutcome::Failed,
        v1::NetworkConnectOutcome::Unspecified => {
            return Err(ProtocolError::InvalidNetwork("outcome"));
        }
    };
    let errno = network
        .errno
        .map(u16::try_from)
        .transpose()
        .map_err(|_| ProtocolError::InvalidNetwork("errno"))?;
    let mut value = NetworkConnect::new(
        address_family,
        destination_address,
        destination_port,
        outcome,
        errno,
    )
    .map_err(|_| ProtocolError::InvalidNetwork("network_connect"))?;
    if let Some(context) = &network.dns_context {
        value = value.with_dns_context(decode_dns_context(context)?);
    }
    Ok(EventPayload::NetworkConnect(value))
}

fn encode_ip(address: IpAddr) -> Vec<u8> {
    match address {
        IpAddr::V4(value) => value.octets().to_vec(),
        IpAddr::V6(value) => value.octets().to_vec(),
    }
}

fn decode_ip(value: &[u8]) -> Result<IpAddr, ProtocolError> {
    match value.len() {
        4 => Ok(IpAddr::V4(Ipv4Addr::from(
            <[u8; 4]>::try_from(value).unwrap(),
        ))),
        16 => Ok(IpAddr::V6(Ipv6Addr::from(
            <[u8; 16]>::try_from(value).unwrap(),
        ))),
        _ => Err(ProtocolError::InvalidDns("address")),
    }
}

fn encode_dns_query(value: NetworkDnsQuery) -> v1::NetworkDnsQuery {
    v1::NetworkDnsQuery {
        transaction_id: u32::from(value.transaction_id),
        direction: encode_direction(value.direction).into(),
        transport: encode_transport(value.transport).into(),
        resolver_address: encode_ip(value.resolver_address),
        name: value.name.into(),
        query_type: encode_query_type(value.query_type).into(),
    }
}

fn encode_dns_response(value: NetworkDnsResponse) -> v1::NetworkDnsResponse {
    v1::NetworkDnsResponse {
        transaction_id: u32::from(value.transaction_id),
        direction: encode_direction(value.direction).into(),
        transport: encode_transport(value.transport).into(),
        resolver_address: encode_ip(value.resolver_address),
        name: value.name.into(),
        query_type: encode_query_type(value.query_type).into(),
        response_code: encode_response_code(value.response_code).into(),
        truncated: value.truncated,
        answers: value
            .answers
            .into_iter()
            .map(|answer| v1::DnsAddressAnswer {
                name: answer.name.into(),
                address: encode_ip(answer.address),
                ttl_seconds: answer.ttl_seconds,
            })
            .collect(),
        cname_chain: value
            .cname_chain
            .into_iter()
            .map(|cname| v1::DnsCname {
                alias: cname.alias.into(),
                canonical: cname.canonical.into(),
                ttl_seconds: cname.ttl_seconds,
            })
            .collect(),
        effective_ttl_seconds: value.effective_ttl_seconds,
    }
}

fn encode_dns_context(value: DnsContext) -> v1::DnsContext {
    v1::DnsContext {
        names: value.names.into_iter().map(Into::into).collect(),
        observed_at_unix_nanos: value.observed_at.timestamp_nanos_opt().unwrap_or_default(),
        expires_at_unix_nanos: value.expires_at.timestamp_nanos_opt().unwrap_or_default(),
        confidence: v1::DnsConfidence::ObservedRecently.into(),
        ambiguous: value.ambiguous,
    }
}

fn decode_dns_query(value: &v1::NetworkDnsQuery) -> Result<EventPayload, ProtocolError> {
    Ok(EventPayload::NetworkDnsQuery(NetworkDnsQuery {
        transaction_id: u16::try_from(value.transaction_id)
            .map_err(|_| ProtocolError::InvalidDns("transaction_id"))?,
        direction: decode_direction(value.direction)?,
        transport: decode_transport(value.transport)?,
        resolver_address: decode_ip(&value.resolver_address)?,
        name: DnsName::new(value.name.clone()).map_err(|_| ProtocolError::InvalidDns("name"))?,
        query_type: decode_query_type(value.query_type)?,
    }))
}

fn decode_dns_response(value: &v1::NetworkDnsResponse) -> Result<EventPayload, ProtocolError> {
    let response = NetworkDnsResponse {
        transaction_id: u16::try_from(value.transaction_id)
            .map_err(|_| ProtocolError::InvalidDns("transaction_id"))?,
        direction: decode_direction(value.direction)?,
        transport: decode_transport(value.transport)?,
        resolver_address: decode_ip(&value.resolver_address)?,
        name: DnsName::new(value.name.clone()).map_err(|_| ProtocolError::InvalidDns("name"))?,
        query_type: decode_query_type(value.query_type)?,
        response_code: decode_response_code(value.response_code)?,
        truncated: value.truncated,
        answers: value
            .answers
            .iter()
            .map(|answer| {
                DnsAddressAnswer::new(
                    DnsName::new(answer.name.clone())
                        .map_err(|_| ProtocolError::InvalidDns("answer.name"))?,
                    decode_ip(&answer.address)?,
                    answer.ttl_seconds,
                )
                .map_err(|_| ProtocolError::InvalidDns("answer.ttl"))
            })
            .collect::<Result<_, ProtocolError>>()?,
        cname_chain: value
            .cname_chain
            .iter()
            .map(|cname| {
                DnsCname::new(
                    DnsName::new(cname.alias.clone())
                        .map_err(|_| ProtocolError::InvalidDns("cname.alias"))?,
                    DnsName::new(cname.canonical.clone())
                        .map_err(|_| ProtocolError::InvalidDns("cname.canonical"))?,
                    cname.ttl_seconds,
                )
                .map_err(|_| ProtocolError::InvalidDns("cname.ttl"))
            })
            .collect::<Result<_, ProtocolError>>()?,
        effective_ttl_seconds: value.effective_ttl_seconds,
    };
    response
        .validate()
        .map_err(|_| ProtocolError::InvalidDns("response"))?;
    Ok(EventPayload::NetworkDnsResponse(response))
}

fn decode_dns_context(value: &v1::DnsContext) -> Result<DnsContext, ProtocolError> {
    let timestamp = |nanos: i64| {
        let subsecond = u32::try_from(nanos.rem_euclid(1_000_000_000))
            .map_err(|_| ProtocolError::InvalidDns("context.timestamp"))?;
        DateTime::<Utc>::from_timestamp(nanos.div_euclid(1_000_000_000), subsecond)
            .ok_or(ProtocolError::InvalidDns("context.timestamp"))
    };
    if value.confidence != i32::from(v1::DnsConfidence::ObservedRecently) {
        return Err(ProtocolError::InvalidDns("context.confidence"));
    }
    let names = value
        .names
        .iter()
        .map(|name| {
            DnsName::new(name.clone()).map_err(|_| ProtocolError::InvalidDns("context.name"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let context = DnsContext::new(
        names,
        timestamp(value.observed_at_unix_nanos)?,
        timestamp(value.expires_at_unix_nanos)?,
    )
    .map_err(|_| ProtocolError::InvalidDns("context"))?;
    if context.ambiguous != value.ambiguous {
        return Err(ProtocolError::InvalidDns("context.ambiguous"));
    }
    Ok(context)
}

fn encode_direction(value: DnsDirection) -> v1::DnsDirection {
    match value {
        DnsDirection::Egress => v1::DnsDirection::Egress,
        DnsDirection::Ingress => v1::DnsDirection::Ingress,
    }
}
fn encode_transport(value: DnsTransport) -> v1::DnsTransport {
    match value {
        DnsTransport::Udp => v1::DnsTransport::Udp,
        DnsTransport::Tcp => v1::DnsTransport::Tcp,
    }
}
fn encode_query_type(value: DnsQueryType) -> v1::DnsQueryType {
    match value {
        DnsQueryType::A => v1::DnsQueryType::A,
        DnsQueryType::Aaaa => v1::DnsQueryType::Aaaa,
    }
}
fn encode_response_code(value: DnsResponseCode) -> v1::DnsResponseCode {
    match value {
        DnsResponseCode::NoError => v1::DnsResponseCode::NoError,
        DnsResponseCode::FormErr => v1::DnsResponseCode::FormErr,
        DnsResponseCode::ServFail => v1::DnsResponseCode::ServFail,
        DnsResponseCode::NxDomain => v1::DnsResponseCode::NxDomain,
        DnsResponseCode::NotImp => v1::DnsResponseCode::NotImp,
        DnsResponseCode::Refused => v1::DnsResponseCode::Refused,
    }
}
fn decode_direction(value: i32) -> Result<DnsDirection, ProtocolError> {
    match v1::DnsDirection::try_from(value).ok() {
        Some(v1::DnsDirection::Egress) => Ok(DnsDirection::Egress),
        Some(v1::DnsDirection::Ingress) => Ok(DnsDirection::Ingress),
        _ => Err(ProtocolError::InvalidDns("direction")),
    }
}
fn decode_transport(value: i32) -> Result<DnsTransport, ProtocolError> {
    match v1::DnsTransport::try_from(value).ok() {
        Some(v1::DnsTransport::Udp) => Ok(DnsTransport::Udp),
        Some(v1::DnsTransport::Tcp) => Ok(DnsTransport::Tcp),
        _ => Err(ProtocolError::InvalidDns("transport")),
    }
}
fn decode_query_type(value: i32) -> Result<DnsQueryType, ProtocolError> {
    match v1::DnsQueryType::try_from(value).ok() {
        Some(v1::DnsQueryType::A) => Ok(DnsQueryType::A),
        Some(v1::DnsQueryType::Aaaa) => Ok(DnsQueryType::Aaaa),
        _ => Err(ProtocolError::InvalidDns("query_type")),
    }
}
fn decode_response_code(value: i32) -> Result<DnsResponseCode, ProtocolError> {
    match v1::DnsResponseCode::try_from(value).ok() {
        Some(v1::DnsResponseCode::NoError) => Ok(DnsResponseCode::NoError),
        Some(v1::DnsResponseCode::FormErr) => Ok(DnsResponseCode::FormErr),
        Some(v1::DnsResponseCode::ServFail) => Ok(DnsResponseCode::ServFail),
        Some(v1::DnsResponseCode::NxDomain) => Ok(DnsResponseCode::NxDomain),
        Some(v1::DnsResponseCode::NotImp) => Ok(DnsResponseCode::NotImp),
        Some(v1::DnsResponseCode::Refused) => Ok(DnsResponseCode::Refused),
        _ => Err(ProtocolError::InvalidDns("response_code")),
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
    use prost::Message;

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
    fn optional_release_round_trips_and_old_messages_remain_valid() {
        let mut original = event();
        original.attribution.release = Some("1.7.2".into());
        assert_eq!(
            RuntimeEvent::try_from(v1::RuntimeEvent::from(original.clone())).unwrap(),
            original
        );
        let mut wire = v1::RuntimeEvent::from(event());
        wire.attribution.as_mut().unwrap().release = None;
        assert!(
            RuntimeEvent::try_from(wire)
                .unwrap()
                .attribution
                .release
                .is_none()
        );
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

    fn network_event() -> RuntimeEvent {
        let mut event = event();
        event.payload = EventPayload::NetworkConnect(
            NetworkConnect::new(
                NetworkAddressFamily::Ipv6,
                "2001:db8::7".parse().unwrap(),
                443,
                NetworkConnectOutcome::Failed,
                Some(111),
            )
            .unwrap(),
        );
        event
    }

    #[test]
    fn network_event_round_trips_and_unknown_fields_are_ignored() {
        let original = network_event();
        let wire = v1::RuntimeEvent::from(original.clone());
        let mut encoded = wire.encode_to_vec();
        encoded.extend_from_slice(&[0xa0, 0x06, 0x01]);
        let decoded_wire = v1::RuntimeEvent::decode(encoded.as_slice()).unwrap();
        assert_eq!(RuntimeEvent::try_from(decoded_wire).unwrap(), original);
        let legacy = event();
        assert_eq!(
            RuntimeEvent::try_from(v1::RuntimeEvent::from(legacy.clone())).unwrap(),
            legacy
        );
    }

    #[test]
    fn rejects_invalid_network_fields() {
        let cases = [
            (0, vec![203, 0, 113, 1], 443, 1, None),
            (1, vec![203, 0, 113], 443, 1, None),
            (1, vec![203, 0, 113, 1], 0, 1, None),
            (1, vec![203, 0, 113, 1], 443, 0, None),
            (1, vec![203, 0, 113, 1], 443, 1, Some(1)),
            (1, vec![203, 0, 113, 1], 443, 2, Some(111)),
            (1, vec![203, 0, 113, 1], 443, 3, None),
            (1, vec![203, 0, 113, 1], 443, 3, Some(4096)),
        ];
        for (family, address, port, outcome, errno) in cases {
            let mut wire = v1::RuntimeEvent::from(event());
            wire.payload = Some(v1::runtime_event::Payload::NetworkConnect(
                v1::NetworkConnect {
                    address_family: family,
                    destination_address: address,
                    destination_port: port,
                    outcome,
                    errno,
                    dns_context: None,
                },
            ));
            assert!(matches!(
                RuntimeEvent::try_from(wire),
                Err(ProtocolError::InvalidNetwork(_))
            ));
        }
    }

    #[test]
    fn dns_query_response_and_connect_context_round_trip() {
        let name = DnsName::new("api.example.com").unwrap();
        let mut query = event();
        query.payload = EventPayload::NetworkDnsQuery(NetworkDnsQuery {
            transaction_id: 42,
            direction: DnsDirection::Egress,
            transport: DnsTransport::Udp,
            resolver_address: "10.96.0.10".parse().unwrap(),
            name: name.clone(),
            query_type: DnsQueryType::A,
        });
        assert_eq!(
            RuntimeEvent::try_from(v1::RuntimeEvent::from(query.clone())).unwrap(),
            query
        );

        let mut response = event();
        response.payload = EventPayload::NetworkDnsResponse(NetworkDnsResponse {
            transaction_id: 42,
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
            RuntimeEvent::try_from(v1::RuntimeEvent::from(response.clone())).unwrap(),
            response
        );

        let observed_at = Utc::now();
        let context = DnsContext::new(
            vec![name],
            observed_at,
            observed_at + chrono::Duration::seconds(60),
        )
        .unwrap();
        let mut connect = network_event();
        if let EventPayload::NetworkConnect(value) = &mut connect.payload {
            value.dns_context = Some(context);
        }
        assert_eq!(
            RuntimeEvent::try_from(v1::RuntimeEvent::from(connect.clone())).unwrap(),
            connect
        );
    }

    #[test]
    fn rejects_malformed_dns_wire_fields() {
        let mut wire = v1::RuntimeEvent::from(event());
        wire.payload = Some(v1::runtime_event::Payload::NetworkDnsQuery(
            v1::NetworkDnsQuery {
                transaction_id: 70_000,
                direction: v1::DnsDirection::Egress.into(),
                transport: v1::DnsTransport::Udp.into(),
                resolver_address: vec![10, 0, 0, 1],
                name: "UPPER.example".into(),
                query_type: v1::DnsQueryType::A.into(),
            },
        ));
        assert!(matches!(
            RuntimeEvent::try_from(wire),
            Err(ProtocolError::InvalidDns(_))
        ));

        let mut wire = v1::RuntimeEvent::from(network_event());
        if let Some(v1::runtime_event::Payload::NetworkConnect(network)) = &mut wire.payload {
            network.dns_context = Some(v1::DnsContext {
                names: vec!["one.example".into(), "two.example".into()],
                observed_at_unix_nanos: 1,
                expires_at_unix_nanos: 2,
                confidence: v1::DnsConfidence::ObservedRecently.into(),
                ambiguous: false,
            });
        }
        assert!(matches!(
            RuntimeEvent::try_from(wire),
            Err(ProtocolError::InvalidDns("context.ambiguous"))
        ));
    }
}
