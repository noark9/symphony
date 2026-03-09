//! Typed configuration layer with defaults and environment resolution.
//!
//! Implements SPEC §5.3 (front matter schema), §6 (config resolution).

use crate::models::{*, AgentKind};
use serde_yaml::Value;
use std::env;
use tracing::warn;

// ─── Defaults from §5.3 / §6.4 ───

const DEFAULT_ACTIVE_STATES: &[&str] = &["Todo", "In Progress"];
const DEFAULT_TERMINAL_STATES: &[&str] = &["Closed", "Cancelled", "Canceled", "Duplicate", "Done"];
const DEFAULT_POLL_INTERVAL_MS: u64 = 30_000;
const DEFAULT_MAX_CONCURRENT_AGENTS: usize = 10;
const DEFAULT_MAX_RETRY_BACKOFF_MS: u64 = 300_000;
const DEFAULT_GEMINI_COMMAND: &str = "gemini --experimental-acp";
const DEFAULT_GEMINI_TURN_TIMEOUT_MS: u64 = 3_600_000;
const DEFAULT_GEMINI_READ_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_GEMINI_STALL_TIMEOUT_MS: i64 = 300_000;
const DEFAULT_HOOK_TIMEOUT_MS: u64 = 60_000;

/// Errors from config validation (§6.3).
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("tracker.kind is required for dispatch")]
    MissingTrackerKind,

    #[error("unsupported tracker kind: {0}")]
    UnsupportedTrackerKind(String),

    #[error("tracker.vault_dir is required when tracker.kind is obsidian")]
    MissingTrackerVaultDir,

    #[error("gemini.command must not be empty")]
    MissingGeminiCommand,
}

/// Parse a `ServiceConfig` from workflow YAML config value.
pub fn parse_config(yaml: &Value) -> ServiceConfig {
    ServiceConfig {
        tracker: parse_tracker(yaml),
        polling: parse_polling(yaml),
        workspace: parse_workspace(yaml),
        hooks: parse_hooks(yaml),
        agent: parse_agent(yaml),
        gemini: parse_gemini(yaml),
        server: parse_server(yaml),
    }
}

/// Validate config for dispatch readiness (§6.3).
pub fn validate_config(config: &ServiceConfig) -> Result<(), ConfigError> {
    // tracker.kind required and supported
    match &config.tracker.kind {
        None => return Err(ConfigError::MissingTrackerKind),
        Some(kind) if kind != "obsidian" => {
            return Err(ConfigError::UnsupportedTrackerKind(kind.clone()));
        }
        _ => {}
    }

    // vault_dir required for obsidian
    if config.tracker.kind.as_deref() == Some("obsidian") && config.tracker.vault_dir.is_none() {
        return Err(ConfigError::MissingTrackerVaultDir);
    }

    // gemini.command must not be empty
    if config.gemini.command.trim().is_empty() {
        return Err(ConfigError::MissingGeminiCommand);
    }

    Ok(())
}

// ─── Section parsers ───

fn parse_tracker(yaml: &Value) -> TrackerConfig {
    let tracker = yaml.get("tracker");

    TrackerConfig {
        kind: get_str(tracker, "kind"),
        vault_dir: get_str(tracker, "vault_dir").map(|s| resolve_path(&s)),
        active_states: get_string_list(tracker, "active_states")
            .unwrap_or_else(|| DEFAULT_ACTIVE_STATES.iter().map(|s| s.to_string()).collect()),
        terminal_states: get_string_list(tracker, "terminal_states")
            .unwrap_or_else(|| DEFAULT_TERMINAL_STATES.iter().map(|s| s.to_string()).collect()),
    }
}

fn parse_polling(yaml: &Value) -> PollingConfig {
    let polling = yaml.get("polling");
    PollingConfig {
        interval_ms: get_u64(polling, "interval_ms").unwrap_or(DEFAULT_POLL_INTERVAL_MS),
    }
}

fn parse_workspace(yaml: &Value) -> WorkspaceConfig {
    let ws = yaml.get("workspace");
    let root = get_str(ws, "root")
        .map(|s| resolve_path(&s))
        .unwrap_or_else(default_workspace_root);

    WorkspaceConfig { root }
}

fn parse_hooks(yaml: &Value) -> HooksConfig {
    let hooks = yaml.get("hooks");
    HooksConfig {
        after_create: get_str(hooks, "after_create"),
        before_run: get_str(hooks, "before_run"),
        after_run: get_str(hooks, "after_run"),
        before_remove: get_str(hooks, "before_remove"),
        timeout_ms: get_u64(hooks, "timeout_ms").unwrap_or(DEFAULT_HOOK_TIMEOUT_MS),
    }
}

