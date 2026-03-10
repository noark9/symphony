//! Orchestrator: poll loop, dispatch, reconciliation, and retry management.
//!
//! Implements SPEC §7, §8, §16.

use crate::acp::{AgentEvent, AgentEventType};
use crate::agent_runner::{self, AgentRunnerConfig};
use crate::config;
use crate::models::*;
use crate::prompt;
use crate::tracker::obsidian::ObsidianTracker;
use crate::tracker::Tracker;
use crate::workflow;
use crate::workspace::WorkspaceManager;
use chrono::Utc;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{self, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Shared orchestrator state, accessible from API handlers.
pub type SharedState = Arc<RwLock<OrchestratorInner>>;

/// Inner orchestrator state.
pub struct OrchestratorInner {
    pub running: HashMap<String, RunningEntry>,
    pub claimed: HashSet<String>,
    pub retry_attempts: HashMap<String, RetryEntry>,
    pub completed: HashSet<String>,
    pub gemini_totals: GeminiTotals,
    pub gemini_rate_limits: Option<serde_json::Value>,
    pub config: ServiceConfig,
    pub workflow: WorkflowDefinition,
    pub workspace_manager: WorkspaceManager,
    pub tracker: ObsidianTracker,
    pub ended_seconds: f64,
}

/// API snapshot of orchestrator state (§13.3, §13.7.2).
#[derive(Debug, Serialize)]
pub struct StateSnapshot {
    pub generated_at: String,
    pub counts: SnapshotCounts,
    pub running: Vec<RunningSnapshot>,
    pub retrying: Vec<RetrySnapshot>,
    pub gemini_totals: GeminiTotalsSnapshot,
    pub rate_limits: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SnapshotCounts {
    pub running: usize,
    pub retrying: usize,
}

#[derive(Debug, Serialize)]
pub struct RunningSnapshot {
    pub issue_id: String,
    pub issue_identifier: String,
    pub state: String,
    pub session_id: Option<String>,
    pub turn_count: u32,
    pub last_event: Option<String>,
    pub last_message: Option<String>,
    pub started_at: String,
    pub last_event_at: Option<String>,
    pub tokens: TokensSnapshot,
}

#[derive(Debug, Serialize)]
pub struct TokensSnapshot {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize)]
pub struct RetrySnapshot {
    pub issue_id: String,
    pub issue_identifier: String,
    pub attempt: u32,
    pub due_at: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GeminiTotalsSnapshot {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub seconds_running: f64,
}

/// Issue-specific detail view (§13.7.2).
#[derive(Debug, Serialize)]
pub struct IssueDetail {
    pub issue_identifier: String,
    pub issue_id: String,
    pub status: String,
    pub workspace: WorkspaceDetail,
    pub running: Option<RunningSnapshot>,
    pub retry: Option<RetrySnapshot>,
    pub last_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceDetail {
    pub path: String,
}

impl OrchestratorInner {
    /// Create a state snapshot for the API (§13.3).
    pub fn snapshot(&self) -> StateSnapshot {
        let now = Utc::now();

        // Calculate live seconds_running (§13.5)
        let active_seconds: f64 = self
            .running
            .values()
            .map(|r| {
                (now - r.started_at).num_milliseconds() as f64 / 1000.0
            })
            .sum();

        let total_seconds = self.ended_seconds + active_seconds;

        StateSnapshot {
            generated_at: now.to_rfc3339(),
            counts: SnapshotCounts {
                running: self.running.len(),
                retrying: self.retry_attempts.len(),
            },
            running: self
                .running
                .values()
                .map(|r| RunningSnapshot {
                    issue_id: r.issue.id.clone(),
                    issue_identifier: r.identifier.clone(),
                    state: r.issue.state.clone(),
                    session_id: r.session_id.clone(),
                    turn_count: r.turn_count,
                    last_event: r.last_acp_event.clone(),
                    last_message: r.last_acp_message.clone(),
                    started_at: r.started_at.to_rfc3339(),
                    last_event_at: r.last_acp_timestamp.map(|t| t.to_rfc3339()),
                    tokens: TokensSnapshot {
                        input_tokens: r.gemini_input_tokens,
                        output_tokens: r.gemini_output_tokens,
                        total_tokens: r.gemini_total_tokens,
                    },
                })
                .collect(),
            retrying: self
                .retry_attempts
                .values()
                .map(|r| RetrySnapshot {
                    issue_id: r.issue_id.clone(),
                    issue_identifier: r.identifier.clone(),
                    attempt: r.attempt,
                    due_at: chrono::DateTime::from_timestamp_millis(r.due_at_ms as i64)
                        .map(|t| t.to_rfc3339())
                        .unwrap_or_default(),
                    error: r.error.clone(),
                })
                .collect(),
            gemini_totals: GeminiTotalsSnapshot {
                input_tokens: self.gemini_totals.input_tokens,
                output_tokens: self.gemini_totals.output_tokens,
                total_tokens: self.gemini_totals.total_tokens,
                seconds_running: total_seconds,
            },
            rate_limits: self.gemini_rate_limits.clone(),
        }
    }

    /// Get issue-specific detail for the API.
    pub fn issue_detail(&self, identifier: &str) -> Option<IssueDetail> {
        // Find in running
        let running_entry = self
            .running
            .values()
            .find(|r| r.identifier == identifier);

        // Find in retry
        let retry_entry = self
            .retry_attempts
            .values()
            .find(|r| r.identifier == identifier);

        if running_entry.is_none() && retry_entry.is_none() {
            return None;
        }

        let (issue_id, status) = if let Some(r) = running_entry {
            (r.issue.id.clone(), "running".to_string())
        } else if let Some(r) = retry_entry {
            (r.issue_id.clone(), "retrying".to_string())
        } else {
            return None;
        };

        let workspace_path = self.workspace_manager.workspace_path_for(identifier);

        Some(IssueDetail {
            issue_identifier: identifier.to_string(),
            issue_id,
            status,
            workspace: WorkspaceDetail {
                path: workspace_path,
            },
            running: running_entry.map(|r| RunningSnapshot {
                issue_id: r.issue.id.clone(),
                issue_identifier: r.identifier.clone(),
                state: r.issue.state.clone(),
                session_id: r.session_id.clone(),
                turn_count: r.turn_count,
                last_event: r.last_acp_event.clone(),
                last_message: r.last_acp_message.clone(),
                started_at: r.started_at.to_rfc3339(),
                last_event_at: r.last_acp_timestamp.map(|t| t.to_rfc3339()),
                tokens: TokensSnapshot {
                    input_tokens: r.gemini_input_tokens,
                    output_tokens: r.gemini_output_tokens,
                    total_tokens: r.gemini_total_tokens,
                },
            }),
            retry: retry_entry.map(|r| RetrySnapshot {
                issue_id: r.issue_id.clone(),
                issue_identifier: r.identifier.clone(),
                attempt: r.attempt,
                due_at: chrono::DateTime::from_timestamp_millis(r.due_at_ms as i64)
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default(),
                error: r.error.clone(),
            }),
            last_error: retry_entry.and_then(|r| r.error.clone()),
        })
    }
}

/// Start the orchestrator (§16.1).
pub async fn start_orchestrator(
    workflow_path: PathBuf,
    port_override: Option<u16>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Symphony starting...");

    // Load initial workflow
    let workflow = workflow::load_workflow(&workflow_path)?;
    let config = config::parse_config(&workflow.config);
    config::validate_config(&config)?;

    let effective_port = port_override.or(config.server.port);

    // Initialize components
    let tracker = ObsidianTracker::new(
        config.tracker.vault_dir.clone().unwrap_or_default(),
        config.tracker.issues_dir.clone(),
        config.tracker.active_states.clone(),
        config.tracker.terminal_states.clone(),
    );

    let workspace_manager = WorkspaceManager::new(&config.workspace.root);

    let inner = OrchestratorInner {
        running: HashMap::new(),
        claimed: HashSet::new(),
        retry_attempts: HashMap::new(),
        completed: HashSet::new(),
        gemini_totals: GeminiTotals::default(),
        gemini_rate_limits: None,
        config: config.clone(),
        workflow,
        workspace_manager,
        tracker,
        ended_seconds: 0.0,
    };

    let shared_state: SharedState = Arc::new(RwLock::new(inner));

    // Startup terminal workspace cleanup (§8.6)
    startup_terminal_cleanup(shared_state.clone()).await;

    // Start workflow file watcher (§6.2)
    let watcher_state = shared_state.clone();
    let watcher_path = workflow_path.clone();
    tokio::spawn(async move {
        watch_workflow_file(watcher_path, watcher_state).await;
    });

    // Start HTTP server if port configured (§13.7)
    if let Some(port) = effective_port {
        let server_state = shared_state.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::server::start_server(port, server_state).await {
                error!(error = %e, "HTTP server failed");
            }
        });
    }

    // Start poll loop (§16.1)
    let poll_state = shared_state.clone();
    run_poll_loop(poll_state, workflow_path).await;

    Ok(())
}

/// Run the poll loop (§16.2).
async fn run_poll_loop(state: SharedState, workflow_path: PathBuf) {
    // Immediate first tick
    poll_tick(state.clone(), &workflow_path).await;

    loop {
        let interval_ms = {
            let s = state.read().await;
            s.config.polling.interval_ms
        };

        time::sleep(Duration::from_millis(interval_ms)).await;
        poll_tick(state.clone(), &workflow_path).await;
    }
}

/// Single poll tick (§16.2).
async fn poll_tick(state: SharedState, _workflow_path: &Path) {
    debug!("poll tick starting");

    // Step 1: Reconcile running issues (§16.3)
    reconcile_running_issues(state.clone()).await;

    // Step 2: Validate dispatch config (§6.3)
    {
        let s = state.read().await;
        if let Err(e) = config::validate_config(&s.config) {
            error!(error = %e, "dispatch validation failed, skipping dispatch");
            return;
        }
    }

    // Step 3: Fetch candidate issues
    let candidates = {
        let s = state.read().await;
        s.tracker.fetch_candidate_issues().await
    };

    let mut issues = match candidates {
        Ok(issues) => issues,
        Err(e) => {
            error!(error = %e, "failed to fetch candidate issues");
            return;
        }
    };

    // Step 4: Sort by dispatch priority (§8.2)
    sort_for_dispatch(&mut issues);

    // Step 5: Dispatch eligible issues
    for issue in issues {
        let should_dispatch = {
            let s = state.read().await;
            available_slots(&s) > 0 && should_dispatch_issue(&issue, &s)
        };

        if !should_dispatch {
            if available_slots_check(&state).await == 0 {
                break;
            }
            continue;
        }

        dispatch_issue(state.clone(), issue, None).await;
    }

    debug!("poll tick complete");
}

/// Check available slots without holding lock long.
async fn available_slots_check(state: &SharedState) -> usize {
    let s = state.read().await;
    available_slots(&s)
}

/// Calculate available dispatch slots (§8.3).
fn available_slots(state: &OrchestratorInner) -> usize {
    let running = state.running.len();
    let max = state.config.agent.max_concurrent_agents;
    if running >= max {
        0
    } else {
        max - running
    }
}

/// Check if an issue should be dispatched (§8.2).
fn should_dispatch_issue(issue: &Issue, state: &OrchestratorInner) -> bool {
    // Must have required fields
    if issue.id.is_empty()
        || issue.identifier.is_empty()
        || issue.title.is_empty()
        || issue.state.is_empty()
    {
        return false;
    }

    let norm_state = normalize_state(&issue.state);
    let active: HashSet<String> = state
        .config
        .tracker
        .active_states
        .iter()
        .map(|s| normalize_state(s))
        .collect();
    let terminal: HashSet<String> = state
        .config
        .tracker
        .terminal_states
        .iter()
        .map(|s| normalize_state(s))
        .collect();

    // Must be in active_states and not in terminal_states
    if !active.contains(&norm_state) || terminal.contains(&norm_state) {
        return false;
    }

    // Not already running or claimed
    if state.running.contains_key(&issue.id) || state.claimed.contains(&issue.id) {
        return false;
    }

    // Per-state concurrency check (§8.3)
    if let Some(&max_per_state) = state
        .config
        .agent
        .max_concurrent_agents_by_state
        .get(&norm_state)
    {
        let current_count = state
            .running
            .values()
            .filter(|r| normalize_state(&r.issue.state) == norm_state)
            .count();
        if current_count >= max_per_state {
            return false;
        }
    }

    // Blocker rule for Todo state (§8.2)
    if norm_state == "todo" {
        let has_non_terminal_blocker = issue.blocked_by.iter().any(|b| {
            if let Some(blocker_state) = &b.state {
                !terminal.contains(&normalize_state(blocker_state))
            } else {
                true // unknown state = non-terminal
            }
        });
        if has_non_terminal_blocker {
            return false;
        }
    }

    true
}

/// Sort issues for dispatch priority (§8.2).
fn sort_for_dispatch(issues: &mut [Issue]) {
    issues.sort_by(|a, b| {
        // Priority ascending (lower = higher priority, null sorts last)
        let pa = a.priority.unwrap_or(i32::MAX);
        let pb = b.priority.unwrap_or(i32::MAX);
        pa.cmp(&pb)
            .then_with(|| {
                // created_at oldest first
                a.created_at.cmp(&b.created_at)
            })
            .then_with(|| {
                // identifier lexicographic tie-breaker
                a.identifier.cmp(&b.identifier)
            })
    });
}

/// Dispatch a single issue (§16.4).
async fn dispatch_issue(state: SharedState, issue: Issue, attempt: Option<u32>) {
    let issue_id = issue.id.clone();
    let identifier = issue.identifier.clone();

    info!(
        issue_id = %issue_id,
        issue_identifier = %identifier,
        attempt = ?attempt,
        "dispatching issue"
    );

    let cancel_token = CancellationToken::new();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentEvent>();

    // Get config for the worker
    let (workspace_root, hooks_config, gemini_config, prompt_template, vault_dir, hook_timeout_ms) = {
        let s = state.read().await;
        (
            s.config.workspace.root.clone(),
            s.config.hooks.clone(),
            s.config.gemini.clone(),
            s.workflow.prompt_template.clone(),
            s.config.tracker.vault_dir.clone(),
            s.config.hooks.timeout_ms,
        )
    };

    // Add to running map
    {
        let mut s = state.write().await;
        s.running.insert(
            issue_id.clone(),
            RunningEntry {
                identifier: identifier.clone(),
                issue: issue.clone(),
                session_id: None,
                gemini_cli_pid: None,
                last_acp_message: None,
                last_acp_event: None,
                last_acp_timestamp: None,
                gemini_input_tokens: 0,
                gemini_output_tokens: 0,
                gemini_total_tokens: 0,
                last_reported_input_tokens: 0,
                last_reported_output_tokens: 0,
                last_reported_total_tokens: 0,
                retry_attempt: attempt,
                started_at: Utc::now(),
                turn_count: 0,
                cancel_token: cancel_token.clone(),
            },
        );
        s.claimed.insert(issue_id.clone());
        s.retry_attempts.remove(&issue_id);
    }

    // Spawn the worker task
    let worker_state = state.clone();
    let worker_issue = issue.clone();
    let worker_cancel = cancel_token.clone();
    let worker_issue_id = issue_id.clone();
    let worker_identifier = identifier.clone();

    tokio::spawn(async move {
        let result = run_worker(
            &workspace_root,
            &hooks_config,
            &gemini_config,
            &prompt_template,
            vault_dir.as_deref(),
            hook_timeout_ms,
            &worker_issue,
            attempt,
            worker_cancel,
            event_tx,
        )
        .await;

        // Worker exit handling (§16.6)
        let mut s = worker_state.write().await;
        if let Some(running_entry) = s.running.remove(&worker_issue_id) {
            // Record session completion totals
            let duration = (Utc::now() - running_entry.started_at).num_milliseconds() as f64 / 1000.0;
            s.ended_seconds += duration;
            s.gemini_totals.input_tokens += running_entry.gemini_input_tokens;
            s.gemini_totals.output_tokens += running_entry.gemini_output_tokens;
            s.gemini_totals.total_tokens += running_entry.gemini_total_tokens;

            match result {
                Ok(()) => {
                    // Normal exit → continuation retry (§16.6)
                    s.completed.insert(worker_issue_id.clone());
                    schedule_retry(
                        &mut s,
                        &worker_issue_id,
                        &worker_identifier,
                        1,
                        None,
                        1000, // 1s continuation delay
                    );
                }
                Err(e) => {
                    // Abnormal exit → exponential backoff retry (§16.6)
                    let next_attempt = running_entry.retry_attempt.unwrap_or(0) + 1;
                    let max_backoff = s.config.agent.max_retry_backoff_ms;
                    let delay = calculate_backoff(next_attempt, max_backoff);
                    schedule_retry(
                        &mut s,
                        &worker_issue_id,
                        &worker_identifier,
                        next_attempt,
                        Some(format!("worker exited: {}", e)),
                        delay,
                    );
                }
            }
        }
    });

    // Spawn event receiver to update running entry
    let event_state = state.clone();
    let event_issue_id = issue_id.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let mut s = event_state.write().await;
            if let Some(entry) = s.running.get_mut(&event_issue_id) {
                entry.last_acp_event = Some(format!("{:?}", event.event));
                entry.last_acp_timestamp = Some(event.timestamp);
                if let Some(msg) = &event.message {
                    entry.last_acp_message = Some(msg.clone());
                }
                if let Some(pid) = &event.gemini_cli_pid {
                    entry.gemini_cli_pid = Some(pid.clone());
                }
                if let Some(session_id) = &event.session_id {
                    entry.session_id = Some(session_id.clone());
                }

                // Update token counts (§13.5)
                if let Some(usage) = &event.usage {
                    if let Some(input) = usage.input_tokens {
                        if input > entry.last_reported_input_tokens {
                            let delta = input - entry.last_reported_input_tokens;
                            entry.gemini_input_tokens += delta;
                            entry.last_reported_input_tokens = input;
                        }
                    }
                    if let Some(output) = usage.output_tokens {
                        if output > entry.last_reported_output_tokens {
                            let delta = output - entry.last_reported_output_tokens;
                            entry.gemini_output_tokens += delta;
                            entry.last_reported_output_tokens = output;
                        }
                    }
                    if let Some(total) = usage.total_tokens {
                        if total > entry.last_reported_total_tokens {
                            let delta = total - entry.last_reported_total_tokens;
                            entry.gemini_total_tokens += delta;
                            entry.last_reported_total_tokens = total;
                        }
                    }
                }

                if event.event == AgentEventType::TurnCompleted {
                    entry.turn_count += 1;
                }
            }

            // Update rate limits
            // (handled via extract in event processing above)
        }
    });
}

