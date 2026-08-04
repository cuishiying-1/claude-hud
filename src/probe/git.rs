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

use std::path::Path;

const TTL_BRANCH_DIRTY_SECS: u64 = 30;
const TTL_AHEAD_BEHIND_SECS: u64 = 60;

/// Probe git with a TTL cache: fresh cache values are reused without
/// spawning git. `branch` value "" is the cached "not a repo" sentinel.
pub fn probe_git_cached(state_path: &Path) -> Option<GitStatus> {
    let now = crate::core::state::now_secs();
    let mut st = crate::core::state::StateFile::read(state_path);
    let mut status = GitStatus {
        branch: String::new(),
        is_dirty: false,
        ahead: 0,
        behind: 0,
    };
    let mut changed = false;

    // branch + not-a-repo sentinel (30s)
    if now.saturating_sub(st.cache.git.branch.ts) <= TTL_BRANCH_DIRTY_SECS {
        if st.cache.git.branch.value.is_empty() {
            return None;
        }
        status.branch = st.cache.git.branch.value.clone();
    } else {
        let branch = run_cmd(&["branch", "--show-current"])
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        st.cache.git.branch = crate::core::state::CachedValue {
            value: branch.clone(),
            ts: now,
        };
        changed = true;
        if branch.is_empty() {
            let _ = st.write(state_path);
            return None;
        }
        status.branch = branch;
    }

    // dirty (30s)
    if now.saturating_sub(st.cache.git.dirty.ts) <= TTL_BRANCH_DIRTY_SECS {
        status.is_dirty = st.cache.git.dirty.value;
    } else {
        status.is_dirty = run_cmd(&["status", "--porcelain"])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        st.cache.git.dirty = crate::core::state::CachedValue {
            value: status.is_dirty,
            ts: now,
        };
        changed = true;
    }

    // ahead/behind (60s)
    if now.saturating_sub(st.cache.git.ahead_behind.ts) <= TTL_AHEAD_BEHIND_SECS {
        let (a, b) = parse_ab(&st.cache.git.ahead_behind.value);
        status.ahead = a;
        status.behind = b;
    } else {
        status.ahead = run_cmd(&["rev-list", "--count", "@{u}..HEAD"])
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        status.behind = run_cmd(&["rev-list", "--count", "HEAD..@{u}"])
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        st.cache.git.ahead_behind = crate::core::state::CachedValue {
            value: format!("{}/{}", status.ahead, status.behind),
            ts: now,
        };
        changed = true;
    }

    if changed {
        let _ = st.write(state_path);
    }
    Some(status)
}

fn parse_ab(s: &str) -> (usize, usize) {
    let mut it = s.splitn(2, '/');
    let a = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let b = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::{now_secs, CachedValue, GitCache, StateFile};
    use std::path::{Path, PathBuf};

    // NOTE: unique per test — all tests share the process PID, and the
    // harness runs them in parallel, so a shared path would race.
    fn tmp_state(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("hud-git-cache-{}-{}.json", std::process::id(), name));
        p
    }

    fn seed(p: &Path, branch: &str, dirty: bool, ab: &str, ts: u64) {
        let mut st = StateFile::default();
        st.cache.git = GitCache {
            branch: CachedValue { value: branch.into(), ts },
            dirty: CachedValue { value: dirty, ts },
            ahead_behind: CachedValue { value: ab.into(), ts },
        };
        st.write(p).unwrap();
    }

    #[test]
    fn fresh_cache_reused_without_spawning() {
        let p = tmp_state("fresh");
        let _ = std::fs::remove_file(&p);
        seed(&p, "fake-cached-branch", true, "3/1", now_secs());
        let s = probe_git_cached(&p).expect("cached branch non-empty");
        assert_eq!(s.branch, "fake-cached-branch");
        assert!(s.is_dirty);
        assert_eq!((s.ahead, s.behind), (3, 1));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn stale_cache_reprobes() {
        let p = tmp_state("stale");
        let _ = std::fs::remove_file(&p);
        seed(&p, "fake-cached-branch", false, "0/0", 0); // ts 0 = 从未新鲜
        let s = probe_git_cached(&p).expect("real repo has a branch");
        assert_ne!(s.branch, "fake-cached-branch");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn cached_not_a_repo_returns_none() {
        let p = tmp_state("notrepo");
        let _ = std::fs::remove_file(&p);
        seed(&p, "", false, "0/0", now_secs()); // 空 branch = 非 git 仓库（已缓存）
        assert!(probe_git_cached(&p).is_none());
        let _ = std::fs::remove_file(&p);
    }
}