fn parse_agent(yaml: &Value) -> AgentConfig {
    let agent = yaml.get("agent");

    let max_concurrent_agents = get_u64(agent, "max_concurrent_agents")
        .map(|v| v as usize)
        .unwrap_or(DEFAULT_MAX_CONCURRENT_AGENTS);

    let max_retry_backoff_ms =
        get_u64(agent, "max_retry_backoff_ms").unwrap_or(DEFAULT_MAX_RETRY_BACKOFF_MS);

    let by_state = agent
        .and_then(|a| a.get("max_concurrent_agents_by_state"))
        .and_then(|v| v.as_mapping())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| {
                    let key = k.as_str()?.trim().to_lowercase();
                    let val = v.as_u64().filter(|&n| n > 0)? as usize;
                    Some((key, val))
                })
                .collect()
        })
        .unwrap_or_default();

    AgentConfig {
        max_concurrent_agents,
        max_retry_backoff_ms,
        max_concurrent_agents_by_state: by_state,
    }
}

fn parse_gemini(yaml: &Value) -> GeminiConfig {
    // Prefer `agent_runner:` section, fall back to `gemini:` for backward compat
    let section = yaml.get("agent_runner").or_else(|| yaml.get("gemini"));

    // Parse kind from the section (default: gemini_acp)
    let kind = get_str(section, "kind")
        .and_then(|s| AgentKind::from_str_loose(&s))
        .unwrap_or(AgentKind::GeminiAcp);

    // Default command depends on the kind
    let default_command = kind.default_command().to_string();

    GeminiConfig {
        kind,
        command: get_str(section, "command").unwrap_or(default_command),
        turn_timeout_ms: get_u64(section, "turn_timeout_ms")
            .unwrap_or(DEFAULT_GEMINI_TURN_TIMEOUT_MS),
        read_timeout_ms: get_u64(section, "read_timeout_ms")
            .unwrap_or(DEFAULT_GEMINI_READ_TIMEOUT_MS),
        stall_timeout_ms: get_i64(section, "stall_timeout_ms")
            .unwrap_or(DEFAULT_GEMINI_STALL_TIMEOUT_MS),
        log_agent_output: get_bool(section, "log_agent_output").unwrap_or(true),
    }
}

fn parse_server(yaml: &Value) -> ServerConfig {
    let server = yaml.get("server");
    ServerConfig {
        port: get_u64(server, "port").map(|v| v as u16),
    }
}

// ─── Helpers ───

fn get_str(parent: Option<&Value>, key: &str) -> Option<String> {
    parent?
        .get(key)?
        .as_str()
        .map(|s| resolve_env_value(s))
}

fn get_u64(parent: Option<&Value>, key: &str) -> Option<u64> {
    let val = parent?.get(key)?;
    // Support both integer and string representation
    val.as_u64().or_else(|| val.as_str()?.parse().ok())
}

fn get_i64(parent: Option<&Value>, key: &str) -> Option<i64> {
    let val = parent?.get(key)?;
    val.as_i64().or_else(|| val.as_str()?.parse().ok())
}

fn get_bool(parent: Option<&Value>, key: &str) -> Option<bool> {
    let val = parent?.get(key)?;
    val.as_bool().or_else(|| {
        val.as_str().and_then(|s| match s.to_lowercase().as_str() {
            "true" | "yes" | "1" => Some(true),
            "false" | "no" | "0" => Some(false),
            _ => None,
        })
    })
}

fn get_string_list(parent: Option<&Value>, key: &str) -> Option<Vec<String>> {
    let val = parent?.get(key)?;
    if let Some(seq) = val.as_sequence() {
        let list: Vec<String> = seq.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        if list.is_empty() { None } else { Some(list) }
    } else if let Some(s) = val.as_str() {
        // Support comma-separated string
        let list: Vec<String> = s.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if list.is_empty() { None } else { Some(list) }
    } else {
        None
    }
}

/// Resolve `$VAR_NAME` environment variable indirection (§6.1).
fn resolve_env_value(value: &str) -> String {
    if let Some(var_name) = value.strip_prefix('$') {
        env::var(var_name).unwrap_or_else(|_| {
            warn!("environment variable {} not set, using raw value", var_name);
            value.to_string()
        })
    } else {
        value.to_string()
    }
}

/// Resolve path: expand `~` and `$VAR` (§6.1).
fn resolve_path(value: &str) -> String {
    let expanded = resolve_env_value(value);
    if expanded.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            return home.join(&expanded[1..].trim_start_matches('/')).display().to_string();
        }
    }
    expanded
}

