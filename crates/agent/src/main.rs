#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("okoscope-agent requires Linux; userspace libraries and tests support this host");
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        path::PathBuf,
        sync::{Arc, atomic::Ordering},
        time::{Duration, Instant},
    };

    use agent::{
        attribution::{AttributionCache, resolve_and_count, run_watches},
        cgroup,
        config::AgentConfig,
        counters::Counters,
        delivery::{EventBuffer, EventRateLimiter, PendingBatch},
        dns_runtime::DnsProcessor,
        file_runtime::{FileModifyAggregator, translate_rename_scope},
        observer::Observer,
        session::{connect_with_backoff, handle_control},
        syscall::{self, Architecture},
    };
    use agent_ebpf_common::KernelEvent;
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

    #[allow(clippy::too_many_lines)]
    pub async fn main() -> Result<()> {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(EnvFilter::from_default_env())
            .init();
        let args = Args::parse();
        let architecture = Architecture::current().context("unsupported CPU architecture")?;
        let (config, credential) = load_config(&args, architecture).await?;
        let counters = Arc::new(Counters::default());
        let cache = start_attribution_cache().await?;
        let mut observer = load_observer(&args, &config, architecture)?;
        let mut cgroup_resolver =
            cgroup::CgroupResolver::new("/sys/fs/cgroup").context("index host cgroup hierarchy")?;
        let mut buffer = EventBuffer::new(config.safety.queue_capacity, config.safety.batch_size);
        let mut rate_limiter = EventRateLimiter::new(config.safety.max_events_per_second);
        let mut dns_rate_limiter =
            EventRateLimiter::new(config.observation.network.dns.max_events_per_second);
        let mut inbound_rate_limiter =
            EventRateLimiter::new(config.observation.network.max_accepted_events_per_second);
        let mut dns_processor = DnsProcessor::new(&config.observation.network.dns);
        let mut file_aggregator = FileModifyAggregator::default();
        loop {
            let hello = hello(&config, &counters.snapshot());
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
                        while let Some(decoded) = observer.next_file_event() {
                            let Ok(decoded) = decoded else {
                                counters.decode_failed.fetch_add(1, Ordering::Relaxed);
                                counters.file_decode_failed.fetch_add(1, Ordering::Relaxed);
                                counters.file_path_invalid.fetch_add(1, Ordering::Relaxed);
                                continue;
                            };
                            let fd = decoded.kernel.fd;
                            let generation = decoded.kernel.descriptor_generation;
                            let Some(event) = runtime_file_event(
                                decoded, &mut cgroup_resolver, &cache, &counters, &config,
                            ) else { continue };
                            let (ready, dropped) = file_aggregator.observe(event, fd, generation, Instant::now());
                            if dropped {
                                counters.file_aggregation_capacity.fetch_add(1, Ordering::Relaxed);
                            }
                            for event in ready {
                                if rate_limiter.allow() { buffer.push(event, &counters); }
                                else {
                                    counters.capacity_dropped.fetch_add(1, Ordering::Relaxed);
                                    counters.file_rate_limited.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        for event in file_aggregator.drain_expired(Instant::now()) {
                            if rate_limiter.allow() { buffer.push(event, &counters); }
                            else {
                                counters.capacity_dropped.fetch_add(1, Ordering::Relaxed);
                                counters.file_rate_limited.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        while let Some(packet) = observer.next_dns_packet()? {
                            let Some((process, payload)) = dns_processor.process(&packet, &counters) else { continue };
                            let container = cgroup_resolver.resolve(process.pid, process.cgroup_id).ok();
                            let Some(attribution) = resolve_and_count(
                                &cache, &counters, container.as_deref(), &config.identity.node_name,
                                &config.scope.workloads,
                            ) else { continue };
                            let event = RuntimeEvent {
                                id: Uuid::new_v4(), observed_at: Utc::now(),
                                schema_version: EVENT_SCHEMA_VERSION, attribution, process, payload,
                            };
                            if dns_rate_limiter.allow() && rate_limiter.allow() {
                                buffer.push(event, &counters);
                            } else {
                                counters.dns_rate_limited.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        while let Some(kernel) = observer.next_inbound_event()? {
                            let is_accept = kernel.event_kind == agent_ebpf_common::EVENT_KIND_NETWORK_ACCEPT;
                            let Some(event) = runtime_inbound_event(
                                &kernel, &mut cgroup_resolver, &cache, &counters, &config,
                            ) else { continue };
                            if (!is_accept || inbound_rate_limiter.allow()) && rate_limiter.allow() {
                                buffer.push(event, &counters);
                            } else {
                                counters.inbound_rate_limited.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        while let Some(kernel) = observer.next_event()? {
                            let Some(event) = runtime_event(&kernel, architecture, &mut cgroup_resolver, &cache, &counters, &config, &mut dns_processor) else { continue };
                            if rate_limiter.allow() {
                                buffer.push(event, &counters);
                            } else {
                                counters.capacity_dropped.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        if let Some(batch) = buffer.next_batch(&counters) { send_batch(&session.sender, batch).await?; }
                    }
                    _ = heartbeat.tick() => {
                        counters.update_network_kernel(observer.network_counters()?);
                        counters.update_inbound_kernel(observer.inbound_kernel_counters()?);
                        counters.update_dns_kernel(observer.dns_kernel_counters()?);
                        counters.update_file_kernel(observer.file_kernel_counters()?);
                        let snapshot = counters.snapshot();
                        tracing::info!(?snapshot, "agent status");
                        session.sender.send(AgentMessage { protocol_version: event_model::PROTOCOL_VERSION, message: Some(agent_message::Message::Heartbeat(Heartbeat { sent_at_unix_nanos: Utc::now().timestamp_nanos_opt().unwrap_or_default(), drop_counters: Some(snapshot.into()) })) }).await?;
                    }
                    message = session.incoming.message() => {
                        if let Ok(Some(message)) = message {
                            match message.message {
                                Some(server_message::Message::BatchAcknowledgement(ack)) => { buffer.acknowledge(ack.sequence, &counters); }
                                Some(server_message::Message::Control(control)) => {
                                    let result = handle_control(control);
                                    session.sender.send(AgentMessage { protocol_version: event_model::PROTOCOL_VERSION, message: Some(agent_message::Message::ControlResult(result)) }).await?;
                                }
                                Some(server_message::Message::SessionAccepted(accepted)) => tracing::info!(agent_id=%accepted.agent_id, "agent session accepted"),
                                None => tracing::warn!("unsupported server message"),
                            }
                        } else {
                            tracing::warn!("agent session disconnected; reconnecting");
                            break;
                        }
                    }
                    _ = tokio::signal::ctrl_c() => {
                        for event in file_aggregator.drain_all() {
                            if rate_limiter.allow() { buffer.push(event, &counters); }
                            else {
                                counters.capacity_dropped.fetch_add(1, Ordering::Relaxed);
                                counters.file_rate_limited.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        while let Some(batch) = buffer.next_batch(&counters) {
                            send_batch(&session.sender, batch).await?;
                        }
                        return Ok(());
                    },
                }
            }
        }
    }

    async fn start_attribution_cache() -> Result<Arc<AttributionCache>> {
        let cache = Arc::new(AttributionCache::new(Duration::from_secs(30)));
        let watch_cache = cache.clone();
        let watch_client = kube::Client::try_default().await?;
        tokio::spawn(async move {
            if let Err(error) = run_watches(watch_client, watch_cache).await {
                tracing::error!(%error, "Kubernetes attribution watch stopped");
            }
        });
        Ok(cache)
    }

    fn load_observer(
        args: &Args,
        config: &AgentConfig,
        architecture: Architecture,
    ) -> Result<Observer> {
        Observer::load(
            &args.ebpf_object,
            &config.observation.syscalls,
            agent::observer::ObservationPrograms {
                network_connect: config.observation.network.connect.into(),
                network_listen: config.observation.network.listen.into(),
                network_accept: config.observation.network.accept.into(),
                dns: config.observation.network.dns.enabled.into(),
                files: config.observation.files.enabled.into(),
            },
            architecture,
        )
    }

    async fn load_config(args: &Args, architecture: Architecture) -> Result<(AgentConfig, String)> {
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
        Ok((config, credential))
    }

    fn runtime_event(
        kernel: &KernelEvent,
        architecture: Architecture,
        cgroup_resolver: &mut cgroup::CgroupResolver,
        cache: &AttributionCache,
        counters: &Counters,
        config: &AgentConfig,
        dns_processor: &mut DnsProcessor,
    ) -> Option<RuntimeEvent> {
        let pid = u32::try_from(kernel.pid_tgid & u64::from(u32::MAX))
            .expect("PID is encoded in the low 32 bits");
        let tgid =
            u32::try_from(kernel.pid_tgid >> 32).expect("TGID is encoded in the high 32 bits");
        let container = match cgroup_resolver.resolve(pid, kernel.cgroup_id) {
            Ok(container) => Some(container),
            Err(error) => {
                tracing::debug!(pid, cgroup_id = kernel.cgroup_id, %error, "kernel event cgroup resolution failed");
                None
            }
        };
        let attribution = resolve_and_count(
            cache,
            counters,
            container.as_deref(),
            &config.identity.node_name,
            &config.scope.workloads,
        )?;
        let command = command(&kernel.command);
        let mut payload = if kernel.event_kind == agent_ebpf_common::EVENT_KIND_EXEC {
            let executable = std::fs::read_link(format!("/proc/{pid}/exe")).map_or_else(
                |_| command.clone(),
                |path| path.to_string_lossy().into_owned(),
            );
            EventPayload::ProcessExec(ProcessExec {
                executable,
                parent_command: None,
            })
        } else if kernel.event_kind == agent_ebpf_common::EVENT_KIND_SYSCALL {
            let Some(name) = syscall::name_for_number(kernel.syscall_id, architecture) else {
                counters.unsupported.fetch_add(1, Ordering::Relaxed);
                return None;
            };
            EventPayload::Syscall(SyscallEvent { name: name.into() })
        } else if kernel.event_kind == agent_ebpf_common::EVENT_KIND_NETWORK_CONNECT {
            let Ok(network) = agent::kernel_event::network_connect(kernel) else {
                counters
                    .connect_decode_failed
                    .fetch_add(1, Ordering::Relaxed);
                return None;
            };
            EventPayload::NetworkConnect(network)
        } else {
            counters.unsupported.fetch_add(1, Ordering::Relaxed);
            return None;
        };
        dns_processor.attach_context(kernel.cgroup_id, &mut payload);
        Some(RuntimeEvent {
            id: Uuid::new_v4(),
            observed_at: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION,
            attribution,
            process: ProcessIdentity {
                cgroup_id: kernel.cgroup_id,
                pid,
                tgid,
                command,
            },
            payload,
        })
    }

    fn runtime_inbound_event(
        kernel: &agent_ebpf_common::InboundKernelEvent,
        cgroup_resolver: &mut cgroup::CgroupResolver,
        cache: &AttributionCache,
        counters: &Counters,
        config: &AgentConfig,
    ) -> Option<RuntimeEvent> {
        let pid = u32::try_from(kernel.pid_tgid & u64::from(u32::MAX))
            .expect("PID is encoded in the low 32 bits");
        let tgid =
            u32::try_from(kernel.pid_tgid >> 32).expect("TGID is encoded in the high 32 bits");
        let container = cgroup_resolver.resolve(pid, kernel.cgroup_id).ok();
        let attribution = resolve_and_count(
            cache,
            counters,
            container.as_deref(),
            &config.identity.node_name,
            &config.scope.workloads,
        )?;
        let Ok(payload) = agent::kernel_event::inbound_payload(kernel) else {
            counters
                .inbound_decode_failed
                .fetch_add(1, Ordering::Relaxed);
            return None;
        };
        Some(RuntimeEvent {
            id: Uuid::new_v4(),
            observed_at: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION,
            attribution,
            process: ProcessIdentity {
                cgroup_id: kernel.cgroup_id,
                pid,
                tgid,
                command: command(&kernel.command),
            },
            payload,
        })
    }

    fn runtime_file_event(
        decoded: agent::kernel_event::DecodedFileEvent,
        cgroup_resolver: &mut cgroup::CgroupResolver,
        cache: &AttributionCache,
        counters: &Counters,
        config: &AgentConfig,
    ) -> Option<RuntimeEvent> {
        let kernel = decoded.kernel;
        let pid = u32::try_from(kernel.pid_tgid & u64::from(u32::MAX)).ok()?;
        let tgid = u32::try_from(kernel.pid_tgid >> 32).ok()?;
        let container = cgroup_resolver.resolve(pid, kernel.cgroup_id).ok();
        let attribution = resolve_and_count(
            cache,
            counters,
            container.as_deref(),
            &config.identity.node_name,
            &config.scope.workloads,
        );
        let Some(attribution) = attribution else {
            counters
                .file_attribution_failed
                .fetch_add(1, Ordering::Relaxed);
            return None;
        };
        let payload = match decoded.payload {
            EventPayload::FileCreate(value) if config.observation.files.observes(&value.path) => {
                EventPayload::FileCreate(value)
            }
            EventPayload::FileModify(value) if config.observation.files.observes(&value.path) => {
                EventPayload::FileModify(value)
            }
            EventPayload::FileDelete(value) if config.observation.files.observes(&value.path) => {
                EventPayload::FileDelete(value)
            }
            EventPayload::FileRename(value) => {
                let old_observed = config.observation.files.observes(&value.old_path);
                let new_observed = config.observation.files.observes(&value.new_path);
                let Some(payload) = translate_rename_scope(value, old_observed, new_observed)
                else {
                    counters.filtered.fetch_add(1, Ordering::Relaxed);
                    counters.file_filtered.fetch_add(1, Ordering::Relaxed);
                    return None;
                };
                payload
            }
            EventPayload::FileCreate(_)
            | EventPayload::FileModify(_)
            | EventPayload::FileDelete(_) => {
                counters.filtered.fetch_add(1, Ordering::Relaxed);
                counters.file_filtered.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            _ => return None,
        };
        Some(RuntimeEvent {
            id: Uuid::new_v4(),
            observed_at: Utc::now(),
            schema_version: EVENT_SCHEMA_VERSION,
            attribution,
            process: ProcessIdentity {
                cgroup_id: kernel.cgroup_id,
                pid,
                tgid,
                command: command(&kernel.command),
            },
            payload,
        })
    }

    fn hello(config: &AgentConfig, snapshot: &agent::counters::CounterSnapshot) -> AgentHello {
        AgentHello {
            agent_version: env!("CARGO_PKG_VERSION").into(),
            node_name: config.identity.node_name.clone(),
            architecture: std::env::consts::ARCH.into(),
            kernel_release: kernel_release(),
            capabilities: config.observation.capabilities(),
            drop_counters: Some((*snapshot).into()),
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
