//! Agent runner: workspace + prompt + ACP client integration.
//!
//! Implements SPEC §10.7, §16.5.
//! Supports multiple agent kinds: Gemini ACP, Claude ACP, Gemini prompt mode.

use crate::acp::{
    self, AcpMessage, AgentEvent, AgentEventType, ObsidianMarkdownUpdaterInput,
};
use crate::models::{AgentKind, Issue};
use chrono::Utc;
use serde_json::Value;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Agent runner errors.
#[derive(Debug, thiserror::Error)]
pub enum AgentRunnerError {
    #[error("agent command not found: {0}")]
    AgentNotFound(String),

    #[error("invalid workspace cwd: {0}")]
    InvalidWorkspaceCwd(String),

    #[error("response timeout")]
    ResponseTimeout,

    #[error("turn timeout")]
    TurnTimeout,

    #[error("process exited unexpectedly: {0}")]
    ProcessExit(String),

    #[error("turn failed: {0}")]
    TurnFailed(String),

    #[error("turn cancelled")]
    TurnCancelled,

    #[error("startup failed: {0}")]
    StartupFailed(String),
}

/// Configuration for the agent runner.
#[derive(Debug, Clone)]
pub struct AgentRunnerConfig {
    pub kind: AgentKind,
    pub agent_command: String,
    pub turn_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub vault_dir: Option<String>,
    /// Whether to log agent stdout/stderr to the backend console.
    pub log_agent_output: bool,
}

/// Run a single agent session, dispatching to the appropriate protocol by kind.
pub async fn run_agent_session(
    config: &AgentRunnerConfig,
    workspace_path: &str,
    prompt: &str,
    issue: &Issue,
    cancel_token: CancellationToken,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), AgentRunnerError> {
    // Safety invariant: validate workspace cwd (§9.5)
    let ws_path = Path::new(workspace_path);
    if !ws_path.is_dir() {
        return Err(AgentRunnerError::InvalidWorkspaceCwd(
            workspace_path.to_string(),
        ));
    }

    match config.kind {
        AgentKind::GeminiAcp => {
            run_acp_session(config, workspace_path, prompt, issue, cancel_token, event_tx).await
        }
        AgentKind::ClaudePrompt | AgentKind::GeminiPrompt => {
            run_prompt_session(config, workspace_path, prompt, cancel_token, event_tx).await
        }
    }
}

// ─── ACP Protocol Session (Gemini ACP / Claude ACP) ───

/// Run an ACP (JSON-RPC over stdio) session (§10.7, §16.5).
/// Works identically for Gemini CLI and Claude Code — they share the same ACP protocol.
async fn run_acp_session(
    config: &AgentRunnerConfig,
    workspace_path: &str,
    prompt: &str,
    issue: &Issue,
    cancel_token: CancellationToken,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), AgentRunnerError> {
    // Launch agent process via bash -lc (§10.1)
    if config.log_agent_output {
        info!("[agent:cmd] {}", config.agent_command);
        let prompt_preview = if prompt.len() > 500 {
            format!("{}... (truncated, {} chars total)", &prompt[..500], prompt.len())
        } else {
            prompt.to_string()
        };
        info!("[agent:prompt] {}", prompt_preview);
    }
    let mut child = launch_agent_process(&config.agent_command, workspace_path)?;

    let pid = child.id().map(|p| p.to_string());
    let stdin = child.stdin.take().expect("stdin should be piped");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let _stderr = child.stderr.take();

    let mut writer = stdin;
    let mut reader = BufReader::new(stdout);

    // ACP Session startup handshake (§10.2)
    let init_msg = AcpMessage::request(
        1,
        "initialize",
        serde_json::json!({
            "protocolVersion": "2025-draft",
            "capabilities": {
                "tools": {
                    "supported": ["obsidian_markdown_updater"]
                }
            },
            "clientInfo": {
                "name": "symphony",
                "version": "0.1.0"
            }
        }),
    );

    send_message(&mut writer, &init_msg).await?;

    // Read init response with timeout
    let init_response = read_message_with_timeout(&mut reader, config.read_timeout_ms).await?;

    if init_response.error.is_some() {
        let err_msg = init_response
            .error
            .map(|e| e.message)
            .unwrap_or_else(|| "unknown error".to_string());
        return Err(AgentRunnerError::StartupFailed(err_msg));
    }

    // Send initialized notification
    let initialized = AcpMessage::notification("notifications/initialized", serde_json::json!({}));
    send_message(&mut writer, &initialized).await?;

    // Emit session_started event
    let _ = event_tx.send(AgentEvent {
        event: AgentEventType::SessionStarted,
        timestamp: Utc::now(),
        gemini_cli_pid: pid.clone(),
        usage: None,
        message: None,
        session_id: None,
        thread_id: None,
        turn_id: None,
    });

    // Send task prompt as a turn (§10.3)
    let turn_msg = AcpMessage::request(
        2,
        "tasks/send",
        serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "message": {
                "role": "user",
                "parts": [{"text": prompt}]
            }
        }),
    );
    send_message(&mut writer, &turn_msg).await?;

    // Stream turn processing with timeout (§10.3)
    let turn_timeout = Duration::from_millis(config.turn_timeout_ms);
    let turn_result = tokio::select! {
        result = stream_turn(&mut reader, &mut writer, &event_tx, pid.as_deref(), config, &cancel_token, issue) => result,
        _ = tokio::time::sleep(turn_timeout) => Err(AgentRunnerError::TurnTimeout),
        _ = cancel_token.cancelled() => Err(AgentRunnerError::TurnCancelled),
    };

    // Gracefully close session
    let cancel_msg = AcpMessage::request(3, "tasks/cancel", serde_json::json!({"id": "all"}));
    let _ = send_message(&mut writer, &cancel_msg).await;
    let _ = child.kill().await;

    turn_result
}