/// Run the worker task (§16.5).
async fn run_worker(
    workspace_root: &str,
    hooks: &HooksConfig,
    gemini: &GeminiConfig,
    prompt_template: &str,
    vault_dir: Option<&str>,
    hook_timeout_ms: u64,
    issue: &Issue,
    attempt: Option<u32>,
    cancel_token: CancellationToken,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_manager = WorkspaceManager::new(workspace_root);

    // Step 1: Create/reuse workspace
    let workspace = ws_manager
        .create_for_issue(
            &issue.identifier,
            hooks.after_create.as_deref(),
            hook_timeout_ms,
        )
        .await?;

    // Step 2: Run before_run hook
    if let Some(script) = &hooks.before_run {
        ws_manager
            .run_before_run_hook(&workspace.path, script, hook_timeout_ms)
            .await?;
    }

    // Step 3: Build prompt
    let rendered_prompt = prompt::render_prompt(prompt_template, issue, attempt)?;

    // Step 4: Run agent session
    let runner_config = AgentRunnerConfig {
        kind: gemini.kind.clone(),
        agent_command: gemini.command.clone(),
        turn_timeout_ms: gemini.turn_timeout_ms,
        read_timeout_ms: gemini.read_timeout_ms,
        vault_dir: vault_dir.map(String::from),
        log_agent_output: gemini.log_agent_output,
    };

    let result = agent_runner::run_agent_session(
        &runner_config,
        &workspace.path,
        &rendered_prompt,
        issue,
        cancel_token,
        event_tx,
    )
    .await;

    // Step 5: Run after_run hook (best-effort)
    if let Some(script) = &hooks.after_run {
        ws_manager
            .run_after_run_hook(&workspace.path, script, hook_timeout_ms)
            .await;
    }

    result.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
}

