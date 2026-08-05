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
    pub model: String,
    pub transcript_path: Option<String>,
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

/// ⑭ 单周聚合（周环比用）。
#[derive(Debug, Clone, PartialEq)]
pub struct WeekAgg {
    pub cost: f64,
    pub sessions: usize,
    pub tokens: u64,
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
                    mod_used TEXT NOT NULL DEFAULT '',
                    model TEXT NOT NULL DEFAULT '',
                    transcript_path TEXT NOT NULL DEFAULT ''
                );",
            )
            .map_err(|e| format!("create table: {}", e))?;
        self.migrate()?;
        Ok(())
    }

    /// 旧库补列（model / transcript_path）：PRAGMA 检查 → ALTER ADD COLUMN。
    /// 新库 CREATE TABLE 已含新列，此路径为空操作。
    fn migrate(&self) -> Result<(), String> {
        let cols: Vec<String> = self
            .conn
            .prepare("PRAGMA table_info(sessions)")
            .map_err(|e| format!("pragma: {}", e))?
            .query_map([], |row| row.get(1))
            .map_err(|e| format!("pragma row: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        if !cols.iter().any(|c| c == "model") {
            self.conn
                .execute_batch("ALTER TABLE sessions ADD COLUMN model TEXT NOT NULL DEFAULT ''")
                .map_err(|e| format!("add model column: {}", e))?;
        }
        if !cols.iter().any(|c| c == "transcript_path") {
            self.conn
                .execute_batch(
                    "ALTER TABLE sessions ADD COLUMN transcript_path TEXT NOT NULL DEFAULT ''",
                )
                .map_err(|e| format!("add transcript_path column: {}", e))?;
        }
        Ok(())
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
    ) -> Result<(), String> {
        let dur_secs = data.cost.total_duration_ms / 1000;
        let total_tokens =
            data.context_window.total_input_tokens + data.context_window.total_output_tokens;

        self.conn
            .execute(
                "INSERT INTO sessions (duration_secs, total_cost_usd, total_tokens, agent_count,
                                       model, transcript_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    dur_secs,
                    data.cost.total_cost_usd,
                    total_tokens,
                    agent_count,
                    data.model.id,
                    data.transcript_path.as_deref().unwrap_or("")
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
                "SELECT id, started_at, duration_secs, total_cost_usd, total_tokens, agent_count,
                        model, transcript_path
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
                    model: row.get(6)?,
                    transcript_path: {
                        let s: String = row.get(7)?;
                        if s.is_empty() {
                            None
                        } else {
                            Some(s)
                        }
                    },
                })
            })
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    /// ⑤ sessions 分页列表：id 降序，可选起始日期过滤（YYYY-MM-DD 前缀比较）。
    pub fn sessions_page(
        &self,
        limit: usize,
        offset: usize,
        date_from: Option<&str>,
    ) -> Result<Vec<SessionRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, started_at, duration_secs, total_cost_usd, total_tokens, agent_count,
                        model, transcript_path
                 FROM sessions
                 WHERE (?1 IS NULL OR started_at >= ?1)
                 ORDER BY id DESC LIMIT ?2 OFFSET ?3",
            )
            .map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt
            .query_map(
                rusqlite::params![date_from, limit as i64, offset as i64],
                |row| {
                    Ok(SessionRecord {
                        id: row.get(0)?,
                        started_at: row.get(1)?,
                        duration_secs: row.get(2)?,
                        total_cost_usd: row.get(3)?,
                        total_tokens: row.get::<_, i64>(4)? as u64,
                        agent_count: row.get::<_, i64>(5)? as usize,
                        model: row.get(6)?,
                        transcript_path: {
                            let s: String = row.get(7)?;
                            if s.is_empty() {
                                None
                            } else {
                                Some(s)
                            }
                        },
                    })
                },
            )
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// ⑥ 单会话详情：按主键查，无 → None。
    pub fn session_by_id(&self, id: i64) -> Result<Option<SessionRecord>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, started_at, duration_secs, total_cost_usd, total_tokens, agent_count,
                        model, transcript_path
                 FROM sessions WHERE id = ?1",
            )
            .map_err(|e| format!("prepare: {}", e))?;
        let mut rows = stmt
            .query_map([id], |row| {
                Ok(SessionRecord {
                    id: row.get(0)?,
                    started_at: row.get(1)?,
                    duration_secs: row.get(2)?,
                    total_cost_usd: row.get(3)?,
                    total_tokens: row.get::<_, i64>(4)? as u64,
                    agent_count: row.get::<_, i64>(5)? as usize,
                    model: row.get(6)?,
                    transcript_path: {
                        let s: String = row.get(7)?;
                        if s.is_empty() {
                            None
                        } else {
                            Some(s)
                        }
                    },
                })
            })
            .map_err(|e| format!("query: {}", e))?;
        rows.next().transpose().map_err(|e| format!("row: {}", e))
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

    /// ⑭ 双周聚合：本周 vs 上周（SQLite %Y-%W 周键；上周 = now-7 天的周键，
    /// 跨年自动处理）。返回 (this_week, last_week)，无会话的周为 None。
    pub fn weekly_compare(&self) -> Result<(Option<WeekAgg>, Option<WeekAgg>), String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT strftime('%Y-%W', started_at) AS wk,
                        COUNT(*), COALESCE(SUM(total_cost_usd),0), COALESCE(SUM(total_tokens),0)
                 FROM sessions WHERE started_at >= datetime('now', '-14 days')
                 GROUP BY wk",
            )
            .map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? as usize,
                    row.get::<_, f64>(2)?,
                    row.get::<_, i64>(3)? as u64,
                ))
            })
            .map_err(|e| format!("query: {}", e))?
            .filter_map(|r| r.ok())
            .collect::<Vec<(String, usize, f64, u64)>>();
        let this_key: String = self
            .conn
            .query_row("SELECT strftime('%Y-%W', 'now')", [], |r| r.get(0))
            .map_err(|e| format!("this week key: {}", e))?;
        let last_key: String = self
            .conn
            .query_row("SELECT strftime('%Y-%W', 'now', '-7 days')", [], |r| r.get(0))
            .map_err(|e| format!("last week key: {}", e))?;
        let agg = |key: &str| {
            rows.iter()
                .find(|(wk, ..)| wk == key)
                .map(|(_, n, c, t)| WeekAgg {
                    cost: *c,
                    sessions: *n,
                    tokens: *t,
                })
        };
        Ok((agg(&this_key), agg(&last_key)))
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
        store.record_session(&session(1.0, 1000, 500, 60_000), 1).unwrap();
        store.record_session(&session(3.5, 2000, 800, 3_600_000), 2).unwrap();
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

    #[test]
    fn weekly_compare_both_weeks() {
        let store = mem_store();
        for _ in 0..2 {
            store.record_session(&session(1.0, 500, 500, 60), 2).unwrap();
        }
        for days in [8, 9] {
            let sql = format!(
                "INSERT INTO sessions (started_at, duration_secs, total_cost_usd, \
                 total_tokens, agent_count, model, transcript_path) \
                 VALUES (datetime('now', '-{} days'), 60, 2.0, 1000, 1, 'm', '')",
                days
            );
            store.conn.execute(&sql, []).unwrap();
        }
        let (this, last) = store.weekly_compare().unwrap();
        let this = this.expect("this week present");
        let last = last.expect("last week present");
        assert_eq!(this.sessions, 2);
        assert_eq!(this.cost, 2.0);
        assert_eq!(this.tokens, 2000);
        assert_eq!(last.sessions, 2);
        assert_eq!(last.cost, 4.0);
        assert_eq!(last.tokens, 2000);
    }

    #[test]
    fn weekly_compare_empty_db_none() {
        let store = mem_store();
        let (this, last) = store.weekly_compare().unwrap();
        assert_eq!(this, None);
        assert_eq!(last, None);
    }

    #[test]
    fn weekly_compare_no_last_week() {
        let store = mem_store();
        store.record_session(&session(1.0, 500, 500, 60), 2).unwrap();
        let (this, last) = store.weekly_compare().unwrap();
        assert!(this.is_some());
        assert_eq!(last, None);
    }

    #[test]
    fn sessions_page_orders_desc_and_limits() {
        let store = mem_store();
        store.record_session(&session(1.0, 1000, 500, 60_000), 1).unwrap();
        store.record_session(&session(2.0, 2000, 800, 120_000), 2).unwrap();
        store.record_session(&session(3.0, 3000, 900, 180_000), 3).unwrap();
        let page = store.sessions_page(2, 0, None).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].id, 3);
        assert_eq!(page[1].id, 2);
    }

    #[test]
    fn sessions_page_offset_skips() {
        let store = mem_store();
        store.record_session(&session(1.0, 1000, 500, 60_000), 1).unwrap();
        store.record_session(&session(2.0, 2000, 800, 120_000), 2).unwrap();
        let page = store.sessions_page(10, 1, None).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].id, 1);
    }

    #[test]
    fn sessions_page_date_filter() {
        let store = mem_store();
        store.record_session(&session(1.0, 1000, 500, 60_000), 1).unwrap();
        // started_at 默认 now；把两条会话改成不同日期再过滤
        store
            .conn
            .execute("UPDATE sessions SET started_at = '2026-08-01 10:00:00' WHERE id = 1", [])
            .unwrap();
        store.record_session(&session(2.0, 2000, 800, 120_000), 2).unwrap();
        store
            .conn
            .execute("UPDATE sessions SET started_at = '2026-08-04 10:00:00' WHERE id = 2", [])
            .unwrap();
        let page = store.sessions_page(10, 0, Some("2026-08-03")).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].id, 2);
        // 未来日期 → 空
        let none = store.sessions_page(10, 0, Some("2099-01-01")).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn session_by_id_found_and_missing() {
        let store = mem_store();
        store.record_session(&session(1.5, 1000, 500, 60_000), 2).unwrap();
        let found = store.session_by_id(1).unwrap().expect("id 1 exists");
        assert_eq!(found.id, 1);
        assert!((found.total_cost_usd - 1.5).abs() < 1e-9);
        assert_eq!(found.agent_count, 2);
        assert!(store.session_by_id(99).unwrap().is_none());
    }

    #[test]
    fn record_session_stores_model_and_transcript_path() {
        let store = mem_store();
        let mut d = session(1.0, 1000, 500, 60_000);
        d.model.id = "claude-sonnet-4-6".into();
        d.transcript_path = Some("/tmp/x.jsonl".into());
        store.record_session(&d, 1).unwrap();
        let r = store.session_by_id(1).unwrap().expect("recorded");
        assert_eq!(r.model, "claude-sonnet-4-6");
        assert_eq!(r.transcript_path.as_deref(), Some("/tmp/x.jsonl"));
    }

    #[test]
    fn legacy_schema_migrates_new_columns() {
        // 旧库：sessions 表无 model/transcript_path 列 → init_schema 后列补齐
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at TEXT NOT NULL DEFAULT (datetime('now')),
                duration_secs INTEGER NOT NULL DEFAULT 0,
                total_cost_usd REAL NOT NULL DEFAULT 0.0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                agent_count INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        let store = HistoryStore { conn };
        store.init_schema().unwrap();
        store.record_session(&session(1.0, 1000, 500, 60_000), 1).unwrap();
        let r = store.session_by_id(1).unwrap().expect("works after migrate");
        assert_eq!(r.model, "m");
        assert!(r.transcript_path.is_none());
    }
}
