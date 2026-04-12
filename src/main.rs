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
}

pub struct AppState {
    pub tx: broadcast::Sender<SystemMetrics>,
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
    let app_state = Arc::new(AppState { tx: tx.clone() });

    // Detect GPU once at startup, create metrics collector
    let gpu_source = metrics::gpu::detect_source();

    // Spawn the metrics collector on a dedicated OS thread to avoid
    // blocking the async runtime with synchronous sysfs/procfs reads
    std::thread::Builder::new()
        .name("wtop-metrics".into())
        .spawn(move || {
            let mut collector = metrics::MetricsCollector::new(gpu_source);

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
        .fallback(handlers::static_handler)
        .with_state(app_state)
        .layer(axum::middleware::from_fn(handlers::security_headers));

    let addr = format!("{}:{}", bind_addr, args.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {addr}"))?;

    info!(
        "wtop v{} listening on http://{}",
        env!("CARGO_PKG_VERSION"),
        addr
    );
    if !args.localhost_only && bind_addr.to_string() == "0.0.0.0" {
        warn!("Listening on all interfaces. Use --localhost-only for local access only.");
    }

    axum::serve(listener, app).await.context("Server error")?;

    Ok(())
}
