use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct Counters {
    pub filtered: AtomicU64,
    pub unattributed: AtomicU64,
    pub unsupported: AtomicU64,
    pub decode_failed: AtomicU64,
    pub capacity_dropped: AtomicU64,
    pub kernel_lost: AtomicU64,
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
    pub sent: u64,
    pub retried: u64,
    pub acknowledged: u64,
}

impl Counters {
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
            sent: load(&self.sent),
            retried: load(&self.retried),
            acknowledged: load(&self.acknowledged),
        }
    }
}
