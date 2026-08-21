use std::{path::Path, time::Duration};

use anyhow::{Context, Result};
use protocol::v1::{
    AgentHello, AgentMessage, ControlResult, ControlStatus, DropCounters, agent_message,
    agent_service_client::AgentServiceClient, control_message, server_message,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{
    Request, Streaming,
    metadata::MetadataValue,
    transport::{Certificate, Channel, ClientTlsConfig, Endpoint},
};

use crate::{config::ServerConfig, counters::CounterSnapshot};

#[derive(Debug)]
pub struct SessionHandle {
    pub sender: mpsc::Sender<AgentMessage>,
    pub incoming: Streaming<protocol::v1::ServerMessage>,
}

pub async fn connect_with_backoff(
    config: &ServerConfig,
    credential: &str,
    hello: AgentHello,
) -> Result<SessionHandle> {
    let mut delay = Duration::from_millis(250);
    loop {
        match open_once(config, credential, hello.clone()).await {
            Ok(session) => return Ok(session),
            Err(error) if delay < Duration::from_secs(30) => {
                tracing::warn!(%error, ?delay, "agent session connection failed; retrying");
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(30));
            }
            Err(error) => return Err(error),
        }
    }
}

async fn open_once(
    config: &ServerConfig,
    credential: &str,
    hello: AgentHello,
) -> Result<SessionHandle> {
    let channel = channel(config).await?;
    let mut client = AgentServiceClient::new(channel);
    let (sender, receiver) = mpsc::channel(32);
    sender
        .send(AgentMessage {
            protocol_version: event_model::PROTOCOL_VERSION,
            message: Some(agent_message::Message::Hello(hello)),
        })
        .await?;
    let mut request = Request::new(ReceiverStream::new(receiver));
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {credential}"))?,
    );
    let incoming = client.open_session(request).await?.into_inner();
    Ok(SessionHandle { sender, incoming })
}

async fn channel(config: &ServerConfig) -> Result<Channel> {
    let mut endpoint = Endpoint::from_shared(config.endpoint.clone())?;
    if config.development_plaintext {
        return endpoint
            .connect()
            .await
            .context("connect plaintext development channel");
    }
    let ca_path = config
        .ca_file
        .as_deref()
        .context("caFile is required for TLS")?;
    let ca = tokio::fs::read(Path::new(ca_path)).await?;
    endpoint =
        endpoint.tls_config(ClientTlsConfig::new().ca_certificate(Certificate::from_pem(ca)))?;
    endpoint.connect().await.context("connect TLS channel")
}

#[must_use]
pub fn handle_control(control: protocol::v1::ControlMessage) -> ControlResult {
    let (status, detail) = match control.command {
        Some(control_message::Command::Ping(_)) => (ControlStatus::Applied, "pong"),
        None => (
            ControlStatus::Unsupported,
            "unsupported or missing typed command",
        ),
    };
    ControlResult {
        request_id: control.request_id,
        status: status.into(),
        detail: detail.into(),
    }
}

#[must_use]
pub fn server_message_kind(message: &protocol::v1::ServerMessage) -> &'static str {
    match message.message {
        Some(server_message::Message::SessionAccepted(_)) => "session_accepted",
        Some(server_message::Message::BatchAcknowledgement(_)) => "batch_acknowledgement",
        Some(server_message::Message::Control(_)) => "control",
        None => "unknown",
    }
}

