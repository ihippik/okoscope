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
}
