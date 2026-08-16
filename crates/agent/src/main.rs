#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("okoscope-agent requires Linux; userspace libraries and tests support this host");
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        path::PathBuf,
        sync::{Arc, atomic::Ordering},
        time::Duration,
    };

    use agent::{
        attribution::{AttributionCache, resolve_and_count, run_watches},
        cgroup,
        config::AgentConfig,
        counters::Counters,
        delivery::{EventBuffer, PendingBatch},
        observer::Observer,
        session::{connect_with_backoff, handle_control},
        syscall::{self, Architecture},
    };
    use anyhow::{Context, Result};
    use chrono::Utc;
    use clap::Parser;
    use event_model::{
        EVENT_SCHEMA_VERSION, EventPayload, ProcessExec, ProcessIdentity, RuntimeEvent,
        SyscallEvent,
    };
    use protocol::v1::{
        AgentHello, AgentMessage, EventBatch, Heartbeat, agent_message, server_message,
    };
    use tracing_subscriber::EnvFilter;
    use uuid::Uuid;

    #[derive(Debug, Parser)]
    struct Args {
        #[arg(
            long,
            env = "OKOSCOPE_CONFIG",
            default_value = "/etc/okoscope/agent.yaml"
        )]
        config: PathBuf,
        #[arg(
            long,
            env = "OKOSCOPE_EBPF_OBJECT",
            default_value = "/opt/okoscope/agent-ebpf"
        )]
        ebpf_object: PathBuf,
    }

    pub async fn main() -> Result<()> {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(EnvFilter::from_default_env())
            .init();
        let args = Args::parse();
        let architecture = Architecture::current().context("unsupported CPU architecture")?;
        let yaml = tokio::fs::read_to_string(&args.config)
            .await
            .context("read agent configuration")?;
        let mut config = AgentConfig::from_yaml(&yaml, architecture)?;
        if let Ok(node_name) = std::env::var("OKOSCOPE_NODE_NAME") {
            config.identity.node_name = node_name;
        }
        let credential = tokio::fs::read_to_string(&config.server.credential_file)
            .await
            .context("read cluster credential")?;
        let counters = Arc::new(Counters::default());
        let cache = Arc::new(AttributionCache::new(Duration::from_secs(30)));
        let watch_cache = cache.clone();
        let watch_client = kube::Client::try_default().await?;
        tokio::spawn(async move {
            if let Err(error) = run_watches(watch_client, watch_cache).await {
                tracing::error!(%error, "Kubernetes attribution watch stopped");
            }
        });
        let mut observer = Observer::load(
            &args.ebpf_object,
            &config.observation.syscalls,
            architecture,
        )?;
        let mut cgroup_resolver =
            cgroup::CgroupResolver::new("/sys/fs/cgroup").context("index host cgroup hierarchy")?;
        let mut buffer = EventBuffer::new(config.safety.queue_capacity, config.safety.batch_size);
        loop {
            let hello = hello(&config, counters.snapshot());
            let mut session =
                connect_with_backoff(&config.server, credential.trim(), hello).await?;
            for batch in buffer.replay_pending(&counters) {
                send_batch(&session.sender, batch).await?;
            }
            let mut poll = tokio::time::interval(Duration::from_millis(10));
            let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = poll.tick() => {
                        while let Some(kernel) = observer.next_event()? {
                            let pid = kernel.pid_tgid as u32;
                            let tgid = (kernel.pid_tgid >> 32) as u32;
                            let container = match cgroup_resolver.resolve(pid, kernel.cgroup_id) {
                                Ok(container) => Some(container),
                                Err(error) => {
                                    tracing::debug!(pid, cgroup_id = kernel.cgroup_id, %error, "kernel event cgroup resolution failed");
                                    None
                                }
                            };
                            let Some(attribution) = resolve_and_count(&cache, &counters, container.as_deref(), &config.identity.node_name, &config.scope.workloads) else { continue };
                            let command = command(&kernel.command);
                            let payload = if kernel.event_kind == agent_ebpf_common::EVENT_KIND_EXEC {
                                let executable = std::fs::read_link(format!("/proc/{pid}/exe")).map_or_else(|_| command.clone(), |path| path.to_string_lossy().into_owned());
                                EventPayload::ProcessExec(ProcessExec { executable, parent_command: None })
                            } else if kernel.event_kind == agent_ebpf_common::EVENT_KIND_SYSCALL {
                                let Some(name) = syscall::name_for_number(kernel.syscall_id, architecture) else { counters.unsupported.fetch_add(1, Ordering::Relaxed); continue };
                                EventPayload::Syscall(SyscallEvent { name: name.into() })
                            } else { counters.unsupported.fetch_add(1, Ordering::Relaxed); continue };
                            let event = RuntimeEvent { id: Uuid::new_v4(), observed_at: Utc::now(), schema_version: EVENT_SCHEMA_VERSION, attribution,
                                process: ProcessIdentity { cgroup_id: kernel.cgroup_id, pid, tgid, command }, payload };
                            buffer.push(event, &counters);
                        }
                        if let Some(batch) = buffer.next_batch(&counters) { send_batch(&session.sender, batch).await?; }
                    }
                    _ = heartbeat.tick() => {
                        let snapshot = counters.snapshot();
                        tracing::info!(?snapshot, "agent status");
                        session.sender.send(AgentMessage { protocol_version: event_model::PROTOCOL_VERSION, message: Some(agent_message::Message::Heartbeat(Heartbeat { sent_at_unix_nanos: Utc::now().timestamp_nanos_opt().unwrap_or_default(), drop_counters: Some(snapshot.into()) })) }).await?;
                    }
                    message = session.incoming.message() => {
                        match message {
                            Ok(Some(message)) => match message.message {
                                Some(server_message::Message::BatchAcknowledgement(ack)) => { buffer.acknowledge(ack.sequence, &counters); }
                                Some(server_message::Message::Control(control)) => {
                                    let result = handle_control(control);
                                    session.sender.send(AgentMessage { protocol_version: event_model::PROTOCOL_VERSION, message: Some(agent_message::Message::ControlResult(result)) }).await?;
                                }
                                Some(server_message::Message::SessionAccepted(accepted)) => tracing::info!(agent_id=%accepted.agent_id, "agent session accepted"),
                                None => tracing::warn!("unsupported server message"),
                            },
                            Ok(None) | Err(_) => { tracing::warn!("agent session disconnected; reconnecting"); break; }
                        }
                    }
                    _ = tokio::signal::ctrl_c() => return Ok(()),
                }
            }
        }
    }

    fn hello(config: &AgentConfig, snapshot: agent::counters::CounterSnapshot) -> AgentHello {
        let mut capabilities = Vec::new();
        if config.observation.process_exec {
            capabilities.push("process.exec/v1".into());
        }
        capabilities.extend(
            config
                .observation
                .syscalls
                .iter()
                .map(|name| format!("syscall.{name}/v1")),
        );
        AgentHello {
            agent_version: env!("CARGO_PKG_VERSION").into(),
            node_name: config.identity.node_name.clone(),
            architecture: std::env::consts::ARCH.into(),
            kernel_release: kernel_release(),
            capabilities,
            drop_counters: Some(snapshot.into()),
        }
    }

    fn kernel_release() -> String {
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map_or_else(|_| "unknown".into(), |value| value.trim().into())
    }
    fn command(bytes: &[u8; 16]) -> String {
        String::from_utf8_lossy(
            &bytes[..bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(bytes.len())],
        )
        .into_owned()
    }
    async fn send_batch(
        sender: &tokio::sync::mpsc::Sender<AgentMessage>,
        batch: PendingBatch,
    ) -> Result<()> {
        sender
            .send(AgentMessage {
                protocol_version: event_model::PROTOCOL_VERSION,
                message: Some(agent_message::Message::EventBatch(EventBatch {
                    sequence: batch.sequence,
                    events: batch.events.into_iter().map(Into::into).collect(),
                })),
            })
            .await?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    linux::main().await
}
