use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use protocol::v1::agent_service_server::AgentServiceServer;
use server::{
    backfill::{BackfillOptions, run as run_backfill},
    bootstrap::{BootstrapConfig, bootstrap},
    database::{migrate, verify_schema},
    health, metrics,
    notification::NotificationService,
    notification_config::NotificationArgs,
    session::AgentSessionService,
    transport::TransportSecurity,
    web_api::WebApiConfig,
};
use sqlx::postgres::PgPoolOptions;
use tonic::transport::Server;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

async fn wait_for_shutdown(mut shutdown: tokio::sync::watch::Receiver<bool>) {
    while !*shutdown.borrow() {
        if shutdown.changed().await.is_err() {
            break;
        }
    }
}

#[derive(Debug)]
struct ServerOptions {
    grpc_addr: SocketAddr,
    health_addr: SocketAddr,
    development_plaintext: bool,
    tls_certificate: Option<PathBuf>,
    tls_private_key: Option<PathBuf>,
}

async fn serve(
    options: ServerOptions,
    pool: sqlx::PgPool,
    notifications: Option<NotificationService>,
    notification_ready: bool,
    web_api_config: WebApiConfig,
) -> Result<()> {
    let security = if options.development_plaintext {
        TransportSecurity::DevelopmentPlaintext
    } else {
        TransportSecurity::Tls {
            certificate: options
                .tls_certificate
                .context("--tls-certificate is required outside development mode")?,
            private_key: options
                .tls_private_key
                .context("--tls-private-key is required outside development mode")?,
        }
    };
    let mut grpc = Server::builder();
    if let Some(tls) = security.tls_config().await? {
        grpc = grpc.tls_config(tls)?;
    }
    let (shutdown_sender, shutdown_receiver) = tokio::sync::watch::channel(false);
    let signal_sender = shutdown_sender.clone();
    let signal_task = tokio::spawn(async move {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(error=%error, "failed to listen for shutdown signal");
        }
        let _ = signal_sender.send(true);
    });
    let worker_task = notifications
        .as_ref()
        .filter(|service| service.config.enabled)
        .map(|service| {
            tokio::spawn(server::notification::worker::run(
                service.clone(),
                shutdown_receiver.clone(),
            ))
        });
    let grpc_server = grpc
        .add_service(AgentServiceServer::new(AgentSessionService::new(
            pool.clone(),
        )))
        .serve_with_shutdown(
            options.grpc_addr,
            wait_for_shutdown(shutdown_receiver.clone()),
        );
    let listener = tokio::net::TcpListener::bind(options.health_addr).await?;
    let health_server = axum::serve(
        listener,
        health::router(
            pool,
            notification_ready,
            notifications.clone(),
            &web_api_config,
        ),
    )
    .with_graceful_shutdown(wait_for_shutdown(shutdown_receiver));
    tracing::info!(grpc_addr = %options.grpc_addr, health_addr = %options.health_addr, "okoscope server started");
    tokio::try_join!(async { grpc_server.await.context("gRPC server") }, async {
        health_server.await.context("health server")
    })?;
    let _ = shutdown_sender.send(true);
    signal_task.abort();
    if let Some(worker_task) = worker_task {
        metrics::record_notification_drain_started();
        let drain = notifications.map_or(std::time::Duration::from_secs(15), |service| {
            service.config.shutdown_drain
        });
        let drained = tokio::time::timeout(drain, worker_task).await.is_ok();
        metrics::record_notification_drain(drained);
        if drained {
            tracing::info!("notification worker drained");
        } else {
            tracing::warn!("notification worker drain timed out");
        }
    }
    Ok(())
}

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
    cluster_credential: Option<String>,
    #[arg(long, env = "OKOSCOPE_API_CREDENTIAL")]
    api_credential: Option<String>,
    #[command(flatten)]
    notification: NotificationArgs,
    #[arg(long, env = "OKOSCOPE_CORS_ORIGINS", value_delimiter = ',')]
    cors_origins: Vec<String>,
    #[command(subcommand)]
    command: Option<Command>,
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

