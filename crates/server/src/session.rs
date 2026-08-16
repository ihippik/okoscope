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
    let agent_id: Uuid = sqlx::query_scalar("INSERT INTO agents (id, organization_id, cluster_id, node_name, agent_version, capabilities) VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (cluster_id, node_name) DO UPDATE SET agent_version=EXCLUDED.agent_version, capabilities=EXCLUDED.capabilities, last_seen_at=now() RETURNING id")
        .bind(Uuid::new_v4()).bind(scope.organization_id).bind(scope.cluster_id).bind(&hello.node_name).bind(&hello.agent_version).bind(serde_json::json!(hello.capabilities)).fetch_one(pool).await?;
    let session_id = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_sessions (id, organization_id, cluster_id, agent_id, protocol_version) VALUES ($1,$2,$3,$4,$5)")
        .bind(session_id).bind(scope.organization_id).bind(scope.cluster_id).bind(agent_id).bind(i32::try_from(event_model::PROTOCOL_VERSION).unwrap_or(i32::MAX)).execute(pool).await?;
    Ok((agent_id, session_id))
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
