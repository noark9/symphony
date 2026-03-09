use liquid::ParserBuilder;
use crate::domain::models::Issue;

pub fn render_prompt(template_str: &str, issue: &Issue, attempt: Option<u32>) -> Result<String, liquid::Error> {
    let parser = ParserBuilder::with_stdlib()
        .build()?;

    let template = parser.parse(template_str)?;

    // We want unknown variables to trigger an error so we only inject 'attempt' if there's a need,
    // or we always inject attempt as some default if expected, but user said "optional attempt integer".
    // If the template expects {{ attempt }} and attempt is None, rendering will fail natively in strict mode.

    // So let's create our globals.
    let mut globals = liquid::object!({
        "issue": {
            "id": issue.id,
            "identifier": issue.identifier,
            "title": issue.title,
            "description": issue.description,
            "state": issue.state,
            "labels": issue.labels,
            "blocked_by": issue.blocked_by,
        }
    });

    if let Some(a) = attempt {
        globals.insert("attempt".into(), liquid::model::Value::scalar(a as i32));
    }

    template.render(&globals)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_issue() -> Issue {
        Issue {
            id: "1".to_string(),
            identifier: "ISSUE-1".to_string(),
            title: "Test Issue".to_string(),
            description: Some("Description".to_string()),
            state: "Open".to_string(),
            labels: vec!["bug".to_string(), "ui".to_string()],
            blocked_by: None,
        }
    }

    #[test]
    fn test_render_success() {
        let issue = dummy_issue();
        let tpl = "ID: {{issue.identifier}}, Labels: {% for l in issue.labels %}{{l}} {% endfor %}Attempt: {{attempt}}";
        let res = render_prompt(tpl, &issue, Some(2)).unwrap();
        assert_eq!(res, "ID: ISSUE-1, Labels: bug ui Attempt: 2");
    }

    #[test]
    fn test_render_unknown_variable() {
        let issue = dummy_issue();
        let tpl = "ID: {{issue.identifier}}, Unknown: {{issue.unknown}}";
        let res = render_prompt(tpl, &issue, None);
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("Unknown index") || err_msg.contains("Unknown variable"), "Error was: {}", err_msg);
    }

    #[test]
    fn test_render_unknown_filter() {
        let issue = dummy_issue();
        let tpl = "ID: {{issue.identifier | unknown_filter}}";
        let res = render_prompt(tpl, &issue, None);
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("Unknown filter"), "Error was: {}", err_msg);
    }
}
