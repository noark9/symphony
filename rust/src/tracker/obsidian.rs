//! Obsidian vault filesystem tracker adapter.
//!
//! Implements SPEC §11.2–§11.3.

use super::{Tracker, TrackerError};
use crate::models::{BlockerRef, Issue};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use tracing::warn;

/// Obsidian tracker that reads `.md` files from a local vault directory.
pub struct ObsidianTracker {
    vault_dir: PathBuf,
    active_states: Vec<String>,
    terminal_states: Vec<String>,
}

impl ObsidianTracker {
    pub fn new(vault_dir: String, active_states: Vec<String>, terminal_states: Vec<String>) -> Self {
        Self {
            vault_dir: PathBuf::from(vault_dir),
            active_states,
            terminal_states,
        }
    }

    /// Update configuration (for dynamic reload).
    pub fn update_config(
        &mut self,
        vault_dir: String,
        active_states: Vec<String>,
        terminal_states: Vec<String>,
    ) {
        self.vault_dir = PathBuf::from(vault_dir);
        self.active_states = active_states;
        self.terminal_states = terminal_states;
    }

    /// Scan vault directory for `.md` files and parse them as issues.
    fn scan_vault(&self, filter_states: &[String]) -> Result<Vec<Issue>, TrackerError> {
        if !self.vault_dir.exists() {
            return Err(TrackerError::VaultDirNotFound(
                self.vault_dir.display().to_string(),
            ));
        }

        let mut issues = Vec::new();

        let entries = std::fs::read_dir(&self.vault_dir).map_err(|e| {
            TrackerError::FileSystemError(format!(
                "failed to read vault dir {}: {}",
                self.vault_dir.display(),
                e
            ))
        })?;

        let normalized_filter: Vec<String> = filter_states
            .iter()
            .map(|s| s.trim().to_lowercase())
            .collect();

        for entry in entries {
            let entry = entry.map_err(|e| TrackerError::FileSystemError(e.to_string()))?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }

            match parse_obsidian_issue(&path) {
                Ok(issue) => {
                    let normalized_issue_state = issue.state.trim().to_lowercase();
                    if normalized_filter.contains(&normalized_issue_state) {
                        issues.push(issue);
                    }
                }
                Err(e) => {
                    warn!("skipping {}: {}", path.display(), e);
                    continue;
                }
            }
        }

        Ok(issues)
    }

    /// Fetch a single issue by its identifier (file basename).
    fn fetch_issue_by_id(&self, issue_id: &str) -> Result<Option<Issue>, TrackerError> {
        // Issue ID matches filename (without .md extension)
        let path = self.vault_dir.join(format!("{}.md", issue_id));
        if !path.exists() {
            return Ok(None);
        }

        match parse_obsidian_issue(&path) {
            Ok(issue) => Ok(Some(issue)),
            Err(e) => {
                warn!("failed to parse {}: {}", path.display(), e);
                Ok(None)
            }
        }
    }
}

#[async_trait]
impl Tracker for ObsidianTracker {
    async fn fetch_candidate_issues(&self) -> Result<Vec<Issue>, TrackerError> {
        self.scan_vault(&self.active_states)
    }

    async fn fetch_issues_by_states(&self, state_names: &[String]) -> Result<Vec<Issue>, TrackerError> {
        if state_names.is_empty() {
            return Ok(Vec::new());
        }
        self.scan_vault(state_names)
    }

    async fn fetch_issue_states_by_ids(&self, issue_ids: &[String]) -> Result<Vec<Issue>, TrackerError> {
        let mut results = Vec::new();
        for id in issue_ids {
            if let Some(issue) = self.fetch_issue_by_id(id)? {
                results.push(issue);
            }
        }
        Ok(results)
    }
}

