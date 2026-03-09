import re

with open("rust_flutter/backend/src/orchestrator/engine.rs", "r") as f:
    content = f.read()

# Add cancel_tx once
content = re.sub(
    r"pub last_heartbeat: DateTime<Utc>,",
    "pub last_heartbeat: DateTime<Utc>,\n    pub cancel_tx: Option<tokio::sync::mpsc::Sender<()>>,",
    content
)

# Modify start_issue
content = re.sub(
    r"pub fn start_issue\(&mut self, issue_id: &str\) -> bool \{",
    "pub fn start_issue(&mut self, issue_id: &str) -> Option<tokio::sync::mpsc::Receiver<()>> {",
    content
)
content = re.sub(
    r"return false; // Idempotency: already running",
    "return None; // Idempotency: already running",
    content
)
content = re.sub(
    r"if !self\.try_claim\(issue_id\) \{\n\s*return false;\n\s*\}",
    "if !self.try_claim(issue_id) {\n                return None;\n            }",
    content
)

# ActiveSession insert
start_issue_replacement = """        let now = Utc::now();
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        self.running.insert(issue_id.to_string(), ActiveSession {
            issue_id: issue_id.to_string(),
            started_at: now,
            last_heartbeat: now,
            cancel_tx: Some(tx),
        });

        Some(rx)"""
content = re.sub(
    r"        let now = Utc::now\(\);\n        self\.running\.insert\(issue_id\.to_string\(\), ActiveSession \{\n            issue_id: issue_id\.to_string\(\),\n            started_at: now,\n            last_heartbeat: now,\n        \}\);\n\n        true",
    start_issue_replacement,
    content
)

# handle_exit
handle_exit_replacement = """    pub fn handle_exit(&mut self, issue_id: &str, abnormal: bool) {
        if let Some(mut session) = self.running.remove(issue_id) {
            if let Some(tx) = session.cancel_tx.take() {
                let _ = tx.try_send(());
            }
        }
        self.claimed.remove(issue_id);"""
content = re.sub(
    r"    pub fn handle_exit\(&mut self, issue_id: &str, abnormal: bool\) \{\n        self\.running\.remove\(issue_id\);\n        self\.claimed\.remove\(issue_id\);",
    handle_exit_replacement,
    content
)

# poll signature
content = re.sub(
    r"pub fn poll\(&mut self, candidate_issues: Vec<Issue>\) -> Vec<String> \{",
    "pub fn poll(&mut self, candidate_issues: Vec<Issue>) -> Vec<(String, tokio::sync::mpsc::Receiver<()>)> {",
    content
)

# poll push loop
poll_dispatch_replacement = """            if let Some(rx) = self.start_issue(id) {
                dispatchable.push((id.clone(), rx));
                available_slots -= 1;
            }"""
content = re.sub(
    r"            if self\.start_issue\(id\) \{\n                dispatchable\.push\(id\.clone\(\)\);\n                available_slots -= 1;\n            \}",
    poll_dispatch_replacement,
    content
)

with open("rust_flutter/backend/src/orchestrator/engine.rs", "w") as f:
    f.write(content)
