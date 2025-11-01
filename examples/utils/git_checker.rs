use std::process::Command;

#[derive(Debug)]
pub struct GitInfo {
    pub commit_sha: String,
    pub commit_short_sha: String,
    pub branch: String,
}

pub fn check_git_status() -> Result<GitInfo, String> {
    // Check if git is available
    let git_available = Command::new("git").arg("--version").output().is_ok();

    if !git_available {
        return Err("Git is not installed or not available in PATH".to_string());
    }

    // Check for uncommitted changes
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map_err(|e| format!("Failed to run git status: {}", e))?;

    let status_str = String::from_utf8_lossy(&status_output.stdout);
    if !status_str.trim().is_empty() {
        return Err(format!(
            "Git working directory has uncommitted changes:\n{}\n\nPlease commit or stash changes before running the test.",
            status_str
        ));
    }

    // Get commit SHA
    let sha_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|e| format!("Failed to get commit SHA: {}", e))?;

    let commit_sha = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();

    // Get short SHA
    let short_sha_output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map_err(|e| format!("Failed to get short commit SHA: {}", e))?;

    let commit_short_sha = String::from_utf8_lossy(&short_sha_output.stdout)
        .trim()
        .to_string();

    // Get current branch
    let branch_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map_err(|e| format!("Failed to get branch name: {}", e))?;

    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    Ok(GitInfo {
        commit_sha,
        commit_short_sha,
        branch,
    })
}