/// Parse a single Obsidian Markdown file into an Issue (§11.2–§11.3).
fn parse_obsidian_issue(path: &Path) -> Result<Issue, TrackerError> {
    let filename = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| TrackerError::MarkdownParseError {
            file: path.display().to_string(),
            reason: "invalid filename".to_string(),
        })?;

    let content = std::fs::read_to_string(path).map_err(|e| TrackerError::FileSystemError(
        format!("failed to read {}: {}", path.display(), e),
    ))?;

    // Parse YAML frontmatter
    if !content.starts_with("---") {
        return Err(TrackerError::MissingYamlFrontmatter(
            path.display().to_string(),
        ));
    }

    let after_first = &content[3..];
    let closing_pos = after_first.find("\n---").ok_or_else(|| {
        TrackerError::MarkdownParseError {
            file: path.display().to_string(),
            reason: "no closing --- for YAML frontmatter".to_string(),
        }
    })?;

    let yaml_str = &after_first[..closing_pos];
    let yaml: serde_yaml::Value = serde_yaml::from_str(yaml_str).map_err(|e| {
        TrackerError::MarkdownParseError {
            file: path.display().to_string(),
            reason: e.to_string(),
        }
    })?;

    let get_str = |key: &str| -> Option<String> {
        yaml.get(key)?.as_str().map(|s| s.to_string())
    };

    let state = get_str("status")
        .or_else(|| get_str("state"))
        .unwrap_or_else(|| "Unknown".to_string());

    let title = get_str("title").unwrap_or_else(|| filename.to_string());

    let description = get_str("description").or_else(|| {
        // Use body content after frontmatter as description
        let body_start = closing_pos + 4;
        if body_start < after_first.len() {
            let body = after_first[body_start..].trim();
            if body.is_empty() { None } else { Some(body.to_string()) }
        } else {
            None
        }
    });

    let priority = yaml
        .get("priority")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);

    let labels = yaml
        .get("labels")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
                .collect()
        })
        .unwrap_or_default();

    let blocked_by = parse_blocked_by(&yaml);

    let created_at = yaml
        .get("created_at")
        .or_else(|| yaml.get("created"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<DateTime<Utc>>().ok());

    let updated_at = yaml
        .get("updated_at")
        .or_else(|| yaml.get("updated"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<DateTime<Utc>>().ok());

    let branch_name = get_str("branch");
    let url = get_str("url");
    let id = get_str("id").unwrap_or_else(|| filename.to_string());
    let identifier = get_str("identifier").unwrap_or_else(|| filename.to_string());

    Ok(Issue {
        id,
        identifier,
        title,
        description,
        priority,
        state,
        branch_name,
        url,
        labels,
        blocked_by,
        created_at,
        updated_at,
    })
}

/// Parse `blocked_by` from YAML frontmatter (§11.3).
/// Blockers are derived from inverse relations where relation type is `blocks`.
fn parse_blocked_by(yaml: &serde_yaml::Value) -> Vec<BlockerRef> {
    // Try direct blocked_by field
    if let Some(blocked) = yaml.get("blocked_by").and_then(|v| v.as_sequence()) {
        return blocked
            .iter()
            .map(|v| {
                if let Some(s) = v.as_str() {
                    BlockerRef {
                        id: Some(s.to_string()),
                        identifier: Some(s.to_string()),
                        state: None,
                    }
                } else {
                    BlockerRef {
                        id: v.get("id").and_then(|v| v.as_str()).map(String::from),
                        identifier: v.get("identifier").and_then(|v| v.as_str()).map(String::from),
                        state: v.get("state").and_then(|v| v.as_str()).map(String::from),
                    }
                }
            })
            .collect();
    }

    // Try relations with type=blocks (inverse)
    if let Some(relations) = yaml.get("relations").and_then(|v| v.as_sequence()) {
        return relations
            .iter()
            .filter(|rel| {
                rel.get("type")
                    .and_then(|v| v.as_str())
                    .map(|s| s == "blocks")
                    .unwrap_or(false)
            })
            .map(|rel| BlockerRef {
                id: rel.get("id").and_then(|v| v.as_str()).map(String::from),
                identifier: rel.get("identifier").and_then(|v| v.as_str()).map(String::from),
                state: rel.get("state").and_then(|v| v.as_str()).map(String::from),
            })
            .collect();
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_vault() -> TempDir {
        let dir = TempDir::new().unwrap();

        // Create a Todo issue
        fs::write(
            dir.path().join("TEST-1.md"),
            r#"---
id: test-1
identifier: TEST-1
title: Fix the bug
status: Todo
priority: 1
labels:
  - bug
  - urgent
---
This is the issue description.
"#,
        )
        .unwrap();

        // Create an In Progress issue
        fs::write(
            dir.path().join("TEST-2.md"),
            r#"---
id: test-2
identifier: TEST-2
title: Add feature
status: In Progress
priority: 2
---
Feature description.
"#,
        )
        .unwrap();

        // Create a Done issue
        fs::write(
            dir.path().join("TEST-3.md"),
            r#"---
id: test-3
identifier: TEST-3
title: Old task
status: Done
---
"#,
        )
        .unwrap();

        dir
    }

    #[tokio::test]
    async fn test_fetch_candidate_issues() {
        let vault = create_test_vault();
        let tracker = ObsidianTracker::new(
            vault.path().display().to_string(),
            vec!["Todo".to_string(), "In Progress".to_string()],
            vec!["Done".to_string()],
        );

        let issues = tracker.fetch_candidate_issues().await.unwrap();
        assert_eq!(issues.len(), 2);

        let ids: Vec<&str> = issues.iter().map(|i| i.identifier.as_str()).collect();
        assert!(ids.contains(&"TEST-1"));
        assert!(ids.contains(&"TEST-2"));
    }

    #[tokio::test]
    async fn test_fetch_issues_by_states() {
        let vault = create_test_vault();
        let tracker = ObsidianTracker::new(
            vault.path().display().to_string(),
            vec!["Todo".to_string()],
            vec!["Done".to_string()],
        );

        let done = tracker
            .fetch_issues_by_states(&["Done".to_string()])
            .await
            .unwrap();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].identifier, "TEST-3");
    }

    #[tokio::test]
    async fn test_fetch_empty_states() {
        let vault = create_test_vault();
        let tracker = ObsidianTracker::new(
            vault.path().display().to_string(),
            vec![],
            vec![],
        );

        let result = tracker.fetch_issues_by_states(&[]).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_issue_states_by_ids() {
        let vault = create_test_vault();
        let tracker = ObsidianTracker::new(
            vault.path().display().to_string(),
            vec!["Todo".to_string()],
            vec!["Done".to_string()],
        );

        let result = tracker
            .fetch_issue_states_by_ids(&["test-1".to_string()])
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].state, "Todo");
    }

    #[test]
    fn test_issue_normalization() {
        let vault = create_test_vault();
        let path = vault.path().join("TEST-1.md");
        let issue = parse_obsidian_issue(&path).unwrap();

        assert_eq!(issue.id, "test-1");
        assert_eq!(issue.identifier, "TEST-1");
        assert_eq!(issue.title, "Fix the bug");
        assert_eq!(issue.priority, Some(1));
        assert_eq!(issue.labels, vec!["bug", "urgent"]);
        assert!(issue.description.is_some());
    }
}
