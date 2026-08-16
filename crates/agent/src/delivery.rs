use std::{
    collections::{BTreeMap, VecDeque},
    sync::atomic::Ordering,
};

use event_model::RuntimeEvent;

use crate::counters::Counters;

#[derive(Clone, Debug)]
pub struct PendingBatch {
    pub sequence: u64,
    pub events: Vec<RuntimeEvent>,
}

#[derive(Debug)]
pub struct EventBuffer {
    capacity: usize,
    batch_size: usize,
    next_sequence: u64,
    queued: VecDeque<RuntimeEvent>,
    pending: BTreeMap<u64, PendingBatch>,
}

impl EventBuffer {
    #[must_use]
    pub fn new(capacity: usize, batch_size: usize) -> Self {
        assert!(capacity > 0 && batch_size > 0 && batch_size <= capacity);
        Self {
            capacity,
            batch_size,
            next_sequence: 1,
            queued: VecDeque::new(),
            pending: BTreeMap::new(),
        }
    }

    pub fn push(&mut self, event: RuntimeEvent, counters: &Counters) -> bool {
        let occupied = self.queued.len()
            + self
                .pending
                .values()
                .map(|batch| batch.events.len())
                .sum::<usize>();
        if occupied >= self.capacity {
            counters.capacity_dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.queued.push_back(event);
        true
    }

    pub fn next_batch(&mut self, counters: &Counters) -> Option<PendingBatch> {
        if self.queued.is_empty() {
            return None;
        }
        let count = self.batch_size.min(self.queued.len());
        let events = self.queued.drain(..count).collect();
        let batch = PendingBatch {
            sequence: self.next_sequence,
            events,
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        counters
            .sent
            .fetch_add(batch.events.len() as u64, Ordering::Relaxed);
        self.pending.insert(batch.sequence, batch.clone());
        Some(batch)
    }

    pub fn acknowledge(&mut self, sequence: u64, counters: &Counters) -> bool {
        let Some(batch) = self.pending.remove(&sequence) else {
            return false;
        };
        counters
            .acknowledged
            .fetch_add(batch.events.len() as u64, Ordering::Relaxed);
        true
    }

    #[must_use]
    pub fn replay_pending(&self, counters: &Counters) -> Vec<PendingBatch> {
        let batches: Vec<_> = self.pending.values().cloned().collect();
        counters.retried.fetch_add(
            batches.iter().map(|batch| batch.events.len() as u64).sum(),
            Ordering::Relaxed,
        );
        batches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use event_model::{
        EVENT_SCHEMA_VERSION, EventPayload, KubernetesAttribution, ProcessExec, ProcessIdentity,
    };
    use uuid::Uuid;

    fn event() -> RuntimeEvent {
        RuntimeEvent {
            id: Uuid::new_v4(),
            observed_at: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION,
            attribution: KubernetesAttribution {
                project_id: Uuid::new_v4(),
                application_id: Uuid::new_v4(),
                node_name: "node".into(),
                namespace: "ns".into(),
                pod_uid: "p".into(),
                pod_name: "p".into(),
                container_id: "c".into(),
                container_name: "c".into(),
                workload_uid: "w".into(),
                workload_kind: "Deployment".into(),
                workload_name: "app".into(),
            },
            process: ProcessIdentity {
                cgroup_id: 1,
                pid: 1,
                tgid: 1,
                command: "sh".into(),
            },
            payload: EventPayload::ProcessExec(ProcessExec {
                executable: "/bin/sh".into(),
                parent_command: None,
            }),
        }
    }

    #[test]
    fn bounds_batches_retries_and_acknowledges() {
        let counters = Counters::default();
        let mut buffer = EventBuffer::new(2, 2);
        assert!(buffer.push(event(), &counters));
        assert!(buffer.push(event(), &counters));
        assert!(!buffer.push(event(), &counters));
        let batch = buffer.next_batch(&counters).unwrap();
        assert_eq!(batch.events.len(), 2);
        assert_eq!(buffer.replay_pending(&counters).len(), 1);
        assert!(buffer.acknowledge(batch.sequence, &counters));
        let snapshot = counters.snapshot();
        assert_eq!(snapshot.capacity_dropped, 1);
        assert_eq!(snapshot.acknowledged, 2);
        assert_eq!(snapshot.retried, 2);
    }
}
