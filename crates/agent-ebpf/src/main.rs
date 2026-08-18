#![no_std]
#![no_main]
#![allow(static_mut_refs)]

use agent_ebpf_common::{
    ADDRESS_FAMILY_IPV4, ADDRESS_FAMILY_IPV6, CONNECT_OUTCOME_FAILED, CONNECT_OUTCOME_IN_PROGRESS,
    CONNECT_OUTCOME_SUCCEEDED, EVENT_KIND_EXEC, EVENT_KIND_NETWORK_CONNECT, EVENT_KIND_SYSCALL,
    KernelEvent, NETWORK_ADDRESS_LEN, NETWORK_COUNTER_CAPACITY, NETWORK_COUNTER_CORRELATION_MISS,
    NETWORK_COUNTER_COUNT, NETWORK_COUNTER_DECODE_FAILED, NETWORK_COUNTER_KERNEL_LOST,
    NETWORK_COUNTER_UNSUPPORTED_FAMILY, PendingConnect,
};
use aya_ebpf::{
    helpers::{
        bpf_get_current_cgroup_id, bpf_get_current_comm, bpf_get_current_pid_tgid,
        bpf_ktime_get_ns, bpf_probe_read_user,
    },
    macros::{map, tracepoint},
    maps::{HashMap, PerCpuArray, RingBuf},
    programs::TracePointContext,
};

#[map]
static mut EVENTS: RingBuf = RingBuf::with_byte_size(256 * 1024, 0);

#[map]
static mut SYSCALL_ALLOWLIST: HashMap<u32, u8> = HashMap::with_max_entries(64, 0);

#[map]
static mut PENDING_CONNECTS: HashMap<u64, PendingConnect> = HashMap::with_max_entries(4096, 0);

#[map]
static mut NETWORK_COUNTERS: PerCpuArray<u64> =
    PerCpuArray::with_max_entries(NETWORK_COUNTER_COUNT, 0);

const AF_INET: u16 = 2;
const AF_INET6: u16 = 10;
const EINPROGRESS: i64 = 115;

