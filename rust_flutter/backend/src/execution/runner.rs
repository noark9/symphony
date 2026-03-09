use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use serde_json::{json, Value};
use crate::tracker::updater::update_obsidian_markdown;

pub struct AgentRunner;

impl AgentRunner {
    pub async fn run_agent(workspace_path: &Path, gemini_command: &str, rendered_prompt: String, vault_dir: &Path, issue_id: String, mut cancel_rx: tokio::sync::mpsc::Receiver<()>, engine: std::sync::Arc<tokio::sync::Mutex<crate::orchestrator::engine::OrchestratorEngine>>, totals: std::sync::Arc<tokio::sync::Mutex<crate::api::state::GeminiTotals>>) -> std::io::Result<()> {
        let start_time = chrono::Utc::now();
        let mut child = Command::new("bash")
            .arg("-lc")
            .arg(gemini_command)
            .current_dir(workspace_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take().expect("Failed to open stdout");
        let stderr = child.stderr.take().expect("Failed to open stderr");
        let mut stdin = child.stdin.take().expect("Failed to open stdin");

        if let Err(e) = stdin.write_all(format!("{}\n", rendered_prompt).as_bytes()).await {
            eprintln!("Failed to write initial prompt to stdin: {}", e);
        }

        let vault_dir_owned = vault_dir.to_path_buf();

        // Spawn a task to read stdout and write to stdin
        let totals_clone = totals.clone();
        let engine_clone = engine.clone();
        let stdout_task = tokio::spawn(async move {
            let totals = totals_clone;
            let engine = engine_clone;
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

                    println!("Parsed JSON from stdout: {:?}", json_val);

                    // Intercept ACP tool calls
                    if let Some(method) = json_val.get("method").and_then(|m| m.as_str()) {
                        let id = json_val.get("id").cloned();
                        if id.is_some() {
                            let mut response: Option<Value> = None;

                            if method == "obsidian_markdown_updater" {
                                if let Some(params) = json_val.get("params") {
                                    let issue_identifier = params.get("issue_identifier").and_then(|v| v.as_str());
                                    let new_state = params.get("new_state").and_then(|v| v.as_str());
                                    let content_append = params.get("content_append").and_then(|v| v.as_str());

                                    if let Some(identifier) = issue_identifier {
                                        match update_obsidian_markdown(&vault_dir_owned, identifier, new_state, content_append) {
                                            Ok(_) => {
                                                response = Some(json!({
                                                    "jsonrpc": "2.0",
                                                    "id": id,
                                                    "result": { "success": true }
                                                }));
                                            }
                                            Err(err) => {
                                                response = Some(json!({
                                                    "jsonrpc": "2.0",
                                                    "id": id,
                                                    "error": { "code": -32603, "message": err }
                                                }));
                                            }
                                        }
                                    } else {
                                        response = Some(json!({
                                            "jsonrpc": "2.0",
                                            "id": id,
                                            "error": { "code": -32602, "message": "Missing issue_identifier" }
                                        }));
                                    }
                                }
                            } else {
                                // Unsupported tool call
                                response = Some(json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "error": { "code": -32601, "message": format!("Method not found: {}", method) }
                                }));
                            }

                            if let Some(mut resp_json) = response {
                                // Add jsonrpc if missing
                                resp_json["jsonrpc"] = json!("2.0");
                                let resp_str = serde_json::to_string(&resp_json).unwrap();
                                if let Err(e) = stdin.write_all(format!("{}\n", resp_str).as_bytes()).await {
                                    eprintln!("Failed to write to stdin: {}", e);
                                }
                            }
                        }
                    }
                } else {
                    println!("Unparseable stdout line: {}", line);
                }
            }
        });

        // Spawn a task to read stderr
        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                eprintln!("Agent stderr: {}", line);
            }
        });

        let exit_result = tokio::select! {
            status = child.wait() => {
                status
            }
            _ = cancel_rx.recv() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "Agent was cancelled"))
            }
        };

        // Wait for tasks to finish
        let _ = tokio::join!(stdout_task, stderr_task);
        let elapsed = chrono::Utc::now().signed_duration_since(start_time).num_seconds();
        if elapsed > 0 {
            totals.lock().await.total_runtime_seconds += elapsed as u64;
        }

        match exit_result {
            Ok(status) if !status.success() => {
                return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("Agent exited with status: {}", status)));
            }
            Err(e) => return Err(e),
            _ => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[tokio::test]
    async fn test_run_agent_json_stdout() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        let vault_dir = tempdir().unwrap();

        // Add a mock obsidian markdown file
        let issue_file = vault_dir.path().join("ISSUE-1.md");
        fs::write(&issue_file, "---\nstatus: todo\n---\n\nBody.").unwrap();

        // Create a dummy script that writes JSON to stdout, reads from stdin, and writes text to stderr
        let script_content = r#"#!/bin/bash
