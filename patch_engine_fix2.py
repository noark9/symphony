import re

with open("rust_flutter/backend/src/orchestrator/engine.rs", "r") as f:
    content = f.read()

# Fix the duplicate loop push block
replacement = """            if let Some(rx) = self.start_issue(id) {
                dispatchable.push((id.clone(), rx));
                available_slots -= 1;
            }
        }"""
content = re.sub(
    r"            if let Some\(rx\) = self\.start_issue\(id\) \{\n                dispatchable\.push\(\(id\.clone\(\), rx\)\);\n                available_slots -= 1;\n            \}\n                dispatchable\.push\(id\.clone\(\)\);\n                available_slots -= 1;\n            \}\n        \}",
    replacement,
    content
)

with open("rust_flutter/backend/src/orchestrator/engine.rs", "w") as f:
    f.write(content)
