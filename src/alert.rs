use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::core::config::{AlertsConfig, BudgetConfig};
use crate::core::session::SessionData;

/// Notification kinds, keyed by the same strings in state.json's alerts
/// segment (snake_case, e.g. "cost_threshold").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    ContextCritical,
    CostThreshold,
    RateLimit,
    Budget,
    Compaction,
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

    /// Read the last-fired timestamp for a kind (0 = never fired).
    pub fn fired_at(&self, kind: AlertKind) -> u64 {
        self.last_fired.get(&kind).copied().unwrap_or(0)
    }

    /// Record a fire timestamp (跨进程冷却写入，随 state.alerts 持久化)。
    pub fn mark_fired(&mut self, kind: AlertKind, now: u64) {
        self.last_fired.insert(kind, now);
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
    lang: crate::core::i18n::Language,
) {
    for kind in fired {
        match kind {
            AlertKind::ContextCritical => {
                crate::notify::context_critical(data.context_window.used_percentage, lang)
            }
            AlertKind::CostThreshold => {
                crate::notify::cost_threshold(effective_cost, cfg.cost_threshold_usd, symbol, lang)
            }
            AlertKind::RateLimit => {
                crate::notify::rate_limit_warning(data.rate_limits.five_hour.used_percentage, lang)
            }
            AlertKind::Budget => {}
            // ④ 压缩通知不走 send_notifications（run_pipeline 单独接线）。
            AlertKind::Compaction => {}
        }
    }
}

/// ⑳ 预算档位检查（纯函数，无 OS 副作用）：
/// cost ≥ cap×pct/100 的最高档位 > 已触发档位 → 触发；冷却窗口内不重复发
/// （档位单调 + 冷却双保险：单调防回落重发，冷却防跨进程竞态）。
/// 与 check_alerts 同用 AlertCooldown（Budget 键），触发时内部 mark_fired。
pub fn check_budget(
    cost: f64,
    cfg: &BudgetConfig,
    cooldown_minutes: u64,
    last_tier: usize,
    cooldown: &mut AlertCooldown,
    now: u64,
) -> Option<usize> {
    if cfg.cap_usd <= 0.0 || cost <= 0.0 {
        return None;
    }
    let tier = cfg
        .warn_pcts
        .iter()
        .enumerate()
        .filter(|(_, pct)| cost >= cfg.cap_usd * **pct / 100.0)
        .map(|(i, _)| i + 1)
        .max()
        .unwrap_or(0);
    if tier == 0 || tier <= last_tier {
        return None;
    }
    let window = cooldown_minutes.saturating_mul(60);
    let last = cooldown.fired_at(AlertKind::Budget);
    if last != 0 && now.saturating_sub(last) < window {
        return None;
    }
    cooldown.mark_fired(AlertKind::Budget, now);
    Some(tier)
}

