mod handlers;
mod metrics;
mod models;

use anyhow::{Context, Result};
use axum::{Router, routing::get};
use clap::Parser;
use models::SystemMetrics;
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Broadcast channel capacity for SSE subscribers.
const BROADCAST_CAPACITY: usize = 16;

/// Interval between metric collections in milliseconds.
const METRICS_INTERVAL_MS: u64 = 1500;

#[derive(Parser, Debug)]
#[command(name = "wtop", about = "Web-based system monitor", version)]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value_t = 3000)]
    port: u16,

    /// Address to bind to
    #[arg(short, long, default_value = "0.0.0.0")]
    bind: IpAddr,

    /// Only listen on localhost (overrides --bind)
    #[arg(long)]
    localhost_only: bool,

    /// Require Bearer token for all requests (optional)
    #[arg(long, env = "WTOP_AUTH_TOKEN")]
    auth_token: Option<String>,

    /// Path to TLS certificate file (enables HTTPS, requires --tls-key)
    #[cfg(feature = "tls")]
    #[arg(long)]
    tls_cert: Option<String>,

    /// Path to TLS private key file (enables HTTPS, requires --tls-cert)
    #[cfg(feature = "tls")]
    #[arg(long)]
    tls_key: Option<String>,
}

pub struct AppState {
    pub tx: broadcast::Sender<SystemMetrics>,
    pub auth_token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let bind_addr = if args.localhost_only {
        "127.0.0.1".parse().unwrap()
    } else {
        args.bind
    };

    let (tx, _) = broadcast::channel::<SystemMetrics>(BROADCAST_CAPACITY);
    if args.auth_token.is_some() {
        info!("Authentication enabled (Bearer token required)");
    }
    let app_state = Arc::new(AppState {
        tx: tx.clone(),
        auth_token: args.auth_token,
    });

    // Detect GPU once at startup, create metrics collector
    let gpu_sources = metrics::gpu::detect_sources();

    // Spawn the metrics collector on a dedicated OS thread to avoid
    // blocking the async runtime with synchronous sysfs/procfs reads
    std::thread::Builder::new()
        .name("wtop-metrics".into())
        .spawn(move || {
            let mut collector = metrics::MetricsCollector::new(gpu_sources);

            loop {
                std::thread::sleep(Duration::from_millis(METRICS_INTERVAL_MS));

                let collected = collector.collect();

                if let Err(e) = tx.send(collected) {
                    debug!("No SSE subscribers connected: {}", e);
                }
            }
        })
        .context("Failed to spawn metrics collector thread")?;

    let app = Router::new()
        .route("/events", get(handlers::sse_handler))
        .route("/version", get(handlers::version_handler))
        .route("/api/metrics", get(handlers::api_metrics_handler))
        .route("/api/export", get(handlers::api_export_handler))
        .fallback(handlers::static_handler)
        .with_state(app_state.clone())
        .layer(axum::middleware::from_fn_with_state(
            app_state,
            handlers::security_and_auth,
        ));

    let addr = format!("{}:{}", bind_addr, args.port);

    if !args.localhost_only && bind_addr.to_string() == "0.0.0.0" {
        warn!("Listening on all interfaces. Use --localhost-only for local access only.");
    }

    #[cfg(feature = "tls")]
    if let (Some(cert), Some(key)) = (args.tls_cert, args.tls_key) {
        let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert, &key)
            .await
            .with_context(|| format!("Failed to load TLS cert={cert} key={key}"))?;

        info!(
            "wtop v{} listening on https://{}",
            env!("CARGO_PKG_VERSION"),
            addr
        );

        axum_server::bind_rustls(addr.parse().context("Invalid address")?, tls_config)
            .serve(app.into_make_service())
            .await
            .context("TLS server error")?;

        return Ok(());
    }

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {addr}"))?;

    info!(
        "wtop v{} listening on http://{}",
        env!("CARGO_PKG_VERSION"),
        addr
    );

    axum::serve(listener, app).await.context("Server error")?;

    Ok(())
}
