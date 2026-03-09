//! Tracker trait and module for issue tracker integration.
//!
//! Implements SPEC §11.

pub mod obsidian;

use crate::models::Issue;
use async_trait::async_trait;

/// Error types for tracker operations (§11.4).
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("unsupported tracker kind: {0}")]
    UnsupportedTrackerKind(String),

    #[error("missing tracker vault directory")]
    MissingTrackerVaultDir,

    #[error("vault directory not found: {0}")]
    VaultDirNotFound(String),

    #[error("markdown parse error in {file}: {reason}")]
    MarkdownParseError { file: String, reason: String },

    #[error("missing YAML frontmatter in {0}")]
    MissingYamlFrontmatter(String),

    #[error("file system error: {0}")]
    FileSystemError(String),
}

/// Tracker adapter trait (§11.1).
#[async_trait]
pub trait Tracker: Send + Sync {
    /// Fetch candidate issues in active states (§11.1.1).
    async fn fetch_candidate_issues(&self) -> Result<Vec<Issue>, TrackerError>;

    /// Fetch issues by specific state names (§11.1.2, for startup terminal cleanup).
    async fn fetch_issues_by_states(&self, state_names: &[String]) -> Result<Vec<Issue>, TrackerError>;

    /// Fetch current issue states for given IDs (§11.1.3, for reconciliation).
    async fn fetch_issue_states_by_ids(&self, issue_ids: &[String]) -> Result<Vec<Issue>, TrackerError>;
}