// ─── Prompt Mode Session (Gemini -p) ───

/// Run a prompt-mode agent session (one-shot, non-interactive).
///
/// Launches `<command> -p "<prompt>"`, reads stdout and stderr until process exit.
/// No ACP handshake, no JSON-RPC, no tool calls.
async fn run_prompt_session(
    config: &AgentRunnerConfig,
    workspace_path: &str,
    prompt: &str,
    cancel_token: CancellationToken,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), AgentRunnerError> {
    // Build the full command: <command> -p '<escaped_prompt>' [permission flags]
    let escaped_prompt = prompt.replace('\'', "'\\''");
    let permission_flag = match config.kind {
        AgentKind::ClaudePrompt => " --dangerously-skip-permissions",
        AgentKind::GeminiPrompt => " --yolo",
        _ => "",
    };
    let full_command = format!("{} -p '{}'{}", config.agent_command, escaped_prompt, permission_flag);

    info!(
        command = %config.agent_command,
        workspace = %workspace_path,
        "launching prompt-mode agent session"
    );

    if config.log_agent_output {
        // Log the full command (truncate very long prompts)
        let display_cmd = if full_command.len() > 500 {
            format!("{}... (truncated)", &full_command[..500])
        } else {
            full_command.clone()
        };
        info!("[agent:cmd] {}", display_cmd);
    }

    let mut child = launch_agent_process(&full_command, workspace_path)?;

    let pid = child.id().map(|p| p.to_string());
    let stdout = child.stdout.take().expect("stdout should be piped");
    let stderr = child.stderr.take().expect("stderr should be piped");

    // Emit session_started event
    let _ = event_tx.send(AgentEvent {
        event: AgentEventType::SessionStarted,
        timestamp: Utc::now(),
        gemini_cli_pid: pid.clone(),
        usage: None,
        message: None,
        session_id: None,
        thread_id: None,
        turn_id: None,
    });

    let turn_timeout = Duration::from_millis(config.turn_timeout_ms);

    // Spawn stderr reader — collect and emit lines as events
    let stderr_pid = pid.clone();
    let stderr_event_tx = event_tx.clone();
    let stderr_lines = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let stderr_lines_writer = stderr_lines.clone();
    let log_stderr = config.log_agent_output;
    let stderr_handle = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line_buf = String::new();
        loop {
            line_buf.clear();
            match reader.read_line(&mut line_buf).await {
                Ok(0) => break,
                Ok(_) => {
                    let line = line_buf.trim_end().to_string();
                    if !line.is_empty() {
                        if log_stderr {
                            info!("[agent:stderr] {}", line);
                        }
                        let _ = stderr_event_tx.send(AgentEvent {
                            event: AgentEventType::Notification,
                            timestamp: Utc::now(),
                            gemini_cli_pid: stderr_pid.clone(),
                            usage: None,
                            message: Some(format!("[stderr] {}", line)),
                            session_id: None,
                            thread_id: None,
                            turn_id: None,
                        });
                        stderr_lines_writer.lock().await.push(line);
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Read stdout line by line until EOF, with overall timeout
    let mut stdout_reader = BufReader::new(stdout);
    let mut line_buf = String::new();
    let mut output_lines = Vec::new();

    let read_result = tokio::select! {
        result = async {
            loop {
                line_buf.clear();
                match stdout_reader.read_line(&mut line_buf).await {
                    Ok(0) => break Ok(()), // EOF
                    Ok(_) => {
                        let line = line_buf.trim_end().to_string();
                        if !line.is_empty() {
                            if config.log_agent_output {
                                info!("[agent:stdout] {}", line);
                            }
                            let _ = event_tx.send(AgentEvent {
                                event: AgentEventType::Notification,
                                timestamp: Utc::now(),
                                gemini_cli_pid: pid.clone(),
                                usage: None,
                                message: Some(line.clone()),
                                session_id: None,
                                thread_id: None,
                                turn_id: None,
                            });
                            output_lines.push(line);
                        }
                    }
                    Err(e) => break Err(AgentRunnerError::ProcessExit(e.to_string())),
                }
            }
        } => result,
        _ = tokio::time::sleep(turn_timeout) => Err(AgentRunnerError::TurnTimeout),
        _ = cancel_token.cancelled() => Err(AgentRunnerError::TurnCancelled),
    };

    // Wait for stderr reader to finish
    let _ = stderr_handle.await;
    let stderr_output = stderr_lines.lock().await;

    // Build combined output summary for error messages
    let build_output_summary = |stdout_lines: &[String], stderr_lines: &[String]| -> String {
        let mut parts = Vec::new();
        // Last N lines of stderr (most useful for errors)
        if !stderr_lines.is_empty() {
            let tail: Vec<&str> = stderr_lines.iter().rev().take(10).rev().map(|s| s.as_str()).collect();
            parts.push(format!("stderr:\n{}", tail.join("\n")));
        }
        // Last N lines of stdout
        if !stdout_lines.is_empty() {
            let tail: Vec<&str> = stdout_lines.iter().rev().take(5).rev().map(|s| s.as_str()).collect();
            parts.push(format!("stdout:\n{}", tail.join("\n")));
        }
        if parts.is_empty() {
            "no output captured".to_string()
        } else {
            parts.join("\n---\n")
        }
    };

    // Wait for process to finish and check exit code
    match read_result {
        Ok(()) => {
            match child.wait().await {
                Ok(status) if status.success() => {
                    let _ = event_tx.send(AgentEvent {
                        event: AgentEventType::TurnCompleted,
                        timestamp: Utc::now(),
                        gemini_cli_pid: pid.clone(),
                        usage: None,
                        message: Some(format!("prompt session completed ({} output lines)", output_lines.len())),
                        session_id: None,
                        thread_id: None,
                        turn_id: None,
                    });
                    Ok(())
                }
                Ok(status) => {
                    let exit_code = status.code().unwrap_or(-1);
                    let output_summary = build_output_summary(&output_lines, &stderr_output);
                    let error_msg = format!(
                        "prompt session exited with code {}\n{}",
                        exit_code, output_summary
                    );
                    warn!("[agent:error] {}", error_msg);
                    let _ = event_tx.send(AgentEvent {
                        event: AgentEventType::TurnFailed,
                        timestamp: Utc::now(),
                        gemini_cli_pid: pid.clone(),
                        usage: None,
                        message: Some(error_msg.clone()),
                        session_id: None,
                        thread_id: None,
                        turn_id: None,
                    });
                    Err(AgentRunnerError::TurnFailed(error_msg))
                }
                Err(e) => {
                    let output_summary = build_output_summary(&output_lines, &stderr_output);
                    Err(AgentRunnerError::ProcessExit(format!("{}\n{}", e, output_summary)))
                }
            }
        }
        Err(e) => {
            let _ = child.kill().await;
            Err(e)
        }
    }
}

// ─── Shared Helpers ───

/// Launch the agent process with pseudo-TTY for line-buffered output.
///
/// Uses `script -q /dev/null` on macOS to allocate a pseudo-TTY,
/// which forces line-buffered stdout. Without this, many CLI tools
/// (e.g. Claude, Gemini) use full buffering when piped, causing
/// no output until the process exits.
fn launch_agent_process(
    command: &str,
    workspace_path: &str,
) -> Result<Child, AgentRunnerError> {
    // Use `script` to allocate a pseudo-TTY for line-buffered output
    // macOS: script -q /dev/null bash -lc "command"
    Command::new("script")
        .arg("-q")
        .arg("/dev/null")
        .arg("bash")
        .arg("-lc")
        .arg(command)
        .current_dir(workspace_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AgentRunnerError::AgentNotFound(format!("{}: {}", command, e)))
}

/// Stream turn messages until completion (§10.3).
async fn stream_turn(
    reader: &mut BufReader<tokio::process::ChildStdout>,
    writer: &mut tokio::process::ChildStdin,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    pid: Option<&str>,
    config: &AgentRunnerConfig,
    cancel_token: &CancellationToken,
    issue: &Issue,
) -> Result<(), AgentRunnerError> {
    let mut line_buf = String::new();

    loop {
        if cancel_token.is_cancelled() {
            return Err(AgentRunnerError::TurnCancelled);
        }

        line_buf.clear();
        let read_result = timeout(
            Duration::from_millis(config.turn_timeout_ms),
            reader.read_line(&mut line_buf),
        )
        .await;

        match read_result {
            Ok(Ok(0)) => {
                // EOF — process exited
                return Err(AgentRunnerError::ProcessExit("stdout closed".to_string()));
            }
            Ok(Ok(_)) => {
                let line = line_buf.trim();
                if line.is_empty() {
                    continue;
                }

                // Parse JSON-RPC message from stdout only (§10.3)
                match serde_json::from_str::<AcpMessage>(line) {
                    Ok(msg) => {
                        let result = handle_acp_message(
                            msg, writer, event_tx, pid, config, issue,
                        )
                        .await;
                        match result {
                            MessageResult::Continue => {}
                            MessageResult::TurnComplete => {
                                let _ = event_tx.send(AgentEvent {
                                    event: AgentEventType::TurnCompleted,
                                    timestamp: Utc::now(),
                                    gemini_cli_pid: pid.map(String::from),
                                    usage: None,
                                    message: None,
                                    session_id: None,
                                    thread_id: None,
                                    turn_id: None,
                                });
                                return Ok(());
                            }
                            MessageResult::Error(e) => {
                                let _ = event_tx.send(AgentEvent {
                                    event: AgentEventType::TurnFailed,
                                    timestamp: Utc::now(),
                                    gemini_cli_pid: pid.map(String::from),
                                    usage: None,
                                    message: Some(e.clone()),
                                    session_id: None,
                                    thread_id: None,
                                    turn_id: None,
                                });
                                return Err(AgentRunnerError::TurnFailed(e));
                            }
                        }
                    }
                    Err(_) => {
                        // Non-JSON line from stdout — log as diagnostics
                        debug!(line = line, "non-JSON stdout line");
                    }
                }
            }
            Ok(Err(e)) => {
                return Err(AgentRunnerError::ProcessExit(e.to_string()));
            }
            Err(_) => {
                return Err(AgentRunnerError::TurnTimeout);
            }
        }
    }
}

enum MessageResult {
    Continue,
    TurnComplete,
    Error(String),
}

/// Handle an incoming ACP message (§10.3–§10.5).
async fn handle_acp_message(
    msg: AcpMessage,
    writer: &mut tokio::process::ChildStdin,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    pid: Option<&str>,
    config: &AgentRunnerConfig,
    issue: &Issue,
) -> MessageResult {
    // Check for tool calls (§10.5)
    if msg.is_request() {
        if let Some(method) = &msg.method {
            if method == "tools/call" {
                return handle_tool_call(msg, writer, event_tx, pid, config, issue).await;
            }
        }
    }

    // Check for turn completion
    if msg.is_response() {
        if msg.error.is_some() {
            let err_msg = msg.error.map(|e| e.message).unwrap_or_default();
            return MessageResult::Error(err_msg);
        }
        // Check if this signals turn completion
        if let Some(result) = &msg.result {
            if result.get("status").and_then(|v| v.as_str()) == Some("completed") {
                return MessageResult::TurnComplete;
            }
        }
        return MessageResult::TurnComplete;
    }

    // Notifications — emit events
    if msg.is_notification() {
        if let Some(params) = &msg.params {
            // Extract token usage and emit event
            let usage = acp::extract_token_usage(params);
            let message = params
                .get("message")
                .or_else(|| params.get("text"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let _ = event_tx.send(AgentEvent {
                event: AgentEventType::Notification,
                timestamp: Utc::now(),
                gemini_cli_pid: pid.map(String::from),
                usage,
                message,
                session_id: None,
                thread_id: None,
                turn_id: None,
            });
        }
    }

    MessageResult::Continue
}

/// Handle a tool call from the agent (§10.5).
async fn handle_tool_call(
    msg: AcpMessage,
    writer: &mut tokio::process::ChildStdin,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    pid: Option<&str>,
    config: &AgentRunnerConfig,
    _issue: &Issue,
) -> MessageResult {
    let msg_id = msg.id.clone().unwrap_or(Value::Null);
    let params = msg.params.unwrap_or(Value::Null);

    let tool_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let tool_args = params.get("arguments").cloned().unwrap_or(Value::Null);

    match tool_name {
        "obsidian_markdown_updater" => {
            let result = if let Some(vault_dir) = &config.vault_dir {
                match serde_json::from_value::<ObsidianMarkdownUpdaterInput>(tool_args) {
                    Ok(input) => acp::execute_obsidian_markdown_updater(vault_dir, &input),
                    Err(e) => serde_json::json!({
                        "success": false,
                        "error": format!("invalid arguments: {}", e)
                    }),
                }
            } else {
                serde_json::json!({
                    "success": false,
                    "error": "vault_dir not configured"
                })
            };

            let response = AcpMessage::response(msg_id, result);
            if let Err(e) = send_message(writer, &response).await {
                warn!(error = %e, "failed to send tool response");
            }
        }
        _ => {
            // Unsupported tool — fail without stalling (§10.5)
            let _ = event_tx.send(AgentEvent {
                event: AgentEventType::UnsupportedToolCall,
                timestamp: Utc::now(),
                gemini_cli_pid: pid.map(String::from),
                usage: None,
                message: Some(format!("unsupported tool: {}", tool_name)),
                session_id: None,
                thread_id: None,
                turn_id: None,
            });

            let response = AcpMessage::error_response(
                msg_id,
                -32601,
                &format!("unsupported tool: {}", tool_name),
            );
            if let Err(e) = send_message(writer, &response).await {
                warn!(error = %e, "failed to send tool error response");
            }
        }
    }

    MessageResult::Continue
}

/// Send a JSON-RPC message to the agent process.
async fn send_message(
    writer: &mut tokio::process::ChildStdin,
    msg: &AcpMessage,
) -> Result<(), AgentRunnerError> {
    let json = serde_json::to_string(msg)
        .map_err(|e| AgentRunnerError::TurnFailed(format!("failed to serialize message: {}", e)))?;

    writer
        .write_all(json.as_bytes())
        .await
        .map_err(|e| AgentRunnerError::ProcessExit(format!("write failed: {}", e)))?;

    writer
        .write_all(b"\n")
        .await
        .map_err(|e| AgentRunnerError::ProcessExit(format!("write newline failed: {}", e)))?;

    writer
        .flush()
        .await
        .map_err(|e| AgentRunnerError::ProcessExit(format!("flush failed: {}", e)))?;

    Ok(())
}

/// Read a single message with timeout.
async fn read_message_with_timeout(
    reader: &mut BufReader<tokio::process::ChildStdout>,
    timeout_ms: u64,
) -> Result<AcpMessage, AgentRunnerError> {
    let mut line = String::new();
    let duration = Duration::from_millis(timeout_ms);

    loop {
        line.clear();
        let read_result = timeout(duration, reader.read_line(&mut line)).await;

        match read_result {
            Ok(Ok(0)) => return Err(AgentRunnerError::ProcessExit("stdout closed during handshake".to_string())),
            Ok(Ok(_)) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<AcpMessage>(trimmed) {
                    Ok(msg) => return Ok(msg),
                    Err(_) => {
                        debug!(line = trimmed, "non-JSON line during handshake, skipping");
                        continue;
                    }
                }
            }
            Ok(Err(e)) => return Err(AgentRunnerError::ProcessExit(e.to_string())),
            Err(_) => return Err(AgentRunnerError::ResponseTimeout),
        }
    }
}
