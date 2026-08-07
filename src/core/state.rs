use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use super::config::AppConfig;
use super::session::{CurrentUsage, SessionData};
use super::transcript::TranscriptSegment;
use crate::alert::AlertKind;

/// Five-segment shared state file: the only data layer between the 5s render
/// process and long-running processes (dashboard / serve / doctor).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StateFile {
    #[serde(default)]
    pub snapshot: SnapshotSegment,
    #[serde(default)]
    pub transcript: TranscriptSegment,
    #[serde(default)]
    pub cache: CacheSegment,
    #[serde(default)]
    pub alerts: HashMap<AlertKind, u64>,
    #[serde(default)]
    pub last_error: Option<LastError>,
    /// mod use 的历史切换记录（`mod use -` 往返 toggle）。
    #[serde(default)]
    pub previous_mod: Option<String>,
    /// ⑳ 已触发的最高预算档位（1-based，单调递进；0 = 未触发）。
    #[serde(default)]
    pub budget_tier: usize,
    /// 结账去重（⑨+）：path → 最近结账时刻。同一 path 在冷却期内最多
    /// 结账一次（path 抖动 A→B→A→B 时防同一会话 double-billing）。
    /// 单槽记忆（只记最后一次）在交替振荡下相位错位，无法去重，故用表。
    #[serde(default)]
    pub checkout_billed: HashMap<String, u64>,
}

/// Last render failure, written before exit so doctor can surface it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LastError {
    #[serde(default)]
    pub ts_iso: String,
    #[serde(default)]
    pub msg: String,
}

