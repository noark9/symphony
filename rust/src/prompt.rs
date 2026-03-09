//! Prompt construction and template rendering.
//!
//! Implements SPEC §12. Uses Liquid-compatible template engine.

use crate::models::Issue;
use liquid::ParserBuilder;
use serde_json;

/// Errors from prompt rendering (§12.4).
#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    #[error("template parse error: {0}")]
    TemplateParse(String),

    #[error("template render error: {0}")]
    TemplateRender(String),
}

/// Default fallback prompt when prompt body is empty (§5.4).
const DEFAULT_PROMPT: &str = "You are working on an issue from Obsidian.";

/// Render the prompt template with issue and attempt context (§12.1–§12.3).
///
/// Template input variables:
/// - `issue` — normalized issue object
/// - `attempt` — integer or null (retry/continuation metadata)
pub fn render_prompt(
    template_str: &str,
    issue: &Issue,
    attempt: Option<u32>,
) -> Result<String, PromptError> {
    let template_str = if template_str.trim().is_empty() {
        DEFAULT_PROMPT
    } else {
        template_str
    };

    let parser = ParserBuilder::with_stdlib()
        .build()
        .map_err(|e| PromptError::TemplateParse(e.to_string()))?;

    let template = parser
        .parse(template_str)
        .map_err(|e| PromptError::TemplateParse(e.to_string()))?;

    // Convert issue to a liquid-compatible object via JSON → liquid::Object
    let issue_json = serde_json::to_value(issue)
        .map_err(|e| PromptError::TemplateRender(e.to_string()))?;
    let issue_obj = json_to_liquid_value(&issue_json);

    let attempt_val = match attempt {
        Some(n) => liquid_core::Value::scalar(n as i64),
        None => liquid_core::Value::Nil,
    };

    let globals = liquid::object!({
        "issue": issue_obj,
        "attempt": attempt_val,
    });

    template
        .render(&globals)
        .map_err(|e| PromptError::TemplateRender(e.to_string()))
}

/// Build a continuation turn prompt (for multi-turn sessions).
/// Continuation turns send guidance, not the full original prompt.
pub fn build_continuation_prompt(
    issue: &Issue,
    turn_number: u32,
    _max_turns: u32,
) -> String {
    format!(
        "Continue working on issue {}. This is turn {} of the current session. \
         The issue is still in an active state. Please continue from where you left off.",
        issue.identifier, turn_number
    )
}

/// Convert a serde_json::Value to a liquid_core::Value.
fn json_to_liquid_value(val: &serde_json::Value) -> liquid_core::Value {
    match val {
        serde_json::Value::Null => liquid_core::Value::Nil,
        serde_json::Value::Bool(b) => liquid_core::Value::scalar(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                liquid_core::Value::scalar(i)
            } else if let Some(f) = n.as_f64() {
                liquid_core::Value::scalar(f)
            } else {
                liquid_core::Value::Nil
            }
        }
        serde_json::Value::String(s) => liquid_core::Value::scalar(s.clone()),
        serde_json::Value::Array(arr) => {
            let items: Vec<liquid_core::Value> = arr.iter().map(json_to_liquid_value).collect();
            liquid_core::Value::Array(items)
        }
        serde_json::Value::Object(map) => {
            let obj: liquid::Object = map
                .iter()
                .map(|(k, v)| {
                    (k.clone().into(), json_to_liquid_value(v))
                })
                .collect();
            liquid_core::Value::Object(obj)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_issue() -> Issue {
        Issue {
            id: "test-id".to_string(),
            identifier: "TEST-1".to_string(),
            title: "Fix the bug".to_string(),
            description: Some("Something is broken".to_string()),
            priority: Some(1),
            state: "Todo".to_string(),
            branch_name: None,
            url: None,
            labels: vec!["bug".to_string()],
            blocked_by: vec![],
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn test_render_simple_prompt() {
        let template = "Work on: {{ issue.title }}";
        let result = render_prompt(template, &test_issue(), None).unwrap();
        assert_eq!(result, "Work on: Fix the bug");
    }

    #[test]
    fn test_render_with_attempt() {
        let template = "Issue: {{ issue.identifier }}, attempt: {{ attempt }}";
        let result = render_prompt(template, &test_issue(), Some(3)).unwrap();
        assert_eq!(result, "Issue: TEST-1, attempt: 3");
    }

    #[test]
    fn test_render_empty_template_uses_default() {
        let result = render_prompt("", &test_issue(), None).unwrap();
        assert_eq!(result, DEFAULT_PROMPT);
    }

    #[test]
    fn test_continuation_prompt() {
        let prompt = build_continuation_prompt(&test_issue(), 3, 10);
        assert!(prompt.contains("TEST-1"));
        assert!(prompt.contains("turn 3"));
    }
}
