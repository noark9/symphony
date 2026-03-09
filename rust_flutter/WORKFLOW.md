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
    git clone --depth 1 <your_git_repository_url_here> .
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
3. If this is a bug, write a targeted failing test or reproduce it locally first before fixing.
4. If this is a feature, outline the architecture changes needed across `rust_flutter/backend` (Rust) and `rust_flutter/frontend` (Flutter).
5. Attempt to locate the exact files to modify.

## PR feedback sweep protocol (required)

When a ticket has an attached PR, run this protocol before moving to `Human Review`:

1. Identify the PR number from issue links/attachments.
2. Gather feedback from all channels:
   - Top-level PR comments (`gh pr view --comments`).
   - Inline review comments (`gh api repos/<owner>/<repo>/pulls/<pr>/comments`).
   - Review summaries/states (`gh pr view --json reviews`).
3. Treat every actionable reviewer comment (human or bot), including inline review comments, as blocking until one of these is true:
   - code/test/docs updated to address it, or
   - explicit, justified pushback reply is posted on that thread.
4. Update the workpad plan/checklist to include each feedback item and its resolution status.
5. Re-run validation after feedback-driven changes and push updates.
6. Repeat this sweep until there are no outstanding actionable comments.

## Blocked-access escape hatch (required behavior)

Use this only when completion is blocked by missing required tools or missing auth/permissions that cannot be resolved in-session.

- GitHub is **not** a valid blocker by default. Always try fallback strategies first.
- Do not move to `Human Review` for GitHub access/auth until all fallback strategies have been attempted and documented in the workpad.
- If a non-GitHub required tool is missing, or required non-GitHub auth is unavailable, move the ticket to `Human Review` using `obsidian_markdown_updater` with a short blocker brief in the workpad that includes:
  - what is missing,
  - why it blocks required acceptance/validation,
  - exact human action needed to unblock.
- Keep the brief concise and action-oriented.

## Step 2: Execution phase (Todo -> In Progress -> Human Review)

1. Determine current repo state (`branch`, `git status`, `HEAD`).
2. If current issue state is `Todo`, move it to `In Progress` using `obsidian_markdown_updater`; otherwise leave the current state unchanged.
3. Load the existing workpad comment and treat it as the active execution checklist. Edit it liberally whenever reality changes.
4. Implement against the hierarchical TODOs and keep the comment current:
    - Check off completed items.
    - Add newly discovered items in the appropriate section.
    - Update the workpad immediately after each meaningful milestone.
5. Run validation/tests required for the scope.
    - For backend logic: `cd rust_flutter/backend && cargo test`
    - For frontend logic: `cd rust_flutter/frontend && flutter test`
    - You may make temporary local proof edits to validate assumptions but revert every temporary proof edit before commit/push.
6. Before every `git push` attempt, run the required validation for your scope and confirm it passes.
7. Merge latest `origin/main` into branch, resolve conflicts, and rerun checks.
8. Update the workpad with final checklist status and validation notes. Add a short `### Confusions` section at the bottom when any part of task execution was unclear/confusing.
9. Before moving to `Human Review`, poll PR feedback and checks:
    - Run the full PR feedback sweep protocol.
    - Confirm PR checks are passing (green) after the latest changes.
    - Confirm every required ticket-provided validation/test-plan item is explicitly marked complete in the workpad.
10. Only then move issue to `Human Review` using `obsidian_markdown_updater`.

## Step 3: Human Review and merge handling

1. When the issue is in `Human Review`, do not code or change ticket content. Wait and poll for updates as needed.
2. If review feedback requires changes, transition the issue to `Rework` and follow the rework flow.
3. If approved, human moves the issue to `Merging`.
4. When the issue is in `Merging`, run your project's merge pipeline.
5. After merge is complete, move the issue to `Done` using `obsidian_markdown_updater`.

## Step 4: Rework handling

1. Treat `Rework` as a full approach reset, not incremental patching.
2. Re-read the full issue body and all human comments; explicitly identify what will be done differently this attempt.
3. Start over from the normal kickoff flow:
   - Move it to `In Progress` using `obsidian_markdown_updater`.
   - Build a fresh plan/checklist and execute end-to-end.

## Completion bar before Human Review

- Step 1/2 checklist is fully complete and accurately reflected in the single workpad.
- Validation/tests are green for the latest commit.
- PR feedback sweep is complete and no actionable comments remain.
- PR checks are green, branch is pushed.

## Guardrails

- All Rust code must go into `rust_flutter/backend`.
- All Flutter code must go into `rust_flutter/frontend`.
- Do not edit the issue body/description for planning or progress tracking. Instead, use a dedicated `## Codex Workpad` section in the issue.
- Do not move to `Human Review` unless the `Completion bar before Human Review` is satisfied.
- In `Human Review`, do not make changes; wait and poll.
- If state is terminal (`Done`), do nothing and shut down.

## Workpad template

Use this exact structure for the persistent workpad and keep it updated in place throughout execution:

````md
## Codex Workpad

```text
<hostname>:<abs-path>@<short-sha>
```

### Plan

- [ ] 1\. Parent task
  - [ ] 1.1 Child task
  - [ ] 1.2 Child task
- [ ] 2\. Parent task

### Acceptance Criteria

- [ ] Criterion 1
- [ ] Criterion 2

### Validation

- [ ] targeted tests: `<command>`

### Notes

- <short progress note with timestamp>

### Confusions

- <only include when something was confusing during execution>
````
