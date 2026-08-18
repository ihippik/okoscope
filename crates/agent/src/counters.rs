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

impl Counters {
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
            sent: load(&self.sent),
            retried: load(&self.retried),
            acknowledged: load(&self.acknowledged),
        }
    }
}