#[derive(Debug, Subcommand)]
enum Command {
    /// Apply embedded database migrations and exit without starting listeners.
    Migrate,
    /// Validate notification configuration and database readiness without starting listeners.
    NotificationCheck,
    Backfill {
        #[arg(long)]
        organization_id: Uuid,
        #[arg(long)]
        project_id: Uuid,
        #[arg(long, default_value_t = 1)]
        fingerprint_version: i16,
        #[arg(long, default_value_t = 500)]
        batch_size: i64,
        #[arg(long, default_value_t = 0)]
        throttle_ms: u64,
    },
}

async fn check_notifications(
    pool: &sqlx::PgPool,
    config: &server::notification_config::NotificationConfig,
) -> Result<()> {
    verify_schema(pool)
        .await
        .context("database schema readiness")?;
    let enabled_destinations: i64 =
        sqlx::query_scalar("SELECT count(*) FROM webhook_destinations WHERE enabled=true")
            .fetch_one(pool)
            .await
            .context("count enabled webhook destinations")?;
    tracing::info!(
        enabled = config.enabled,
        concurrency = config.concurrency,
        claim_size = config.claim_size,
        poll_ms = config.poll_interval.as_millis(),
        lease_seconds = config.lease_duration.as_secs(),
        timeout_seconds = config.request_timeout.as_secs(),
        max_attempts = config.max_attempts,
        drain_seconds = config.shutdown_drain.as_secs(),
        enabled_destinations,
        "notification configuration check passed"
    );
    Ok(())
}

async fn run_command(command: Option<Command>, pool: &sqlx::PgPool) -> Result<bool> {
    let Some(command) = command else {
        return Ok(false);
    };
    let Command::Backfill {
        organization_id,
        project_id,
        fingerprint_version,
        batch_size,
        throttle_ms,
    } = command
    else {
        return Ok(false);
    };
    let stats = run_backfill(
        pool,
        BackfillOptions {
            organization_id,
            project_id,
            fingerprint_version,
            batch_size,
            throttle: std::time::Duration::from_millis(throttle_ms),
        },
    )
    .await
    .context("backfill runtime event groups")?;
    tracing::info!(?stats, "runtime event backfill complete");
    Ok(true)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    let server_options = ServerOptions {
        grpc_addr: args.grpc_addr,
        health_addr: args.health_addr,
        development_plaintext: args.development_plaintext,
        tls_certificate: args.tls_certificate.clone(),
        tls_private_key: args.tls_private_key.clone(),
    };
    let notification_config = args
        .notification
        .build(args.development_plaintext)
        .map_err(anyhow::Error::msg)
        .context("notification delivery configuration")?;
    let web_api_config = WebApiConfig::new(args.cors_origins.clone())
        .map_err(anyhow::Error::msg)
        .context("web API configuration")?;
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&args.database_url)
        .await
        .context("connect PostgreSQL")?;
    if matches!(args.command, Some(Command::Migrate)) {
        let report = migrate(&pool).await.context("migrate database")?;
        tracing::info!(
            required_migration = report.required,
            applied_migration = report.applied,
            "database migration complete"
        );
        return Ok(());
    }
    if matches!(args.command, Some(Command::NotificationCheck)) {
        check_notifications(&pool, &notification_config).await?;
        return Ok(());
    }
    let notification_ready = true;
    let notifications = NotificationService::new(pool.clone(), notification_config);
    if args.migrate {
        let report = migrate(&pool)
            .await
            .context("apply development startup migrations")?;
        tracing::info!(
            required_migration = report.required,
            applied_migration = report.applied,
            "development startup migration complete"
        );
    }
    verify_schema(&pool)
        .await
        .context("database schema readiness")?;
    if run_command(args.command, &pool).await? {
        return Ok(());
    }
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
            cluster_credential: args
                .cluster_credential
                .context("--cluster-credential is required when serving")?,
            api_credential: args
                .api_credential
                .context("--api-credential is required when serving")?,
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

    serve(
        server_options,
        pool,
        notifications,
        notification_ready,
        web_api_config,
    )
    .await
}
