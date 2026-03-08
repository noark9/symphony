use serde::{Deserialize, Serialize};
use regex::Regex;
use std::env;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowConfig {
    #[serde(default)]
    pub tracker: TrackerConfig,
    #[serde(default)]
    pub polling: PollingConfig,
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub gemini: GeminiConfig,
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            tracker: TrackerConfig::default(),
            polling: PollingConfig::default(),
            workspace: WorkspaceConfig::default(),
            agent: AgentConfig::default(),
            gemini: GeminiConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackerConfig {
    pub kind: Option<String>,
    pub vault_path: Option<String>,
    pub issues_folder: Option<String>,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            kind: None,
            vault_path: None,
            issues_folder: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PollingConfig {
    #[serde(default = "default_interval_ms")]
    pub interval_ms: u64,
}

fn default_interval_ms() -> u64 {
    30000
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            interval_ms: default_interval_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceConfig {
    #[serde(default = "default_workspace_root")]
    pub root: String,
}

fn default_workspace_root() -> String {
    "~/.symphony/workspace".to_string()
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            root: default_workspace_root(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AgentConfig {
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GeminiConfig {
    pub api_key_env: Option<String>,
}

static ENV_REGEX: OnceLock<Regex> = OnceLock::new();

/// Expands `~` to the home directory and environment variables in the format `$VAR` or `${VAR}`.
pub fn expand_path(path: &str) -> String {
    let mut expanded = path.to_string();

    // Expand home directory `~`
    if expanded.starts_with('~') {
        if let Some(home_dir) = dirs::home_dir() {
            expanded = expanded.replacen('~', &home_dir.to_string_lossy(), 1);
        } else if let Ok(home) = env::var("HOME") {
            expanded = expanded.replacen('~', &home, 1);
        }
    }

    // Expand environment variables
    let re = ENV_REGEX.get_or_init(|| Regex::new(r"\$\{?([A-Za-z0-9_]+)\}?").unwrap());
    let result = re.replace_all(&expanded, |caps: &regex::Captures| {
        let var_name = &caps[1];
        env::var(var_name).unwrap_or_else(|_| "".to_string())
    });

    result.to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowDocument {
    pub config: WorkflowConfig,
    pub markdown_body: String,
}

pub fn parse_workflow_doc(content: &str) -> Result<WorkflowDocument, Box<dyn std::error::Error>> {
    let mut config = WorkflowConfig::default();
    let markdown_body: String;

    if content.starts_with("---\n") || content.starts_with("---\r\n") {
        let nl_len = if content.starts_with("---\r\n") { 5 } else { 4 };
        if let Some(end_idx) = content[nl_len..].find("\n---") {
            let yaml_content = &content[nl_len..nl_len + end_idx];

            // +4 for "\n---", and then maybe an extra "\r" or "\n" after it
            let after_dashes = nl_len + end_idx + 4;

            let mut md_content = &content[after_dashes..];
            // skip an extra \n or \r\n if present right after the closing ---
            if md_content.starts_with("\r\n") {
                md_content = &md_content[2..];
            } else if md_content.starts_with('\n') {
                md_content = &md_content[1..];
            }

            if !yaml_content.trim().is_empty() {
                let parsed_config: WorkflowConfig = serde_yaml::from_str(yaml_content)?;
                config = parsed_config;
            }
            markdown_body = md_content.to_string();
        } else {
            if content.starts_with("---\n---\n") {
                markdown_body = content[8..].to_string();
            } else if content.starts_with("---\r\n---\r\n") {
                markdown_body = content[10..].to_string();
            } else {
                markdown_body = content.to_string();
            }
        }
    } else {
        markdown_body = content.to_string();
    }

    // Apply path expansions for the paths defined in the config
    if let Some(ref mut vault_path) = config.tracker.vault_path {
        *vault_path = expand_path(vault_path);
    }
    if let Some(ref mut issues_folder) = config.tracker.issues_folder {
        *issues_folder = expand_path(issues_folder);
    }

    config.workspace.root = expand_path(&config.workspace.root);

    Ok(WorkflowDocument {
        config,
        markdown_body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_workflow_doc() {
        let yaml = r#"---
tracker:
  kind: obsidian
  vault_path: ~/my_vault
  issues_folder: ~/my_vault/issues
polling:
  interval_ms: 10000
workspace:
  root: /tmp/workspace
agent:
  model: gemini-pro
gemini:
  api_key_env: MY_API_KEY
---
# Workflow description
This is the workflow."#;

        let doc = parse_workflow_doc(yaml).unwrap();

        assert_eq!(doc.config.tracker.kind, Some("obsidian".to_string()));
        assert!(doc.config.tracker.vault_path.unwrap().ends_with("my_vault"));
        assert!(doc.config.tracker.issues_folder.unwrap().ends_with("my_vault/issues"));
        assert_eq!(doc.config.polling.interval_ms, 10000);
        assert_eq!(doc.config.workspace.root, "/tmp/workspace".to_string());
        assert_eq!(doc.config.agent.model, Some("gemini-pro".to_string()));
        assert_eq!(doc.config.gemini.api_key_env, Some("MY_API_KEY".to_string()));

        assert_eq!(doc.markdown_body, "# Workflow description\nThis is the workflow.");
    }

    #[test]
    fn test_parse_workflow_doc_defaults() {
        let yaml = "---\n---\nSome body content.";

        let doc = parse_workflow_doc(yaml).unwrap();

        assert_eq!(doc.config.polling.interval_ms, 30000);
        assert!(doc.config.workspace.root.ends_with(".symphony/workspace"));
        assert_eq!(doc.config.tracker.kind, None);
        assert_eq!(doc.markdown_body, "Some body content.");
    }

    #[test]
    fn test_expand_path_home() {
        let expanded = expand_path("~/test");
        assert!(!expanded.starts_with('~'));
        assert!(expanded.ends_with("/test"));
    }

    #[test]
    fn test_expand_path_env() {
        unsafe { env::set_var("TEST_VAR", "test_val"); }
        let expanded = expand_path("/path/to/${TEST_VAR}/end");
        assert_eq!(expanded, "/path/to/test_val/end");
    }
}
