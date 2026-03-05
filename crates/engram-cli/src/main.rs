use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use engram_core::config::Config;
use engram_core::storage::QdrantStorage;

#[derive(Parser)]
#[command(name = "engram")]
#[command(version)]
#[command(about = "Rust-native AI agent memory system", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to configuration file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize the memory system (create directories, collections)
    Init {
        /// Path to data directory
        #[arg(short, long)]
        data_dir: Option<String>,

        /// Force re-initialization
        #[arg(short, long)]
        force: bool,
    },

    /// Show system status and health
    Status {
        /// Output format (text, json)
        #[arg(short = 'o', long, default_value = "text")]
        format: String,
    },

    /// View or update configuration
    Config {
        /// Configuration key to get/set (e.g., "server.port", "extraction.mode")
        key: Option<String>,

        /// Value to set
        value: Option<String>,

        /// List all configuration values
        #[arg(short, long)]
        list: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = match cli.verbose {
        0 => "engram=info,engram_cli=info",
        1 => "engram=debug,engram_cli=debug",
        _ => "engram=trace,engram_cli=trace",
    };

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| filter.into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let mut config = Config::load(&cli.config);

    match cli.command {
        Commands::Init { data_dir, force } => {
            cmd_init(&mut config, &cli.config, data_dir, force).await
        }
        Commands::Status { format } => {
            cmd_status(&config, &format).await
        }
        Commands::Config { key, value, list } => {
            cmd_config(&mut config, &cli.config, key, value, list)
        }
    }
}

async fn cmd_init(
    config: &mut Config,
    config_path: &Path,
    data_dir: Option<String>,
    force: bool,
) -> Result<()> {
    // Update data_dir if provided
    if let Some(dir) = data_dir {
        config.data_dir = dir.clone();
        config.qdrant.path = Some(format!("{}/qdrant", dir));
    }

    let data_path = PathBuf::from(&config.data_dir);

    // Check if already initialized
    if data_path.exists() && !force {
        println!("\x1b[33m!\x1b[0m Data directory already exists: {}", config.data_dir);
        println!("  Use --force to re-initialize");
        return Ok(());
    }

    println!("\x1b[34m>\x1b[0m Initializing engram in {}", config.data_dir);

    // Create data directory
    std::fs::create_dir_all(&data_path)
        .context("Failed to create data directory")?;
    println!("  \x1b[32m✓\x1b[0m Created data directory");

    // Initialize Qdrant
    println!("  \x1b[34m>\x1b[0m Initializing Qdrant collections...");
    let qdrant = QdrantStorage::new(config.qdrant.clone())
        .await
        .context("Failed to connect to Qdrant")?;
    qdrant.initialize().await
        .context("Failed to initialize Qdrant collections")?;
    println!("  \x1b[32m✓\x1b[0m Created 4 collections (world, experience, opinion, observation)");

    // Save configuration
    config.save(config_path)
        .context("Failed to save configuration")?;
    println!("  \x1b[32m✓\x1b[0m Saved configuration to {}", config_path.display());

    println!("\n\x1b[32m✓\x1b[0m Initialization complete!");
    println!("\nNext steps:");
    println!("  1. Check status:  engram status");

    Ok(())
}

async fn cmd_status(config: &Config, format: &str) -> Result<()> {
    let mut status = SystemStatus::default();

    // Check Qdrant
    match QdrantStorage::new(config.qdrant.clone()).await {
        Ok(qdrant) => {
            status.qdrant_connected = true;
            if let Ok(counts) = qdrant.get_collection_counts().await {
                status.total_memories = counts.iter().map(|(_, c)| c).sum();
                status.collection_counts = counts;
            }
        }
        Err(e) => {
            status.qdrant_error = Some(e.to_string());
        }
    }

    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        _ => {
            print_status_text(&status);
        }
    }

    Ok(())
}

fn print_status_text(status: &SystemStatus) {
    println!("\n\x1b[1mEngram Status\x1b[0m\n");

    // Qdrant status
    if status.qdrant_connected {
        println!("\x1b[32m●\x1b[0m Qdrant: Connected");
        println!("  Collections:");
        for (name, count) in &status.collection_counts {
            println!("    - {}: {} memories", name, count);
        }
        println!("  Total: {} memories", status.total_memories);
    } else {
        println!("\x1b[31m●\x1b[0m Qdrant: Disconnected");
        if let Some(err) = &status.qdrant_error {
            println!("  Error: {}", err);
        }
    }

    println!();
}

fn cmd_config(
    config: &mut Config,
    config_path: &Path,
    key: Option<String>,
    value: Option<String>,
    list: bool,
) -> Result<()> {
    if list {
        // List all config values
        println!("\n\x1b[1mConfiguration\x1b[0m ({})\n", config_path.display());
        println!("{}", toml::to_string_pretty(config)?);
        return Ok(());
    }

    match (key, value) {
        (Some(k), Some(v)) => {
            // Set a value
            config.set(&k, &v)
                .map_err(|e| anyhow::anyhow!(e))?;
            config.save(config_path)?;
            println!("\x1b[32m✓\x1b[0m Set {} = {}", k, v);
        }
        (Some(k), None) => {
            // Get a value
            match config.get(&k) {
                Some(v) => println!("{} = {}", k, v),
                None => println!("Unknown key: {}", k),
            }
        }
        (None, _) => {
            println!("Usage:");
            println!("  engram config --list           # List all settings");
            println!("  engram config <key>            # Get a setting");
            println!("  engram config <key> <value>    # Set a setting");
            println!("\nAvailable keys:");
            println!("  data_dir, server.host, server.port, qdrant.mode,");
            println!("  extraction.mode, extraction.confidence_threshold,");
            println!("  retrieval.top_k");
        }
    }

    Ok(())
}

#[derive(Debug, Default, serde::Serialize)]
struct SystemStatus {
    qdrant_connected: bool,
    qdrant_error: Option<String>,
    collection_counts: Vec<(String, u64)>,
    total_memories: u64,
}
