#![no_std]

pub const COMMAND_LEN: usize = 16;
pub const EVENT_KIND_EXEC: u8 = 1;
pub const EVENT_KIND_SYSCALL: u8 = 2;
pub const EVENT_KIND_NETWORK_CONNECT: u8 = 3;
pub const ADDRESS_FAMILY_IPV4: u8 = 1;
pub const ADDRESS_FAMILY_IPV6: u8 = 2;
pub const CONNECT_OUTCOME_SUCCEEDED: u8 = 1;
pub const CONNECT_OUTCOME_IN_PROGRESS: u8 = 2;
pub const CONNECT_OUTCOME_FAILED: u8 = 3;
pub const NETWORK_ADDRESS_LEN: usize = 16;
pub const NETWORK_COUNTER_CAPACITY: u32 = 0;
pub const NETWORK_COUNTER_CORRELATION_MISS: u32 = 1;
pub const NETWORK_COUNTER_DECODE_FAILED: u32 = 2;
pub const NETWORK_COUNTER_UNSUPPORTED_FAMILY: u32 = 3;
pub const NETWORK_COUNTER_KERNEL_LOST: u32 = 4;
pub const NETWORK_COUNTER_COUNT: u32 = 5;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KernelEvent {
    pub timestamp_ns: u64,
    pub cgroup_id: u64,
    pub pid_tgid: u64,
    pub syscall_id: u32,
    pub connect_result: i32,
    pub event_kind: u8,
    pub address_family: u8,
    pub connect_outcome: u8,
    pub padding: u8,
    pub destination_port: u16,
    pub errno: u16,
    pub destination_address: [u8; NETWORK_ADDRESS_LEN],
    pub command: [u8; COMMAND_LEN],
}

impl KernelEvent {
    pub const SIZE: usize = core::mem::size_of::<Self>();
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PendingConnect {
    pub timestamp_ns: u64,
    pub cgroup_id: u64,
    pub pid_tgid: u64,
    pub destination_address: [u8; NETWORK_ADDRESS_LEN],
    pub destination_port: u16,
    pub address_family: u8,
    pub padding: [u8; 5],
    pub command: [u8; COMMAND_LEN],
}

const _: [(); 72] = [(); core::mem::size_of::<KernelEvent>()];
const _: [(); 64] = [(); core::mem::size_of::<PendingConnect>()];
