use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct Counters {
    pub filtered: AtomicU64,
    pub unattributed: AtomicU64,
    pub unsupported: AtomicU64,
    pub decode_failed: AtomicU64,
    pub capacity_dropped: AtomicU64,
    pub kernel_lost: AtomicU64,
    pub connect_correlation_capacity: AtomicU64,
    pub connect_correlation_miss: AtomicU64,
    pub connect_decode_failed: AtomicU64,
    pub connect_unsupported_family: AtomicU64,
    pub connect_kernel_lost: AtomicU64,
    pub dns_packet_decode_failed: AtomicU64,
    pub dns_malformed_compression: AtomicU64,
    pub dns_truncated: AtomicU64,
    pub dns_unsupported_record: AtomicU64,
    pub dns_correlation_miss: AtomicU64,
    pub dns_correlation_capacity: AtomicU64,
    pub dns_tcp_reassembly: AtomicU64,
    pub dns_rate_limited: AtomicU64,
    pub dns_capacity: AtomicU64,
    pub dns_kernel_lost: AtomicU64,
    pub dns_kernel_unsupported_framing: AtomicU64,
    pub dns_attribution_failed: AtomicU64,
    pub dns_oversize: AtomicU64,
    pub sent: AtomicU64,
    pub retried: AtomicU64,
    pub acknowledged: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CounterSnapshot {
    pub filtered: u64,
    pub unattributed: u64,
    pub unsupported: u64,
    pub decode_failed: u64,
    pub capacity_dropped: u64,
    pub kernel_lost: u64,
    pub connect_correlation_capacity: u64,
    pub connect_correlation_miss: u64,
    pub connect_decode_failed: u64,
    pub connect_unsupported_family: u64,
    pub connect_kernel_lost: u64,
    pub dns_packet_decode_failed: u64,
    pub dns_malformed_compression: u64,
    pub dns_truncated: u64,
    pub dns_unsupported_record: u64,
    pub dns_correlation_miss: u64,
    pub dns_correlation_capacity: u64,
    pub dns_tcp_reassembly: u64,
    pub dns_rate_limited: u64,
    pub dns_capacity: u64,
    pub dns_kernel_lost: u64,
    pub dns_kernel_unsupported_framing: u64,
    pub dns_attribution_failed: u64,
    pub dns_oversize: u64,
    pub sent: u64,
    pub retried: u64,
    pub acknowledged: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetworkKernelCounters {
    pub correlation_capacity: u64,
    pub correlation_miss: u64,
    pub decode_failed: u64,
    pub unsupported_family: u64,
    pub kernel_lost: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DnsKernelCounters {
    pub unsupported_framing: u64,
    pub attribution_failed: u64,
    pub decode_failed: u64,
    pub oversize: u64,
    pub ring_lost: u64,
}

impl Counters {
    pub fn update_dns_kernel(&self, value: DnsKernelCounters) {
        self.dns_packet_decode_failed
            .store(value.decode_failed, Ordering::Relaxed);
        self.dns_kernel_unsupported_framing
            .store(value.unsupported_framing, Ordering::Relaxed);
        self.dns_oversize.store(value.oversize, Ordering::Relaxed);
        self.dns_kernel_lost
            .store(value.ring_lost, Ordering::Relaxed);
        self.dns_attribution_failed
            .store(value.attribution_failed, Ordering::Relaxed);
    }

    pub fn update_network_kernel(&self, value: NetworkKernelCounters) {
        self.connect_correlation_capacity
            .store(value.correlation_capacity, Ordering::Relaxed);
        self.connect_correlation_miss
            .store(value.correlation_miss, Ordering::Relaxed);
        self.connect_decode_failed
            .store(value.decode_failed, Ordering::Relaxed);
        self.connect_unsupported_family
            .store(value.unsupported_family, Ordering::Relaxed);
        self.connect_kernel_lost
            .store(value.kernel_lost, Ordering::Relaxed);
    }

    #[must_use]
    pub fn snapshot(&self) -> CounterSnapshot {
        let load = |value: &AtomicU64| value.load(Ordering::Relaxed);
        CounterSnapshot {
            filtered: load(&self.filtered),
            unattributed: load(&self.unattributed),
            unsupported: load(&self.unsupported),
            decode_failed: load(&self.decode_failed),
            capacity_dropped: load(&self.capacity_dropped),
            kernel_lost: load(&self.kernel_lost),
            connect_correlation_capacity: load(&self.connect_correlation_capacity),
            connect_correlation_miss: load(&self.connect_correlation_miss),
            connect_decode_failed: load(&self.connect_decode_failed),
            connect_unsupported_family: load(&self.connect_unsupported_family),
            connect_kernel_lost: load(&self.connect_kernel_lost),
            dns_packet_decode_failed: load(&self.dns_packet_decode_failed),
            dns_malformed_compression: load(&self.dns_malformed_compression),
            dns_truncated: load(&self.dns_truncated),
            dns_unsupported_record: load(&self.dns_unsupported_record),
            dns_correlation_miss: load(&self.dns_correlation_miss),
            dns_correlation_capacity: load(&self.dns_correlation_capacity),
            dns_tcp_reassembly: load(&self.dns_tcp_reassembly),
            dns_rate_limited: load(&self.dns_rate_limited),
            dns_capacity: load(&self.dns_capacity),
            dns_kernel_lost: load(&self.dns_kernel_lost),
            dns_kernel_unsupported_framing: load(&self.dns_kernel_unsupported_framing),
            dns_attribution_failed: load(&self.dns_attribution_failed),
            dns_oversize: load(&self.dns_oversize),
            sent: load(&self.sent),
            retried: load(&self.retried),
            acknowledged: load(&self.acknowledged),
        }
    }
}