/// Default workspace root: `<system-temp>/symphony_workspaces`
fn default_workspace_root() -> String {
    let mut path = env::temp_dir();
    path.push("symphony_workspaces");
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_yaml() -> Value {
        serde_yaml::from_str(
            r#"
tracker:
  kind: obsidian
  vault_dir: /tmp/test_vault
  active_states:
    - Todo
    - In Progress
polling:
  interval_ms: 5000
workspace:
  root: /tmp/workspaces
agent:
  max_concurrent_agents: 5
  max_retry_backoff_ms: 60000
gemini:
  command: gemini-test
  turn_timeout_ms: 1800000
"#,
        )
        .unwrap()
    }

    #[test]
    fn test_parse_config() {
        let config = parse_config(&sample_yaml());
        assert_eq!(config.tracker.kind.as_deref(), Some("obsidian"));
        assert_eq!(config.tracker.vault_dir.as_deref(), Some("/tmp/test_vault"));
        assert_eq!(config.polling.interval_ms, 5000);
        assert_eq!(config.workspace.root, "/tmp/workspaces");
        assert_eq!(config.agent.max_concurrent_agents, 5);
        assert_eq!(config.gemini.command, "gemini-test");
        assert_eq!(config.gemini.turn_timeout_ms, 1_800_000);
        // Legacy gemini: section defaults to GeminiAcp kind
        assert_eq!(config.gemini.kind, AgentKind::GeminiAcp);
    }

    #[test]
    fn test_parse_config_defaults() {
        let yaml = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        let config = parse_config(&yaml);
        assert_eq!(config.polling.interval_ms, DEFAULT_POLL_INTERVAL_MS);
        assert_eq!(config.agent.max_concurrent_agents, DEFAULT_MAX_CONCURRENT_AGENTS);
        assert_eq!(config.gemini.command, DEFAULT_GEMINI_COMMAND);
        assert_eq!(config.hooks.timeout_ms, DEFAULT_HOOK_TIMEOUT_MS);
        assert_eq!(config.gemini.kind, AgentKind::GeminiAcp);
    }

    #[test]
    fn test_validate_config() {
        let config = parse_config(&sample_yaml());
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_missing_tracker_kind() {
        let yaml = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        let config = parse_config(&yaml);
        assert!(matches!(validate_config(&config), Err(ConfigError::MissingTrackerKind)));
    }

    #[test]
    fn test_comma_separated_states() {
        let yaml: Value = serde_yaml::from_str(
            r#"
tracker:
  kind: obsidian
  vault_dir: /tmp/v
  active_states: "Todo, In Progress, Review"
"#,
        )
        .unwrap();
        let config = parse_config(&yaml);
        assert_eq!(config.tracker.active_states, vec!["Todo", "In Progress", "Review"]);
    }

    #[test]
    fn test_agent_runner_section_claude() {
        let yaml: Value = serde_yaml::from_str(
            r#"
tracker:
  kind: obsidian
  vault_dir: /tmp/v
agent_runner:
  kind: claude_prompt
  turn_timeout_ms: 7200000
"#,
        )
        .unwrap();
        let config = parse_config(&yaml);
        assert_eq!(config.gemini.kind, AgentKind::ClaudePrompt);
        // Default command for claude_prompt is just "claude"
        assert_eq!(config.gemini.command, "claude");
        assert_eq!(config.gemini.turn_timeout_ms, 7_200_000);
    }

    #[test]
    fn test_agent_runner_section_gemini_prompt() {
        let yaml: Value = serde_yaml::from_str(
            r#"
tracker:
  kind: obsidian
  vault_dir: /tmp/v
agent_runner:
  kind: gemini_prompt
"#,
        )
        .unwrap();
        let config = parse_config(&yaml);
        assert_eq!(config.gemini.kind, AgentKind::GeminiPrompt);
        // Default command for prompt mode is just "gemini"
        assert_eq!(config.gemini.command, "gemini");
    }

    #[test]
    fn test_agent_runner_overrides_gemini_section() {
        let yaml: Value = serde_yaml::from_str(
            r#"
tracker:
  kind: obsidian
  vault_dir: /tmp/v
gemini:
  command: old-gemini
agent_runner:
  kind: claude_prompt
  command: claude -p
"#,
        )
        .unwrap();
        let config = parse_config(&yaml);
        // agent_runner takes precedence over gemini
        assert_eq!(config.gemini.kind, AgentKind::ClaudePrompt);
        assert_eq!(config.gemini.command, "claude -p");
    }

    #[test]
    fn test_fallback_to_gemini_section() {
        let yaml: Value = serde_yaml::from_str(
            r#"
tracker:
  kind: obsidian
  vault_dir: /tmp/v
gemini:
  command: gemini --experimental-acp
  turn_timeout_ms: 1800000
"#,
        )
        .unwrap();
        let config = parse_config(&yaml);
        // No agent_runner section → falls back to gemini section
        assert_eq!(config.gemini.kind, AgentKind::GeminiAcp);
        assert_eq!(config.gemini.command, "gemini --experimental-acp");
        assert_eq!(config.gemini.turn_timeout_ms, 1_800_000);
    }
}
