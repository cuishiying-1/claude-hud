use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::config::AlertsConfig;
use crate::core::session::SessionData;

/// Notification kinds, keyed by the same strings in state.json's alerts
/// segment (snake_case, e.g. "cost_threshold").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    ContextCritical,
    CostThreshold,
    RateLimit,
}

/// Cross-process cooldown state. render 是唯一权威：从 state.json 加载、
/// 判定后回写；dashboard 只在启动时 seed 一次、运行期仅内存。
#[derive(Debug, Clone, Default)]
pub struct AlertCooldown {
    last_fired: HashMap<AlertKind, u64>,
}

impl AlertCooldown {
    /// Seed from persisted state (render) or the initial snapshot (dashboard).
    pub fn from_state(alerts: &HashMap<AlertKind, u64>) -> Self {
        Self { last_fired: alerts.clone() }
    }

    /// Persistable view of the cooldown map (state.json `alerts` segment).
    pub fn to_state(&self) -> HashMap<AlertKind, u64> {
        self.last_fired.clone()
    }
}

/// Pure threshold check + cooldown. Returns kinds that fired now (threshold
/// crossed AND cooldown expired); each returned kind is marked as fired in
/// `cooldown`, so the next call within the cooldown window returns nothing.
/// Threshold 0 = disabled. No OS side effects — trivially unit-testable.
pub fn check_alerts(
    data: &SessionData,
    cfg: &AlertsConfig,
    cooldown: &mut AlertCooldown,
    now: u64,
) -> Vec<AlertKind> {
    let mut fired = Vec::new();
    if cfg.context_critical_pct > 0.0
        && data.context_window.used_percentage >= cfg.context_critical_pct
    {
        fired.push(AlertKind::ContextCritical);
    }
    if cfg.cost_threshold_usd > 0.0 && data.cost.total_cost_usd >= cfg.cost_threshold_usd {
        fired.push(AlertKind::CostThreshold);
    }
    if cfg.rate_limit_pct > 0.0
        && data.rate_limits.five_hour.used_percentage >= cfg.rate_limit_pct
    {
        fired.push(AlertKind::RateLimit);
    }
    let window = cfg.cooldown_minutes.saturating_mul(60);
    fired.retain(|kind| {
        let last = cooldown.last_fired.get(kind).copied().unwrap_or(0);
        if now.saturating_sub(last) >= window {
            cooldown.last_fired.insert(*kind, now);
            true
        } else {
            false
        }
    });
    fired
}

/// Send OS notifications for fired alerts (best-effort; notify::send logs
/// its own failures and never panics).
pub fn send_notifications(
    fired: &[AlertKind],
    data: &SessionData,
    cfg: &AlertsConfig,
    symbol: &str,
    effective_cost: f64,
) {
    for kind in fired {
        match kind {
            AlertKind::ContextCritical => {
                crate::notify::context_critical(data.context_window.used_percentage)
            }
            AlertKind::CostThreshold => {
                crate::notify::cost_threshold(effective_cost, cfg.cost_threshold_usd, symbol)
            }
            AlertKind::RateLimit => {
                crate::notify::rate_limit_warning(data.rate_limits.five_hour.used_percentage)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(pct: f64, cost: f64, rate: f64) -> SessionData {
        let json = format!(
            r#"{{"model":{{"id":"m","display_name":"m"}},
                "context_window":{{"used_percentage":{pct},"total_input_tokens":1,
                "context_window_size":100}},
                "cost":{{"total_cost_usd":{cost},"total_duration_ms":1}},
                "rate_limits":{{"five_hour":{{"used_percentage":{rate}}},
                "seven_day":{{"used_percentage":0}}}}}}"#
        );
        SessionData::from_stdin_json(&json).unwrap()
    }

    fn cfg() -> AlertsConfig {
        AlertsConfig {
            context_critical_pct: 95.0,
            cost_threshold_usd: 10.0,
            rate_limit_pct: 90.0,
            cooldown_minutes: 10,
        }
    }

    #[test]
    fn threshold_crossing_fires_once_per_cooldown() {
        let data = session(96.0, 12.0, 95.0);
        let mut cd = AlertCooldown::default();
        let fired = check_alerts(&data, &cfg(), &mut cd, 1000);
        assert_eq!(fired.len(), 3);
        // 冷却窗口内第二次调用：不再触发
        let again = check_alerts(&data, &cfg(), &mut cd, 1001);
        assert!(again.is_empty());
    }

    #[test]
    fn cooldown_expiry_refires() {
        let data = session(96.0, 0.0, 0.0);
        let mut cd = AlertCooldown::default();
        check_alerts(&data, &cfg(), &mut cd, 1000);
        // 窗口 600s：10 分钟前触发过 → 重新触发
        let fired = check_alerts(&data, &cfg(), &mut cd, 1601);
        assert!(fired.contains(&AlertKind::ContextCritical));
    }

    #[test]
    fn zero_threshold_disables_alert() {
        let mut c = cfg();
        c.context_critical_pct = 0.0;
        let data = session(100.0, 0.0, 0.0);
        let mut cd = AlertCooldown::default();
        assert!(check_alerts(&data, &c, &mut cd, 1).is_empty());
    }

    #[test]
    fn from_state_to_state_round_trip() {
        let mut map = HashMap::new();
        map.insert(AlertKind::CostThreshold, 42);
        let cd = AlertCooldown::from_state(&map);
        assert_eq!(cd.to_state().get(&AlertKind::CostThreshold), Some(&42));
    }
}
