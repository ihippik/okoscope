use std::{collections::BTreeMap, sync::Arc, time::Duration};

use chrono::Utc;
use event_model::RuntimeEvent;
use protocol::v1::{AgentHello, AgentMessage, Heartbeat, agent_message, server_message};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::{
    config::{LoadedApplicationCredential, SafetyLimits, ServerConfig},
    counters::Counters,
    delivery::EventBuffer,
    session::{connect_with_backoff, handle_control},
};

#[derive(Debug)]
pub struct ApplicationStreams {
    routes: BTreeMap<Uuid, mpsc::Sender<RuntimeEvent>>,
    shutdown: watch::Sender<bool>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    counters: Arc<Counters>,
}

impl ApplicationStreams {
    pub fn start(
        server: &ServerConfig,
        credentials: Vec<LoadedApplicationCredential>,
        hello: &AgentHello,
        safety: &SafetyLimits,
        counters: Arc<Counters>,
    ) -> Self {
        let (shutdown, _) = watch::channel(false);
        let mut routes = BTreeMap::new();
        let mut tasks = Vec::with_capacity(credentials.len());
        for credential in credentials {
            let (sender, receiver) = mpsc::channel(safety.queue_capacity);
            routes.insert(credential.route_id, sender);
            tasks.push(tokio::spawn(run_stream(
                server.clone(),
                credential,
                hello.clone(),
                safety.queue_capacity,
                safety.batch_size,
                receiver,
                shutdown.subscribe(),
                counters.clone(),
            )));
        }
        Self {
            routes,
            shutdown,
            tasks,
            counters,
        }
    }

    pub fn route(&self, event: RuntimeEvent) -> bool {
        let route_id = event.attribution.application_id;
        let Some(sender) = self.routes.get(&route_id) else {
            self.counters
                .unattributed
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return false;
        };
        match sender.try_send(event) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.counters
                    .capacity_dropped
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        for task in self.tasks {
            let _ = task.await;
        }
    }

