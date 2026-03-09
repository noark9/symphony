//! Core domain models for the Symphony automation service.
//!
//! Implements the entities defined in SPEC §4.1.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// ─── §4.1.1 Issue ───

/// A normalized issue record used by orchestration, prompt rendering, and observability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    /// Stable tracker-internal ID.
    pub id: String,
    /// Human-readable ticket key (e.g. `ABC-123`).
    pub identifier: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Lower numbers = higher priority in dispatch sorting.
    #[serde(default)]
    pub priority: Option<i32>,
    /// Current tracker state name.
    pub state: String,
    #[serde(default)]
    pub branch_name: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// Normalized to lowercase.
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<BlockerRef>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

/// A blocker reference within an Issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockerRef {
    pub id: Option<String>,
    pub identifier: Option<String>,
    pub state: Option<String>,
}

// ─── §4.1.2 Workflow Definition ───

/// Parsed `WORKFLOW.md` payload.
#[derive(Debug, Clone)]
pub struct WorkflowDefinition {
    /// YAML front matter root object.
    pub config: serde_yaml::Value,
    /// Markdown body after front matter, trimmed.
    pub prompt_template: String,
}

// ─── §4.1.3 Service Config (Typed View) ───

/// Typed runtime values derived from WorkflowDefinition.config plus environment resolution.
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    pub tracker: TrackerConfig,
    pub polling: PollingConfig,
    pub workspace: WorkspaceConfig,
    pub hooks: HooksConfig,
    pub agent: AgentConfig,
    pub gemini: GeminiConfig,
    pub server: ServerConfig,
}

#[derive(Debug, Clone)]
pub struct TrackerConfig {
    pub kind: Option<String>,
    pub vault_dir: Option<String>,
    pub active_states: Vec<String>,
    pub terminal_states: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PollingConfig {
    pub interval_ms: u64,
}

#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    pub root: String,
}

#[derive(Debug, Clone)]
pub struct HooksConfig {
    pub after_create: Option<String>,
    pub before_run: Option<String>,
    pub after_run: Option<String>,
    pub before_remove: Option<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub max_concurrent_agents: usize,
    pub max_retry_backoff_ms: u64,
    pub max_concurrent_agents_by_state: HashMap<String, usize>,
}

/// Agent runner kind — determines which protocol to use.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentKind {
    /// Gemini CLI with ACP (JSON-RPC over stdio): `gemini --experimental-acp`
    GeminiAcp,
    /// Claude Code prompt mode (one-shot): `claude -p "<prompt>"`
    ClaudePrompt,
    /// Gemini CLI prompt mode (one-shot): `gemini -p "<prompt>"`
    GeminiPrompt,
}

impl AgentKind {
    /// Parse from string, case-insensitive.
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().replace('-', "_").as_str() {
            "gemini_acp" | "gemini" => Some(Self::GeminiAcp),
            "claude_prompt" | "claude" => Some(Self::ClaudePrompt),
            "gemini_prompt" | "prompt" => Some(Self::GeminiPrompt),
            _ => None,
        }
    }

    /// Default shell command for this agent kind.
    pub fn default_command(&self) -> &'static str {
        match self {
            Self::GeminiAcp => "gemini --experimental-acp",
            Self::ClaudePrompt => "claude",
            Self::GeminiPrompt => "gemini",
        }
    }

    /// Whether this kind uses the ACP (JSON-RPC) protocol.
    pub fn is_acp(&self) -> bool {
        matches!(self, Self::GeminiAcp)
    }
}

#[derive(Debug, Clone)]
pub struct GeminiConfig {
    pub kind: AgentKind,
    pub command: String,
    pub turn_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub stall_timeout_ms: i64,
    /// Whether to log agent stdout/stderr to the backend console.
    pub log_agent_output: bool,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub port: Option<u16>,
}

// ─── §4.1.4 Workspace ───

/// Filesystem workspace assigned to one issue identifier.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub path: String,
    pub workspace_key: String,
    pub created_now: bool,
}

// ─── §4.1.5 Run Attempt ───

