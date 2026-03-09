import re

with open("rust_flutter/backend/src/orchestrator/engine.rs", "r") as f:
    lines = f.readlines()

new_lines = []
in_active_session = False
in_start_issue = False
in_handle_exit = False
in_poll = False
in_test_start_issue = False
in_test_poll = False
in_test_poll_retry = False
in_test_handle_exit1 = False
in_test_handle_exit2 = False
in_test_detect = False

for line in lines:
    if "pub struct ActiveSession {" in line:
        in_active_session = True
        new_lines.append(line)
        continue

    if in_active_session and "pub last_heartbeat: DateTime<Utc>," in line:
        new_lines.append(line)
        new_lines.append("    pub cancel_tx: Option<tokio::sync::mpsc::Sender<()>>,\n")
        in_active_session = False
        continue

    if "pub fn start_issue(&mut self, issue_id: &str) -> bool {" in line:
        in_start_issue = True
        new_lines.append("    pub fn start_issue(&mut self, issue_id: &str) -> Option<tokio::sync::mpsc::Receiver<()>> {\n")
        continue

    if in_start_issue and "return false; // Idempotency: already running" in line:
        new_lines.append("            return None; // Idempotency: already running\n")
        continue

    if in_start_issue and "return false;" in line and "try_claim" not in line:
        new_lines.append("                return None;\n")
        continue

    if in_start_issue and "let now = Utc::now();" in line:
        new_lines.append(line)
        new_lines.append("        let (tx, rx) = tokio::sync::mpsc::channel(1);\n")
        continue

    if in_start_issue and "last_heartbeat: now," in line:
        new_lines.append(line)
        new_lines.append("            cancel_tx: Some(tx),\n")
        continue

    if in_start_issue and line.strip() == "true":
        new_lines.append("        Some(rx)\n")
        in_start_issue = False
        continue

    if "pub fn handle_exit(&mut self, issue_id: &str, abnormal: bool) {" in line:
        in_handle_exit = True
        new_lines.append(line)
        continue

    if in_handle_exit and "self.running.remove(issue_id);" in line:
        new_lines.append("        if let Some(mut session) = self.running.remove(issue_id) {\n")
        new_lines.append("            if let Some(tx) = session.cancel_tx.take() {\n")
        new_lines.append("                let _ = tx.try_send(());\n")
        new_lines.append("            }\n")
        new_lines.append("        }\n")
        continue

    if in_handle_exit and "self.claimed.remove(issue_id);" in line:
        new_lines.append(line)
        in_handle_exit = False
        continue

    if "pub fn poll(&mut self, candidate_issues: Vec<Issue>) -> Vec<String> {" in line:
        in_poll = True
        new_lines.append("    pub fn poll(&mut self, candidate_issues: Vec<Issue>) -> Vec<(String, tokio::sync::mpsc::Receiver<()>)> {\n")
        continue

    if in_poll and "if self.start_issue(id) {" in line:
        new_lines.append("            if let Some(rx) = self.start_issue(id) {\n")
        continue

    if in_poll and "dispatchable.push(id.clone());" in line:
        new_lines.append("                dispatchable.push((id.clone(), rx));\n")
        continue

    if in_poll and "return dispatchable;" in line:
        new_lines.append(line)
        if "}" in line: # if it was the end of the fn
            pass
        continue

    if "fn test_start_issue() {" in line:
        in_test_start_issue = True
        new_lines.append(line)
        continue
    if in_test_start_issue and "assert!(engine.start_issue(\"issue-1\"));" in line:
        new_lines.append("        assert!(engine.start_issue(\"issue-1\").is_some());\n")
        continue
    if in_test_start_issue and "assert!(!engine.start_issue(\"issue-1\")); // idempotency" in line:
        new_lines.append("        assert!(engine.start_issue(\"issue-1\").is_none()); // idempotency\n")
        in_test_start_issue = False
        continue

    if "fn test_finish_issue() {" in line:
        new_lines.append(line)
        continue
    if "engine.start_issue(\"issue-1\");" in line and "fn test_finish_issue" not in "".join(new_lines[-5:]): # Just replace all standalone calls in tests
        new_lines.append("        let _ = engine.start_issue(\"issue-1\");\n")
        continue

    if "fn test_poll() {" in line:
        new_lines.append(line)
        in_test_poll = True
        continue
    if in_test_poll and "assert_eq!(dispatched[0], \"high-prio\");" in line:
        new_lines.append("        assert_eq!(dispatched[0].0, \"high-prio\");\n")
        continue
    if in_test_poll and "assert_eq!(dispatched[1], \"med-prio\");" in line:
        new_lines.append("        assert_eq!(dispatched[1].0, \"med-prio\");\n")
        continue
    if in_test_poll and "assert_eq!(dispatched3[0], \"low-prio\");" in line:
        new_lines.append("        assert_eq!(dispatched3[0].0, \"low-prio\");\n")
        in_test_poll = False
        continue

    new_lines.append(line)

with open("rust_flutter/backend/src/orchestrator/engine.rs", "w") as f:
    f.writelines(new_lines)
