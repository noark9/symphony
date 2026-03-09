import re

with open("rust_flutter/backend/src/main.rs", "r") as f:
    content = f.read()

replacement = """    // Orchestrator loop (mock implementation for now)
    let orchestrator_clone = orchestrator.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(workflow_config.polling.interval_ms)) => {
                    let _ = orchestrator_clone.lock().await.poll(vec![]);
                }
                _ = refresh_rx.recv() => {
                    println!("Received forced refresh signal.");
                    let _ = orchestrator_clone.lock().await.poll(vec![]);
                }
            }
        }
    });"""

content = re.sub(
    r"    // Orchestrator loop \(mock implementation for now\)\n    tokio::spawn\(async move \{\n        loop \{\n            tokio::select! \{\n                _ = tokio::time::sleep\(tokio::time::Duration::from_millis\(workflow_config\.polling\.interval_ms\)\) => \{\n                    // Regular poll\n                    // In a real implementation, this is where we'd fetch issues and call `engine\.poll\(candidates\)`\n                \}\n                _ = refresh_rx\.recv\(\) => \{\n                    println!\(\"Received forced refresh signal\.\"\);\n                    // In a real implementation, this would trigger an immediate fetch and poll\n                \}\n            \}\n        \}\n    \}\);",
    replacement,
    content
)

with open("rust_flutter/backend/src/main.rs", "w") as f:
    f.write(content)
