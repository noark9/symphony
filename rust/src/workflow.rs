//! WORKFLOW.md loader — parses YAML front matter + Markdown body.
//!
//! Implements SPEC §5.1–§5.5.

use crate::models::WorkflowDefinition;
use std::path::Path;

/// Error types for workflow loading (§5.5).
#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("missing workflow file at {path}: {reason}")]
    MissingWorkflowFile { path: String, reason: String },

    #[error("workflow parse error: {0}")]
    WorkflowParseError(String),

    #[error("workflow front matter must decode to a map")]
    FrontMatterNotAMap,
}

/// Load and parse a `WORKFLOW.md` file.
///
/// Parsing rules (§5.2):
/// - If the file starts with `---`, parse lines until the next `---` as YAML front matter.
/// - Remaining lines become the prompt body.
/// - If front matter is absent, entire file is prompt body with empty config map.
/// - YAML front matter must decode to a map/object.
/// - Prompt body is trimmed before use.
pub fn load_workflow(path: &Path) -> Result<WorkflowDefinition, WorkflowError> {
    let content = std::fs::read_to_string(path).map_err(|e| WorkflowError::MissingWorkflowFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    parse_workflow(&content)
}

/// Parse workflow content from a string (testable without filesystem).
pub fn parse_workflow(content: &str) -> Result<WorkflowDefinition, WorkflowError> {
    let (config, prompt_template) = if content.starts_with("---") {
        // Find the closing `---`
        let after_first = &content[3..];
        let closing_pos = after_first
            .find("\n---")
            .ok_or_else(|| WorkflowError::WorkflowParseError(
                "front matter opened with --- but no closing --- found".to_string(),
            ))?;

        let yaml_str = &after_first[..closing_pos];
        let body_start = closing_pos + 4; // skip past "\n---"
        let body = if body_start < after_first.len() {
            after_first[body_start..].trim().to_string()
        } else {
            String::new()
        };

        let yaml_value: serde_yaml::Value = serde_yaml::from_str(yaml_str)
            .map_err(|e| WorkflowError::WorkflowParseError(e.to_string()))?;

        // Must be a map
        if !yaml_value.is_mapping() && !yaml_value.is_null() {
            return Err(WorkflowError::FrontMatterNotAMap);
        }

        let config = if yaml_value.is_null() {
            serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
        } else {
            yaml_value
        };

        (config, body)
    } else {
        (
            serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
            content.trim().to_string(),
        )
    };

    Ok(WorkflowDefinition {
        config,
        prompt_template,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_with_front_matter() {
        let content = r#"---
tracker:
  kind: obsidian
  vault_dir: /tmp/vault
polling:
  interval_ms: 5000
---
Hello {{ issue.title }}!
"#;
        let wf = parse_workflow(content).unwrap();
        assert!(wf.config.is_mapping());
        assert_eq!(wf.prompt_template, "Hello {{ issue.title }}!");
    }

    #[test]
    fn test_parse_no_front_matter() {
        let content = "Just a prompt body";
        let wf = parse_workflow(content).unwrap();
        assert!(wf.config.is_mapping());
        assert_eq!(wf.prompt_template, "Just a prompt body");
    }

    #[test]
    fn test_parse_non_map_front_matter() {
        let content = "---\n- list item\n---\nbody";
        let result = parse_workflow(content);
        assert!(matches!(result, Err(WorkflowError::FrontMatterNotAMap)));
    }

    #[test]
    fn test_parse_empty_front_matter() {
        let content = "---\n---\nbody text here";
        let wf = parse_workflow(content).unwrap();
        assert!(wf.config.is_mapping());
        assert_eq!(wf.prompt_template, "body text here");
    }
}
