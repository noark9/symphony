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
