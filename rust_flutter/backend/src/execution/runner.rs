use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use serde_json::{json, Value};
use crate::tracker::updater::update_obsidian_markdown;

pub struct AgentRunner;

impl AgentRunner {
    pub async fn run_agent(workspace_path: &Path, gemini_command: &str, vault_dir: &Path) -> std::io::Result<()> {
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

        let vault_dir_owned = vault_dir.to_path_buf();

        // Spawn a task to read stdout and write to stdin
        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Ok(json_val) = serde_json::from_str::<Value>(&line) {
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

        let _status = child.wait().await?;

        // Wait for tasks to finish
        let _ = tokio::join!(stdout_task, stderr_task);

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
        let result = AgentRunner::run_agent(workspace_path, &gemini_command, vault_dir.path()).await;

        assert!(result.is_ok());

        // Verify the markdown file was updated
        let updated_md = fs::read_to_string(&issue_file).unwrap();
        assert!(updated_md.contains("status: in-progress"));
        assert!(updated_md.contains("Updated via test"));
    }
}
