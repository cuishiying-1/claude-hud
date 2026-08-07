//! 活跃窗口全局扫描：独立增量尾读 projects/**/*.jsonl。
//! 取代 windows/*.json 作为 serve/TUI 窗口视图数据源（无 statusLine 的
//! 窗口也可见）。位置缓存常驻进程，每 5s 只读新增字节。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::config::AppConfig;
use super::data_source::file_mtime_secs;
use super::pricing::{self, PricingTable, PriceEntry};
use super::state;
use super::transcript::{TranscriptReader, TranscriptSummary};
use super::windows::{
    sort_windows, status_for, window_dir_name, window_key, WindowInfo, WindowStatus,
};

/// 候选窗口时间窗：最近 10 分钟内写入的 transcript 参与扫描。
pub const SCAN_WINDOW_SECS: u64 = 600;

/// 默认 projects 根：CLAUDE_HUD_DIR 注入时（黑盒/测试）扫描隔离目录，
/// 否则扫描 Claude Code 真实 projects 目录。
pub fn default_projects_root() -> PathBuf {
    if let Ok(p) = std::env::var("CLAUDE_HUD_DIR") {
        return PathBuf::from(p).join("projects");
    }
    dirs::home_dir().unwrap_or_default().join(".claude").join("projects")
}

/// 模型窗口表（[models] → 内置表，与 resolve_context_window 同链），
/// 供 scanner 从 transcript 累计 token 估算 used_pct。
pub fn model_windows_from_config(config: &AppConfig) -> HashMap<String, u64> {
    let mut out = HashMap::new();
    let mut ids: Vec<String> = config.models.keys().cloned().collect();
    for id in pricing::builtin_models().keys() {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    for id in ids {
        if let Some(w) = pricing::model_window(config, &id) {
            out.insert(id, w);
        }
    }
    out
}

/// 独立进程的增量扫描器。
pub struct WindowsScanner {
    projects_root: PathBuf,
    /// 增量尾读位置缓存：path → reader（进程内常驻，只解析新增字节）。
    readers: HashMap<PathBuf, TranscriptReader>,
    /// 语言合并单价表（zh → cny）。
    pricing: PricingTable,
    /// 模型窗口表（used_pct 估算）。
    model_windows: HashMap<String, u64>,
}

impl WindowsScanner {
    pub fn new(
        projects_root: PathBuf,
        pricing: PricingTable,
        model_windows: HashMap<String, u64>,
    ) -> Self {
        Self {
            projects_root,
            readers: HashMap::new(),
            pricing,
            model_windows,
        }
    }

    /// 全量扫描（真实时钟 + 默认窗口）。
    pub fn scan(&mut self) -> Vec<WindowInfo> {
        self.scan_with(state::now_secs(), SCAN_WINDOW_SECS)
    }

    /// 参数化扫描（测试注入 now/窗口，避免 filetime 依赖）。
    fn scan_with(&mut self, now: u64, window_secs: u64) -> Vec<WindowInfo> {
        let mut candidates = collect_candidates(&self.projects_root, now, window_secs);
        candidates.sort_by(|a, b| b.1.cmp(&a.1)); // mtime 新 → 旧
        let mut out: Vec<WindowInfo> = Vec::new();
        for (path, mtime) in &candidates {
            let reader = self
                .readers
                .entry(path.clone())
                .or_insert_with(|| TranscriptReader::new(path.clone()));
            let summary = reader.read_updates();
            out.push(window_info_from(
                path, *mtime, &summary, &self.pricing, &self.model_windows, now,
            ));
        }
        // 位置缓存只清理已删除文件（窗口恢复写入时 last_pos 仍有效）
        self.readers.retain(|p, _| p.exists());
        sort_windows(&mut out);
        out
    }
}

/// 递归收集 projects_root 下 mtime 在 [now-window, now] 内的 *.jsonl。
fn collect_candidates(root: &Path, now: u64, window_secs: u64) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return out;
    }
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                if let Some(m) = file_mtime_secs(&p) {
                    if now.saturating_sub(m) <= window_secs {
                        out.push((p, m));
                    }
                }
            }
        }
    }
    out
}

