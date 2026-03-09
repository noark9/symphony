# Symphony Automation Service

Symphony is a long-running automation service. The backend is built in Rust.

## Project Setup

The Rust backend is located in the `backend` directory.
It is initialized as a standard Rust binary project and uses the following core dependencies:
- **tokio**: Async runtime
- **serde**: Serialization / Deserialization
- **chrono**: Date and time handling

## Domain Models

The core domain models are defined in `backend/src/domain/models.rs`. The structs implemented include:
- `Issue`: Represents a task/issue with fields like id, title, state, and labels.
- `Workspace`: Tracks the workspace for agent execution.
- `RunAttempt`: Represents the state of an agent execution attempt.
- `LiveSession`: Tracks active subprocess sessions.
- `RetryEntry`: Tracks retry scheduling for failed attempts.

## Running Checks and Tests

To verify that the code compiles correctly, run:

```bash
cd backend
cargo check
```

To run the unit tests (which include instantiation tests for the domain models), run:

```bash
cd backend
cargo test
```

## Configuration Loader

The Symphony backend includes a Configuration Loader that parses a `WORKFLOW.md` file containing YAML front matter.
This configuration governs how the orchestrator tracks issues, polls for updates, manages the workspace, and configures the agent.

### Example `WORKFLOW.md`

```yaml
---
tracker:
  kind: obsidian
  vault_path: ~/my_vault
  issues_folder: ~/my_vault/issues
polling:
  interval_ms: 10000
workspace:
  root: /tmp/workspace
agent:
  model: gemini-pro
gemini:
  api_key_env: MY_API_KEY
---

# Workflow Contract

This document describes the workflow.
```

The loader expands `~` to the user's home directory and `$VAR` or `${VAR}` to environment variables in path configurations.
If no values are specified, it applies fallback defaults, such as `30000` for `interval_ms` and `~/.symphony/workspace` for the workspace root.

### Testing the Configuration Loader

To run the unit tests specifically for the config parsing and default value fallbacks, use the following command:

```bash
cd backend
cargo test config::loader
```

## Obsidian Tracker Implementation

The Symphony backend integrates with Obsidian as a local task tracker. It parses markdown files within an Obsidian vault directory to extract issue metadata from YAML frontmatter and convert them into core `Issue` models.

The tracker implementation (`backend/src/tracker/obsidian.rs`) provides the `fetch_candidate_issues` function which:
- Scans a provided `vault_dir` path for `.md` files.
- Checks that the file contains valid YAML frontmatter.
- Extracts properties like `status`, `priority`, and `labels`.
- Filters the issues based on provided `active_states` (e.g., `["todo", "in-progress"]`).
- Uses the markdown file's name (without the `.md` extension) as the Issue `identifier` and `id`.
- Emits custom errors (`MissingYamlFrontmatter`, `MalformedYamlFrontmatter`, `FileSystemError`) without crashing the application when invalid files are encountered.

### Testing the Obsidian Tracker

You can manually verify the parsing behavior by creating a dummy local vault directory:

1. **Create a dummy vault:**
   ```bash
   mkdir -p /tmp/dummy_vault
   ```
2. **Add a sample issue markdown file:**
   ```bash
   cat << 'FILE_EOF' > /tmp/dummy_vault/ISSUE-100.md
   ---
   status: todo
   priority: high
   labels:
     - feature
     - backend
   ---

   This is a description of the dummy issue.
   FILE_EOF
   ```

To run the unit tests specifically for the Obsidian Tracker logic (which automatically sets up an isolated temporary vault), run the following command:

```bash
cd backend
cargo test tracker::obsidian
```

## Workspace Manager & ACP Agent Runner

The Symphony backend execution engine manages isolated per-issue directories and handles running agents via standard Agent Client Protocol (ACP) over stdio.

### Workspace Manager

The `WorkspaceManager` (`backend/src/execution/workspace.rs`) is responsible for managing isolated per-issue directories inside the configured workspace root (e.g., `~/.symphony/workspace`).
It sanitizes issue identifiers to allow only `[A-Za-z0-9._-]` characters, mapping a tracker issue ID like `ISSUE-123!` to a valid directory name like `ISSUE-123_`.