# Send a tool call
echo '{"jsonrpc": "2.0", "id": 1, "method": "obsidian_markdown_updater", "params": {"issue_identifier": "ISSUE-1", "new_state": "in-progress", "content_append": "Updated via test"}}'
# Read response
read response
# Echo response to stderr so we can verify it (optional)
echo "Received: $response" >&2
# Send an unsupported tool call
echo '{"jsonrpc": "2.0", "id": 2, "method": "unknown_tool", "params": {}}'
read response2
echo "Received2: $response2" >&2
"#;
        let script_path = workspace_path.join("dummy_agent.sh");
        fs::write(&script_path, script_content).unwrap();

        // Make the script executable
        Command::new("chmod")
            .arg("+x")
            .arg(&script_path)
            .status()
            .await
            .unwrap();

        let gemini_command = format!("{}", script_path.display());

        let engine = std::sync::Arc::new(tokio::sync::Mutex::new(crate::orchestrator::engine::OrchestratorEngine::new(2, 5000, 30000)));
        let totals = std::sync::Arc::new(tokio::sync::Mutex::new(crate::api::state::GeminiTotals {
            prompt_tokens: 0,
            candidate_tokens: 0,
            total_requests: 0,
            total_runtime_seconds: 0,
        }));
        let (_cancel_tx, cancel_rx) = tokio::sync::mpsc::channel(1);

        let result = AgentRunner::run_agent(workspace_path, &gemini_command, "Test prompt".to_string(), vault_dir.path(), "ISSUE-1".to_string(), cancel_rx, engine, totals).await;

        assert!(result.is_ok());

        // Verify the markdown file was updated
        let updated_md = fs::read_to_string(&issue_file).unwrap();
        assert!(updated_md.contains("status: in-progress"));
        assert!(updated_md.contains("Updated via test"));
    }

    #[tokio::test]
    async fn test_run_agent_token_accounting() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        let vault_dir = tempdir().unwrap();

        // Create a dummy script that writes JSON with tokens to stdout
        let script_content = r#"#!/bin/bash
echo '{"total_token_usage": {"prompt_tokens": 100, "candidate_tokens": 50, "total_requests": 1}}'
sleep 0.1
echo '{"total_token_usage": {"prompt_tokens": 150, "candidate_tokens": 80, "total_requests": 2}}'
"#;
        let script_path = workspace_path.join("dummy_agent_tokens.sh");
        fs::write(&script_path, script_content).unwrap();

        Command::new("chmod").arg("+x").arg(&script_path).status().await.unwrap();

        let gemini_command = format!("{}", script_path.display());
        let engine = std::sync::Arc::new(tokio::sync::Mutex::new(crate::orchestrator::engine::OrchestratorEngine::new(2, 5000, 30000)));
        let totals = std::sync::Arc::new(tokio::sync::Mutex::new(crate::api::state::GeminiTotals {
            prompt_tokens: 0,
            candidate_tokens: 0,
            total_requests: 0,
            total_runtime_seconds: 0,
        }));
        let (_cancel_tx, cancel_rx) = tokio::sync::mpsc::channel(1);

        let result = AgentRunner::run_agent(workspace_path, &gemini_command, "Test prompt".to_string(), vault_dir.path(), "ISSUE-TOKENS".to_string(), cancel_rx, engine, totals.clone()).await;
        assert!(result.is_ok());

        let t = totals.lock().await;
        assert_eq!(t.prompt_tokens, 150);
        assert_eq!(t.candidate_tokens, 80);
        assert_eq!(t.total_requests, 2);
    }

    #[tokio::test]
    async fn test_orchestrator_stall_kills_agent() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        let vault_dir = tempdir().unwrap();

        // Create a dummy script that sleeps indefinitely
        let script_content = r#"#!/bin/bash
sleep 10
"#;
        let script_path = workspace_path.join("dummy_agent_sleep.sh");
        fs::write(&script_path, script_content).unwrap();
        Command::new("chmod").arg("+x").arg(&script_path).status().await.unwrap();

        let gemini_command = format!("{}", script_path.display());
        let engine = std::sync::Arc::new(tokio::sync::Mutex::new(crate::orchestrator::engine::OrchestratorEngine::new(2, 5000, 30000)));
        let totals = std::sync::Arc::new(tokio::sync::Mutex::new(crate::api::state::GeminiTotals {
            prompt_tokens: 0,
            candidate_tokens: 0,
            total_requests: 0,
            total_runtime_seconds: 0,
        }));
        let (cancel_tx, cancel_rx) = tokio::sync::mpsc::channel(1);

        // Send cancel signal shortly after starting
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            let _ = cancel_tx.try_send(());
        });

        let start = std::time::Instant::now();
        let result = AgentRunner::run_agent(workspace_path, &gemini_command, "Test prompt".to_string(), vault_dir.path(), "ISSUE-STALL".to_string(), cancel_rx, engine, totals).await;
        let elapsed = start.elapsed();

        assert!(result.is_err());
        // Should have completed much faster than the 10s sleep
        assert!(elapsed.as_millis() < 2000);
    }
}