/// Reconcile running issues (§16.3, §8.5).
async fn reconcile_running_issues(state: SharedState) {
    // Part A: Stall detection
    reconcile_stalled_runs(state.clone()).await;

    // Part B: Tracker state refresh
    let running_ids: Vec<String> = {
        let s = state.read().await;
        s.running.keys().cloned().collect()
    };

    if running_ids.is_empty() {
        return;
    }

    let refreshed = {
        let s = state.read().await;
        s.tracker.fetch_issue_states_by_ids(&running_ids).await
    };

    let refreshed_issues = match refreshed {
        Ok(issues) => issues,
        Err(e) => {
            debug!(error = %e, "state refresh failed, keeping workers running");
            return;
        }
    };

    let mut s = state.write().await;
    let active_states: HashSet<String> = s
        .config
        .tracker
        .active_states
        .iter()
        .map(|s| normalize_state(s))
        .collect();
    let terminal_states: HashSet<String> = s
        .config
        .tracker
        .terminal_states
        .iter()
        .map(|s| normalize_state(s))
        .collect();

    for issue in &refreshed_issues {
        let norm_state = normalize_state(&issue.state);

        if terminal_states.contains(&norm_state) {
            // Terminal → stop and clean workspace
            if let Some(entry) = s.running.remove(&issue.id) {
                entry.cancel_token.cancel();
                let duration =
                    (Utc::now() - entry.started_at).num_milliseconds() as f64 / 1000.0;
                s.ended_seconds += duration;
                info!(
                    issue_id = %issue.id,
                    issue_identifier = %entry.identifier,
                    "terminated (terminal state), cleaning workspace"
                );
            }
            s.claimed.remove(&issue.id);
            s.retry_attempts.remove(&issue.id);
            // Workspace cleanup happens asynchronously
            let ws_root = s.config.workspace.root.clone();
            let identifier = issue.identifier.clone();
            let hook = s.config.hooks.before_remove.clone();
            let timeout_ms = s.config.hooks.timeout_ms;
            tokio::spawn(async move {
                let mgr = WorkspaceManager::new(&ws_root);
                mgr.remove_workspace(&identifier, hook.as_deref(), timeout_ms)
                    .await;
            });
        } else if active_states.contains(&norm_state) {
            // Still active → update snapshot
            if let Some(entry) = s.running.get_mut(&issue.id) {
                entry.issue = issue.clone();
            }
        } else {
            // Neither active nor terminal → stop without cleanup
            if let Some(entry) = s.running.remove(&issue.id) {
                entry.cancel_token.cancel();
                let duration =
                    (Utc::now() - entry.started_at).num_milliseconds() as f64 / 1000.0;
                s.ended_seconds += duration;
                info!(
                    issue_id = %issue.id,
                    issue_identifier = %entry.identifier,
                    "terminated (non-active state)"
                );
            }
            s.claimed.remove(&issue.id);
            s.retry_attempts.remove(&issue.id);
        }
    }
}

