#![no_std]

pub const COMMAND_LEN: usize = 16;
pub const EVENT_KIND_EXEC: u8 = 1;
pub const EVENT_KIND_SYSCALL: u8 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KernelEvent {
    pub timestamp_ns: u64,
    pub cgroup_id: u64,
    pub pid_tgid: u64,
    pub syscall_id: u32,
    pub event_kind: u8,
    pub padding: [u8; 3],
    pub command: [u8; COMMAND_LEN],
}

impl KernelEvent {
    pub const SIZE: usize = core::mem::size_of::<Self>();
}