**Testing Path Sanitization & Creation:**
You can verify the path sanitization and workspace creation logic by running the unit tests:
```bash
cd backend
cargo test execution::workspace
```

### ACP Agent Runner

The `AgentRunner` (`backend/src/execution/runner.rs`) uses `tokio::process::Command` to spawn the agent.
It executes `bash -lc <gemini.command>`, strictly setting the subprocess working directory to the isolated workspace path provided by the Workspace Manager.

It implements line-delimited JSON-RPC over `stdio`, parsing standard output (`stdout`) as JSON (the ACP standard) while safely capturing standard error (`stderr`) strictly for logging.

**Testing the ACP Runner:**
The unit test `test_run_agent_json_stdout` sets up a mock subprocess that acts like an agent. The mock script outputs `{"jsonrpc": "2.0", "method": "test"}` to stdout and some debug info to stderr. You can verify it accurately parses the stdout JSON and handles the subprocess properly:
```bash
cd backend
cargo test execution::runner
```

## Orchestrator Engine

The Orchestrator Engine (`backend/src/orchestrator/engine.rs`) manages the concurrency, reliability, and execution state of issues as they are processed by the agents.

### Core Logic

- **State Tracking:** Tracks issues currently in `running` and `claimed` sets to enforce idempotency and avoid duplicate agent spawns.
- **Poll Loop:** Reconciles active runs, sorts candidate issues fetched from the tracker (based on priority and date), and dispatches up to the `max_concurrent_agents` limit.
- **Stall Detection:** Checks if any active session has exceeded the `gemini.stall_timeout_ms` threshold. Stalled sessions are automatically transitioned.
- **Retry Logic:**
  - Normal Exits: Retries cleanly after 1000ms.
  - Abnormal Exits: Applies an exponential backoff strategy, capped at a configured `max_retry_backoff_ms`.

### Testing the Orchestrator Engine

To run the unit tests specifically for the Orchestrator Engine (verifying state transitions, idempotency checks, backoff algorithms, and polling loop mechanics), use the following command:

```bash
cd backend
cargo test orchestrator::engine
```

## Running the HTTP API Server

The Symphony automation service optionally exposes an HTTP API using `axum` on the port defined in the `WORKFLOW.md` config or via the CLI.

To start the backend server locally, run:

```bash
cd backend
cargo run
```

You can optionally override the port using the `-p` or `--port` flag:

```bash
cargo run -- --port 8080
```

### API Endpoints

Once the server is running, you can interact with the following API endpoints using `curl` or any HTTP client.

**1. Get State:**
Returns JSON with current counts, running sessions, the retry queue, and Gemini token totals.

```bash
curl http://localhost:3000/api/v1/state
```

**2. Get Issue Status:**
Returns detailed session logs and status for a specific issue identifier.

```bash
curl http://localhost:3000/api/v1/ISSUE-123
```

**3. Trigger Refresh:**
Forces an immediate orchestrator tick to poll for new tasks.

```bash
curl -X POST http://localhost:3000/api/v1/refresh
```

The API supports CORS and is ready to be consumed by the Flutter Web frontend.

## Frontend Setup

The Symphony web dashboard is built using Flutter and is located in the `frontend` directory. It uses `flutter_bloc` for state management and polls the Rust backend's REST API.

### Installing Dependencies

Before running the dashboard, navigate to the `frontend` directory and install the required dependencies:

```bash
cd frontend
flutter pub get
```

### Running the Web Dashboard Locally

To run the Flutter dashboard in Chrome for development and testing:

```bash
cd frontend
flutter run -d chrome
```

### Verifying the Connection

1. Ensure the Rust backend is running (`cd backend && cargo run`). By default, it will start on `http://0.0.0.0:3000`.
2. Start the Flutter frontend (`flutter run -d chrome`).
3. Open the browser to the provided localhost URL (e.g., `http://localhost:port`).
4. You should see the Symphony Dashboard UI.
5. If the backend is running properly, the dashboard will auto-poll the `http://localhost:3000/api/v1/state` endpoint every 5 seconds and populate the overview cards and lists with the current state (counts, running sessions, retry queue, and Gemini token totals). You can also click the manual refresh icon in the app bar to force an immediate update.