/// Reconcile stalled runs (§8.5 Part A).
async fn reconcile_stalled_runs(state: SharedState) {
    let stall_timeout_ms = {
        let s = state.read().await;
        s.config.gemini.stall_timeout_ms
    };

    if stall_timeout_ms <= 0 {
        return; // Stall detection disabled
    }

    let now = Utc::now();
    let mut stalled_ids = Vec::new();

    {
        let s = state.read().await;
        for (id, entry) in &s.running {
            let last_activity = entry.last_acp_timestamp.unwrap_or(entry.started_at);
            let elapsed_ms = (now - last_activity).num_milliseconds();
            if elapsed_ms > stall_timeout_ms as i64 {
                stalled_ids.push(id.clone());
            }
        }
    }

    for id in stalled_ids {
        let mut s = state.write().await;
        if let Some(entry) = s.running.remove(&id) {
            entry.cancel_token.cancel();
            let duration =
                (Utc::now() - entry.started_at).num_milliseconds() as f64 / 1000.0;
            s.ended_seconds += duration;
            warn!(
                issue_id = %id,
                issue_identifier = %entry.identifier,
                "session stalled, scheduling retry"
            );
            let next_attempt = entry.retry_attempt.unwrap_or(0) + 1;
            let max_backoff = s.config.agent.max_retry_backoff_ms;
            let delay = calculate_backoff(next_attempt, max_backoff);
            schedule_retry(
                &mut s,
                &id,
                &entry.identifier,
                next_attempt,
                Some("session stalled".to_string()),
                delay,
            );
        }
    }
}

