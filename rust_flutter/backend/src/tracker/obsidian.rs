use serde::Deserialize;
use std::fmt;
use std::fs;
use std::path::Path;
use crate::domain::models::Issue;

#[derive(Debug, PartialEq)]
pub enum TrackerError {
    FileSystemError(String),
    MissingYamlFrontmatter(String),
    MalformedYamlFrontmatter(String),
}

impl fmt::Display for TrackerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrackerError::FileSystemError(msg) => write!(f, "File system error: {}", msg),
            TrackerError::MissingYamlFrontmatter(msg) => write!(f, "Missing YAML frontmatter: {}", msg),
            TrackerError::MalformedYamlFrontmatter(msg) => write!(f, "Malformed YAML frontmatter: {}", msg),
        }
    }
}

impl std::error::Error for TrackerError {}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Frontmatter {
    pub status: Option<String>,
    pub priority: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
}

pub fn fetch_candidate_issues(
    vault_dir: &Path,
    active_states: &[&str],
) -> Result<Vec<Issue>, TrackerError> {
    let mut issues = Vec::new();

    let entries = fs::read_dir(vault_dir)
        .map_err(|e| TrackerError::FileSystemError(e.to_string()))?;

    for entry in entries {
        let entry = entry.map_err(|e| TrackerError::FileSystemError(e.to_string()))?;
        let path = entry.path();

        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
            let filename = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();

            let content = fs::read_to_string(&path)
                .map_err(|e| TrackerError::FileSystemError(e.to_string()))?;

            if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
                return Err(TrackerError::MissingYamlFrontmatter(filename));
            }

            let end_of_frontmatter = content[4..]
                .find("\n---\n")
                .or_else(|| content[4..].find("\r\n---\r\n"));

            let end_idx = match end_of_frontmatter {
                Some(idx) => idx + 4,
                None => return Err(TrackerError::MissingYamlFrontmatter(filename)),
            };

            let frontmatter_str = &content[4..end_idx];

            let frontmatter: Frontmatter = serde_yaml::from_str(frontmatter_str)
                .map_err(|e| TrackerError::MalformedYamlFrontmatter(format!("{}: {}", filename, e)))?;

            let status = frontmatter.status.unwrap_or_default();

            if active_states.contains(&status.as_str()) {
                // Find content after frontmatter
                let content_start = end_idx + 5;
                let description = if content.len() > content_start {
                    content[content_start..].trim().to_string()
                } else {
                    String::new()
                };

                issues.push(Issue {
                    id: filename.clone(),
                    identifier: filename.clone(),
                    title: filename.clone(),
                    description: if description.is_empty() { None } else { Some(description) },
                    state: status,
                    labels: frontmatter.labels,
                    blocked_by: None,
                });
            }
        }
    }

    Ok(issues)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_fetch_candidate_issues_success() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("ISSUE-123.md");
        let mut file = File::create(&file_path).unwrap();
        writeln!(
            file,
            "---\nstatus: in-progress\npriority: high\nlabels:\n  - bug\n---\n\nThis is a bug."
        )
        .unwrap();

        let issues = fetch_candidate_issues(dir.path(), &["in-progress"]).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].identifier, "ISSUE-123");
        assert_eq!(issues[0].state, "in-progress");
        assert_eq!(issues[0].labels, vec!["bug"]);
        assert_eq!(issues[0].description.as_deref(), Some("This is a bug."));
    }

    #[test]
    fn test_fetch_candidate_issues_filter() {
        let dir = tempdir().unwrap();

        // Issue 1 (matches filter)
        let file_path1 = dir.path().join("ISSUE-1.md");
        let mut file1 = File::create(&file_path1).unwrap();
        writeln!(file1, "---\nstatus: todo\n---\nBody1").unwrap();

        // Issue 2 (does not match filter)
        let file_path2 = dir.path().join("ISSUE-2.md");
        let mut file2 = File::create(&file_path2).unwrap();
        writeln!(file2, "---\nstatus: done\n---\nBody2").unwrap();

        let issues = fetch_candidate_issues(dir.path(), &["todo"]).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].identifier, "ISSUE-1");
    }

    #[test]
    fn test_fetch_candidate_issues_missing_frontmatter() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("ISSUE-123.md");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "Just some text without frontmatter").unwrap();

        let err = fetch_candidate_issues(dir.path(), &["todo"]).unwrap_err();
        match err {
            TrackerError::MissingYamlFrontmatter(id) => assert_eq!(id, "ISSUE-123"),
            _ => panic!("Expected MissingYamlFrontmatter error"),
        }
    }

    #[test]
    fn test_fetch_candidate_issues_malformed_frontmatter() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("ISSUE-123.md");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "---\nstatus: [unclosed array\n---\n").unwrap();

        let err = fetch_candidate_issues(dir.path(), &["todo"]).unwrap_err();
        match err {
            TrackerError::MalformedYamlFrontmatter(msg) => assert!(msg.contains("ISSUE-123")),
            _ => panic!("Expected MalformedYamlFrontmatter error"),
        }
    }
}