/// ④ 压缩临近检查（纯函数）：eta ≤ threshold 且冷却过期 → 触发并 mark_fired。
/// 复用 AlertCooldown（Compaction 键）跨进程去重；threshold 0 = 关闭。
/// eta None（数据不足/速率为 0）→ 不触发。
pub fn check_compaction(
    eta_minutes: Option<u64>,
    threshold_minutes: u64,
    cooldown_minutes: u64,
    cooldown: &mut AlertCooldown,
    now: u64,
) -> bool {
    if threshold_minutes == 0 {
        return false;
    }
    let Some(eta) = eta_minutes else { return false };
    if eta > threshold_minutes {
        return false;
    }
    let window = cooldown_minutes.saturating_mul(60);
    let last = cooldown.fired_at(AlertKind::Compaction);
    if last != 0 && now.saturating_sub(last) < window {
        return false;
    }
    cooldown.mark_fired(AlertKind::Compaction, now);
    true
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
            compaction_eta_minutes: 15,
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

    use crate::core::config::BudgetConfig;

    fn budget_cfg() -> BudgetConfig {
        BudgetConfig { cap_usd: 5.0, warn_pcts: vec![50.0, 80.0, 100.0] }
    }

    #[test]
    fn budget_tier_progression_fires_each_tier_once() {
        let mut cd = AlertCooldown::default();
        // 40% → 无档位
        assert!(check_budget(2.0, &budget_cfg(), 10, 0, &mut cd, 1000).is_none());
        // 60% ≥ 50% → tier 1（首触发：fired_at=0 视为过期）
        assert_eq!(check_budget(3.0, &budget_cfg(), 10, 0, &mut cd, 1001), Some(1));
        // 同 tier（回落再升）→ 单调不发
        assert!(check_budget(3.0, &budget_cfg(), 10, 1, &mut cd, 1002).is_none());
        // 冷却窗口内档位更高也被挡（双保险：冷却优先）
        assert!(check_budget(4.5, &budget_cfg(), 10, 1, &mut cd, 1003).is_none());
        // 冷却过期（now - last ≥ 600）且档位更高 → tier 2
        assert_eq!(check_budget(4.5, &budget_cfg(), 10, 1, &mut cd, 1700), Some(2));
        // 再跨窗口 → tier 3
        assert_eq!(check_budget(6.0, &budget_cfg(), 10, 2, &mut cd, 2400), Some(3));
    }

    #[test]
    fn budget_cooldown_window_blocks_refire() {
        let mut cd2 = AlertCooldown::default();
        assert_eq!(check_budget(6.0, &budget_cfg(), 10, 0, &mut cd2, 1000), Some(3));
        // 窗口 600s 内：last_tier 传低值模拟跨进程竞态 → 冷却挡（双保险第二层）
        assert!(check_budget(6.0, &budget_cfg(), 10, 0, &mut cd2, 1500).is_none());
        // 冷却过期（now - last >= 600）且档位更高 → 重发
        assert_eq!(check_budget(7.0, &budget_cfg(), 10, 0, &mut cd2, 2000), Some(3));
    }

    #[test]
    fn budget_disabled_when_cap_zero_or_cost_zero() {
        let mut cd = AlertCooldown::default();
        let off = BudgetConfig { cap_usd: 0.0, warn_pcts: vec![50.0] };
        assert!(check_budget(100.0, &off, 10, 0, &mut cd, 1).is_none());
        assert!(check_budget(0.0, &budget_cfg(), 10, 0, &mut cd, 1).is_none());
    }

    #[test]
    fn budget_warn_pcts_out_of_order_converges_to_highest() {
        let mut cd = AlertCooldown::default();
        let messy = BudgetConfig { cap_usd: 10.0, warn_pcts: vec![100.0, 50.0, 80.0] };
        // 6.0：仅 50%（index 1）满足 → tier 2（按枚举序映射档位号）
        assert_eq!(check_budget(6.0, &messy, 10, 0, &mut cd, 1), Some(2));
        // 11.0：三档全满足 → 收敛到最高档位 tier 3（不按乱序 index 0 判为 1）
        let mut cd2 = AlertCooldown::default();
        assert_eq!(check_budget(11.0, &messy, 10, 0, &mut cd2, 1), Some(3));
    }

    #[test]
    fn compaction_fires_when_eta_below_threshold() {
        let mut cd = AlertCooldown::default();
        // eta 10min < threshold 15min → 触发
        assert!(check_compaction(Some(10), 15, 10, &mut cd, 1000));
        // 冷却窗口内（同 now 再查）→ 不重复
        assert!(!check_compaction(Some(5), 15, 10, &mut cd, 1001));
    }

    #[test]
    fn compaction_eta_above_threshold_no_fire() {
        let mut cd = AlertCooldown::default();
        assert!(!check_compaction(Some(20), 15, 10, &mut cd, 1000));
        assert!(!check_compaction(None, 15, 10, &mut cd, 1001)); // 数据不足
    }

    #[test]
    fn compaction_disabled_when_threshold_zero() {
        let mut cd = AlertCooldown::default();
        assert!(!check_compaction(Some(1), 0, 10, &mut cd, 1000));
    }

    #[test]
    fn compaction_cooldown_expiry_refires() {
        let mut cd = AlertCooldown::default();
        assert!(check_compaction(Some(10), 15, 10, &mut cd, 1000));
        // 窗口 600s 过期后重发
        assert!(check_compaction(Some(8), 15, 10, &mut cd, 1601));
    }
}
