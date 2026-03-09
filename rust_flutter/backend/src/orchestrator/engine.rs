use std::collections::{HashSet, HashMap};
use chrono::{DateTime, Utc};
use crate::domain::models::Issue;

#[derive(Debug)]
pub struct RetryInfo {
    pub attempt_count: u32,
    pub next_retry_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct ActiveSession {
    pub issue_id: String,
    pub started_at: DateTime<Utc>,
    pub last_heartbeat: DateTime<Utc>,
    pub cancel_tx: Option<tokio::sync::mpsc::Sender<()>>,
}

pub struct OrchestratorEngine {
    pub running: HashMap<String, ActiveSession>,
    pub claimed: HashSet<String>,
    pub retry_attempts: HashMap<String, RetryInfo>,

    pub max_concurrent_agents: usize,
    pub stall_timeout_ms: i64,
    pub max_retry_backoff_ms: i64,
}

impl OrchestratorEngine {
    pub fn new(max_concurrent_agents: usize, stall_timeout_ms: i64, max_retry_backoff_ms: i64) -> Self {
        Self {
            running: HashMap::new(),
            claimed: HashSet::new(),
            retry_attempts: HashMap::new(),
            max_concurrent_agents,
            stall_timeout_ms,
            max_retry_backoff_ms,
        }
    }
}

impl OrchestratorEngine {
    pub fn try_claim(&mut self, issue_id: &str) -> bool {
        if self.claimed.contains(issue_id) || self.running.contains_key(issue_id) {
            return false;
        }
        self.claimed.insert(issue_id.to_string());
        true
    }

    pub fn start_issue(&mut self, issue_id: &str) -> Option<tokio::sync::mpsc::Receiver<()>> {
        if self.running.contains_key(issue_id) {
            return None; // Idempotency: already running
        }

        if !self.claimed.contains(issue_id) {
            if !self.try_claim(issue_id) {
                return None;
            }
        }

        let now = Utc::now();
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        self.running.insert(issue_id.to_string(), ActiveSession {
            issue_id: issue_id.to_string(),
            started_at: now,
            last_heartbeat: now,
            cancel_tx: Some(tx),
        });

        Some(rx)
    }

    pub fn finish_issue(&mut self, issue_id: &str) {
        if let Some(mut session) = self.running.remove(issue_id) {
            if let Some(tx) = session.cancel_tx.take() {
                let _ = tx.try_send(());
            }
        }
        self.claimed.remove(issue_id);
        self.retry_attempts.remove(issue_id);
    }
}

impl OrchestratorEngine {
    pub fn heartbeat(&mut self, issue_id: &str) {
        if let Some(session) = self.running.get_mut(issue_id) {
            session.last_heartbeat = Utc::now();
        }
    }