    pub fn route_count(&self) -> usize {
        self.routes.len()
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_stream(
    server: ServerConfig,
    credential: LoadedApplicationCredential,
    hello: AgentHello,
    queue_capacity: usize,
    batch_size: usize,
    mut receiver: mpsc::Receiver<RuntimeEvent>,
    mut shutdown: watch::Receiver<bool>,
    counters: Arc<Counters>,
) {
    let mut buffer = EventBuffer::new(queue_capacity, batch_size);
    loop {
        if *shutdown.borrow() {
            return;
        }
        let session = connect_with_backoff(&server, credential.token.trim(), hello.clone()).await;
        let mut session = match session {
            Ok(session) => session,
            Err(error) => {
                tracing::warn!(
                    route_id=%credential.route_id,
                    credential_path=%credential.canonical_path,
                    %error,
                    "Application stream connection failed"
                );
                continue;
            }
        };
        for batch in buffer.replay_pending(&counters) {
            if send_batch(&session.sender, batch).await.is_err() {
                break;
            }
        }
        let mut flush = tokio::time::interval(Duration::from_millis(10));
        let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
        loop {
            tokio::select! {
                event = receiver.recv() => {
                    let Some(event) = event else { return };
                    buffer.push(event, &counters);
                }
                _ = flush.tick() => {
                    if let Some(batch) = buffer.next_batch(&counters)
                        && send_batch(&session.sender, batch).await.is_err()
                    {
                        break;
                    }
                }
                _ = heartbeat.tick() => {
                    let message = AgentMessage {
                        protocol_version: event_model::PROTOCOL_VERSION,
                        message: Some(agent_message::Message::Heartbeat(Heartbeat {
                            sent_at_unix_nanos: Utc::now().timestamp_nanos_opt().unwrap_or_default(),
                            drop_counters: Some(counters.snapshot().into()),
                        })),
                    };
                    if session.sender.send(message).await.is_err() { break; }
                }
                incoming = session.incoming.message() => {
                    match incoming {
                        Ok(Some(message)) => match message.message {
                            Some(server_message::Message::BatchAcknowledgement(ack)) => {
                                buffer.acknowledge(ack.sequence, &counters);
                            }
                            Some(server_message::Message::Control(control)) => {
                                let result = handle_control(control);
                                if session.sender.send(AgentMessage {
                                    protocol_version: event_model::PROTOCOL_VERSION,
                                    message: Some(agent_message::Message::ControlResult(result)),
                                }).await.is_err() { break; }
                            }
                            Some(server_message::Message::SessionAccepted(accepted)) => {
                                tracing::info!(
                                    route_id=%credential.route_id,
                                    application_id=%accepted.application_id,
                                    cluster_id=%accepted.cluster_id,
                                    "Application stream accepted"
                                );
                            }
                            None => tracing::warn!(route_id=%credential.route_id, "unsupported server message"),
                        },
                        Ok(None) | Err(_) => break,
                    }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { return; }
                }
            }
        }
        tracing::warn!(route_id=%credential.route_id, "Application stream disconnected; reconnecting");
    }
}

async fn send_batch(
    sender: &mpsc::Sender<AgentMessage>,
    batch: crate::delivery::PendingBatch,
) -> Result<(), mpsc::error::SendError<AgentMessage>> {
    sender
        .send(AgentMessage {
            protocol_version: event_model::PROTOCOL_VERSION,
            message: Some(agent_message::Message::EventBatch(
                protocol::v1::EventBatch {
                    sequence: batch.sequence,
                    events: batch.events.into_iter().map(Into::into).collect(),
                },
            )),
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use event_model::{
        EVENT_SCHEMA_VERSION, EventPayload, KubernetesAttribution, ProcessExec, ProcessIdentity,
    };

    fn event(route_id: Uuid) -> RuntimeEvent {
        RuntimeEvent {
            id: Uuid::new_v4(),
            observed_at: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION,
            attribution: KubernetesAttribution {
                project_id: Uuid::nil(),
                application_id: route_id,
                node_name: "node".into(),
                namespace: "ns".into(),
                pod_uid: "pod".into(),
                pod_name: "pod".into(),
                container_id: "container".into(),
                container_name: "container".into(),
                workload_uid: "workload".into(),
                workload_kind: "Deployment".into(),
                workload_name: "app".into(),
                release: None,
            },
            process: ProcessIdentity {
                cgroup_id: 1,
                pid: 1,
                tgid: 1,
                command: "app".into(),
            },
            payload: EventPayload::ProcessExec(ProcessExec {
                executable: "/app".into(),
                parent_command: None,
            }),
        }
    }

    #[test]
    fn unknown_route_is_isolated_and_counted() {
        let streams = ApplicationStreams {
            routes: BTreeMap::new(),
            shutdown: watch::channel(false).0,
            tasks: Vec::new(),
            counters: Arc::new(Counters::default()),
        };
        assert!(!streams.route(event(Uuid::new_v4())));
        assert_eq!(streams.counters.snapshot().unattributed, 1);
    }

    #[test]
    fn distinct_routes_do_not_cross_deliver_or_share_failure() {
        let healthy_route = Uuid::new_v4();
        let failed_route = Uuid::new_v4();
        let (healthy_sender, mut healthy_receiver) = mpsc::channel(1);
        let (failed_sender, failed_receiver) = mpsc::channel(1);
        drop(failed_receiver);
        let streams = ApplicationStreams {
            routes: BTreeMap::from([
                (healthy_route, healthy_sender),
                (failed_route, failed_sender),
            ]),
            shutdown: watch::channel(false).0,
            tasks: Vec::new(),
            counters: Arc::new(Counters::default()),
        };

        assert!(!streams.route(event(failed_route)));
        assert!(streams.route(event(healthy_route)));
        assert_eq!(
            healthy_receiver
                .try_recv()
                .unwrap()
                .attribution
                .application_id,
            healthy_route
        );
        assert!(healthy_receiver.try_recv().is_err());
    }

    #[test]
    fn full_route_drops_only_that_route_and_counts_capacity() {
        let full_route = Uuid::new_v4();
        let healthy_route = Uuid::new_v4();
        let (full_sender, _full_receiver) = mpsc::channel(1);
        let (healthy_sender, mut healthy_receiver) = mpsc::channel(1);
        let streams = ApplicationStreams {
            routes: BTreeMap::from([(full_route, full_sender), (healthy_route, healthy_sender)]),
            shutdown: watch::channel(false).0,
            tasks: Vec::new(),
            counters: Arc::new(Counters::default()),
        };

        assert!(streams.route(event(full_route)));
        assert!(!streams.route(event(full_route)));
        assert!(streams.route(event(healthy_route)));
        assert_eq!(streams.counters.snapshot().capacity_dropped, 1);
        assert_eq!(
            healthy_receiver
                .try_recv()
                .unwrap()
                .attribution
                .application_id,
            healthy_route
        );
    }
}
