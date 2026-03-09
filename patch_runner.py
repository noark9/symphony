import re

with open("rust_flutter/backend/src/execution/runner.rs", "r") as f:
    content = f.read()

# Update signature
content = re.sub(
    r"pub async fn run_agent\(workspace_path: &Path, gemini_command: &str, vault_dir: &Path\) -> std::io::Result<\(\)> \{",
    "pub async fn run_agent(workspace_path: &Path, gemini_command: &str, vault_dir: &Path, issue_id: String, mut cancel_rx: tokio::sync::mpsc::Receiver<()>, engine: std::sync::Arc<tokio::sync::Mutex<crate::orchestrator::engine::OrchestratorEngine>>, totals: std::sync::Arc<tokio::sync::Mutex<crate::api::state::GeminiTotals>>) -> std::io::Result<()> {",
    content
)

# Insert start time
content = re.sub(
    r"        let mut child = Command::new",
    "        let start_time = chrono::Utc::now();\n        let mut child = Command::new",
    content
)

# Modify stdout parsing loop to add heartbeat and token deltas
stdout_task_replacement = """        let stdout_task = tokio::spawn(async move {
            let mut last_prompt_tokens = 0;
            let mut last_candidate_tokens = 0;
            let mut last_requests = 0;

            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                engine.lock().await.heartbeat(&issue_id);

                if let Ok(json_val) = serde_json::from_str::<Value>(&line) {
                    // Try to extract tokens
                    let mut p_tok = None;
                    let mut c_tok = None;
                    let mut reqs = None;

                    let extract_tokens = |val: &Value| -> (Option<u64>, Option<u64>, Option<u64>) {
                        (
                            val.get("prompt_tokens").and_then(|v| v.as_u64()),
                            val.get("candidate_tokens").and_then(|v| v.as_u64()),
                            val.get("total_requests").and_then(|v| v.as_u64()),
                        )
                    };

                    let (p, c, r) = extract_tokens(&json_val);
                    if p.is_some() || c.is_some() || r.is_some() {
                        p_tok = p.or(p_tok); c_tok = c.or(c_tok); reqs = r.or(reqs);
                    } else if let Some(usage) = json_val.get("usage") {
                        let (p, c, r) = extract_tokens(usage);
                        p_tok = p.or(p_tok); c_tok = c.or(c_tok); reqs = r.or(reqs);
                    } else if let Some(total_usage) = json_val.get("total_token_usage") {
                        let (p, c, r) = extract_tokens(total_usage);
                        p_tok = p.or(p_tok); c_tok = c.or(c_tok); reqs = r.or(reqs);
                    }

                    if p_tok.is_some() || c_tok.is_some() || reqs.is_some() {
                        let p = p_tok.unwrap_or(last_prompt_tokens);
                        let c = c_tok.unwrap_or(last_candidate_tokens);
                        let r = reqs.unwrap_or(last_requests);

                        let mut t = totals.lock().await;
                        t.prompt_tokens += p.saturating_sub(last_prompt_tokens);
                        t.candidate_tokens += c.saturating_sub(last_candidate_tokens);
                        t.total_requests += r.saturating_sub(last_requests);

                        last_prompt_tokens = p;
                        last_candidate_tokens = c;
                        last_requests = r;
                    }

                    println!("Parsed JSON from stdout: {:?}", json_val);"""

content = re.sub(
    r"        let stdout_task = tokio::spawn\(async move \{\n            let mut reader = BufReader::new\(stdout\)\.lines\(\);\n            while let Ok\(Some\(line\)\) = reader\.next_line\(\)\.await \{\n                if let Ok\(json_val\) = serde_json::from_str::<Value>\(&line\) \{\n                    println!\(\"Parsed JSON from stdout: \{\:\?\}\", json_val\);",
    stdout_task_replacement,
    content
)

# Modify the child wait to select!
child_wait_replacement = """        tokio::select! {
            status = child.wait() => {
                let _ = status?;
            }
            _ = cancel_rx.recv() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }"""
content = re.sub(
    r"        let _status = child\.wait\(\)\.await\?;",
    child_wait_replacement,
    content
)

# Calculate total runtime seconds at the end
# Note: In the original, it is:
#         let _ = tokio::join!(stdout_task, stderr_task);
#         Ok(())
end_replacement = """        let _ = tokio::join!(stdout_task, stderr_task);
        let elapsed = chrono::Utc::now().signed_duration_since(start_time).num_seconds();
        if elapsed > 0 {
            // Need to lock again, but we don't have totals directly.
            // Wait, totals was moved into the tokio::spawn for stdout_task.
            // We need to clone totals before passing to stdout_task or clone it for the end.
"""
# We must correctly clone Arc inside run_agent. Let's fix this.

content = re.sub(
    r"        let stdout_task = tokio::spawn\(async move \{",
    "        let totals_clone = totals.clone();\n        let engine_clone = engine.clone();\n        let stdout_task = tokio::spawn(async move {\n            let totals = totals_clone;\n            let engine = engine_clone;",
    content
)

end_replacement2 = """        let _ = tokio::join!(stdout_task, stderr_task);
        let elapsed = chrono::Utc::now().signed_duration_since(start_time).num_seconds();
        if elapsed > 0 {
            totals.lock().await.total_runtime_seconds += elapsed as u64;
        }

        Ok(())"""

content = re.sub(
    r"        let _ = tokio::join!\(stdout_task, stderr_task\);\n\n        Ok\(\(\)\)",
    end_replacement2,
    content
)

with open("rust_flutter/backend/src/execution/runner.rs", "w") as f:
    f.write(content)
