//! 多会话监控数据层:窗口 key 派生、新鲜度三态、目录名提取。
//! 每个 Claude Code 窗口的 render 把快照写进 windows/<key>.json,
//! 监控端(serve/dashboard/totals)扫描目录按时间戳判活,无锁并发安全。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// 窗口 key:transcript_path → 64 位哈希 hex(DefaultHasher::new 固定 seed,
/// 跨进程稳定)。同会话内 transcript 路径不变 → key 稳定;不同窗口
/// (不同 cwd/会话)路径不同 → key 不同。无 transcript → "default"。
pub fn window_key(transcript_path: Option<&str>) -> String {
    match transcript_path {
        Some(p) if !p.is_empty() => {
            let mut h = DefaultHasher::new();
            p.hash(&mut h);
            format!("{:016x}", h.finish())
        }
        _ => "default".to_string(),
    }
}

/// 窗口新鲜度三态:≤10s 活跃 / ≤5min 空闲 / >5min 已结束;>24h 返回 None(隐藏)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowStatus {
    #[default]
    Active,
    Idle,
    Ended,
}

pub fn status_for(now_secs: u64, ts: u64) -> Option<WindowStatus> {
    let age = now_secs.saturating_sub(ts);
    if age >= 24 * 3600 {
        return None;
    }
    if age <= 10 {
        Some(WindowStatus::Active)
    } else if age <= 300 {
        Some(WindowStatus::Idle)
    } else {
        Some(WindowStatus::Ended)
    }
}

/// 窗口显示名:transcript_path 的父目录名(Claude Code projects 编码后的
/// cwd 目录,用户可识别)。
pub fn window_dir_name(transcript_path: Option<&str>) -> String {
    match transcript_path {
        Some(p) => Path::new(p)
            .parent()
            .and_then(|d| d.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        None => String::new(),
    }
}

/// 单个窗口的快照摘要(监控端展示用)。
#[derive(Debug, Clone, Default)]
pub struct WindowInfo {
    pub key: String,
    pub dir_name: String,
    pub status: WindowStatus,
    pub model: String,
    pub used_pct: f64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost: f64,
    pub agent_count: usize,
    pub ts: u64,
    pub corrupt: bool,
}

/// 扫描 windows/ 目录(默认位置),目录缺失 → 空列表。
pub fn scan_windows(now_secs: u64) -> Vec<WindowInfo> {
    let dir = crate::core::config::AppConfig::windows_dir().unwrap_or_default();
    scan_dir(&dir, now_secs)
}

/// 扫描指定目录:每个 <key>.json 读为 StateFile;解析失败 → corrupt 标记;
/// ≥24h 陈旧文件与隐藏/非 .json 文件跳过;排序活跃优先、同级按 ts 新→旧。
pub fn scan_dir(dir: &std::path::Path, now_secs: u64) -> Vec<WindowInfo> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut out: Vec<WindowInfo> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let key = name.trim_end_matches(".json").to_string();
        let raw = match std::fs::read_to_string(entry.path()) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let st: crate::core::state::StateFile = match serde_json::from_str(&raw) {
            Ok(s) => s,
            Err(_) => {
                out.push(WindowInfo {
                    key,
                    corrupt: true,
                    ..Default::default()
                });
                continue;
            }
        };
        let snap = &st.snapshot;
        let Some(status) = status_for(now_secs, snap.timestamp_secs) else {
            continue;
        };
        out.push(WindowInfo {
            key,
            dir_name: window_dir_name(snap.transcript_path.as_deref()),
            status,
            model: snap.model.display_name.clone(),
            used_pct: snap.context_window.used_percentage,
            tokens_in: snap.context_window.total_input_tokens,
            tokens_out: snap.context_window.total_output_tokens,
            cost: snap.cost.total_cost_usd,
            agent_count: snap.agent_count,
            ts: snap.timestamp_secs,
            corrupt: false,
        });
    }
    out.sort_by(|a, b| {
        status_rank(&b.status)
            .cmp(&status_rank(&a.status))
            .then(b.ts.cmp(&a.ts))
    });
    out
}

fn status_rank(s: &WindowStatus) -> u8 {
    match s {
        WindowStatus::Active => 2,
        WindowStatus::Idle => 1,
        WindowStatus::Ended => 0,
    }
}