/// Schedule a retry (§8.4).
fn schedule_retry(
    state: &mut OrchestratorInner,
    issue_id: &str,
    identifier: &str,
    attempt: u32,
    error: Option<String>,
    delay_ms: u64,
) {
    let due_at_ms = Utc::now().timestamp_millis() as u64 + delay_ms;

    info!(
        issue_id = %issue_id,
        issue_identifier = %identifier,
        attempt = attempt,
        delay_ms = delay_ms,
        "scheduling retry"
    );

    state.retry_attempts.insert(
        issue_id.to_string(),
        RetryEntry {
            issue_id: issue_id.to_string(),
            identifier: identifier.to_string(),
            attempt,
            due_at_ms,
            error,
        },
    );
}

/// Calculate exponential backoff delay (§8.4).
fn calculate_backoff(attempt: u32, max_backoff_ms: u64) -> u64 {
    let base_delay: u64 = 10_000; // 10s
    let delay = base_delay.saturating_mul(1u64.wrapping_shl(attempt.saturating_sub(1)));
    delay.min(max_backoff_ms)
}

/// Startup terminal workspace cleanup (§8.6, §16.1).
async fn startup_terminal_cleanup(state: SharedState) {
    info!("performing startup terminal workspace cleanup");

    let (terminal_states, ws_root, hook, timeout_ms) = {
        let s = state.read().await;
        (
            s.config.tracker.terminal_states.clone(),
            s.config.workspace.root.clone(),
            s.config.hooks.before_remove.clone(),
            s.config.hooks.timeout_ms,
        )
    };

    let terminal_issues = {
        let s = state.read().await;
        s.tracker.fetch_issues_by_states(&terminal_states).await
    };

    match terminal_issues {
        Ok(issues) => {
            let mgr = WorkspaceManager::new(&ws_root);
            for issue in &issues {
                mgr.remove_workspace(&issue.identifier, hook.as_deref(), timeout_ms)
                    .await;
            }
            info!(count = issues.len(), "startup cleanup complete");
        }
        Err(e) => {
            warn!(error = %e, "startup terminal cleanup failed, continuing");
        }
    }
}

