use agent_ebpf_common::{COMMAND_LEN, KernelEvent};
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("kernel event has size {actual}, expected {expected}")]
    InvalidSize { actual: usize, expected: usize },
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
    let mut command = [0_u8; COMMAND_LEN];
    command.copy_from_slice(&bytes[32..32 + COMMAND_LEN]);
    Ok(KernelEvent {
        timestamp_ns: u64_at(0),
        cgroup_id: u64_at(8),
        pid_tgid: u64_at(16),
        syscall_id: u32_at(24),
        event_kind: bytes[28],
        padding: [bytes[29], bytes[30], bytes[31]],
        command,
    })
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
}
