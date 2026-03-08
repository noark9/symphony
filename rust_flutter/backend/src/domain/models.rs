use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Issue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub description: Option<String>,
    pub state: String,
    pub labels: Vec<String>,
    pub blocked_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    pub path: String,
    pub workspace_key: String,
    pub created_now: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunAttempt {
    pub attempt_number: u32,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub success: bool,
    pub logs: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LiveSession {
    pub session_id: String,
    pub pid: Option<u32>,
    pub started_at: DateTime<Utc>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryEntry {
    pub issue_id: String,
    pub retry_count: u32,
    pub next_retry_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_instantiation() {
        let issue = Issue {
            id: "1".to_string(),
            identifier: "ISSUE-1".to_string(),
            title: "Test Issue".to_string(),
            description: Some("Test Description".to_string()),
            state: "Open".to_string(),
            labels: vec!["bug".to_string()],
            blocked_by: None,
        };
        assert_eq!(issue.id, "1");
        assert_eq!(issue.labels.len(), 1);
    }

    #[test]
    fn test_workspace_instantiation() {
        let ws = Workspace {
            path: "/tmp/workspace".to_string(),
            workspace_key: "key-123".to_string(),
            created_now: Utc::now(),
        };
        assert_eq!(ws.path, "/tmp/workspace");
    }

    #[test]
    fn test_run_attempt_instantiation() {
        let attempt = RunAttempt {
            attempt_number: 1,
            started_at: Utc::now(),
            finished_at: None,
            success: false,
            logs: None,
        };
        assert_eq!(attempt.attempt_number, 1);
    }

    #[test]
    fn test_live_session_instantiation() {
        let session = LiveSession {
            session_id: "sess-1".to_string(),
            pid: Some(1234),
            started_at: Utc::now(),
            is_active: true,
        };
        assert!(session.is_active);
    }

    #[test]
    fn test_retry_entry_instantiation() {
        let retry = RetryEntry {
            issue_id: "1".to_string(),
            retry_count: 0,
            next_retry_at: Utc::now(),
        };
        assert_eq!(retry.retry_count, 0);
    }
}
