import re

with open("rust_flutter/backend/src/orchestrator/engine.rs", "r") as f:
    content = f.read()

# Fix duplicate signature for start_issue
content = re.sub(
    r"pub fn start_issue\(pub fn start_issue\(&mut self, issue_id: &str\) -> Option<tokio::sync::mpsc::Receiver<\(\)>> \{mut self, issue_id: &str\) -> Option<tokio::sync::mpsc::Receiver<\(\)>> \{",
    "pub fn start_issue(&mut self, issue_id: &str) -> Option<tokio::sync::mpsc::Receiver<()>> {",
    content
)

# Fix duplicate signature for poll
content = re.sub(
    r"pub fn poll\(pub fn poll\(&mut self, candidate_issues: Vec<Issue>\) -> Vec<\(String, tokio::sync::mpsc::Receiver<\(\)>\)> \{mut self, candidate_issues: Vec<Issue>\) -> Vec<\(String, tokio::sync::mpsc::Receiver<\(\)>\)> \{",
    "pub fn poll(&mut self, candidate_issues: Vec<Issue>) -> Vec<(String, tokio::sync::mpsc::Receiver<()>)> {",
    content
)

# Remove the bad lines from detect_stalls and handle_exit that were accidentally added
content = re.sub(
    r"        let now = Utc::now\(\);\n        let \(tx, rx\) = tokio::sync::mpsc::channel\(1\);\n        let timeout = self\.stall_timeout_ms;",
    "        let now = Utc::now();\n        let timeout = self.stall_timeout_ms;",
    content
)

content = re.sub(
    r"        let now = Utc::now\(\);\n        let \(tx, rx\) = tokio::sync::mpsc::channel\(1\);\n        let retry_entry = self\.retry_attempts",
    "        let now = Utc::now();\n        let retry_entry = self.retry_attempts",
    content
)

content = re.sub(
    r"        let now = Utc::now\(\);\n        let \(tx, rx\) = tokio::sync::mpsc::channel\(1\);\n        let mut dispatchable",
    "        let now = Utc::now();\n        let mut dispatchable",
    content
)

with open("rust_flutter/backend/src/orchestrator/engine.rs", "w") as f:
    f.write(content)
