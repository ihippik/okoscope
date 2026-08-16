#![no_std]
#![no_main]
#![allow(static_mut_refs)]

use agent_ebpf_common::{EVENT_KIND_EXEC, EVENT_KIND_SYSCALL, KernelEvent};
use aya_ebpf::{
    helpers::{
        bpf_get_current_cgroup_id, bpf_get_current_comm, bpf_get_current_pid_tgid, bpf_ktime_get_ns,
    },
    macros::{map, tracepoint},
    maps::{HashMap, RingBuf},
    programs::TracePointContext,
};

#[map]
static mut EVENTS: RingBuf = RingBuf::with_byte_size(256 * 1024, 0);

#[map]
static mut SYSCALL_ALLOWLIST: HashMap<u32, u8> = HashMap::with_max_entries(64, 0);

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
        event_kind,
        padding: [0; 3],
        command,
    });
    slot.submit(0);
    Ok(())
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