/// 状态英文名(serve JSON 与 Web 端展示用,不依赖语言)。
pub fn status_name(s: &WindowStatus) -> &'static str {
    match s {
        WindowStatus::Active => "active",
        WindowStatus::Idle => "idle",
        WindowStatus::Ended => "ended",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::StateFile;

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("hud-wins-{}-{}", name, std::process::id()))
    }

    fn clean(p: &std::path::Path) {
        if p.exists() {
            let _ = std::fs::remove_dir_all(p);
        }
    }

    fn make_window(dir: &std::path::Path, key: &str, ts: u64, dir_name: &str) {
        let mut st = StateFile::default();
        st.snapshot.timestamp_secs = ts;
        st.snapshot.model.display_name = "op-us".to_string();
        st.snapshot.context_window.used_percentage = 42.0;
        st.snapshot.context_window.total_input_tokens = 100;
        st.snapshot.context_window.total_output_tokens = 50;
        st.snapshot.cost.total_cost_usd = 1.25;
        st.snapshot.agent_count = 2;
        st.snapshot.transcript_path = Some(format!("/home/u/.claude/projects/{}/s.jsonl", dir_name));
        st.write(&dir.join(format!("{}.json", key))).unwrap();
    }

    #[test]
    fn window_key_stable_and_unique() {
        let a = window_key(Some("/x/proj-1/s.jsonl"));
        let b = window_key(Some("/x/proj-1/s.jsonl"));
        assert_eq!(a, b);
        assert_ne!(a, window_key(Some("/x/proj-2/s.jsonl")));
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn window_key_default_fallback() {
        assert_eq!(window_key(None), "default");
        assert_eq!(window_key(Some("")), "default");
    }

    #[test]
    fn status_three_tiers_at_boundaries() {
        assert_eq!(status_for(1000, 990), Some(WindowStatus::Active));
        assert_eq!(status_for(1000, 1000), Some(WindowStatus::Active));
        assert_eq!(status_for(1000, 989), Some(WindowStatus::Idle));
        assert_eq!(status_for(1000, 700), Some(WindowStatus::Idle));
        assert_eq!(status_for(1000, 699), Some(WindowStatus::Ended));
        assert_eq!(status_for(1000, 0), Some(WindowStatus::Ended));
    }

    #[test]
    fn status_hidden_after_24h() {
        let now = 24 * 3600 + 1000;
        assert_eq!(status_for(now, 1000), None); // age = 24h → 隐藏
        assert_eq!(status_for(now, 1001), Some(WindowStatus::Ended)); // 24h-1s → 显示
    }

    #[test]
    fn dir_name_from_projects_path() {
        assert_eq!(
            window_dir_name(Some("/home/u/.claude/projects/D--workspace-proj/s.jsonl")),
            "D--workspace-proj"
        );
        assert_eq!(window_dir_name(None), "");
    }

    #[test]
    fn scan_dir_orders_active_first_then_newest() {
        let dir = tmp_dir("order");
        clean(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        make_window(&dir, "aaa", 1000000, "proj-a"); // age 0 → 活跃
        make_window(&dir, "bbb", 999800, "proj-b"); // age 200 → 空闲
        make_window(&dir, "ccc", 999050, "proj-c"); // age 950 → 已结束
        let wins = scan_dir(&dir, 1000000);
        assert_eq!(wins.len(), 3);
        assert_eq!(wins[0].key, "aaa");
        assert_eq!(wins[0].status, WindowStatus::Active);
        assert_eq!(wins[0].dir_name, "proj-a");
        assert_eq!(wins[0].model, "op-us");
        assert_eq!(wins[0].used_pct, 42.0);
        assert_eq!(wins[0].tokens_in, 100);
        assert_eq!(wins[0].tokens_out, 50);
        assert_eq!(wins[0].cost, 1.25);
        assert_eq!(wins[0].agent_count, 2);
        assert!(!wins[0].corrupt);
        assert_eq!(wins[1].key, "bbb");
        assert_eq!(wins[1].status, WindowStatus::Idle);
        assert_eq!(wins[2].key, "ccc");
        assert_eq!(wins[2].status, WindowStatus::Ended);
        clean(&dir);
    }

    #[test]
    fn scan_dir_marks_corrupt_and_skips_hidden_and_non_json() {
        let dir = tmp_dir("corrupt");
        clean(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        make_window(&dir, "aaa", 1000000, "proj-a");
        std::fs::write(dir.join("bad.json"), "not json").unwrap();
        make_window(&dir, "old", 1000000 - 24 * 3600, "proj-old"); // age = 24h → 隐藏
        std::fs::write(dir.join("readme.txt"), "x").unwrap();
        let wins = scan_dir(&dir, 1000000);
        assert_eq!(wins.len(), 2);
        assert!(!wins[0].corrupt);
        assert!(wins[1].corrupt);
        assert_eq!(wins[1].key, "bad");
        clean(&dir);
    }

    #[test]
    fn scan_dir_missing_dir_is_empty() {
        let dir = tmp_dir("missing");
        clean(&dir);
        assert!(scan_dir(&dir, 1000).is_empty());
    }
}
