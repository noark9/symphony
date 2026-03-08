mod api;
mod config;
mod domain;
mod execution;
mod orchestrator;
mod tracker;

use clap::Parser;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use std::path::PathBuf;

use crate::api::state::{AppState, GeminiTotals};
use crate::orchestrator::engine::OrchestratorEngine;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    port: Option<u16>,

    #[arg(short, long, default_value = "WORKFLOW.md")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("Symphony Automation Service Starting...");

    // Try loading workflow config
    let mut workflow_config = crate::config::loader::WorkflowConfig::default();
    if args.config.exists() {
        if let Ok(content) = std::fs::read_to_string(&args.config) {
            if let Ok(doc) = crate::config::loader::parse_workflow_doc(&content) {
                workflow_config = doc.config;
                println!("Loaded workflow configuration from {:?}", args.config);
            } else {
                eprintln!("Failed to parse workflow document from {:?}", args.config);
            }
        }
    } else {
        println!("Workflow document {:?} not found, using default configuration.", args.config);
    }

    let port = args.port.unwrap_or(workflow_config.server.port);

    // Initialize state
    let engine = OrchestratorEngine::new(
        2,     // max_concurrent_agents
        30000, // stall_timeout_ms
        60000, // max_retry_backoff_ms
    );
    let orchestrator = Arc::new(Mutex::new(engine));

    let gemini_totals = Arc::new(Mutex::new(GeminiTotals {
        prompt_tokens: 0,
        candidate_tokens: 0,
        total_requests: 0,
    }));

    let (refresh_tx, mut refresh_rx) = mpsc::channel(1);

    let app_state = AppState {
        orchestrator: orchestrator.clone(),
        gemini_totals: gemini_totals.clone(),
        refresh_tx,
    };

    // Orchestrator loop (mock implementation for now)
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(workflow_config.polling.interval_ms)) => {
                    // Regular poll
                    // In a real implementation, this is where we'd fetch issues and call `engine.poll(candidates)`
                }
                _ = refresh_rx.recv() => {
                    println!("Received forced refresh signal.");
                    // In a real implementation, this would trigger an immediate fetch and poll
                }
            }
        }
    });

    let app = api::server::app(app_state);
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("Listening on http://{}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
