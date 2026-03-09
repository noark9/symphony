import re

with open("rust_flutter/backend/src/orchestrator/engine.rs", "r") as f:
    content = f.read()

# Fix multiple cancel_tx declarations
content = re.sub(
    r"(    pub cancel_tx: Option<tokio::sync::mpsc::Sender<\(\)>>,\n)+",
    "    pub cancel_tx: Option<tokio::sync::mpsc::Sender<()>>,\n",
    content
)

with open("rust_flutter/backend/src/orchestrator/engine.rs", "w") as f:
    f.write(content)
