//! Workspace management and safety invariants.
//!
//! Implements SPEC §9.

use crate::models::{sanitize_workspace_key, Workspace};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, warn};

/// Workspace manager errors.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("workspace path outside root: {workspace} not under {root}")]
    PathOutsideRoot { workspace: String, root: String },

    #[error("failed to create workspace directory: {0}")]
    CreateFailed(String),

    #[error("after_create hook failed: {0}")]
    AfterCreateHookFailed(String),

    #[error("before_run hook failed: {0}")]
    BeforeRunHookFailed(String),

    #[error("workspace path exists but is not a directory: {0}")]
    NotADirectory(String),
}

/// Workspace manager: creates, reuses, and cleans per-issue workspaces (§9.1–§9.5).
pub struct WorkspaceManager {
    root: PathBuf,
}

impl WorkspaceManager {
    pub fn new(root: &str) -> Self {
        Self {
            root: PathBuf::from(root),
        }
    }

    /// Update workspace root (for dynamic config reload).
    pub fn update_root(&mut self, root: &str) {
        self.root = PathBuf::from(root);
    }

    /// Create or reuse a workspace for the given issue identifier (§9.2).
    pub async fn create_for_issue(
        &self,
        identifier: &str,
        after_create_hook: Option<&str>,
        hook_timeout_ms: u64,
    ) -> Result<Workspace, WorkspaceError> {
        let workspace_key = sanitize_workspace_key(identifier);
        let workspace_path = self.root.join(&workspace_key);

        // Safety invariant 2: workspace path must stay inside workspace root (§9.5)
        self.validate_path_safety(&workspace_path)?;

        let created_now;

        if workspace_path.exists() {
            if !workspace_path.is_dir() {
                return Err(WorkspaceError::NotADirectory(
                    workspace_path.display().to_string(),
                ));
            }
            created_now = false;
            debug!(
                workspace = %workspace_path.display(),
                "reusing existing workspace"
            );
        } else {
            // Create workspace directory (and root if needed)
            std::fs::create_dir_all(&workspace_path).map_err(|e| {
                WorkspaceError::CreateFailed(format!(
                    "failed to create {}: {}",
                    workspace_path.display(),
                    e
                ))
            })?;
            created_now = true;
            info!(
                workspace = %workspace_path.display(),
                "created new workspace"
            );
        }

        // Run after_create hook if workspace was newly created (§9.4)
        if created_now {
            if let Some(script) = after_create_hook {
                if let Err(e) = run_hook("after_create", script, &workspace_path, hook_timeout_ms).await
                {
                    // after_create failure is fatal to workspace creation — remove partial dir
                    let _ = std::fs::remove_dir_all(&workspace_path);
                    return Err(WorkspaceError::AfterCreateHookFailed(e.to_string()));
                }
            }
        }

        Ok(Workspace {
            path: workspace_path.display().to_string(),
            workspace_key,
            created_now,
        })
    }

    /// Run the before_run hook (§9.4). Failure aborts the current run attempt.
    pub async fn run_before_run_hook(
        &self,
        workspace_path: &str,
        script: &str,
        hook_timeout_ms: u64,
    ) -> Result<(), WorkspaceError> {
        run_hook("before_run", script, Path::new(workspace_path), hook_timeout_ms)
            .await
            .map_err(|e| WorkspaceError::BeforeRunHookFailed(e.to_string()))
    }

    /// Run the after_run hook (§9.4). Failure is logged and ignored.
    pub async fn run_after_run_hook(
        &self,
        workspace_path: &str,
        script: &str,
        hook_timeout_ms: u64,
    ) {
        if let Err(e) = run_hook("after_run", script, Path::new(workspace_path), hook_timeout_ms).await
        {
            warn!(hook = "after_run", error = %e, "hook failed (ignored)");
        }
    }

    /// Run the before_remove hook and then remove the workspace directory (§9.4).
    pub async fn remove_workspace(
        &self,
        identifier: &str,
        before_remove_hook: Option<&str>,
        hook_timeout_ms: u64,
    ) {
        let workspace_key = sanitize_workspace_key(identifier);
        let workspace_path = self.root.join(&workspace_key);

        if !workspace_path.exists() {
            return;
        }

        // Run before_remove hook (failure is logged and ignored)
        if let Some(script) = before_remove_hook {
            if let Err(e) =
                run_hook("before_remove", script, &workspace_path, hook_timeout_ms).await
            {
                warn!(
                    hook = "before_remove",
                    workspace = %workspace_path.display(),
                    error = %e,
                    "hook failed (ignored)"
                );
            }
        }

        // Remove workspace directory
        if let Err(e) = std::fs::remove_dir_all(&workspace_path) {
            error!(
                workspace = %workspace_path.display(),
                error = %e,
                "failed to remove workspace"
            );
        } else {
            info!(
                workspace = %workspace_path.display(),
                "removed workspace"
            );
        }
    }

    /// Safety invariant 2: workspace path must be inside workspace root (§9.5).
    fn validate_path_safety(&self, workspace_path: &Path) -> Result<(), WorkspaceError> {
        let abs_root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());

        // For new directories that don't exist yet, check the parent
        let check_path = if workspace_path.exists() {
            workspace_path
                .canonicalize()
                .unwrap_or_else(|_| workspace_path.to_path_buf())
        } else {
            // Ensure the workspace path starts with root
            workspace_path.to_path_buf()
        };

        if !check_path.starts_with(&abs_root) && !workspace_path.starts_with(&self.root) {
            return Err(WorkspaceError::PathOutsideRoot {
                workspace: workspace_path.display().to_string(),
                root: self.root.display().to_string(),
            });
        }

        Ok(())
    }

    /// Get the workspace path for an issue identifier.
    pub fn workspace_path_for(&self, identifier: &str) -> String {
        let key = sanitize_workspace_key(identifier);
        self.root.join(&key).display().to_string()
    }
}

/// Run a shell hook script in the workspace directory with timeout (§9.4).
async fn run_hook(
    hook_name: &str,
    script: &str,
    workspace_path: &Path,
    timeout_ms: u64,
) -> Result<(), String> {
    info!(
        hook = hook_name,
        workspace = %workspace_path.display(),
        "running hook"
    );

    let mut cmd = Command::new("bash");
    cmd.arg("-lc")
        .arg(script)
        .current_dir(workspace_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn hook {}: {}", hook_name, e))?;

    let duration = Duration::from_millis(timeout_ms);
    match timeout(duration, child.wait_with_output()).await {
        Ok(Ok(output)) => {
            if output.status.success() {
                debug!(hook = hook_name, "hook completed successfully");
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let truncated = if stderr.len() > 500 {
                    format!("{}...", &stderr[..500])
                } else {
                    stderr.to_string()
                };
                Err(format!(
                    "hook {} exited with status {}: {}",
                    hook_name,
                    output.status,
                    truncated
                ))
            }
        }
        Ok(Err(e)) => Err(format!("hook {} io error: {}", hook_name, e)),
        Err(_) => Err(format!("hook {} timed out after {}ms", hook_name, timeout_ms)),
    }
}
