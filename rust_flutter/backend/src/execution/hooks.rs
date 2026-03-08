use std::path::Path;
use tokio::process::Command;
use std::time::Duration;
use tokio::time::timeout;
use std::process::Stdio;

pub struct HooksManager;

impl HooksManager {
    async fn execute_hook(
        workspace_path: &Path,
        command: &str,
        timeout_ms: u64,
        abort_on_failure: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut child = Command::new("bash")
            .arg("-lc")
            .arg(command)
            .current_dir(workspace_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        let timeout_duration = Duration::from_millis(timeout_ms);
        match timeout(timeout_duration, child.wait()).await {
            Ok(Ok(status)) => {
                if !status.success() {
                    let err_msg = format!("Hook execution failed with status: {}", status);
                    if abort_on_failure {
                        return Err(err_msg.into());
                    } else {
                        eprintln!("{}", err_msg);
                    }
                }
            }
            Ok(Err(e)) => {
                let err_msg = format!("Failed to wait on hook process: {}", e);
                if abort_on_failure {
                    return Err(err_msg.into());
                } else {
                    eprintln!("{}", err_msg);
                }
            }
            Err(_) => {
                let _ = child.kill().await;
                let err_msg = "Hook execution timed out".to_string();
                if abort_on_failure {
                    return Err(err_msg.into());
                } else {
                    eprintln!("{}", err_msg);
                }
            }
        }

        Ok(())
    }

    pub async fn after_create(workspace_path: &Path, command: Option<&str>, timeout_ms: u64) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(cmd) = command {
            Self::execute_hook(workspace_path, cmd, timeout_ms, true).await?;
        }
        Ok(())
    }

    pub async fn before_run(workspace_path: &Path, command: Option<&str>, timeout_ms: u64) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(cmd) = command {
            Self::execute_hook(workspace_path, cmd, timeout_ms, true).await?;
        }
        Ok(())
    }

    pub async fn after_run(workspace_path: &Path, command: Option<&str>, timeout_ms: u64) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(cmd) = command {
            let _ = Self::execute_hook(workspace_path, cmd, timeout_ms, false).await;
        }
        Ok(())
    }

    pub async fn before_remove(workspace_path: &Path, command: Option<&str>, timeout_ms: u64) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(cmd) = command {
            let _ = Self::execute_hook(workspace_path, cmd, timeout_ms, false).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_execute_hook_success() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        let result = HooksManager::execute_hook(workspace_path, "echo 'hello'", 1000, true).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_hook_failure_abort() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        let result = HooksManager::execute_hook(workspace_path, "exit 1", 1000, true).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_hook_failure_ignore() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        let result = HooksManager::execute_hook(workspace_path, "exit 1", 1000, false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_hook_timeout_abort() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        let result = HooksManager::execute_hook(workspace_path, "sleep 2", 100, true).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_after_create_success() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        let result = HooksManager::after_create(workspace_path, Some("echo 'hello'"), 1000).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_after_create_failure_aborts() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        let result = HooksManager::after_create(workspace_path, Some("exit 1"), 1000).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_after_run_failure_ignored() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();

        let result = HooksManager::after_run(workspace_path, Some("exit 1"), 1000).await;
        assert!(result.is_ok()); // Failure is ignored, returns Ok
    }
}
