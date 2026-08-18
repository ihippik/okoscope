use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use agent_ebpf_common::{COMMAND_LEN, KernelEvent, NETWORK_ADDRESS_LEN};
use event_model::{
    NetworkAddressFamily, NetworkConnect, NetworkConnectError, NetworkConnectOutcome,
};
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("kernel event has size {actual}, expected {expected}")]
    InvalidSize { actual: usize, expected: usize },
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum NetworkDecodeError {
    #[error("unsupported network address family {0}")]
    UnsupportedFamily(u8),
    #[error("unsupported connect outcome {0}")]
    UnsupportedOutcome(u8),
    #[error(transparent)]
    Invalid(#[from] NetworkConnectError),
}

pub fn decode(bytes: &[u8]) -> Result<KernelEvent, DecodeError> {
    if bytes.len() != KernelEvent::SIZE {
        return Err(DecodeError::InvalidSize {
            actual: bytes.len(),
            expected: KernelEvent::SIZE,
        });
    }
    let u64_at = |offset: usize| {
        u64::from_ne_bytes(
            bytes[offset..offset + 8]
                .try_into()
                .expect("validated fixed layout"),
        )
    };
    let u32_at = |offset: usize| {
        u32::from_ne_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("validated fixed layout"),
        )
    };
    let i32_at = |offset: usize| {
        i32::from_ne_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("validated fixed layout"),
        )
    };
    let u16_at = |offset: usize| {
        u16::from_ne_bytes(
            bytes[offset..offset + 2]
                .try_into()
                .expect("validated fixed layout"),
        )
    };
    let mut destination_address = [0_u8; NETWORK_ADDRESS_LEN];
    destination_address.copy_from_slice(&bytes[40..56]);
    let mut command = [0_u8; COMMAND_LEN];
    command.copy_from_slice(&bytes[56..72]);
    Ok(KernelEvent {
        timestamp_ns: u64_at(0),
        cgroup_id: u64_at(8),
        pid_tgid: u64_at(16),
        syscall_id: u32_at(24),
        connect_result: i32_at(28),
        event_kind: bytes[32],
        address_family: bytes[33],
        connect_outcome: bytes[34],
        padding: bytes[35],
        destination_port: u16_at(36),
        errno: u16_at(38),
        destination_address,
        command,
    })
}

pub fn network_connect(event: &KernelEvent) -> Result<NetworkConnect, NetworkDecodeError> {
    let (address_family, destination_address) = match event.address_family {
        agent_ebpf_common::ADDRESS_FAMILY_IPV4 => (
            NetworkAddressFamily::Ipv4,
            IpAddr::V4(Ipv4Addr::from(
                <[u8; 4]>::try_from(&event.destination_address[..4])
                    .expect("fixed IPv4 address slice"),
            )),
        ),
        agent_ebpf_common::ADDRESS_FAMILY_IPV6 => (
            NetworkAddressFamily::Ipv6,
            IpAddr::V6(Ipv6Addr::from(event.destination_address)),
        ),
        family => return Err(NetworkDecodeError::UnsupportedFamily(family)),
    };
    let (outcome, errno) = match event.connect_outcome {
        agent_ebpf_common::CONNECT_OUTCOME_SUCCEEDED => (
            NetworkConnectOutcome::Succeeded,
            (event.errno != 0).then_some(event.errno),
        ),
        agent_ebpf_common::CONNECT_OUTCOME_IN_PROGRESS => {
            (NetworkConnectOutcome::InProgress, Some(event.errno))
        }
        agent_ebpf_common::CONNECT_OUTCOME_FAILED => {
            (NetworkConnectOutcome::Failed, Some(event.errno))
        }
        outcome => return Err(NetworkDecodeError::UnsupportedOutcome(outcome)),
    };
    NetworkConnect::new(
        address_family,
        destination_address,
        event.destination_port,
        outcome,
        errno,
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wrong_size() {
        assert!(matches!(
            decode(&[0; 4]),
            Err(DecodeError::InvalidSize { .. })
        ));
    }

    #[test]
    fn decodes_network_record_with_native_layout() {
        let mut bytes = [0_u8; KernelEvent::SIZE];
        bytes[28..32].copy_from_slice(&(-111_i32).to_ne_bytes());
        bytes[32] = agent_ebpf_common::EVENT_KIND_NETWORK_CONNECT;
        bytes[33] = agent_ebpf_common::ADDRESS_FAMILY_IPV4;
        bytes[34] = agent_ebpf_common::CONNECT_OUTCOME_FAILED;
        bytes[36..38].copy_from_slice(&443_u16.to_ne_bytes());
        bytes[38..40].copy_from_slice(&111_u16.to_ne_bytes());
        bytes[40..44].copy_from_slice(&[203, 0, 113, 7]);
        assert_eq!(decode(&bytes).unwrap().connect_result, -111);
        assert_eq!(decode(&bytes).unwrap().destination_port, 443);
        assert_eq!(
            decode(&bytes).unwrap().destination_address[0..4],
            [203, 0, 113, 7]
        );
        let decoded = decode(&bytes).unwrap();
        let network = network_connect(&decoded).unwrap();
        assert_eq!(network.destination_address.to_string(), "203.0.113.7");
        assert_eq!(network.outcome, NetworkConnectOutcome::Failed);
        assert_eq!(network.errno, Some(111));
    }

    #[test]
    fn decodes_ipv6_and_rejects_invalid_network_semantics() {
        let mut event = KernelEvent {
            timestamp_ns: 1,
            cgroup_id: 2,
            pid_tgid: 3,
            syscall_id: 0,
            connect_result: -115,
            event_kind: agent_ebpf_common::EVENT_KIND_NETWORK_CONNECT,
            address_family: agent_ebpf_common::ADDRESS_FAMILY_IPV6,
            connect_outcome: agent_ebpf_common::CONNECT_OUTCOME_IN_PROGRESS,
            padding: 0,
            destination_port: 443,
            errno: event_model::LINUX_EINPROGRESS,
            destination_address: Ipv6Addr::LOCALHOST.octets(),
            command: [0; COMMAND_LEN],
        };
        assert_eq!(
            network_connect(&event).unwrap().destination_address,
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        );
        event.address_family = 99;
        assert_eq!(
            network_connect(&event),
            Err(NetworkDecodeError::UnsupportedFamily(99))
        );
        event.address_family = agent_ebpf_common::ADDRESS_FAMILY_IPV6;
        event.connect_outcome = agent_ebpf_common::CONNECT_OUTCOME_SUCCEEDED;
        assert_eq!(
            network_connect(&event),
            Err(NetworkDecodeError::Invalid(
                NetworkConnectError::InconsistentOutcome
            ))
        );
    }
}
