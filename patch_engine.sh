#!/bin/bash
sed -i 's/pub last_heartbeat: DateTime<Utc>,/pub last_heartbeat: DateTime<Utc>,\n    pub cancel_tx: Option<tokio::sync::mpsc::Sender<()>>,/' rust_flutter/backend/src/orchestrator/engine.rs

sed -i 's/pub fn start_issue(&mut self, issue_id: &str) -> bool {/pub fn start_issue(&mut self, issue_id: \&str) -> Option<tokio::sync::mpsc::Receiver<()>> {/' rust_flutter/backend/src/orchestrator/engine.rs
sed -i 's/return false; \/\/ Idempotency: already running/return None; \/\/ Idempotency: already running/' rust_flutter/backend/src/orchestrator/engine.rs
sed -i 's/return false;/return None;/' rust_flutter/backend/src/orchestrator/engine.rs

sed -i 's/        let now = Utc::now();/        let now = Utc::now();\n        let (tx, rx) = tokio::sync::mpsc::channel(1);/' rust_flutter/backend/src/orchestrator/engine.rs

sed -i 's/            last_heartbeat: now,/            last_heartbeat: now,\n            cancel_tx: Some(tx),/' rust_flutter/backend/src/orchestrator/engine.rs

sed -i 's/        true/        Some(rx)/' rust_flutter/backend/src/orchestrator/engine.rs

cat << 'INNER_EOF' > handle_exit_replacement.txt
    pub fn handle_exit(&mut self, issue_id: &str, abnormal: bool) {
        if let Some(mut session) = self.running.remove(issue_id) {
            if let Some(tx) = session.cancel_tx.take() {
                let _ = tx.try_send(());
            }
        }
        self.claimed.remove(issue_id);
INNER_EOF

sed -i '/pub fn handle_exit(&mut self, issue_id: &str, abnormal: bool) {/,/self.claimed.remove(issue_id);/c\
    pub fn handle_exit(&mut self, issue_id: &str, abnormal: bool) {\
        if let Some(mut session) = self.running.remove(issue_id) {\
            if let Some(tx) = session.cancel_tx.take() {\
                let _ = tx.try_send(());\
            }\
        }\
        self.claimed.remove(issue_id);' rust_flutter/backend/src/orchestrator/engine.rs

sed -i 's/pub fn poll(&mut self, candidate_issues: Vec<Issue>) -> Vec<String> {/pub fn poll(&mut self, candidate_issues: Vec<Issue>) -> Vec<(String, tokio::sync::mpsc::Receiver<()>)> {/' rust_flutter/backend/src/orchestrator/engine.rs

sed -i '/if self.start_issue(id) {/c\
            if let Some(rx) = self.start_issue(id) {\
                dispatchable.push((id.clone(), rx));\
                available_slots -= 1;\
            }' rust_flutter/backend/src/orchestrator/engine.rs
