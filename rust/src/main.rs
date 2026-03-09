//! Symphony automation service — Rust implementation.
//!
//! Entry point: CLI parsing and service startup.

mod acp;
mod agent_runner;
mod config;
mod models;
mod orchestrator;
mod prompt;
mod server;
mod tracker;
mod workflow;
mod workspace;

use clap::Parser;
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};

/// Symphony — Coding agent orchestration service.
#[derive(Parser, Debug)]
#[command(name = "symphony", about = "Coding agent orchestration service")]
struct Cli {
    /// Path to WORKFLOW.md (default: ./WORKFLOW.md)
    #[arg(value_name = "WORKFLOW_PATH")]
    workflow_path: Option<PathBuf>,

    /// HTTP server port (overrides server.port in WORKFLOW.md)
    #[arg(long)]
    port: Option<u16>,
}

#[tokio::main]
async fn main() {
    // Configure structured logging (§13.1)
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(true)
        .with_thread_ids(true)
        .json()
        .init();

    let cli = Cli::parse();

    // Workflow path selection (§17.7)
    let workflow_path = cli.workflow_path.unwrap_or_else(|| PathBuf::from("WORKFLOW.md"));

    // Verify workflow file exists
    if !workflow_path.exists() {
        error!(
            path = %workflow_path.display(),
            "WORKFLOW.md not found"
        );
        std::process::exit(1);
    }

    info!(
        path = %workflow_path.display(),
        "starting Symphony with workflow"
    );

    // Start the orchestrator
    match orchestrator::start_orchestrator(workflow_path, cli.port).await {
        Ok(()) => {
            info!("Symphony shut down normally");
        }
        Err(e) => {
            error!(error = %e, "Symphony failed to start");
            std::process::exit(1);
        }
    }
}
