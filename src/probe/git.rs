use std::process::Command;

/// Snapshot of the current git workspace.
#[derive(Debug, Clone)]
pub struct GitStatus {
    pub branch: String,
    pub is_dirty: bool,
    pub ahead: usize,
    pub behind: usize,
}

/// Probe git for current status. Returns None if not a git repo.
pub fn probe_git() -> Option<GitStatus> {
    let branch = run_cmd(&["branch", "--show-current"])?;
    let branch = branch.trim().to_string();
    if branch.is_empty() {
        return None;
    }

    let is_dirty = run_cmd(&["status", "--porcelain"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    let ahead = run_cmd(&["rev-list", "--count", "@{u}..HEAD"])
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let behind = run_cmd(&["rev-list", "--count", "HEAD..@{u}"])
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    Some(GitStatus {
        branch,
        is_dirty,
        ahead,
        behind,
    })
}

fn run_cmd(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).to_string())
}
