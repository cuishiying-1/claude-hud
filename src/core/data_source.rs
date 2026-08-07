//! 数据源解析：报告路径新鲜度检测 + 同项目目录活跃候选回退。
//! stale 检测必须用文件 mtime（timestamps_reliable=false 时 last_event_secs
//! 只是行号，不可用于新鲜度判定）。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// 报告路径超过该秒数无写入 → 视为 stale（与窗口 Idle/Ended 分界一致）。
pub const STALE_THRESHOLD_SECS: u64 = 300;
/// 候选回退目标必须在此秒数内有过写入（含"正在写入"的文件）。
pub const ACTIVE_THRESHOLD_SECS: u64 = 300;

/// 数据来源标注：Reported = statusLine 报告路径本身；Fallback(path) =
/// 报告路径 stale/缺失后回退到的活跃兄弟文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataSource {
    Reported,
    Fallback(PathBuf),
}

impl Default for DataSource {
    fn default() -> Self {
        DataSource::Reported
    }
}

impl DataSource {
    pub fn name(&self) -> &'static str {
        match self {
            DataSource::Reported => "reported",
            DataSource::Fallback(_) => "fallback",
        }
    }

    pub fn is_fallback(&self) -> bool {
        matches!(self, DataSource::Fallback(_))
    }
}

pub fn file_mtime_secs(path: &Path) -> Option<u64> {
    let meta = path.metadata().ok()?;
    let mtime = meta.modified().ok()?;
    mtime.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// 解析报告路径 → (解析后路径, 数据源)。公开入口用真实时钟。
pub fn resolve_transcript_path(reported: &Path) -> (PathBuf, DataSource) {
    resolve_transcript_path_inner(
        reported,
        now_secs(),
        STALE_THRESHOLD_SECS,
        ACTIVE_THRESHOLD_SECS,
    )
}

/// 参数化内部实现（测试注入 now/阈值，避免 filetime 依赖）。
fn resolve_transcript_path_inner(
    reported: &Path,
    now: u64,
    stale_threshold: u64,
    active_threshold: u64,
) -> (PathBuf, DataSource) {
    let fresh = match file_mtime_secs(reported) {
        Some(m) => now.saturating_sub(m) <= stale_threshold,
        None => false,
    };
    if fresh {
        return (reported.to_path_buf(), DataSource::Reported);
    }
    if let Some(candidate) = latest_active_candidate(reported, now, active_threshold) {
        return (candidate.clone(), DataSource::Fallback(candidate));
    }
    (reported.to_path_buf(), DataSource::Reported)
}

/// 报告路径父目录（同项目）中 mtime 最新且仍在活跃窗口内的 *.jsonl，
/// 排除报告路径自身；无候选 → None。
fn latest_active_candidate(reported: &Path, now: u64, active_threshold: u64) -> Option<PathBuf> {
    let parent = reported.parent()?;
    let entries = fs::read_dir(parent).ok()?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path == reported {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let mtime = match file_mtime_secs(&path) {
            Some(m) => m,
            None => continue,
        };
        if now.saturating_sub(mtime) > active_threshold {
            continue;
        }
        if best.as_ref().map_or(true, |(bm, _)| mtime > *bm) {
            best = Some((mtime, path));
        }
    }
    best.map(|(_, p)| p)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_proj(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("hud-ds-{}-{}", std::process::id(), name))
    }

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "x").unwrap();
    }

    fn clean(p: &Path) {
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn fresh_reported_path_keeps_reported() {
        let dir = tmp_proj("fresh");
        clean(&dir);
        let reported = dir.join("s.jsonl");
        touch(&reported);
        let m = file_mtime_secs(&reported).unwrap();
        // now == mtime → age 0 ≤ 阈值 → Reported
        let (path, src) = resolve_transcript_path_inner(&reported, m, 300, 300);
        assert_eq!(path, reported);
        assert_eq!(src, DataSource::Reported);
        clean(&dir);
    }

    #[test]
    fn stale_reported_falls_back_to_latest_sibling() {
        let dir = tmp_proj("stale");
        clean(&dir);
        let reported = dir.join("stale.jsonl");
        let a = dir.join("a.jsonl");
        let b = dir.join("b.jsonl");
        touch(&reported);
        touch(&a);
        // file_mtime_secs 只取整秒（as_secs），同一秒内写入的 mtime 相等
        // （平局取 readdir 首个）；睡 1.1s 保证跨过至少一个整秒边界 →
        // m(b) > m(a) 严格成立，测试"最新者优先"确定
        std::thread::sleep(std::time::Duration::from_millis(1100));
        touch(&b);
        let m = file_mtime_secs(&reported).unwrap();
        // 阈值 (0, 2)：reported 必 stale（age = 1 > 0）；a/b age ≤ 1 ≤ 2 均合格
        let (path, src) = resolve_transcript_path_inner(&reported, m + 1, 0, 2);
        assert_eq!(path, b, "回退到 mtime 最新者");
        assert_eq!(src, DataSource::Fallback(b));
        clean(&dir);
    }

    #[test]
    fn missing_reported_falls_back_to_sibling() {
        let dir = tmp_proj("missing");
        clean(&dir);
        let reported = dir.join("stale.jsonl"); // 不存在
        let active = dir.join("active.jsonl");
        touch(&active);
        let m = file_mtime_secs(&active).unwrap();
        let (path, src) = resolve_transcript_path_inner(&reported, m, 0, 2);
        assert_eq!(path, active);
        assert!(src.is_fallback());
        clean(&dir);
    }

    #[test]
    fn no_candidate_keeps_reported_honestly() {
        let dir = tmp_proj("nocand");
        clean(&dir);
        let reported = dir.join("stale.jsonl");
        touch(&reported);
        let m = file_mtime_secs(&reported).unwrap();
        // 仅报告路径自身，无其他候选 → 诚实降级为 Reported（不猜测）
        let (path, src) = resolve_transcript_path_inner(&reported, m + 1, 0, 2);
        assert_eq!(path, reported);
        assert_eq!(src, DataSource::Reported);
        clean(&dir);
    }

    #[test]
    fn reported_path_excluded_from_candidates() {
        let dir = tmp_proj("exclude");
        clean(&dir);
        let reported = dir.join("stale.jsonl");
        let a = dir.join("a.jsonl");
        touch(&reported);
        touch(&a);
        let m = file_mtime_secs(&reported).unwrap();
        // reported 自身若未被排除，将是最新候选 → 错误回退到自己；
        // 排除后正确回退到 a
        let (path, _) = resolve_transcript_path_inner(&reported, m + 1, 0, 300);
        assert_eq!(path, a);
        clean(&dir);
    }

    #[test]
    fn non_jsonl_files_ignored() {
        let dir = tmp_proj("ext");
        clean(&dir);
        let reported = dir.join("stale.jsonl");
        let txt = dir.join("notes.txt");
        touch(&reported);
        touch(&txt);
        let m = file_mtime_secs(&reported).unwrap();
        let (path, src) = resolve_transcript_path_inner(&reported, m + 1, 0, 300);
        assert_eq!(path, reported, "txt 不是候选 → 无候选 → Reported");
        assert_eq!(src, DataSource::Reported);
        clean(&dir);
    }

    #[test]
    fn file_mtime_secs_none_for_missing() {
        let dir = tmp_proj("mt");
        clean(&dir);
        assert!(file_mtime_secs(&dir.join("nope.jsonl")).is_none());
        clean(&dir);
    }
}