impl From<CounterSnapshot> for DropCounters {
    fn from(value: CounterSnapshot) -> Self {
        Self {
            filtered: value.filtered,
            unattributed: value.unattributed,
            unsupported: value.unsupported,
            decode_failed: value.decode_failed,
            capacity: value.capacity_dropped,
            kernel_lost: value.kernel_lost,
            connect_correlation_capacity: value.connect_correlation_capacity,
            connect_correlation_miss: value.connect_correlation_miss,
            connect_decode_failed: value.connect_decode_failed,
            connect_unsupported_family: value.connect_unsupported_family,
            connect_kernel_lost: value.connect_kernel_lost,
            dns_packet_decode_failed: value.dns_packet_decode_failed,
            dns_malformed_compression: value.dns_malformed_compression,
            dns_truncated: value.dns_truncated,
            dns_unsupported_record: value.dns_unsupported_record,
            dns_correlation_miss: value.dns_correlation_miss,
            dns_correlation_capacity: value.dns_correlation_capacity,
            dns_tcp_reassembly: value.dns_tcp_reassembly,
            dns_rate_limited: value.dns_rate_limited,
            dns_capacity: value.dns_capacity,
            dns_kernel_lost: value.dns_kernel_lost,
            dns_kernel_unsupported_framing: value.dns_kernel_unsupported_framing,
            dns_attribution_failed: value.dns_attribution_failed,
            dns_oversize: value.dns_oversize,
            inbound_decode_failed: value.inbound_decode_failed,
            inbound_attribution_failed: value.inbound_attribution_failed,
            inbound_unsupported_family: value.inbound_unsupported_family,
            inbound_kernel_lost: value.inbound_kernel_lost,
            inbound_rate_limited: value.inbound_rate_limited,
            inbound_correlation_miss: value.inbound_correlation_miss,
            file_correlation_capacity: value.file_correlation_capacity,
            file_correlation_miss: value.file_correlation_miss,
            file_path_read_failed: value.file_path_read_failed,
            file_path_relative: value.file_path_relative,
            file_path_invalid: value.file_path_invalid,
            file_path_oversize: value.file_path_oversize,
            file_fd_miss: value.file_fd_miss,
            file_filtered: value.file_filtered,
            file_kernel_lost: value.file_kernel_lost,
            file_aggregation_capacity: value.file_aggregation_capacity,
            file_decode_failed: value.file_decode_failed,
            file_attribution_failed: value.file_attribution_failed,
            file_rate_limited: value.file_rate_limited,
            file_unsupported_object: value.file_unsupported_object,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_control_variant_is_unsupported() {
        let result = handle_control(protocol::v1::ControlMessage {
            request_id: "1".into(),
            command: None,
        });
        assert_eq!(result.status, i32::from(ControlStatus::Unsupported));
    }

    #[test]
    fn network_drop_counters_are_transported_additively() {
        let wire = DropCounters::from(CounterSnapshot {
            connect_correlation_capacity: 1,
            connect_correlation_miss: 2,
            connect_decode_failed: 3,
            connect_unsupported_family: 4,
            connect_kernel_lost: 5,
            ..CounterSnapshot::default()
        });
        assert_eq!(wire.connect_correlation_capacity, 1);
        assert_eq!(wire.connect_correlation_miss, 2);
        assert_eq!(wire.connect_decode_failed, 3);
        assert_eq!(wire.connect_unsupported_family, 4);
        assert_eq!(wire.connect_kernel_lost, 5);
    }

    #[test]
    fn dns_drop_counters_are_transported_additively() {
        let wire = DropCounters::from(CounterSnapshot {
            dns_packet_decode_failed: 1,
            dns_malformed_compression: 2,
            dns_truncated: 3,
            dns_unsupported_record: 4,
            dns_correlation_miss: 5,
            dns_correlation_capacity: 6,
            dns_tcp_reassembly: 7,
            dns_rate_limited: 8,
            dns_capacity: 9,
            dns_kernel_lost: 10,
            ..CounterSnapshot::default()
        });
        assert_eq!(wire.dns_packet_decode_failed, 1);
        assert_eq!(wire.dns_malformed_compression, 2);
        assert_eq!(wire.dns_truncated, 3);
        assert_eq!(wire.dns_unsupported_record, 4);
        assert_eq!(wire.dns_correlation_miss, 5);
        assert_eq!(wire.dns_correlation_capacity, 6);
        assert_eq!(wire.dns_tcp_reassembly, 7);
        assert_eq!(wire.dns_rate_limited, 8);
        assert_eq!(wire.dns_capacity, 9);
        assert_eq!(wire.dns_kernel_lost, 10);
    }

    #[test]
    fn inbound_drop_counters_are_transported_additively() {
        let wire = DropCounters::from(CounterSnapshot {
            inbound_decode_failed: 1,
            inbound_attribution_failed: 2,
            inbound_unsupported_family: 3,
            inbound_kernel_lost: 4,
            inbound_rate_limited: 5,
            inbound_correlation_miss: 6,
            ..CounterSnapshot::default()
        });
        assert_eq!(wire.inbound_decode_failed, 1);
        assert_eq!(wire.inbound_attribution_failed, 2);
        assert_eq!(wire.inbound_unsupported_family, 3);
        assert_eq!(wire.inbound_kernel_lost, 4);
        assert_eq!(wire.inbound_rate_limited, 5);
        assert_eq!(wire.inbound_correlation_miss, 6);
    }

    #[test]
    fn file_drop_counters_are_transported_additively() {
        let wire = DropCounters::from(CounterSnapshot {
            file_correlation_capacity: 1,
            file_correlation_miss: 2,
            file_path_read_failed: 3,
            file_path_relative: 4,
            file_path_invalid: 5,
            file_path_oversize: 6,
            file_fd_miss: 7,
            file_filtered: 8,
            file_kernel_lost: 9,
            file_aggregation_capacity: 10,
            file_decode_failed: 11,
            file_attribution_failed: 12,
            file_rate_limited: 13,
            file_unsupported_object: 14,
            ..CounterSnapshot::default()
        });
        assert_eq!(wire.file_correlation_capacity, 1);
        assert_eq!(wire.file_correlation_miss, 2);
        assert_eq!(wire.file_path_read_failed, 3);
        assert_eq!(wire.file_path_relative, 4);
        assert_eq!(wire.file_path_invalid, 5);
        assert_eq!(wire.file_path_oversize, 6);
        assert_eq!(wire.file_fd_miss, 7);
        assert_eq!(wire.file_filtered, 8);
        assert_eq!(wire.file_kernel_lost, 9);
        assert_eq!(wire.file_aggregation_capacity, 10);
        assert_eq!(wire.file_decode_failed, 11);
        assert_eq!(wire.file_attribution_failed, 12);
        assert_eq!(wire.file_rate_limited, 13);
        assert_eq!(wire.file_unsupported_object, 14);
    }
}