    pub fn detect_stalls(&mut self) -> Vec<String> {
        let now = Utc::now();
        let timeout = self.stall_timeout_ms;
        let stalled_issues: Vec<String> = self.running.iter()
            .filter_map(|(id, session)| {
                let duration = now.signed_duration_since(session.last_heartbeat).num_milliseconds();
                if duration > timeout {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();

        stalled_issues
    }
}

impl OrchestratorEngine {
    pub fn handle_exit(&mut self, issue_id: &str, abnormal: bool) {
        if let Some(mut session) = self.running.remove(issue_id) {
            if let Some(tx) = session.cancel_tx.take() {
                let _ = tx.try_send(());
            }
        }
        self.claimed.remove(issue_id);

        let now = Utc::now();
        let retry_entry = self.retry_attempts.entry(issue_id.to_string()).or_insert(RetryInfo {
            attempt_count: 0,
            next_retry_at: now,
        });

        retry_entry.attempt_count += 1;

        let backoff_ms = if abnormal {
            let exponential_ms = 1000 * (2_i64.pow(retry_entry.attempt_count.min(30) as u32 - 1));
            std::cmp::min(exponential_ms, self.max_retry_backoff_ms)
        } else {
            1000
        };

        retry_entry.next_retry_at = now + chrono::Duration::milliseconds(backoff_ms);
    }
}

impl OrchestratorEngine {
    pub fn poll(&mut self, candidate_issues: Vec<Issue>) -> Vec<(String, tokio::sync::mpsc::Receiver<()>)> {
        // Reconcile stalls first
        let stalled = self.detect_stalls();
        for id in &stalled {
            self.handle_exit(id, true);
        }

        let now = Utc::now();
        let mut dispatchable = Vec::new();

        let mut available_slots = self.max_concurrent_agents.saturating_sub(self.running.len());
        if available_slots == 0 {
            return dispatchable;
        }

        // Filter and sort candidates
        let mut sorted_candidates = candidate_issues;

        // Very basic priority parsing for sorting (high/medium/low)
        sorted_candidates.sort_by(|a, b| {
            // First by priority (dummy mapping logic for sorting)
            let a_prio = if a.labels.contains(&"high".to_string()) { 3 } else if a.labels.contains(&"medium".to_string()) { 2 } else { 1 };
            let b_prio = if b.labels.contains(&"high".to_string()) { 3 } else if b.labels.contains(&"medium".to_string()) { 2 } else { 1 };

            b_prio.cmp(&a_prio).then(a.id.cmp(&b.id)) // secondary sort by id for stability
        });

        for issue in sorted_candidates {
            if available_slots == 0 {
                break;
            }

            let id = &issue.id;

            // Check if it's currently running or claimed
            if self.running.contains_key(id) || self.claimed.contains(id) {
                continue;
            }

            // Check if it's in backoff
            if let Some(retry) = self.retry_attempts.get(id) {
                if now < retry.next_retry_at {
                    continue;
                }
            }

            if let Some(rx) = self.start_issue(id) {
                dispatchable.push((id.clone(), rx));
                available_slots -= 1;
            }
        }

        dispatchable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_issue(id: &str, labels: Vec<&str>) -> Issue {
        Issue {
            id: id.to_string(),
            identifier: id.to_string(),
            title: id.to_string(),
            description: None,
            state: "open".to_string(),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            blocked_by: None,
        }
    }

    #[test]
    fn test_try_claim() {
        let mut engine = OrchestratorEngine::new(2, 5000, 30000);
        assert!(engine.try_claim("issue-1"));
        assert!(!engine.try_claim("issue-1")); // already claimed
    }

    #[test]
    fn test_start_issue() {
        let mut engine = OrchestratorEngine::new(2, 5000, 30000);
        assert!(engine.start_issue("issue-1").is_some());
        assert!(engine.running.contains_key("issue-1"));
        assert!(engine.claimed.contains("issue-1"));
        assert!(engine.start_issue("issue-1").is_none()); // idempotency
    }

    #[test]
    fn test_finish_issue() {
        let mut engine = OrchestratorEngine::new(2, 5000, 30000);
        let _ = engine.start_issue("issue-1");
        engine.finish_issue("issue-1");
        assert!(!engine.running.contains_key("issue-1"));
        assert!(!engine.claimed.contains("issue-1"));
    }

    #[test]
    fn test_detect_stalls() {
        let mut engine = OrchestratorEngine::new(2, 100, 30000);
        let _ = engine.start_issue("issue-1");

        if let Some(session) = engine.running.get_mut("issue-1") {
            session.last_heartbeat = Utc::now() - chrono::Duration::milliseconds(150);
        }

        let stalls = engine.detect_stalls();
        assert_eq!(stalls.len(), 1);
        assert_eq!(stalls[0], "issue-1");
    }

    #[test]
    fn test_handle_exit_normal() {
        let mut engine = OrchestratorEngine::new(2, 5000, 30000);
        let _ = engine.start_issue("issue-1");
        engine.handle_exit("issue-1", false); // normal exit

        let retry_info = engine.retry_attempts.get("issue-1").unwrap();
        assert_eq!(retry_info.attempt_count, 1);

        let now = Utc::now();
        let expected_next_retry = now + chrono::Duration::milliseconds(1000);
        // It should be roughly 1 second from now
        assert!((expected_next_retry - retry_info.next_retry_at).num_milliseconds().abs() < 50);
    }

    #[test]
    fn test_handle_exit_abnormal_backoff() {
        let mut engine = OrchestratorEngine::new(2, 5000, 30000);
        let _ = engine.start_issue("issue-1");
        engine.handle_exit("issue-1", true); // 1st failure

        let info1 = engine.retry_attempts.get("issue-1").unwrap();
        assert_eq!(info1.attempt_count, 1);
        let now = Utc::now();
        assert!((info1.next_retry_at - now).num_milliseconds() - 1000 < 50);

        let _ = engine.start_issue("issue-1");
        engine.handle_exit("issue-1", true); // 2nd failure

        let info2 = engine.retry_attempts.get("issue-1").unwrap();
        assert_eq!(info2.attempt_count, 2);
        let now2 = Utc::now();
        assert!((info2.next_retry_at - now2).num_milliseconds() - 2000 < 50);
    }

    #[test]
    fn test_poll() {
        let mut engine = OrchestratorEngine::new(2, 5000, 30000);

        let candidates = vec![
            create_issue("low-prio", vec!["low"]),
            create_issue("high-prio", vec!["high"]),
            create_issue("med-prio", vec!["medium"]),
        ];

        let dispatched = engine.poll(candidates);

        // Max concurrent is 2, so only 2 should be dispatched
        assert_eq!(dispatched.len(), 2);
        // "high-prio" should be first
        assert_eq!(dispatched[0].0, "high-prio");
        assert_eq!(dispatched[1].0, "med-prio");

        // Now try polling again with no available slots
        let candidates2 = vec![create_issue("low-prio", vec!["low"])];
        let dispatched2 = engine.poll(candidates2);
        assert_eq!(dispatched2.len(), 0); // Still running 2

        // Finish one, it should poll the next one
        engine.finish_issue("high-prio");
        let candidates3 = vec![create_issue("low-prio", vec!["low"])];
        let dispatched3 = engine.poll(candidates3);
        assert_eq!(dispatched3.len(), 1);
        assert_eq!(dispatched3[0].0, "low-prio");
    }

    #[test]
    fn test_poll_retry_backoff() {
        let mut engine = OrchestratorEngine::new(2, 5000, 30000);
        let _ = engine.start_issue("issue-1");
        engine.handle_exit("issue-1", false); // normal exit, retry in 1s

        let candidates = vec![create_issue("issue-1", vec![])];
        let dispatched = engine.poll(candidates);

        // Should not be dispatched because it's in backoff
        assert_eq!(dispatched.len(), 0);
    }
}
