import re

with open("rust_flutter/backend/src/execution/runner.rs", "r") as f:
    content = f.read()

# Update test_run_agent_json_stdout signature and add dummy parameters
test_replacement = """        let gemini_command = format!("{}", script_path.display());

        let engine = std::sync::Arc::new(tokio::sync::Mutex::new(crate::orchestrator::engine::OrchestratorEngine::new(2, 5000, 30000)));
        let totals = std::sync::Arc::new(tokio::sync::Mutex::new(crate::api::state::GeminiTotals {
            prompt_tokens: 0,
            candidate_tokens: 0,
            total_requests: 0,
            total_runtime_seconds: 0,
        }));
        let (_cancel_tx, cancel_rx) = tokio::sync::mpsc::channel(1);

        let result = AgentRunner::run_agent(workspace_path, &gemini_command, vault_dir.path(), "ISSUE-1".to_string(), cancel_rx, engine, totals).await;"""
content = re.sub(
    r"        let gemini_command = format!\(\"\{\}\", script_path\.display\(\)\);\n        let result = AgentRunner::run_agent\(workspace_path, &gemini_command, vault_dir\.path\(\)\)\.await;",
    test_replacement,
    content
)

# Add new tests
new_tests = """
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

        let result = AgentRunner::run_agent(workspace_path, &gemini_command, vault_dir.path(), "ISSUE-TOKENS".to_string(), cancel_rx, engine, totals.clone()).await;
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
        let result = AgentRunner::run_agent(workspace_path, &gemini_command, vault_dir.path(), "ISSUE-STALL".to_string(), cancel_rx, engine, totals).await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        // Should have completed much faster than the 10s sleep
        assert!(elapsed.as_millis() < 2000);
    }
}"""
content = re.sub(r"\}\n\Z", new_tests, content)

with open("rust_flutter/backend/src/execution/runner.rs", "w") as f:
    f.write(content)