/// Session snapshot restored by dashboard/serve when stdin is a TTY.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotSegment {
    #[serde(default)]
    pub timestamp_secs: u64,
    #[serde(default)]
    pub model: ModelSnapshot,
    #[serde(default)]
    pub context_window: ContextSnapshot,
    #[serde(default)]
    pub cost: CostSnapshot,
    #[serde(default)]
    pub rate_limits: RateLimitSnapshot,
    #[serde(default)]
    pub transcript_path: Option<String>,
    /// 结账用：快照时刻的活跃代理数（to_session 不还原该字段）。
    #[serde(default)]
    pub agent_count: usize,
    /// 数据源标注（"reported"/"fallback"）；旧快照无该字段 → 空串（视为 reported）。
    #[serde(default)]
    pub data_source: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelSnapshot {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextSnapshot {
    #[serde(default)]
    pub used_percentage: f64,
    #[serde(default)]
    pub total_input_tokens: u64,
    #[serde(default)]
    pub total_output_tokens: u64,
    #[serde(default)]
    pub context_window_size: u64,
    #[serde(default)]
    pub current_usage: CurrentUsage,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostSnapshot {
    #[serde(default)]
    pub total_cost_usd: f64,
    #[serde(default)]
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateLimitSnapshot {
    #[serde(default)]
    pub five_hour_pct: f64,
    #[serde(default)]
    pub seven_day_pct: f64,
}

/// Best-effort probe caches: values are reused without spawning processes
/// while their TTL is unexpired.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheSegment {
    #[serde(default)]
    pub git: GitCache,
    #[serde(default)]
    pub script_throttle: HashMap<String, u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitCache {
    #[serde(default)]
    pub branch: CachedValue<String>,
    #[serde(default)]
    pub dirty: CachedValue<bool>,
    #[serde(default)]
    pub ahead_behind: CachedValue<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CachedValue<T> {
    #[serde(default)]
    pub value: T,
    #[serde(default)]
    pub ts: u64,
}

impl StateFile {
    /// Read the state file; missing or corrupt files yield defaults
    /// (never a hard failure).
    pub fn read(path: &Path) -> StateFile {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return StateFile::default(),
        };
        serde_json::from_str(&content).unwrap_or_default()
    }

    /// Write the whole file atomically; creates parent dirs.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serialize state: {}", e))?;
        write_atomic(path, &json)
    }

    /// Read-modify-write: applies `f` to the on-disk state and writes it back.
    pub fn update(path: &Path, f: impl FnOnce(&mut StateFile)) -> Result<(), String> {
        let mut st = StateFile::read(path);
        f(&mut st);
        st.write(path)
    }

    /// Overlay the on-disk cache into `self` so a full-state persist never
    /// clobbers narrow cache writes made by widgets mid-pipeline.
    pub fn merge_cache_from_disk(&mut self, path: &Path) {
        let disk = StateFile::read(path);
        self.cache.git = disk.cache.git;
        self.cache.script_throttle = disk.cache.script_throttle;
    }

    /// Record a render failure: ISO8601 timestamp + message. Best-effort —
    /// a failure to persist must never mask the original render error.
    pub fn write_last_error(path: &Path, msg: &str) {
        let mut st = StateFile::read(path);
        st.last_error = Some(LastError {
            ts_iso: chrono::DateTime::<chrono::Utc>::from_timestamp(now_secs() as i64, 0)
                .unwrap_or_default()
                .to_rfc3339(),
            msg: msg.to_string(),
        });
        let _ = st.write(path);
    }
}

impl SnapshotSegment {
    /// Create a snapshot from live session data, recording the current
    /// timestamp so freshness checks know how stale the data is.
    pub fn from_session(data: &SessionData, now_secs: u64) -> Self {
        Self {
            timestamp_secs: now_secs,
            model: ModelSnapshot {
                id: data.model.id.clone(),
                display_name: data.model.display_name.clone(),
            },
            context_window: ContextSnapshot {
                used_percentage: data.context_window.used_percentage,
                total_input_tokens: data.context_window.total_input_tokens,
                total_output_tokens: data.context_window.total_output_tokens,
                context_window_size: data.context_window.context_window_size,
                current_usage: data.context_window.current_usage.clone(),
            },
            cost: CostSnapshot {
                total_cost_usd: data.cost.total_cost_usd,
                total_duration_ms: data.cost.total_duration_ms,
            },
            rate_limits: RateLimitSnapshot {
                five_hour_pct: data.rate_limits.five_hour.used_percentage,
                seven_day_pct: data.rate_limits.seven_day.used_percentage,
            },
            transcript_path: data.transcript_path.clone(),
            agent_count: data
                .subagent_status_line
                .as_ref()
                .map(|s| s.agents.len())
                .unwrap_or(0),
            data_source: data.data_source.name().to_string(),
        }
    }

    /// Reconstruct a best-effort SessionData from this snapshot.
    /// The snapshot does NOT capture total_lines_added, total_lines_removed,
    /// or subagent_status_line, so they are fabricated as 0 / None —
    /// callers that need those fields must treat them as unreliable.
    pub fn to_session(&self) -> SessionData {
        SessionData {
            model: super::session::ModelInfo {
                id: self.model.id.clone(),
                display_name: self.model.display_name.clone(),
            },
            context_window: super::session::ContextWindow {
                used_percentage: self.context_window.used_percentage,
                total_input_tokens: self.context_window.total_input_tokens,
                total_output_tokens: self.context_window.total_output_tokens,
                context_window_size: self.context_window.context_window_size,
                current_usage: self.context_window.current_usage.clone(),
            },
            cost: super::session::CostInfo {
                total_cost_usd: self.cost.total_cost_usd,
                total_duration_ms: self.cost.total_duration_ms,
                total_lines_added: 0,
                total_lines_removed: 0,
            },
            rate_limits: super::session::RateLimits {
                five_hour: super::session::RateLimitBucket {
                    used_percentage: self.rate_limits.five_hour_pct,
                },
                seven_day: super::session::RateLimitBucket {
                    used_percentage: self.rate_limits.seven_day_pct,
                },
            },
            transcript_path: self.transcript_path.clone(),
            subagent_status_line: None,
            data_source: if self.data_source == "fallback" {
                crate::core::data_source::DataSource::Fallback(std::path::PathBuf::from(
                    self.transcript_path.clone().unwrap_or_default(),
                ))
            } else {
                crate::core::data_source::DataSource::Reported
            },
        }
    }

    /// True when the snapshot is fresh enough to be presented as live data.
    pub fn is_fresh(&self, now_secs: u64) -> bool {
        self.timestamp_secs != 0
            && now_secs.saturating_sub(self.timestamp_secs) <= SNAPSHOT_MAX_AGE_SECS
    }

    /// Convenience: returns SessionData when the snapshot is fresh, or None
    /// when it has expired — used by read_current_data to pick fresh vs stale.
    pub fn to_session_if_fresh(&self, now_secs: u64) -> Option<SessionData> {
        self.is_fresh(now_secs).then(|| self.to_session())
    }
}

/// Unix epoch seconds — single time source for cache TTLs and cooldowns.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Write atomically (temp file + rename) so a crash mid-write never leaves
/// the target truncated or partially written.
pub fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, content)
        .map_err(|e| format!("write {}: {}", tmp.display(), e))?;
    fs::rename(&tmp, path)
        .map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("rename to {}: {}", path.display(), e)
        })?;
    Ok(())
}