/// Watch WORKFLOW.md for changes and reload (§6.2).
async fn watch_workflow_file(path: PathBuf, state: SharedState) {
    let (tx, mut rx) = mpsc::channel(10);

    let mut debouncer = match new_debouncer(
        Duration::from_millis(500),
        move |events: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
            if let Ok(events) = events {
                for event in events {
                    if event.kind == DebouncedEventKind::Any {
                        let _ = tx.blocking_send(());
                    }
                }
            }
        },
    ) {
        Ok(d) => d,
        Err(e) => {
            error!(error = %e, "failed to create file watcher");
            return;
        }
    };

    if let Some(parent) = path.parent() {
        if let Err(e) = debouncer.watcher().watch(parent, notify::RecursiveMode::NonRecursive) {
            error!(error = %e, "failed to watch workflow directory");
            return;
        }
    }

    info!(path = %path.display(), "watching WORKFLOW.md for changes");

    while rx.recv().await.is_some() {
        info!("WORKFLOW.md changed, reloading");
        match workflow::load_workflow(&path) {
            Ok(new_workflow) => {
                let new_config = config::parse_config(&new_workflow.config);
                if let Err(e) = config::validate_config(&new_config) {
                    error!(error = %e, "reloaded config is invalid, keeping last known good");
                    continue;
                }

                let mut s = state.write().await;
                // Update tracker config
                s.tracker.update_config(
                    new_config.tracker.vault_dir.clone().unwrap_or_default(),
                    new_config.tracker.issues_dir.clone(),
                    new_config.tracker.active_states.clone(),
                    new_config.tracker.terminal_states.clone(),
                );
                // Update workspace root
                s.workspace_manager.update_root(&new_config.workspace.root);
                // Update config and workflow
                s.config = new_config;
                s.workflow = new_workflow;
                info!("workflow reloaded successfully");
            }
            Err(e) => {
                error!(error = %e, "failed to reload WORKFLOW.md, keeping last known good");
            }
        }
    }
}