#[repr(C)]
#[derive(Clone, Copy)]
struct SockAddrIn {
    family: u16,
    port: [u8; 2],
    address: [u8; 4],
    zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SockAddrIn6 {
    family: u16,
    port: [u8; 2],
    flow_info: u32,
    address: [u8; 16],
    scope_id: u32,
}

#[tracepoint]
pub fn okoscope_exec(_ctx: TracePointContext) -> u32 {
    match emit(EVENT_KIND_EXEC, 0) {
        Ok(()) => 0,
        Err(error) => error,
    }
}

#[tracepoint]
pub fn okoscope_sys_enter(ctx: TracePointContext) -> u32 {
    match try_sys_enter(&ctx) {
        Ok(()) => 0,
        Err(error) => error,
    }
}

#[tracepoint]
pub fn okoscope_connect_enter(ctx: TracePointContext) -> u32 {
    match try_connect_enter(&ctx) {
        Ok(()) => 0,
        Err(error) => error,
    }
}

#[tracepoint]
pub fn okoscope_connect_exit(ctx: TracePointContext) -> u32 {
    match try_connect_exit(&ctx) {
        Ok(()) => 0,
        Err(error) => error,
    }
}

fn try_sys_enter(ctx: &TracePointContext) -> Result<(), u32> {
    let syscall_id: u64 = unsafe { ctx.read_at(8).map_err(|_| 1_u32)? };
    let syscall_id = syscall_id as u32;
    if unsafe { SYSCALL_ALLOWLIST.get(&syscall_id) }.is_none() {
        return Ok(());
    }
    emit(EVENT_KIND_SYSCALL, syscall_id)
}

fn emit(event_kind: u8, syscall_id: u32) -> Result<(), u32> {
    let Some(mut slot) = (unsafe { EVENTS.reserve::<KernelEvent>(0) }) else {
        return Err(2);
    };
    let command = bpf_get_current_comm().unwrap_or([0; 16]);
    slot.write(KernelEvent {
        timestamp_ns: unsafe { bpf_ktime_get_ns() },
        cgroup_id: unsafe { bpf_get_current_cgroup_id() },
        pid_tgid: bpf_get_current_pid_tgid(),
        syscall_id,
        connect_result: 0,
        event_kind,
        address_family: 0,
        connect_outcome: 0,
        padding: 0,
        destination_port: 0,
        errno: 0,
        destination_address: [0; NETWORK_ADDRESS_LEN],
        command,
    });
    slot.submit(0);
    Ok(())
}

fn try_connect_enter(ctx: &TracePointContext) -> Result<(), u32> {
    let address_ptr: u64 = unsafe { ctx.read_at(24).map_err(|_| 1_u32)? };
    let address_len: u64 = unsafe { ctx.read_at(32).map_err(|_| 1_u32)? };
    if address_ptr == 0 {
        increment_counter(NETWORK_COUNTER_DECODE_FAILED);
        return Ok(());
    }
    let family = unsafe { bpf_probe_read_user(address_ptr as *const u16) };
    let Ok(family) = family else {
        increment_counter(NETWORK_COUNTER_DECODE_FAILED);
        return Ok(());
    };
    let mut destination_address = [0_u8; NETWORK_ADDRESS_LEN];
    let (address_family, destination_port) = match family {
        AF_INET if address_len >= core::mem::size_of::<SockAddrIn>() as u64 => {
            let Ok(address) = (unsafe { bpf_probe_read_user(address_ptr as *const SockAddrIn) })
            else {
                increment_counter(NETWORK_COUNTER_DECODE_FAILED);
                return Ok(());
            };
            destination_address[..4].copy_from_slice(&address.address);
            (ADDRESS_FAMILY_IPV4, u16::from_be_bytes(address.port))
        }
        AF_INET6 if address_len >= core::mem::size_of::<SockAddrIn6>() as u64 => {
            let Ok(address) = (unsafe { bpf_probe_read_user(address_ptr as *const SockAddrIn6) })
            else {
                increment_counter(NETWORK_COUNTER_DECODE_FAILED);
                return Ok(());
            };
            destination_address.copy_from_slice(&address.address);
            (ADDRESS_FAMILY_IPV6, u16::from_be_bytes(address.port))
        }
        AF_INET | AF_INET6 => {
            increment_counter(NETWORK_COUNTER_DECODE_FAILED);
            return Ok(());
        }
        _ => {
            increment_counter(NETWORK_COUNTER_UNSUPPORTED_FAMILY);
            return Ok(());
        }
    };
    if destination_port == 0 {
        increment_counter(NETWORK_COUNTER_DECODE_FAILED);
        return Ok(());
    }
    let pid_tgid = bpf_get_current_pid_tgid();
    let pending = PendingConnect {
        timestamp_ns: unsafe { bpf_ktime_get_ns() },
        cgroup_id: unsafe { bpf_get_current_cgroup_id() },
        pid_tgid,
        destination_address,
        destination_port,
        address_family,
        padding: [0; 5],
        command: bpf_get_current_comm().unwrap_or([0; 16]),
    };
    if unsafe { PENDING_CONNECTS.insert(&pid_tgid, &pending, 0) }.is_err() {
        increment_counter(NETWORK_COUNTER_CAPACITY);
    }
    Ok(())
}

fn try_connect_exit(ctx: &TracePointContext) -> Result<(), u32> {
    let pid_tgid = bpf_get_current_pid_tgid();
    let Some(pending_ptr) = (unsafe { PENDING_CONNECTS.get_ptr(&pid_tgid) }) else {
        increment_counter(NETWORK_COUNTER_CORRELATION_MISS);
        return Ok(());
    };
    let pending = unsafe { *pending_ptr };
    let _ = unsafe { PENDING_CONNECTS.remove(&pid_tgid) };
    let result: i64 = unsafe { ctx.read_at(16).map_err(|_| 1_u32)? };
    let (connect_outcome, errno) = if result == 0 {
        (CONNECT_OUTCOME_SUCCEEDED, 0)
    } else if result == -EINPROGRESS {
        (CONNECT_OUTCOME_IN_PROGRESS, EINPROGRESS as u16)
    } else if (-4095..0).contains(&result) {
        (CONNECT_OUTCOME_FAILED, (-result) as u16)
    } else {
        increment_counter(NETWORK_COUNTER_DECODE_FAILED);
        return Ok(());
    };
    let Some(mut slot) = (unsafe { EVENTS.reserve::<KernelEvent>(0) }) else {
        increment_counter(NETWORK_COUNTER_KERNEL_LOST);
        return Ok(());
    };
    slot.write(KernelEvent {
        timestamp_ns: pending.timestamp_ns,
        cgroup_id: pending.cgroup_id,
        pid_tgid: pending.pid_tgid,
        syscall_id: 0,
        connect_result: result as i32,
        event_kind: EVENT_KIND_NETWORK_CONNECT,
        address_family: pending.address_family,
        connect_outcome,
        padding: 0,
        destination_port: pending.destination_port,
        errno,
        destination_address: pending.destination_address,
        command: pending.command,
    });
    slot.submit(0);
    Ok(())
}

fn increment_counter(index: u32) {
    if let Some(value) = unsafe { NETWORK_COUNTERS.get_ptr_mut(index) } {
        unsafe {
            *value = (*value).saturating_add(1);
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