/// One execution attempt for one issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAttempt {
    pub issue_id: String,
    pub issue_identifier: String,
    /// `None` for first run, `Some(n)` for retries/continuation.
    pub attempt: Option<u32>,
    pub workspace_path: String,
    pub started_at: DateTime<Utc>,
    pub status: RunStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RunStatus {
    PreparingWorkspace,
    BuildingPrompt,
    LaunchingAgentProcess,
    InitializingSession,
    StreamingTurn,
    Finishing,
    Succeeded,
    Failed,
    TimedOut,
    Stalled,
    CanceledByReconciliation,
}

// ─── §4.1.6 Live Session ───

/// State tracked while a coding-agent subprocess is running.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveSession {
    pub session_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub gemini_cli_pid: Option<String>,
    pub last_acp_event: Option<String>,
    pub last_acp_timestamp: Option<DateTime<Utc>>,
    pub last_acp_message: Option<String>,
    pub gemini_input_tokens: u64,
    pub gemini_output_tokens: u64,
    pub gemini_total_tokens: u64,
    pub last_reported_input_tokens: u64,
    pub last_reported_output_tokens: u64,
    pub last_reported_total_tokens: u64,
    pub turn_count: u32,
}

// ─── §4.1.7 Retry Entry ───

/// Scheduled retry state for an issue.
#[derive(Debug, Clone)]
pub struct RetryEntry {
    pub issue_id: String,
    pub identifier: String,
    pub attempt: u32,
    pub due_at_ms: u64,
    pub error: Option<String>,
}

// ─── §4.1.8 Orchestrator Runtime State ───

/// Running entry for a dispatched issue.
#[derive(Debug, Clone)]
pub struct RunningEntry {
    pub identifier: String,
    pub issue: Issue,
    pub session_id: Option<String>,
    pub gemini_cli_pid: Option<String>,
    pub last_acp_message: Option<String>,
    pub last_acp_event: Option<String>,
    pub last_acp_timestamp: Option<DateTime<Utc>>,
    pub gemini_input_tokens: u64,
    pub gemini_output_tokens: u64,
    pub gemini_total_tokens: u64,
    pub last_reported_input_tokens: u64,
    pub last_reported_output_tokens: u64,
    pub last_reported_total_tokens: u64,
    pub retry_attempt: Option<u32>,
    pub started_at: DateTime<Utc>,
    pub turn_count: u32,
    /// Handle to cancel the worker task.
    pub cancel_token: tokio_util::sync::CancellationToken,
}

/// Aggregate token and runtime totals.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeminiTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub seconds_running: f64,
}

/// Single authoritative in-memory state owned by the orchestrator.
pub struct OrchestratorState {
    pub poll_interval_ms: u64,
    pub max_concurrent_agents: usize,
    pub running: HashMap<String, RunningEntry>,
    pub claimed: HashSet<String>,
    pub retry_attempts: HashMap<String, RetryEntry>,
    pub completed: HashSet<String>,
    pub gemini_totals: GeminiTotals,
    pub gemini_rate_limits: Option<serde_json::Value>,
}

// ─── §4.2 Normalization Helpers ───

/// Sanitize an issue identifier for use as a workspace directory name.
/// Only `[A-Za-z0-9._-]` are allowed; everything else becomes `_`.
pub fn sanitize_workspace_key(identifier: &str) -> String {
    identifier
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Normalize issue state for comparison: trim + lowercase.
pub fn normalize_state(state: &str) -> String {
    state.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_workspace_key() {
        assert_eq!(sanitize_workspace_key("ABC-123"), "ABC-123");
        assert_eq!(sanitize_workspace_key("hello world"), "hello_world");
        assert_eq!(sanitize_workspace_key("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_workspace_key("test.file_name-1"), "test.file_name-1");
    }

    #[test]
    fn test_normalize_state() {
        assert_eq!(normalize_state("  Todo  "), "todo");
        assert_eq!(normalize_state("In Progress"), "in progress");
        assert_eq!(normalize_state("DONE"), "done");
    }
}