/// Current session data: piped stdin (legacy, unchanged) or, when stdin is a
/// TTY, the freshest state.json snapshot (never blocks on the terminal).
///
/// Non-TTY 时 stdin 无输入/解析失败则回退新鲜快照（如 `! claude-hud
/// dashboard` 管道环境），避免全空面板；黑盒注入的 stdin JSON 仍然优先。
pub fn read_current_data() -> Option<SessionData> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        if let Some(d) = read_stdin_json() {
            return Some(d);
        }
    }
    let path = AppConfig::state_path().ok()?;
    let st = StateFile::read(&path);
    st.snapshot.to_session_if_fresh(now_secs())
}

fn read_stdin_json() -> Option<SessionData> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).ok()?;
    SessionData::from_stdin_json(&buf).ok()
}

/// A fresh render snapshot is considered stale after this many seconds.
pub const SNAPSHOT_MAX_AGE_SECS: u64 = 30;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("claude-hud-state-{}-{}", std::process::id(), name));
        p
    }

    fn cleanup(p: &Path) {
        let _ = std::fs::remove_file(p);
        let _ = std::fs::remove_file(p.with_extension("json.tmp"));
    }

    #[test]
    fn write_read_round_trip() {
        let path = tmp_path("roundtrip.json");
        cleanup(&path);
        let mut st = StateFile::default();
        st.snapshot.model.display_name = "deepseek-v4-flash".into();
        st.alerts.insert(AlertKind::CostThreshold, 42);
        st.checkout_billed.insert("/a.jsonl".into(), 42);
        assert!(st.write(&path).is_ok());
        let back = StateFile::read(&path);
        assert_eq!(back.snapshot.model.display_name, "deepseek-v4-flash");
        assert_eq!(back.alerts.get(&AlertKind::CostThreshold), Some(&42));
        assert_eq!(back.checkout_billed.get("/a.jsonl"), Some(&42));
        cleanup(&path);
    }

    #[test]
    fn missing_file_reads_default() {
        let path = tmp_path("missing.json");
        cleanup(&path);
        let st = StateFile::read(&path);
        assert!(st.snapshot.model.display_name.is_empty());
        assert!(st.alerts.is_empty());
    }

    #[test]
    fn corrupt_file_reads_default() {
        let path = tmp_path("corrupt.json");
        cleanup(&path);
        std::fs::write(&path, "{ not json !!").unwrap();
        let st = StateFile::read(&path);
        assert!(st.snapshot.model.display_name.is_empty());
        assert!(st.last_error.is_none());
        cleanup(&path);
    }

    #[test]
    fn snapshot_freshness_window() {
        let now = 1_000_000;
        let mut snap = SnapshotSegment::default();
        assert!(!snap.is_fresh(now)); // ts 0 = never written
        snap.timestamp_secs = now - 10;
        assert!(snap.is_fresh(now));
        snap.timestamp_secs = now - SNAPSHOT_MAX_AGE_SECS - 1;
        assert!(!snap.is_fresh(now));
        assert!(snap.to_session_if_fresh(now).is_none());
        snap.timestamp_secs = now - 5;
        assert!(snap.to_session_if_fresh(now).is_some());
    }

    #[test]
    fn update_applies_read_modify_write() {
        let path = tmp_path("update.json");
        cleanup(&path);
        StateFile::update(&path, |st| st.snapshot.model.display_name = "x".into())
            .unwrap();
        let st = StateFile::read(&path);
        assert_eq!(st.snapshot.model.display_name, "x");
        cleanup(&path);
    }

    #[test]
    fn write_last_error_round_trip() {
        let path = tmp_path("lasterr.json");
        cleanup(&path);
        StateFile::write_last_error(&path, "parse stdin JSON: boom");
        let st = StateFile::read(&path);
        let le = st.last_error.expect("last_error written");
        assert_eq!(le.msg, "parse stdin JSON: boom");
        assert!(!le.ts_iso.is_empty(), "ISO8601 timestamp set");
        assert!(le.ts_iso.starts_with("20"), "ts_iso looks like a date: {}", le.ts_iso);
        cleanup(&path);
    }

    #[test]
    fn previous_mod_round_trip() {
        let path = tmp_path("previous.json");
        cleanup(&path);
        let mut st = StateFile::default();
        st.previous_mod = Some("noir-tabbed".into());
        st.write(&path).unwrap();
        let back = StateFile::read(&path);
        assert_eq!(back.previous_mod.as_deref(), Some("noir-tabbed"));
        cleanup(&path);
    }

    #[test]
    fn snapshot_round_trips_data_source() {
        use crate::core::data_source::DataSource;
        let mut data = super::super::session::SessionData::default();
        data.transcript_path = Some("/x/s.jsonl".to_string());
        data.data_source = DataSource::Fallback(PathBuf::from("/x/s.jsonl"));
        let snap = SnapshotSegment::from_session(&data, 1);
        assert_eq!(snap.data_source, "fallback");
        let back = snap.to_session();
        assert_eq!(back.data_source, DataSource::Fallback(PathBuf::from("/x/s.jsonl")));
        // 旧快照无该字段 → 序列化 default → to_session 重建为 Reported
        let old: SnapshotSegment =
            serde_json::from_str(r#"{"timestamp_secs":1,"transcript_path":"/x/s.jsonl"}"#).unwrap();
        assert_eq!(old.data_source, "");
        assert_eq!(old.to_session().data_source, DataSource::Reported);
        // Reported 往返
        let r = SessionData::default();
        let snap2 = SnapshotSegment::from_session(&r, 1);
        assert_eq!(snap2.data_source, "reported");
    }

    #[test]
    fn from_session_counts_agents() {
        let json = r#"{
            "model": {"id": "m", "display_name": "m"},
            "context_window": {"used_percentage": 1, "total_input_tokens": 1,
                               "context_window_size": 100},
            "cost": {"total_cost_usd": 0.1, "total_duration_ms": 1},
            "subagent_status_line": {"agents": [
                {"name": "a", "model": "x"},
                {"name": "b", "model": "x"}
            ]}
        }"#;
        let data = super::super::session::SessionData::from_stdin_json(json).unwrap();
        let snap = SnapshotSegment::from_session(&data, 42);
        assert_eq!(snap.agent_count, 2);
        assert_eq!(snap.timestamp_secs, 42);
    }

    #[test]
    fn budget_tier_defaults_zero_for_old_state() {
        let old = r#"{"snapshot":{},"transcript":{},"cache":{},"alerts":{},"last_error":null}"#;
        let st: StateFile = serde_json::from_str(old).unwrap();
        assert_eq!(st.budget_tier, 0);
    }
}
