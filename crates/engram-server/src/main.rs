//! Engram server binary entry point.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use engram_server::mcp;
use engram_server::routes;
use engram_server::state::AppState;

/// Engram memory server
#[derive(Parser, Debug)]
#[command(name = "engram-server", version, about)]
struct Args {
    /// Server mode
    #[arg(long, default_value = "rest")]
    mode: Mode,

    /// Host to bind to (overrides config when set explicitly)
    #[arg(long)]
    host: Option<String>,

    /// Port to listen on (overrides config when set explicitly)
    #[arg(long)]
    port: Option<u16>,

    /// Path to engram config TOML
    #[arg(long, default_value = "config/engram.toml")]
    config: PathBuf,

    /// Require auth; fails at startup if ENGRAM_API_TOKENS is not set
    #[arg(long)]
    require_auth: bool,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum Mode {
    /// HTTP REST server
    Rest,
    /// MCP JSON-RPC over stdio
    Mcp,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    let config = if args.config.exists() {
        engram_core::Config::load(&args.config)
    } else {
        tracing::warn!(
            "Config file {} not found, using defaults",
            args.config.display()
        );
        engram_core::Config::default()
    };

    match args.mode {
        Mode::Rest => {
            let state = AppState::from_config(&config, args.require_auth).await?;
            // CLI flags override config; fall back to config values
            let host = args
                .host
                .unwrap_or_else(|| config.server.host.clone());
            let port = args.port.unwrap_or(config.server.port);
            serve_rest(state, &host, port).await
        }
        Mode::Mcp => {
            mcp::serve_mcp(&config).await
        }
    }
}

async fn serve_rest(state: AppState, host: &str, port: u16) -> anyhow::Result<()> {
    let router = routes::build_router(state);

    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    tracing::info!("Engram REST server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("Server shut down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received, draining connections...");
}
