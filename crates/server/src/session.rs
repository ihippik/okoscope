use std::pin::Pin;

use futures::{Stream, StreamExt};
use protocol::{
    v1::{
        AgentHello, AgentMessage, BatchAcknowledgement, ServerMessage, SessionAccepted,
        agent_message, agent_service_server::AgentService, server_message,
    },
    validate_protocol,
};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming, metadata::MetadataMap};
use uuid::Uuid;

use crate::{
    auth::{CredentialAuthenticator, SessionScope},
    ingestion::{IngestionContext, persist_batch},
};

#[derive(Clone, Debug)]
pub struct AgentSessionService {
    pool: PgPool,
    authenticator: CredentialAuthenticator,
}

impl AgentSessionService {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            authenticator: CredentialAuthenticator::new(pool.clone()),
            pool,
        }
    }
}

type ResponseStream = Pin<Box<dyn Stream<Item = Result<ServerMessage, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl AgentService for AgentSessionService {
    type OpenSessionStream = ResponseStream;

    async fn open_session(
        &self,
        request: Request<Streaming<AgentMessage>>,
    ) -> Result<Response<Self::OpenSessionStream>, Status> {
        let credential = bearer(request.metadata())?;
        let scope = self
            .authenticator
            .authenticate(&credential)
            .await
            .map_err(internal)?
            .ok_or_else(|| Status::unauthenticated("invalid or revoked cluster credential"))?;
        let mut incoming = request.into_inner();
        let first = incoming
            .message()
            .await?
            .ok_or_else(|| Status::invalid_argument("hello message is required"))?;
        validate_protocol(first.protocol_version)
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        let hello = match first.message {
            Some(agent_message::Message::Hello(hello)) => hello,
            _ => return Err(Status::invalid_argument("first message must be hello")),
        };
        let (agent_id, _session_id) = register(&self.pool, scope, &hello)
            .await
            .map_err(internal)?;
        let file_activity_capable = hello
            .capabilities
            .iter()
            .any(|value| value == protocol::FILE_ACTIVITY_CAPABILITY);
        let (sender, receiver) = mpsc::channel(32);
        sender
            .send(Ok(ServerMessage {
                protocol_version: event_model::PROTOCOL_VERSION,
                message: Some(server_message::Message::SessionAccepted(SessionAccepted {
                    organization_id: scope.organization_id.to_string(),
                    cluster_id: scope.cluster_id.to_string(),
                    agent_id: agent_id.to_string(),
                    negotiated_protocol_version: event_model::PROTOCOL_VERSION,
                })),
            }))
            .await
            .map_err(|_| Status::unavailable("session response channel closed"))?;
        let pool = self.pool.clone();
        tokio::spawn(async move {
            while let Some(next) = incoming.next().await {
                let result = async {
                    let message = next?;
                    validate_protocol(message.protocol_version).map_err(|error| Status::failed_precondition(error.to_string()))?;
                    match message.message {
                        Some(agent_message::Message::EventBatch(batch)) => {
                            let events = batch.events.into_iter().map(event_model::RuntimeEvent::try_from).collect::<Result<Vec<_>, _>>().map_err(|error| Status::invalid_argument(error.to_string()))?;
                            if !file_activity_capable && events.iter().any(|event| matches!(event.payload,
                                event_model::EventPayload::FileCreate(_)
                                | event_model::EventPayload::FileModify(_)
                                | event_model::EventPayload::FileDelete(_)
                                | event_model::EventPayload::FileRename(_))) {
                                return Err(Status::failed_precondition("file activity event requires file.activity.syscall-path/v1 capability"));
                            }
                            let accepted = persist_batch(&pool, IngestionContext { scope, agent_id }, &events).await.map_err(internal)?;
                            sender.send(Ok(ServerMessage { protocol_version: event_model::PROTOCOL_VERSION, message: Some(server_message::Message::BatchAcknowledgement(BatchAcknowledgement { sequence: batch.sequence, accepted_events: accepted })) })).await.map_err(|_| Status::unavailable("session response channel closed"))?;
                        }
                        Some(agent_message::Message::Heartbeat(_)) => { touch_agent(&pool, agent_id).await.map_err(internal)?; }
                        Some(agent_message::Message::ControlResult(result)) => { tracing::info!(agent_id=%agent_id, request_id=%result.request_id, status=result.status, "agent control result"); }
                        Some(agent_message::Message::Hello(_)) => return Err(Status::invalid_argument("hello can only be sent once")),
                        None => return Err(Status::invalid_argument("unknown or missing typed agent message")),
                    }
                    Ok::<(), Status>(())
                }.await;
                if let Err(status) = result {
                    let _ = sender.send(Err(status)).await;
                    break;
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }
}

fn bearer(metadata: &MetadataMap) -> Result<String, Status> {
    let value = metadata
        .get("authorization")
        .ok_or_else(|| Status::unauthenticated("authorization metadata is required"))?
        .to_str()
        .map_err(|_| Status::unauthenticated("authorization metadata is invalid"))?;
    value
        .strip_prefix("Bearer ")
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| Status::unauthenticated("Bearer credential is required"))
}

async fn register(
    pool: &PgPool,
    scope: SessionScope,
    hello: &AgentHello,
) -> Result<(Uuid, Uuid), sqlx::Error> {
    let architecture = platform_value(&hello.architecture, 64);
    let kernel_release = platform_value(&hello.kernel_release, 255);
    let agent_id: Uuid = sqlx::query_scalar("INSERT INTO agents (id, organization_id, cluster_id, node_name, agent_version, architecture, kernel_release, capabilities) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (cluster_id, node_name) DO UPDATE SET agent_version=EXCLUDED.agent_version, architecture=EXCLUDED.architecture, kernel_release=EXCLUDED.kernel_release, capabilities=EXCLUDED.capabilities, last_seen_at=now() RETURNING id")
        .bind(Uuid::new_v4()).bind(scope.organization_id).bind(scope.cluster_id).bind(&hello.node_name).bind(&hello.agent_version).bind(architecture).bind(kernel_release).bind(serde_json::json!(hello.capabilities)).fetch_one(pool).await?;
    let session_id = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_sessions (id, organization_id, cluster_id, agent_id, protocol_version) VALUES ($1,$2,$3,$4,$5)")
        .bind(session_id).bind(scope.organization_id).bind(scope.cluster_id).bind(agent_id).bind(i32::try_from(event_model::PROTOCOL_VERSION).unwrap_or(i32::MAX)).execute(pool).await?;
    Ok((agent_id, session_id))
}

fn platform_value(value: &str, max_len: usize) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()
        && !value.eq_ignore_ascii_case("unknown")
        && value.chars().count() <= max_len)
        .then_some(value)
}

async fn touch_agent(pool: &PgPool, agent_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE agents SET last_seen_at=now() WHERE id=$1")
        .bind(agent_id)
        .execute(pool)
        .await?;
    Ok(())
}

fn internal(error: impl std::fmt::Display) -> Status {
    tracing::error!(%error, "agent session failure");
    Status::internal("internal server error")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::{BootstrapConfig, bootstrap};

    fn config(name: &str) -> BootstrapConfig {
        BootstrapConfig {
            organization_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            cluster_id: Uuid::new_v4(),
            application_id: Uuid::new_v4(),
            organization_slug: name.into(),
            organization_name: name.into(),
            project_slug: "project".into(),
            project_name: "Project".into(),
            cluster_external_id: "cluster".into(),
            cluster_name: "Cluster".into(),
            application_slug: "app".into(),
            application_name: "Application".into(),
            cluster_credential: format!("cluster-{name}"),
            api_credential: format!("api-{name}"),
        }
    }

    fn hello(node_name: &str, architecture: &str, kernel_release: &str) -> AgentHello {
        AgentHello {
            agent_version: "test".into(),
            node_name: node_name.into(),
            architecture: architecture.into(),
            kernel_release: kernel_release.into(),
            capabilities: vec!["process.exec/v1".into()],
            drop_counters: None,
        }
    }

    #[test]
    fn normalizes_platform_values() {
        assert_eq!(platform_value(" x86_64 ", 64), Some("x86_64"));
        assert_eq!(platform_value("", 64), None);
        assert_eq!(platform_value(" UNKNOWN ", 64), None);
        assert_eq!(platform_value(&"x".repeat(65), 64), None);
    }

    #[sqlx::test(migrator = "crate::database::MIGRATOR")]
    #[ignore = "requires a PostgreSQL server with DATABASE_URL"]
    async fn registration_persists_normalizes_and_refreshes_platform(pool: PgPool) {
        let values = config("agent-platform");
        let ids = bootstrap(&pool, &values).await.unwrap();
        let scope = SessionScope {
            organization_id: ids.organization_id,
            cluster_id: ids.cluster_id,
        };

        let (agent_id, _) = register(&pool, scope, &hello("node-a", " x86_64 ", "6.8.1"))
            .await
            .unwrap();
        let stored: (Option<String>, Option<String>) =
            sqlx::query_as("SELECT architecture,kernel_release FROM agents WHERE id=$1")
                .bind(agent_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, (Some("x86_64".into()), Some("6.8.1".into())));

        let (same_agent_id, _) = register(&pool, scope, &hello("node-a", "unknown", "6.9.2"))
            .await
            .unwrap();
        assert_eq!(same_agent_id, agent_id);
        let refreshed: (Option<String>, Option<String>) =
            sqlx::query_as("SELECT architecture,kernel_release FROM agents WHERE id=$1")
                .bind(agent_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(refreshed, (None, Some("6.9.2".into())));
    }
}
