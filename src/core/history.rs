use rusqlite::Connection;
use std::path::PathBuf;

use super::session::SessionData;

/// Cross-session history stored in SQLite.
pub struct HistoryStore {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub id: i64,
    pub started_at: String,
    pub duration_secs: u64,
    pub total_cost_usd: f64,
    pub total_tokens: u64,
    pub agent_count: usize,
    pub lines_added: u64,
    pub lines_removed: u64,
    pub mod_used: String,
}

#[derive(Debug, Clone, Default)]
pub struct WeeklyStats {
    pub total_cost: f64,
    pub total_tokens: u64,
    pub total_sessions: usize,
    pub avg_duration_min: f64,
    pub avg_agents_per_session: f64,
}

/// ㉑ 周报五指标（MAX 口径，与 weekly_stats 的 AVG 口径独立）。
#[derive(Debug, Clone, Default)]
pub struct WeeklyReport {
    pub sessions: usize,
    pub total_cost: f64,
    pub total_tokens: u64,
    pub longest_duration_secs: u64,
    pub highest_cost_usd: f64,
}

impl HistoryStore {
    /// Open or create the history database.
    pub fn open() -> Result<Self, String> {
        let path = Self::db_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {}", e))?;
        }
        let conn = Connection::open(&path).map_err(|e| format!("open db: {}", e))?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Create the sessions table if missing（open 与内存测试共用）。
    fn init_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS sessions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    started_at TEXT NOT NULL DEFAULT (datetime('now')),
                    duration_secs INTEGER NOT NULL DEFAULT 0,
                    total_cost_usd REAL NOT NULL DEFAULT 0.0,
                    total_tokens INTEGER NOT NULL DEFAULT 0,
                    agent_count INTEGER NOT NULL DEFAULT 0,
                    lines_added INTEGER NOT NULL DEFAULT 0,
                    lines_removed INTEGER NOT NULL DEFAULT 0,
                    mod_used TEXT NOT NULL DEFAULT ''
                );",
            )
            .map_err(|e| format!("create table: {}", e))
    }

    fn db_path() -> Result<PathBuf, String> {
        let base = dirs::home_dir().ok_or_else(|| "no home dir".to_string())?;
        Ok(base
            .join(".claude")
            .join("plugins")
            .join("claude-hud")
            .join("history.db"))
    }

    /// Record a session snapshot.
    pub fn record_session(
        &self,
        data: &SessionData,
        agent_count: usize,
        mod_name: &str,
    ) -> Result<(), String> {
        let dur_secs = data.cost.total_duration_ms / 1000;
        let total_tokens =
            data.context_window.total_input_tokens + data.context_window.total_output_tokens;

        self.conn
            .execute(
                "INSERT INTO sessions (duration_secs, total_cost_usd, total_tokens, agent_count, lines_added, lines_removed, mod_used)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    dur_secs,
                    data.cost.total_cost_usd,
                    total_tokens,
                    agent_count,
                    data.cost.total_lines_added,
                    data.cost.total_lines_removed,
                    mod_name,
                ],
            )
            .map_err(|e| format!("insert session: {}", e))?;

        Ok(())
    }

    /// Get weekly aggregate stats.
    pub fn weekly_stats(&self) -> Result<WeeklyStats, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT COUNT(*), COALESCE(SUM(total_cost_usd),0), COALESCE(SUM(total_tokens),0),
                        COALESCE(AVG(duration_secs),0), COALESCE(AVG(agent_count),0)
                 FROM sessions WHERE started_at >= datetime('now', '-7 days')",
            )
            .map_err(|e| format!("prepare: {}", e))?;

        let result = stmt
            .query_row([], |row| {
                Ok(WeeklyStats {
                    total_sessions: row.get(0)?,
                    total_cost: row.get(1)?,
                    total_tokens: row.get::<_, i64>(2)? as u64,
                    avg_duration_min: row.get::<_, f64>(3)? / 60.0,
                    avg_agents_per_session: row.get(4)?,
                })
            })
            .map_err(|e| format!("query: {}", e))?;

        Ok(result)
    }

    /// Get the last N sessions.
    pub fn recent_sessions(&self, limit: usize) -> Result<Vec<SessionRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, started_at, duration_secs, total_cost_usd, total_tokens,
                        agent_count, lines_added, lines_removed, mod_used
                 FROM sessions ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| format!("prepare: {}", e))?;

        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(SessionRecord {
                    id: row.get(0)?,
                    started_at: row.get(1)?,
                    duration_secs: row.get(2)?,
                    total_cost_usd: row.get(3)?,
                    total_tokens: row.get::<_, i64>(4)? as u64,
                    agent_count: row.get::<_, i64>(5)? as usize,
                    lines_added: row.get::<_, i64>(6)? as u64,
                    lines_removed: row.get::<_, i64>(7)? as u64,
                    mod_used: row.get(8)?,
                })
            })
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    /// Get daily cost trend for the last 7 days.
    pub fn daily_cost_trend(&self) -> Result<Vec<(String, f64)>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT date(started_at), COALESCE(SUM(total_cost_usd),0)
                 FROM sessions WHERE started_at >= datetime('now', '-7 days')
                 GROUP BY date(started_at) ORDER BY date(started_at)",
            )
            .map_err(|e| format!("prepare: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    /// ㉑ 近 7 天周报聚合：会话数 / 成本合计 / token 总量 / 最长会话时长 / 最高成本单会话。
    pub fn weekly_report(&self) -> Result<WeeklyReport, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT COUNT(*), COALESCE(SUM(total_cost_usd),0), COALESCE(SUM(total_tokens),0),
                        COALESCE(MAX(duration_secs),0), COALESCE(MAX(total_cost_usd),0)
                 FROM sessions WHERE started_at >= datetime('now', '-7 days')",
            )
            .map_err(|e| format!("prepare: {}", e))?;
        let result = stmt
            .query_row([], |row| {
                Ok(WeeklyReport {
                    sessions: row.get(0)?,
                    total_cost: row.get(1)?,
                    total_tokens: row.get::<_, i64>(2)? as u64,
                    longest_duration_secs: row.get::<_, i64>(3)? as u64,
                    highest_cost_usd: row.get(4)?,
                })
            })
            .map_err(|e| format!("query: {}", e))?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// 内存库 + 同一 schema（open() 抽取的 init_schema 复用）。
    fn mem_store() -> HistoryStore {
        let conn = Connection::open_in_memory().unwrap();
        let store = HistoryStore { conn };
        store.init_schema().unwrap();
        store
    }

    fn session(cost: f64, tokens_in: u64, tokens_out: u64, dur_ms: u64) -> SessionData {
        let json = format!(
            r#"{{"model":{{"id":"m","display_name":"m"}},
                "context_window":{{"used_percentage":1,"total_input_tokens":{tokens_in},
                "total_output_tokens":{tokens_out},"context_window_size":200000}},
                "cost":{{"total_cost_usd":{cost},"total_duration_ms":{dur_ms}}}}}"#
        );
        SessionData::from_stdin_json(&json).unwrap()
    }

    #[test]
    fn weekly_report_aggregates_five_metrics() {
        let store = mem_store();
        store.record_session(&session(1.0, 1000, 500, 60_000), 1, "glacier").unwrap();
        store.record_session(&session(3.5, 2000, 800, 3_600_000), 2, "glacier").unwrap();
        let r = store.weekly_report().unwrap();
        assert_eq!(r.sessions, 2);
        assert!((r.total_cost - 4.5).abs() < 1e-9);
        assert_eq!(r.total_tokens, 4300);
        assert_eq!(r.longest_duration_secs, 3600);
        assert!((r.highest_cost_usd - 3.5).abs() < 1e-9);
    }

    #[test]
    fn weekly_report_empty_db_is_all_zeros() {
        let store = mem_store();
        let r = store.weekly_report().unwrap();
        assert_eq!(r.sessions, 0);
        assert_eq!(r.total_tokens, 0);
        assert_eq!(r.longest_duration_secs, 0);
        assert!((r.highest_cost_usd - 0.0).abs() < 1e-9);
    }
}
