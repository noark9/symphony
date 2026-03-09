mod api;
mod config;
mod domain;
mod execution;
mod orchestrator;
mod prompt;
mod tracker;

use clap::Parser;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use std::path::PathBuf;

use crate::api::state::{AppState, GeminiTotals};
use crate::tracker::obsidian;
use crate::execution::workspace::WorkspaceManager;
use crate::execution::hooks::HooksManager;
use crate::execution::runner::AgentRunner;
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
    let mut workflow_doc = crate::config::loader::WorkflowDocument { config: crate::config::loader::WorkflowConfig::default(), markdown_body: "".to_string() };
    if args.config.exists() {
        if let Ok(content) = std::fs::read_to_string(&args.config) {
            if let Ok(doc) = crate::config::loader::parse_workflow_doc(&content) {
                workflow_doc = doc;
                println!("Loaded workflow configuration from {:?}", args.config);
            } else {
                eprintln!("Failed to parse workflow document from {:?}", args.config);
            }
        }
    } else {
        println!("Workflow document {:?} not found, using default configuration.", args.config);
    }

    let port = args.port.unwrap_or(workflow_doc.config.server.port);

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
        total_runtime_seconds: 0,
    }));

    let (refresh_tx, mut refresh_rx) = mpsc::channel(1);

    let app_state = AppState {
        orchestrator: orchestrator.clone(),
        gemini_totals: gemini_totals.clone(),
        refresh_tx,
    };

        // Orchestrator loop
    let orchestrator_clone = orchestrator.clone();
    let doc_clone = workflow_doc.clone();
    let gemini_totals_clone = gemini_totals.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(doc_clone.config.polling.interval_ms)) => {
                    poll_and_run(&orchestrator_clone, &doc_clone, &gemini_totals_clone).await;
                }
                _ = refresh_rx.recv() => {
                    println!("Received forced refresh signal.");
                    poll_and_run(&orchestrator_clone, &doc_clone, &gemini_totals_clone).await;
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

async fn poll_and_run(
    orchestrator: &Arc<Mutex<OrchestratorEngine>>,
    doc: &crate::config::loader::WorkflowDocument,
    totals: &Arc<Mutex<GeminiTotals>>
) {
    let vault_dir = doc.config.tracker.vault_path.as_deref().unwrap_or("");
    let vault_path = std::path::Path::new(vault_dir);

    // Fetch candidate issues (assuming "todo" and "in-progress" are active)
    let candidate_issues = match obsidian::fetch_candidate_issues(vault_path, &["todo", "in-progress"]) {
        Ok(issues) => issues,
        Err(e) => {
            eprintln!("Failed to fetch candidate issues: {}", e);
            vec![]
        }
    };

    let mut engine = orchestrator.lock().await;
    let dispatched = engine.poll(candidate_issues);

    // Convert to owned strings to avoid holding the lock
    let mut tasks_to_spawn = Vec::new();
    for (issue_id, cancel_rx) in dispatched {
        tasks_to_spawn.push((issue_id, cancel_rx));
    }

    // Release the lock before spawning tasks that might need it
    drop(engine);

    for (issue_id, cancel_rx) in tasks_to_spawn {
        let engine_clone = orchestrator.clone();
        let doc_clone = doc.clone();
        let config_clone = doc.config.clone();
        let attempt_count = orchestrator.lock().await.retry_attempts.get(&issue_id).map(|r| r.attempt_count).unwrap_or(0);
        let totals_clone = totals.clone();
        let vault_path_owned = vault_path.to_path_buf();

        tokio::spawn(async move {
            let wm = WorkspaceManager::new(&config_clone.workspace.root);

            // Re-fetch the issue to pass to create_workspace
            let issue_details = obsidian::fetch_candidate_issues(&vault_path_owned, &["todo", "in-progress"])
                .unwrap_or_default()
                .into_iter()
                .find(|i| i.id == issue_id);

            let issue = match issue_details {
                Some(i) => i,
                None => {
                    eprintln!("Issue {} disappeared before dispatch", issue_id);
                    engine_clone.lock().await.handle_exit(&issue_id, true);
                    return;
                }
            };

            let workspace = match wm.create_workspace(&issue) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to create workspace for {}: {:?}", issue_id, e);
                    engine_clone.lock().await.handle_exit(&issue_id, true);
                    return;
                }
            };

            let workspace_path = std::path::Path::new(&workspace.path);
            let timeout_ms = config_clone.hooks.timeout_ms;

            let after_create_err = HooksManager::after_create(workspace_path, config_clone.hooks.after_create.as_deref(), timeout_ms).await.err().map(|e| e.to_string());
            if let Some(err_msg) = after_create_err {
                eprintln!("after_create hook failed for {}: {}", issue_id, err_msg);
                engine_clone.lock().await.handle_exit(&issue_id, true);
                return;
            }

            let before_run_err = HooksManager::before_run(workspace_path, config_clone.hooks.before_run.as_deref(), timeout_ms).await.err().map(|e| e.to_string());
            if let Some(err_msg) = before_run_err {
                eprintln!("before_run hook failed for {}: {}", issue_id, err_msg);
                engine_clone.lock().await.handle_exit(&issue_id, true);
                return;
            }

            let gemini_command = config_clone.agent.model.unwrap_or_else(|| "gemini".to_string()); // Default to gemini if not set
            let rendered_prompt = crate::prompt::renderer::render_prompt(&doc_clone.markdown_body, &issue, Some(attempt_count)).unwrap_or_else(|e| { eprintln!("Template render failed: {}", e); String::new() });

            let result = AgentRunner::run_agent(
                workspace_path,
                &gemini_command,
                rendered_prompt,
                &vault_path_owned,
                issue_id.clone(),
                cancel_rx,
                engine_clone.clone(),
                totals_clone
            ).await;

            let abnormal = result.is_err();
            if abnormal {
                eprintln!("Agent failed for {}: {:?}", issue_id, result.err());
            } else {
                println!("Agent completed for {}", issue_id);
            }

            let _ = HooksManager::after_run(workspace_path, config_clone.hooks.after_run.as_deref(), timeout_ms).await;
            let _ = HooksManager::before_remove(workspace_path, config_clone.hooks.before_remove.as_deref(), timeout_ms).await;

            engine_clone.lock().await.handle_exit(&issue_id, abnormal);
        });
    }
}
