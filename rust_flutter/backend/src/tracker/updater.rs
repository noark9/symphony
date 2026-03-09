use serde_yaml::Value;
use std::fs;
use std::path::Path;

pub fn update_obsidian_markdown(
    vault_dir: &Path,
    issue_identifier: &str,
    new_state: Option<&str>,
    content_append: Option<&str>,
) -> Result<(), String> {
    let file_path = vault_dir.join(format!("{}.md", issue_identifier));
    if !file_path.exists() {
        return Err(format!("File {} not found", file_path.display()));
    }

    let content = fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {}", e))?;

    let is_crlf = content.starts_with("---\r\n");
    if !content.starts_with("---\n") && !is_crlf {
        return Err("Missing YAML frontmatter".to_string());
    }

    let end_of_frontmatter_idx = if is_crlf {
        content[4..].find("\r\n---\r\n")
    } else {
        content[4..].find("\n---\n")
    };

    let end_idx = match end_of_frontmatter_idx {
        Some(idx) => idx + 4,
        None => return Err("Malformed YAML frontmatter".to_string()),
    };

    let frontmatter_str = &content[4..end_idx];
    let mut frontmatter: Value = serde_yaml::from_str(frontmatter_str)
        .map_err(|e| format!("Failed to parse YAML: {}", e))?;

    if let Some(state) = new_state {
        if let Some(map) = frontmatter.as_mapping_mut() {
            map.insert(Value::String("status".to_string()), Value::String(state.to_string()));
        }
    }

    let mut new_yaml = serde_yaml::to_string(&frontmatter).map_err(|e| format!("Failed to serialize YAML: {}", e))?;

    // serde_yaml output might not have ---, so we add it back
    if !new_yaml.starts_with("---\n") {
        new_yaml = format!("---\n{}", new_yaml);
    }

    // serde_yaml often outputs ending with just a newline, we need to close the frontmatter block
    if !new_yaml.ends_with("---\n") {
        if new_yaml.ends_with("\n") {
             new_yaml = format!("{}---\n", new_yaml);
        } else {
             new_yaml = format!("{}\n---\n", new_yaml);
        }
    }

    let mut rest = content[end_idx + (if is_crlf { 7 } else { 5 })..].to_string();

    if let Some(append_content) = content_append {
        if !rest.ends_with("\n") && !rest.is_empty() {
            rest.push('\n');
        }
        rest.push_str(append_content);
        if !rest.ends_with("\n") {
            rest.push('\n');
        }
    }

    let final_content = format!("{}{}", new_yaml, rest);
    fs::write(&file_path, final_content).map_err(|e| format!("Failed to write file: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_update_obsidian_markdown() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("ISSUE-1.md");
        fs::write(&file_path, "---\nstatus: in-progress\npriority: high\n---\n\nBody.").unwrap();

        update_obsidian_markdown(
            dir.path(),
            "ISSUE-1",
            Some("done"),
            Some("Update."),
        ).unwrap();

        let updated = fs::read_to_string(&file_path).unwrap();
        assert!(updated.contains("status: done"));
        assert!(updated.contains("priority: high"));
        assert!(updated.contains("Update."));
        // Make sure it doesn't double append newlines improperly at the start of append
        assert!(updated.ends_with("\nUpdate.\n"));
    }

    #[test]
    fn test_update_obsidian_markdown_no_append() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("ISSUE-2.md");
        fs::write(&file_path, "---\nstatus: todo\n---\n\nBody.").unwrap();

        update_obsidian_markdown(
            dir.path(),
            "ISSUE-2",
            Some("in-progress"),
            None,
        ).unwrap();

        let updated = fs::read_to_string(&file_path).unwrap();
        assert!(updated.contains("status: in-progress"));
        assert!(!updated.contains("None"));
    }
}
