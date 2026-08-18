#![no_std]
#![no_main]
#![allow(static_mut_refs)]

use agent_ebpf_common::{
    ADDRESS_FAMILY_IPV4, ADDRESS_FAMILY_IPV6, CONNECT_OUTCOME_FAILED, CONNECT_OUTCOME_IN_PROGRESS,
    CONNECT_OUTCOME_SUCCEEDED, DNS_ADDRESS_LEN, DNS_CAPTURE_BYTES, DNS_COUNTER_ATTRIBUTION_FAILED,
    DNS_COUNTER_COUNT, DNS_COUNTER_DECODE_FAILED, DNS_COUNTER_OVERSIZE, DNS_COUNTER_RING_LOST,
    DNS_COUNTER_UNSUPPORTED_FRAMING, DNS_DIRECTION_EGRESS, DNS_DIRECTION_INGRESS,
    DNS_TRANSPORT_TCP, DNS_TRANSPORT_UDP, DnsPacketRecord, EVENT_KIND_EXEC,
    EVENT_KIND_NETWORK_CONNECT, EVENT_KIND_SYSCALL, KernelEvent, NETWORK_ADDRESS_LEN,
    NETWORK_COUNTER_CAPACITY, NETWORK_COUNTER_CORRELATION_MISS, NETWORK_COUNTER_COUNT,
    NETWORK_COUNTER_DECODE_FAILED, NETWORK_COUNTER_KERNEL_LOST, NETWORK_COUNTER_UNSUPPORTED_FAMILY,
    PendingConnect,
};
use aya_ebpf::{
    EbpfContext,
    helpers::{
        bpf_get_current_cgroup_id, bpf_get_current_comm, bpf_get_current_pid_tgid,
        bpf_get_socket_cookie, bpf_ktime_get_ns, bpf_probe_read_user, bpf_skb_cgroup_id,
    },
    macros::{cgroup_skb, map, tracepoint},
    maps::{HashMap, PerCpuArray, RingBuf},
    programs::{SkBuffContext, TracePointContext},
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

#[map]
static mut DNS_EVENTS: RingBuf = RingBuf::with_byte_size(1024 * 1024, 0);

#[map]
static mut DNS_COUNTERS: PerCpuArray<u64> = PerCpuArray::with_max_entries(DNS_COUNTER_COUNT, 0);

const AF_INET: u16 = 2;
const AF_INET6: u16 = 10;
const EINPROGRESS: i64 = 115;
const AF_INET_U32: u32 = 2;
const AF_INET6_U32: u32 = 10;
const IPPROTO_TCP: u8 = 6;
const IPPROTO_UDP: u8 = 17;
const DNS_PORT: u16 = 53;

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

#[cgroup_skb(egress)]
pub fn okoscope_dns_egress(ctx: SkBuffContext) -> i32 {
    let _ = capture_dns(&ctx, DNS_DIRECTION_EGRESS);
    1
}

#[cgroup_skb(ingress)]
pub fn okoscope_dns_ingress(ctx: SkBuffContext) -> i32 {
    let _ = capture_dns(&ctx, DNS_DIRECTION_INGRESS);
    1
}

fn capture_dns(ctx: &SkBuffContext, direction: u8) -> Result<(), ()> {
    let family = ctx.skb.family();
    let (address_family, protocol, transport_offset, resolver_address) = match family {
        AF_INET_U32 => parse_ipv4(ctx, direction)?,
        AF_INET6_U32 => parse_ipv6(ctx, direction)?,
        _ => return Ok(()),
    };
    let (transport, resolver_port, payload_offset, sequence, tcp_flags) =
        parse_transport(ctx, protocol, transport_offset, direction)?;
    if resolver_port != DNS_PORT {
        return Ok(());
    }
    let cgroup_id = unsafe { bpf_skb_cgroup_id(ctx.as_ptr().cast()) };
    if cgroup_id == 0 {
        increment_dns_counter(DNS_COUNTER_ATTRIBUTION_FAILED);
        return Ok(());
    }
    let packet_len = usize::try_from(ctx.len()).map_err(|_| ())?;
    let payload_len = packet_len.checked_sub(payload_offset).ok_or_else(|| {
        increment_dns_counter(DNS_COUNTER_DECODE_FAILED);
    })?;
    if payload_len == 0 {
        increment_dns_counter(DNS_COUNTER_DECODE_FAILED);
        return Ok(());
    }
    if payload_len > DNS_CAPTURE_BYTES {
        increment_dns_counter(DNS_COUNTER_OVERSIZE);
    }
    let Some(mut slot) = (unsafe { DNS_EVENTS.reserve::<DnsPacketRecord>(0) }) else {
        increment_dns_counter(DNS_COUNTER_RING_LOST);
        return Ok(());
    };
    let captured_len = payload_len.min(DNS_CAPTURE_BYTES);
    let record = unsafe { &mut *slot.as_mut_ptr() };
    record.timestamp_ns = unsafe { bpf_ktime_get_ns() };
    record.cgroup_id = cgroup_id;
    record.socket_cookie = unsafe { bpf_get_socket_cookie(ctx.as_ptr()) };
    // Cgroup skb programs cannot call current-task helpers on all supported kernels.
    // Workload attribution therefore relies on the trusted cgroup id for both directions.
    record.pid_tgid = 0;
    record.sequence = sequence;
    record.payload_len = captured_len as u16;
    record.resolver_port = resolver_port;
    record.address_family = address_family;
    record.transport = transport;
    record.direction = direction;
    record.tcp_flags = tcp_flags;
    record.resolver_address = resolver_address;
    record.command = [0; 16];
    record.payload = [0; DNS_CAPTURE_BYTES];
    let mut index = 0;
    while index < DNS_CAPTURE_BYTES {
        if index >= captured_len {
            break;
        }
        let Ok(byte) = ctx.load::<u8>(payload_offset + index) else {
            increment_dns_counter(DNS_COUNTER_DECODE_FAILED);
            slot.discard(0);
            return Ok(());
        };
        record.payload[index] = byte;
        index += 1;
    }
    slot.submit(0);
    Ok(())
}

fn parse_ipv4(
    ctx: &SkBuffContext,
    direction: u8,
) -> Result<(u8, u8, usize, [u8; DNS_ADDRESS_LEN]), ()> {
    let version_ihl: u8 = ctx.load(0).map_err(|_| ())?;
    if version_ihl >> 4 != 4 {
        increment_dns_counter(DNS_COUNTER_DECODE_FAILED);
        return Err(());
    }
    let header_len = usize::from(version_ihl & 0x0f) * 4;
    if header_len < 20 {
        increment_dns_counter(DNS_COUNTER_DECODE_FAILED);
        return Err(());
    }
    let fragment: u16 = u16::from_be(ctx.load(6).map_err(|_| ())?);
    if fragment & 0x3fff != 0 {
        increment_dns_counter(DNS_COUNTER_UNSUPPORTED_FRAMING);
        return Err(());
    }
    let protocol: u8 = ctx.load(9).map_err(|_| ())?;
    let address_offset = if direction == DNS_DIRECTION_EGRESS {
        16
    } else {
        12
    };
    let address: [u8; 4] = ctx.load(address_offset).map_err(|_| ())?;
    let mut resolver = [0; DNS_ADDRESS_LEN];
    resolver[..4].copy_from_slice(&address);
    Ok((
        agent_ebpf_common::ADDRESS_FAMILY_IPV4,
        protocol,
        header_len,
        resolver,
    ))
}

fn parse_ipv6(
    ctx: &SkBuffContext,
    direction: u8,
) -> Result<(u8, u8, usize, [u8; DNS_ADDRESS_LEN]), ()> {
    let version: u8 = ctx.load(0).map_err(|_| ())?;
    if version >> 4 != 6 {
        increment_dns_counter(DNS_COUNTER_DECODE_FAILED);
        return Err(());
    }
    let protocol: u8 = ctx.load(6).map_err(|_| ())?;
    if protocol != IPPROTO_UDP && protocol != IPPROTO_TCP {
        increment_dns_counter(DNS_COUNTER_UNSUPPORTED_FRAMING);
        return Err(());
    }
    let address_offset = if direction == DNS_DIRECTION_EGRESS {
        24
    } else {
        8
    };
    let resolver: [u8; 16] = ctx.load(address_offset).map_err(|_| ())?;
    Ok((
        agent_ebpf_common::ADDRESS_FAMILY_IPV6,
        protocol,
        40,
        resolver,
    ))
}

fn parse_transport(
    ctx: &SkBuffContext,
    protocol: u8,
    offset: usize,
    direction: u8,
) -> Result<(u8, u16, usize, u32, u8), ()> {
    let source_port = u16::from_be(ctx.load(offset).map_err(|_| ())?);
    let destination_port = u16::from_be(ctx.load(offset + 2).map_err(|_| ())?);
    let resolver_port = if direction == DNS_DIRECTION_EGRESS {
        destination_port
    } else {
        source_port
    };
    match protocol {
        IPPROTO_UDP => {
            let length = usize::from(u16::from_be(ctx.load(offset + 4).map_err(|_| ())?));
            if length < 8 {
                increment_dns_counter(DNS_COUNTER_DECODE_FAILED);
                return Err(());
            }
            Ok((DNS_TRANSPORT_UDP, resolver_port, offset + 8, 0, 0))
        }
        IPPROTO_TCP => {
            let sequence = u32::from_be(ctx.load(offset + 4).map_err(|_| ())?);
            let data_offset: u8 = ctx.load(offset + 12).map_err(|_| ())?;
            let header_len = usize::from(data_offset >> 4) * 4;
            if header_len < 20 {
                increment_dns_counter(DNS_COUNTER_DECODE_FAILED);
                return Err(());
            }
            let flags: u8 = ctx.load(offset + 13).map_err(|_| ())?;
            Ok((
                DNS_TRANSPORT_TCP,
                resolver_port,
                offset + header_len,
                sequence,
                flags,
            ))
        }
        _ => Ok((0, resolver_port, offset, 0, 0)),
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

fn increment_dns_counter(index: u32) {
    if let Some(value) = unsafe { DNS_COUNTERS.get_ptr_mut(index) } {
        unsafe {
            *value = (*value).saturating_add(1);
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
