use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use protocol::v1::agent_service_server::AgentServiceServer;
use server::{
    bootstrap::{BootstrapConfig, bootstrap},
    database::{MIGRATOR, verify_schema},
    health,
    session::AgentSessionService,
    transport::TransportSecurity,
};
use sqlx::postgres::PgPoolOptions;
use tonic::transport::Server;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, env = "OKOSCOPE_DATABASE_URL")]
    database_url: String,
    #[arg(long, env = "OKOSCOPE_GRPC_ADDR", default_value = "0.0.0.0:4317")]
    grpc_addr: SocketAddr,
    #[arg(long, env = "OKOSCOPE_HEALTH_ADDR", default_value = "0.0.0.0:8080")]
    health_addr: SocketAddr,
    #[arg(long, env = "OKOSCOPE_MIGRATE", default_value_t = false)]
    migrate: bool,
    #[arg(long, env = "OKOSCOPE_DEVELOPMENT_PLAINTEXT", default_value_t = false)]
    development_plaintext: bool,
    #[arg(long, env = "OKOSCOPE_TLS_CERTIFICATE")]
    tls_certificate: Option<PathBuf>,
    #[arg(long, env = "OKOSCOPE_TLS_PRIVATE_KEY")]
    tls_private_key: Option<PathBuf>,
    #[arg(long, env = "OKOSCOPE_ORGANIZATION_SLUG", default_value = "local")]
    organization_slug: String,
    #[arg(long, env = "OKOSCOPE_ORGANIZATION_NAME", default_value = "Local")]
    organization_name: String,
    #[arg(long, env = "OKOSCOPE_PROJECT_SLUG", default_value = "demo")]
    project_slug: String,
    #[arg(long, env = "OKOSCOPE_PROJECT_NAME", default_value = "Demo")]
    project_name: String,
    #[arg(long, env = "OKOSCOPE_CLUSTER_EXTERNAL_ID", default_value = "local")]
    cluster_external_id: String,
    #[arg(long, env = "OKOSCOPE_CLUSTER_NAME", default_value = "Local")]
    cluster_name: String,
    #[arg(long, env = "OKOSCOPE_APPLICATION_SLUG", default_value = "payment-api")]
    application_slug: String,
    #[arg(long, env = "OKOSCOPE_APPLICATION_NAME", default_value = "Payment API")]
    application_name: String,
    #[arg(long, env = "OKOSCOPE_CLUSTER_CREDENTIAL")]
    cluster_credential: String,
    #[arg(
        long,
        env = "OKOSCOPE_ORGANIZATION_ID",
        default_value = "018f4f9c-3f9a-7de1-8000-000000000000"
    )]
    organization_id: Uuid,
    #[arg(
        long,
        env = "OKOSCOPE_PROJECT_ID",
        default_value = "018f4f9c-3f9a-7de1-8000-000000000001"
    )]
    project_id: Uuid,
    #[arg(
        long,
        env = "OKOSCOPE_CLUSTER_ID",
        default_value = "018f4f9c-3f9a-7de1-8000-000000000003"
    )]
    cluster_id: Uuid,
    #[arg(
        long,
        env = "OKOSCOPE_APPLICATION_ID",
        default_value = "018f4f9c-3f9a-7de1-8000-000000000002"
    )]
    application_id: Uuid,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&args.database_url)
        .await
        .context("connect PostgreSQL")?;
    if args.migrate {
        MIGRATOR
            .run(&pool)
            .await
            .context("apply database migrations")?;
    }
    verify_schema(&pool)
        .await
        .context("database schema readiness")?;
    let ids = bootstrap(
        &pool,
        &BootstrapConfig {
            organization_id: args.organization_id,
            project_id: args.project_id,
            cluster_id: args.cluster_id,
            application_id: args.application_id,
            organization_slug: args.organization_slug,
            organization_name: args.organization_name,
            project_slug: args.project_slug,
            project_name: args.project_name,
            cluster_external_id: args.cluster_external_id,
            cluster_name: args.cluster_name,
            application_slug: args.application_slug,
            application_name: args.application_name,
            cluster_credential: args.cluster_credential,
        },
    )
    .await
    .context("bootstrap tenant identities")?;
    tracing::info!(
        organization_id = %ids.organization_id,
        project_id = %ids.project_id,
        cluster_id = %ids.cluster_id,
        application_id = %ids.application_id,
        "bootstrap identities ready"
    );

    let security = if args.development_plaintext {
        TransportSecurity::DevelopmentPlaintext
    } else {
        TransportSecurity::Tls {
            certificate: args
                .tls_certificate
                .context("--tls-certificate is required outside development mode")?,
            private_key: args
                .tls_private_key
                .context("--tls-private-key is required outside development mode")?,
        }
    };
    let mut grpc = Server::builder();
    if let Some(tls) = security.tls_config().await? {
        grpc = grpc.tls_config(tls)?;
    }
    let grpc_server = grpc
        .add_service(AgentServiceServer::new(AgentSessionService::new(
            pool.clone(),
        )))
        .serve(args.grpc_addr);
    let listener = tokio::net::TcpListener::bind(args.health_addr).await?;
    let health_server = axum::serve(listener, health::router(pool));
    tracing::info!(grpc_addr = %args.grpc_addr, health_addr = %args.health_addr, "okoscope server started");
    tokio::try_join!(async { grpc_server.await.context("gRPC server") }, async {
        health_server.await.context("health server")
    },)?;
    Ok(())
}