/// Process pending retries (called periodically).
pub async fn process_retries(state: SharedState, _workflow_path: &Path) {
    let now_ms = Utc::now().timestamp_millis() as u64;
    let mut due_retries = Vec::new();

    {
        let s = state.read().await;
        for (id, entry) in &s.retry_attempts {
            if now_ms >= entry.due_at_ms {
                due_retries.push(id.clone());
            }
        }
    }

    for issue_id in due_retries {
        handle_retry(state.clone(), &issue_id).await;
    }
}

/// Handle a single retry (§16.6).
async fn handle_retry(state: SharedState, issue_id: &str) {
    let retry_entry = {
        let mut s = state.write().await;
        s.retry_attempts.remove(issue_id)
    };

    let retry_entry = match retry_entry {
        Some(e) => e,
        None => return,
    };

    // Fetch candidate issues
    let candidates = {
        let s = state.read().await;
        s.tracker.fetch_candidate_issues().await
    };

    let candidates = match candidates {
        Ok(c) => c,
        Err(e) => {
            let mut s = state.write().await;
            let max_backoff = s.config.agent.max_retry_backoff_ms;
            let delay = calculate_backoff(retry_entry.attempt + 1, max_backoff);
            schedule_retry(
                &mut s,
                issue_id,
                &retry_entry.identifier,
                retry_entry.attempt + 1,
                Some(format!("retry poll failed: {}", e)),
                delay,
            );
            return;
        }
    };

    // Find the issue
    let issue = candidates.iter().find(|i| i.id == issue_id);

    match issue {
        None => {
            // Not found → release claim
            let mut s = state.write().await;
            s.claimed.remove(issue_id);
            info!(issue_id = %issue_id, "retry released claim (issue not found)");
        }
        Some(issue) => {
            let has_slots = {
                let s = state.read().await;
                available_slots(&s) > 0
            };

            if has_slots {
                dispatch_issue(state.clone(), issue.clone(), Some(retry_entry.attempt)).await;
            } else {
                let mut s = state.write().await;
                let max_backoff = s.config.agent.max_retry_backoff_ms;
                let delay = calculate_backoff(retry_entry.attempt + 1, max_backoff);
                schedule_retry(
                    &mut s,
                    issue_id,
                    &issue.identifier,
                    retry_entry.attempt + 1,
                    Some("no available orchestrator slots".to_string()),
                    delay,
                );
            }
        }
    }
}

