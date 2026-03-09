import re

with open("rust_flutter/backend/src/orchestrator/engine.rs", "r") as f:
    content = f.read()

# Fix the unused tx,rx in tests that were mistakenly added in earlier failed attempts
content = re.sub(
    r"        let now = Utc::now\(\);\n        let \(tx, rx\) = tokio::sync::mpsc::channel\(1\);\n        let expected_next_retry",
    "        let now = Utc::now();\n        let expected_next_retry",
    content
)

content = re.sub(
    r"        let now = Utc::now\(\);\n        let \(tx, rx\) = tokio::sync::mpsc::channel\(1\);\n        assert!\(\(info1",
    "        let now = Utc::now();\n        assert!((info1",
    content
)

content = re.sub(
    r"    pub fn try_claim\(&mut self, issue_id: &str\) -> bool \{\n        if self\.claimed\.contains\(issue_id\) \|\| self\.running\.contains_key\(issue_id\) \{\n            return None;\n        \}\n        self\.claimed\.insert\(issue_id\.to_string\(\)\);\n        Some\(rx\)\n    \}",
    "    pub fn try_claim(&mut self, issue_id: &str) -> bool {\n        if self.claimed.contains(issue_id) || self.running.contains_key(issue_id) {\n            return false;\n        }\n        self.claimed.insert(issue_id.to_string());\n        true\n    }",
    content
)

content = re.sub(
    r"        engine\.start_issue\(\"issue-1\"\);\n        engine\.finish_issue\(\"issue-1\"\);",
    "        let _ = engine.start_issue(\"issue-1\");\n        engine.finish_issue(\"issue-1\");",
    content
)

with open("rust_flutter/backend/src/orchestrator/engine.rs", "w") as f:
    f.write(content)