## Workspace Hooks & Lifecycle Execution

The Symphony execution engine supports running custom shell commands (hooks) during the workspace lifecycle. These are configured in the `hooks` section of `WORKFLOW.md`.

### Configuration
```yaml
hooks:
  after_create: "./setup.sh"
  before_run: "echo 'Preparing to run'"
  after_run: "echo 'Run finished'"
  before_remove: "rm -rf tmp/"
  timeout_ms: 10000
```

### Hook Behavior
- **`after_create`** and **`before_run`**: The orchestration process will **abort immediately** if either of these hooks return a non-zero exit code or timeout.
- **`after_run`** and **`before_remove`**: Failures and timeouts from these hooks are logged but **ignored** (they will not crash the lifecycle or trigger retries).
- **`timeout_ms`**: Enforces a strict upper bound using Tokio's timeout utilities. The default is `30000` ms (30 seconds).

### Writing a Local Test Script
You can create simple test scripts in your workspace root to verify the hook failures.
For example, create a file named `fail.sh`:

```bash
#!/bin/bash
echo "This hook will fail"
exit 1
```

Make it executable (`chmod +x fail.sh`) and configure your `WORKFLOW.md` to trigger it:
```yaml
hooks:
  after_create: "./fail.sh"
```
When the orchestrator creates a workspace and triggers this hook, the operation will be aborted.

## Liquid Template Rendering

The Symphony backend uses the `liquid` crate to render templates, specifically for prompt templates defined in the `WORKFLOW.md` configuration.

The `prompt::renderer::render_prompt` function takes a template string, an `Issue` object, and an optional `attempt` integer. It uses `ParserBuilder::with_stdlib()` to parse the template.

### Context Variables

The following variables are injected into the template context:
- `issue`: An object containing the issue's properties:
  - `id`
  - `identifier`
  - `title`
  - `description`
  - `state`
  - `labels` (an array of strings)
  - `blocked_by`
- `attempt`: An integer representing the current run attempt number (only injected if provided).

### Strict Rendering

The template rendering is designed to fail strictly if a template references an unknown variable, an unknown index, or an unknown filter. This is a native behavior of the `liquid` parser in this configuration. For instance, if a template includes `{{ unknown_var }}` or `{{ issue.unknown_property }}`, the rendering will return an error.

### Testing Template Rendering

You can verify the strict rendering behavior and nested array iteration using the unit tests in the `prompt::renderer` module:

```bash
cd backend
cargo test prompt::renderer
```

This test suite includes:
- `test_render_success`: Verifies that a valid template renders correctly, including iterating over `issue.labels` using `{% for %}`.
- `test_render_unknown_variable`: Verifies that referencing an unknown property like `{{ issue.unknown }}` results in an error.
- `test_render_unknown_filter`: Verifies that using an unknown filter like `{{ issue.identifier | unknown_filter }}` results in an error.

## Tool Call Interception (Obsidian Updater)

The Symphony backend execution engine dynamically intercepts ACP tool calls from the Gemini CLI stream parser.

Specifically, it implements the `obsidian_markdown_updater` extension tool. When this tool is called by an agent, the backend intercepts it, updates the target Markdown file's YAML frontmatter with the specified `new_state`, and appends any provided `content_append` to the file body in the configured `vault_dir`.

The agent communicates with the backend via the standard ACP over `stdio` stream, and the backend returns a JSON-RPC response message with `success=true` or an error payload directly to the agent's `stdin`. Unsupported tool calls gracefully return an error payload without stalling the session.

### Testing Tool Interception

You can verify the tool call interception feature and markdown updating logic using the unit tests for the ACP runner:

```bash
cd backend
cargo test execution::runner
```

This test:
1. Creates a dummy vault directory and populates an issue markdown file.
2. Creates a mock agent script that echoes a JSON-RPC request for `obsidian_markdown_updater` to stdout.
3. Spawns the agent. The backend reads the request, successfully processes it, updates the markdown file, and sends the JSON-RPC tool response to the agent's stdin.
4. Verifies the markdown file was correctly modified (YAML updated, text appended) in the dummy vault.
