use std::path::{Path, PathBuf};
use std::fs;
use chrono::Utc;
use regex::Regex;

use crate::domain::models::{Issue, Workspace};

#[derive(Debug)]
pub enum WorkspaceError {
    IoError(std::io::Error),
}

impl From<std::io::Error> for WorkspaceError {
    fn from(err: std::io::Error) -> Self {
        WorkspaceError::IoError(err)
    }
}

pub struct WorkspaceManager {
    pub workspace_root: PathBuf,
}

impl WorkspaceManager {
    pub fn new<P: AsRef<Path>>(workspace_root: P) -> Self {
        Self {
            workspace_root: workspace_root.as_ref().to_path_buf(),
        }
    }

    pub fn sanitize_workspace_key(identifier: &str) -> String {
        let re = Regex::new(r"[^A-Za-z0-9._-]").unwrap();
        re.replace_all(identifier, "_").to_string()
    }

    pub fn create_workspace(&self, issue: &Issue) -> Result<Workspace, WorkspaceError> {
        let workspace_key = Self::sanitize_workspace_key(&issue.identifier);
        let path = self.workspace_root.join(&workspace_key);

        if !path.exists() {
            fs::create_dir_all(&path).map_err(WorkspaceError::IoError)?;
        }

        Ok(Workspace {
            path: path.to_string_lossy().to_string(),
            workspace_key,
            created_now: Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_sanitize_workspace_key() {
        assert_eq!(WorkspaceManager::sanitize_workspace_key("ISSUE-123"), "ISSUE-123");
        assert_eq!(WorkspaceManager::sanitize_workspace_key("issue/123!"), "issue_123_");
        assert_eq!(WorkspaceManager::sanitize_workspace_key("A..B_c-d"), "A..B_c-d");
        assert_eq!(WorkspaceManager::sanitize_workspace_key("spaces are bad"), "spaces_are_bad");
    }

    #[test]
    fn test_create_workspace() {
        let dir = tempdir().unwrap();
        let manager = WorkspaceManager::new(dir.path());

        let issue = Issue {
            id: "1".to_string(),
            identifier: "ISSUE-123!".to_string(),
            title: "Test".to_string(),
            description: None,
            state: "open".to_string(),
            labels: vec![],
            blocked_by: None,
        };

        let workspace = manager.create_workspace(&issue).unwrap();
        assert_eq!(workspace.workspace_key, "ISSUE-123_");

        let expected_path = dir.path().join("ISSUE-123_");
        assert_eq!(workspace.path, expected_path.to_string_lossy().to_string());
        assert!(expected_path.exists());
    }
}
