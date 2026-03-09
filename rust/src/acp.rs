//! ACP (Agent Client Protocol) types and message handling.
//!
//! Implements SPEC §10 — JSON-RPC over stdio.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// ACP JSON-RPC message sent to/from the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpMessage {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AcpError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl AcpMessage {
    /// Create a JSON-RPC 2.0 request.
    pub fn request(id: u64, method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(Value::Number(id.into())),
            method: Some(method.to_string()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    /// Create a JSON-RPC 2.0 response.
    pub fn response(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: None,
            params: None,
            result: Some(result),
            error: None,
        }
    }

    /// Create a JSON-RPC 2.0 error response.
    pub fn error_response(id: Value, code: i64, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: None,
            params: None,
            result: None,
            error: Some(AcpError {
                code,
                message: message.to_string(),
                data: None,
            }),
        }
    }

    /// Create a JSON-RPC 2.0 notification (no id).
    pub fn notification(method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: Some(method.to_string()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    /// Check if message is a request (has method and id).
    pub fn is_request(&self) -> bool {
        self.method.is_some() && self.id.is_some()
    }

    /// Check if message is a notification (has method, no id).
    pub fn is_notification(&self) -> bool {
        self.method.is_some() && self.id.is_none()
    }

    /// Check if message is a response (has result or error).
    pub fn is_response(&self) -> bool {
        self.result.is_some() || self.error.is_some()
    }
}

/// Events emitted from ACP client to orchestrator (§10.4).
#[derive(Debug, Clone, Serialize)]
pub struct AgentEvent {
    pub event: AgentEventType,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub gemini_cli_pid: Option<String>,
    pub usage: Option<TokenUsage>,
    pub message: Option<String>,
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventType {
    SessionStarted,
    StartupFailed,
    TurnCompleted,
    TurnFailed,
    TurnCancelled,
    TurnEndedWithError,
    UnsupportedToolCall,
    Notification,
    OtherMessage,
    Malformed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

/// Rate limit information from agent events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    pub requests_remaining: Option<u64>,
    pub requests_limit: Option<u64>,
    pub tokens_remaining: Option<u64>,
    pub tokens_limit: Option<u64>,
}

/// Extract token usage from an ACP event payload (§13.5).
/// Prefers absolute thread totals when available.
pub fn extract_token_usage(payload: &Value) -> Option<TokenUsage> {
    // Try thread/tokenUsage/updated style
    if let Some(usage) = payload
        .get("thread")
        .and_then(|t| t.get("tokenUsage"))
    {
        return Some(extract_usage_fields(usage));
    }

    // Try total_token_usage
    if let Some(usage) = payload.get("total_token_usage") {
        return Some(extract_usage_fields(usage));
    }

    // Try top-level usage
    if let Some(usage) = payload.get("usage") {
        return Some(extract_usage_fields(usage));
    }

    None
}

/// Extract rate limit info from payload.
pub fn extract_rate_limits(payload: &Value) -> Option<RateLimitInfo> {
    let rl = payload.get("rateLimit").or_else(|| payload.get("rate_limit"))?;
    Some(RateLimitInfo {
        requests_remaining: rl.get("requestsRemaining").or_else(|| rl.get("requests_remaining")).and_then(|v| v.as_u64()),
        requests_limit: rl.get("requestsLimit").or_else(|| rl.get("requests_limit")).and_then(|v| v.as_u64()),
        tokens_remaining: rl.get("tokensRemaining").or_else(|| rl.get("tokens_remaining")).and_then(|v| v.as_u64()),
        tokens_limit: rl.get("tokensLimit").or_else(|| rl.get("tokens_limit")).and_then(|v| v.as_u64()),
    })
}

fn extract_usage_fields(usage: &Value) -> TokenUsage {
    TokenUsage {
        input_tokens: usage
            .get("inputTokens")
            .or_else(|| usage.get("input_tokens"))
            .or_else(|| usage.get("promptTokens"))
            .and_then(|v| v.as_u64()),
        output_tokens: usage
            .get("outputTokens")
            .or_else(|| usage.get("output_tokens"))
            .or_else(|| usage.get("completionTokens"))
            .and_then(|v| v.as_u64()),
        total_tokens: usage
            .get("totalTokens")
            .or_else(|| usage.get("total_tokens"))
            .and_then(|v| v.as_u64()),
    }
}

/// Tool call for obsidian_markdown_updater extension (§10.5).
#[derive(Debug, Clone, Deserialize)]
pub struct ObsidianMarkdownUpdaterInput {
    pub issue_identifier: String,
    #[serde(default)]
    pub new_state: Option<String>,
    #[serde(default)]
    pub content_append: Option<String>,
}

/// Execute the obsidian_markdown_updater tool (§10.5).
pub fn execute_obsidian_markdown_updater(
    vault_dir: &str,
    input: &ObsidianMarkdownUpdaterInput,
) -> Value {
    let file_path = std::path::Path::new(vault_dir).join(format!("{}.md", input.issue_identifier));

    if !file_path.exists() {
        return serde_json::json!({
            "success": false,
            "error": format!("file not found: {}", file_path.display())
        });
    }

    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => {
            return serde_json::json!({
                "success": false,
                "error": format!("failed to read file: {}", e)
            });
        }
    };

    let mut new_content = content.clone();

    // Update status in YAML frontmatter
    if let Some(new_state) = &input.new_state {
        if new_content.starts_with("---") {
            if let Some(end_pos) = new_content[3..].find("\n---") {
                let yaml_section = &new_content[3..3 + end_pos];
                let updated_yaml = update_yaml_status(yaml_section, new_state);
                new_content = format!(
                    "---{}---{}",
                    updated_yaml,
                    &new_content[3 + end_pos + 4..]
                );
            }
        }
    }

    // Append content
    if let Some(append) = &input.content_append {
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(append);
        new_content.push('\n');
    }

    match std::fs::write(&file_path, &new_content) {
        Ok(_) => serde_json::json!({
            "success": true
        }),
        Err(e) => serde_json::json!({
            "success": false,
            "error": format!("failed to write file: {}", e)
        }),
    }
}

/// Update the status field in YAML frontmatter.
fn update_yaml_status(yaml_section: &str, new_state: &str) -> String {
    let mut lines: Vec<String> = yaml_section.lines().map(String::from).collect();
    let mut found = false;
    for line in &mut lines {
        if line.starts_with("status:") || line.starts_with("state:") {
            let key = if line.starts_with("status:") {
                "status"
            } else {
                "state"
            };
            *line = format!("{}: {}", key, new_state);
            found = true;
            break;
        }
    }
    if !found {
        lines.push(format!("status: {}", new_state));
    }

    let result = lines.join("\n");
    if result.starts_with('\n') {
        result
    } else {
        format!("\n{}\n", result)
    }
}
