---
tracker:
  kind: obsidian
  vault_path: ~/symphony-vault
  issues_folder: ~/symphony-vault/issues
polling:
  interval_ms: 5000
workspace:
  root: ~/symphony-workspaces
hooks:
  after_create: |
    git clone --depth 1 https://github.com/openai/symphony .
    if command -v cargo >/dev/null 2>&1; then
      cd rust_flutter/backend && cargo check
    fi
    if command -v flutter >/dev/null 2>&1; then
      cd rust_flutter/frontend && flutter pub get
    fi
  before_remove: |
    echo "Workspace cleanup complete."
agent:
  max_concurrent_agents: 10
  model: "acp gemini --model gemini-2.5-pro --tools \"bash,obsidian_markdown_updater\""
---

You are working on an Obsidian tracker ticket `{{ issue.identifier }}`

{% if attempt %}
Continuation context:

- This is retry attempt #{{ attempt }} because the ticket is still in an active state.
- Resume from the current workspace state instead of restarting from scratch.
- Do not repeat already-completed investigation or validation unless needed for new code changes.
- Do not end the turn while the issue remains in an active state unless you are blocked by missing required permissions/secrets.
{% endif %}

Issue context:

Title: {{ issue.title }}
Description:
{{ issue.description }}

Labels:
{% for label in issue.labels %}
- {{ label }}
{% endfor %}

## Step 1: Reproduction and planning phase

1. Read the provided issue description carefully.
2. Formulate a plan in the workpad (create one if it doesn't exist).
3. Attempt to reproduce the issue locally or locate the exact files to modify.

## Step 2: Execution phase

1. Modify the necessary files in `rust_flutter/backend` for Rust code or `rust_flutter/frontend` for Flutter code.
2. Keep the workpad updated with your progress.
3. Validate your changes by running tests.
   - For backend: `cd rust_flutter/backend && cargo test`
   - For frontend: `cd rust_flutter/frontend && flutter test`
4. Once all validation passes, use the `obsidian_markdown_updater` tool to transition the issue state to a terminal state (e.g. `Done`) and append your execution summary.

## Guardrails

- All Rust code must go into `rust_flutter/backend`.
- All Flutter code must go into `rust_flutter/frontend`.
- Do not modify the architectural invariants of the Orchestrator Engine, Polling logic, Workspace Management, or Core Domain Model.
- Ensure any temporary testing code is reverted before finishing.
