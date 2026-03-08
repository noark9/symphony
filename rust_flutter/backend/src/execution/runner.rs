use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use serde_json::Value;

pub struct AgentRunner;

impl AgentRunner {
    pub async fn run_agent(workspace_path: &Path, gemini_command: &str) -> std::io::Result<()> {
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

        // Spawn a task to read stdout
        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Ok(json_val) = serde_json::from_str::<Value>(&line) {
                    println!("Parsed JSON from stdout: {:?}", json_val);
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

        // Create a dummy script that writes JSON to stdout and text to stderr
        let script_content = r#"#!/bin/bash
echo '{"jsonrpc": "2.0", "method": "test"}'
echo 'This is stderr log' >&2
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
        let result = AgentRunner::run_agent(workspace_path, &gemini_command).await;

        assert!(result.is_ok());
    }
}