/// Trigger an immediate refresh (for POST /api/v1/refresh).
pub async fn trigger_refresh(state: SharedState, workflow_path: PathBuf) {
    tokio::spawn(async move {
        poll_tick(state, &workflow_path).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_backoff() {
        assert_eq!(calculate_backoff(1, 300_000), 10_000);
        assert_eq!(calculate_backoff(2, 300_000), 20_000);
        assert_eq!(calculate_backoff(3, 300_000), 40_000);
        assert_eq!(calculate_backoff(4, 300_000), 80_000);
        assert_eq!(calculate_backoff(5, 300_000), 160_000);
        assert_eq!(calculate_backoff(6, 300_000), 300_000); // capped
        assert_eq!(calculate_backoff(10, 300_000), 300_000); // capped
    }

    #[test]
    fn test_sort_for_dispatch() {
        use chrono::TimeZone;

        let mut issues = vec![
            Issue {
                id: "c".into(),
                identifier: "C-1".into(),
                title: "C".into(),
                description: None,
                priority: Some(3),
                state: "Todo".into(),
                branch_name: None,
                url: None,
                labels: vec![],
                blocked_by: vec![],
                created_at: Some(Utc.with_ymd_and_hms(2026, 1, 3, 0, 0, 0).unwrap()),
                updated_at: None,
            },
            Issue {
                id: "a".into(),
                identifier: "A-1".into(),
                title: "A".into(),
                description: None,
                priority: Some(1),
                state: "Todo".into(),
                branch_name: None,
                url: None,
                labels: vec![],
                blocked_by: vec![],
                created_at: Some(Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()),
                updated_at: None,
            },
            Issue {
                id: "b".into(),
                identifier: "B-1".into(),
                title: "B".into(),
                description: None,
                priority: Some(1),
                state: "Todo".into(),
                branch_name: None,
                url: None,
                labels: vec![],
                blocked_by: vec![],
                created_at: Some(Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap()),
                updated_at: None,
            },
        ];

        sort_for_dispatch(&mut issues);
        assert_eq!(issues[0].identifier, "A-1"); // priority 1, oldest
        assert_eq!(issues[1].identifier, "B-1"); // priority 1, newer
        assert_eq!(issues[2].identifier, "C-1"); // priority 3
    }
}