/// 尾读摘要 → WindowInfo（单价成本 ≈，无透传值可回退）。
fn window_info_from(
    path: &Path,
    mtime: u64,
    summary: &TranscriptSummary,
    pricing: &PricingTable,
    model_windows: &HashMap<String, u64>,
    now: u64,
) -> WindowInfo {
    let model = summary.model.clone().unwrap_or_default();
    let t_in = summary.total_tokens.input;
    let t_out = summary.total_tokens.output;
    let cr = summary.total_tokens.cache_read;
    let cc = summary.total_tokens.cache_created;
    let cost = pricing
        .get(&model)
        .map(|p| {
            p.input * t_in as f64
                + p.output * t_out as f64
                + p.cache_read * cr as f64
                + p.cache_creation * cc as f64
        })
        .unwrap_or(0.0);
    let window = model_windows.get(&model).copied().unwrap_or(0);
    let used_pct = if window > 0 {
        ((t_in + t_out) as f64 / window as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    let path_str = path.to_string_lossy().into_owned();
    WindowInfo {
        key: window_key(Some(&path_str)),
        dir_name: window_dir_name(Some(&path_str)),
        status: status_for(now, mtime).unwrap_or(WindowStatus::Ended),
        model,
        used_pct,
        tokens_in: t_in,
        tokens_out: t_out,
        cache_read: cr,
        cache_created: cc,
        cost,
        agent_count: summary.agents.len(),
        ts: mtime,
        corrupt: false,
        data_source: "reported".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_proj(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("hud-scanner-{}-{}", std::process::id(), name))
    }

    fn write_assistant(path: &Path, model: &str, input: u64, output: u64) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let line = format!(
            r#"{{"type":"assistant","message":{{"usage":{{"input_tokens":{input},"output_tokens":{output}}},"model":"{model}"}},"timestamp":"2026-07-31T10:01:00Z"}}"#
        );
        std::fs::write(path, format!("{line}\n")).unwrap();
    }

    fn scanner(proj: &Path) -> WindowsScanner {
        let mut pricing = PricingTable::new();
        pricing.insert(
            "m".to_string(),
            PriceEntry {
                input: 1.0e-6,
                output: 2.0e-6,
                cache_read: 0.0,
                cache_creation: 0.0,
            },
        );
        WindowsScanner::new(
            proj.to_path_buf(),
            pricing,
            HashMap::from([("m".to_string(), 200_000)]),
        )
    }

    #[test]
    fn discovers_active_transcript_with_model_and_cost() {
        let proj = tmp_proj("active");
        let _ = std::fs::remove_dir_all(&proj);
        let path = proj.join("D--workspace-proj").join("s.jsonl");
        write_assistant(&path, "m", 1000, 500);
        let mut s = scanner(&proj);
        let m = file_mtime_secs(&path).unwrap();
        let wins = s.scan_with(m, 600); // now = mtime → age 0 → Active
        assert_eq!(wins.len(), 1);
        let w = &wins[0];
        assert_eq!(w.dir_name, "D--workspace-proj");
        assert_eq!(w.model, "m");
        assert_eq!(w.tokens_in, 1000);
        assert_eq!(w.tokens_out, 500);
        assert!(
            (w.cost - (1000.0 * 1.0e-6 + 500.0 * 2.0e-6)).abs() < 1e-12,
            "单价成本: {}",
            w.cost
        );
        assert!((w.used_pct - 0.75).abs() < 1e-9, "1500/200000: {}", w.used_pct);
        assert_eq!(w.status, WindowStatus::Active);
        assert_eq!(w.data_source, "reported");
        assert_eq!(w.key.len(), 16);
        assert_eq!(w.ts, m);
        let _ = std::fs::remove_dir_all(&proj);
    }

    #[test]
    fn second_scan_reads_only_new_bytes() {
        let proj = tmp_proj("incr");
        let _ = std::fs::remove_dir_all(&proj);
        let path = proj.join("proj").join("s.jsonl");
        write_assistant(&path, "m", 100, 10);
        let mut s = scanner(&proj);
        let m = file_mtime_secs(&path).unwrap();
        let wins = s.scan_with(m, 600);
        assert_eq!(wins[0].tokens_in, 100);
        // 追加新行后二次扫描：last_pos 缓存只累计新增，不重复计数
        let mut content = std::fs::read_to_string(&path).unwrap();
        content.push_str(
            "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":50,\"output_tokens\":5},\"model\":\"m\"},\"timestamp\":\"2026-07-31T10:02:00Z\"}\n",
        );
        std::fs::write(&path, content).unwrap();
        let wins2 = s.scan_with(m, 600);
        assert_eq!(wins2[0].tokens_in, 150);
        assert_eq!(wins2[0].tokens_out, 15);
        let _ = std::fs::remove_dir_all(&proj);
    }

    #[test]
    fn ignores_transcripts_older_than_window() {
        let proj = tmp_proj("stale");
        let _ = std::fs::remove_dir_all(&proj);
        let path = proj.join("proj").join("s.jsonl");
        write_assistant(&path, "m", 100, 10);
        let m = file_mtime_secs(&path).unwrap();
        let mut s = scanner(&proj);
        assert_eq!(s.scan_with(m, 600).len(), 1);
        // 窗口外（now = m + 601 > 600）→ 无候选
        assert!(s.scan_with(m + 601, 600).is_empty());
        let _ = std::fs::remove_dir_all(&proj);
    }

    #[test]
    fn unknown_model_cost_and_pct_zero() {
        let proj = tmp_proj("unknown");
        let _ = std::fs::remove_dir_all(&proj);
        let path = proj.join("proj").join("s.jsonl");
        write_assistant(&path, "no-such-model", 100, 10);
        let mut s = scanner(&proj);
        let m = file_mtime_secs(&path).unwrap();
        let wins = s.scan_with(m, 600);
        assert_eq!(wins[0].cost, 0.0, "无单价 → 0（无透传值可回退）");
        assert_eq!(wins[0].used_pct, 0.0);
        let _ = std::fs::remove_dir_all(&proj);
    }

    #[test]
    fn readers_pruned_for_deleted_files() {
        let proj = tmp_proj("prune");
        let _ = std::fs::remove_dir_all(&proj);
        let path = proj.join("proj").join("s.jsonl");
        write_assistant(&path, "m", 100, 10);
        let m = file_mtime_secs(&path).unwrap();
        let mut s = scanner(&proj);
        s.scan_with(m, 600);
        assert_eq!(s.readers.len(), 1);
        std::fs::remove_file(&path).unwrap();
        s.scan_with(m, 600);
        assert!(s.readers.is_empty(), "已删除文件的 reader 被清理");
        let _ = std::fs::remove_dir_all(&proj);
    }

    #[test]
    fn missing_root_is_empty() {
        let proj = tmp_proj("noroot");
        let _ = std::fs::remove_dir_all(&proj);
        let mut s = scanner(&proj);
        assert!(s.scan_with(1_000_000, 600).is_empty());
        let _ = std::fs::remove_dir_all(&proj);
    }
}


