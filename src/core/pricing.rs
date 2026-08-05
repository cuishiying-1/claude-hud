use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::config::AppConfig;
use super::session::SessionData;
use super::transcript::TranscriptSummary;
use super::widget::WidgetConfig;

/// 模型单价（USD/token）。字段可缺省：缺省按 0 计，重算值偏小并带 ≈ 标注
/// （诚实降级，spec §6 错误矩阵）。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PriceEntry {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_creation: f64,
}

pub type PricingTable = HashMap<String, PriceEntry>;

/// 内置默认价格表（2026-07 官方价目，per-token；cache_read = 0.1×input，
/// cache_creation = 1.25×input，5-min TTL write 口径）。随二进制发布刷新；
/// 用户 [pricing] 可覆盖（见 merged_pricing）。未收录的模型走透传（无 ≈
/// 标注），诚实降级。
pub fn builtin_pricing() -> PricingTable {
    let mut t = PricingTable::new();
    for (model, input, output) in [
        ("claude-opus-4-7", 5.0e-6, 25.0e-6),
        ("claude-opus-4-5", 5.0e-6, 25.0e-6),
        ("claude-sonnet-4-6", 3.0e-6, 15.0e-6),
        ("claude-sonnet-4-5", 3.0e-6, 15.0e-6),
        ("claude-haiku-4-5-20251001", 1.0e-6, 5.0e-6),
        ("claude-haiku-4-5", 1.0e-6, 5.0e-6),
        ("claude-3-5-sonnet", 3.0e-6, 15.0e-6),
        ("claude-3-5-haiku", 0.8e-6, 4.0e-6),
        ("claude-3-opus", 15.0e-6, 75.0e-6),
    ] {
        t.insert(
            model.into(),
            PriceEntry {
                input,
                output,
                cache_read: input * 0.1,
                cache_creation: input * 1.25,
            },
        );
    }
    t
}

/// 用户 [pricing] 覆盖内置表的合并视图（① 查询唯一入口）。
pub fn merged_pricing(config: &AppConfig) -> PricingTable {
    let mut t = builtin_pricing();
    for (k, v) in &config.pricing {
        t.insert(k.clone(), v.clone());
    }
    t
}

/// 三态成本计算（spec §2.1）：
/// - [pricing] 命中 + transcript 有累计 token → 按单价重算（≈ 标注）
/// - 未命中 → 透传 data.cost.total_cost_usd（官方价含 cache）
/// - 命中但无 transcript/token → 透传（无数据可算，不算估算）
pub fn effective_cost(
    data: &SessionData,
    summary: &TranscriptSummary,
    pricing: &PricingTable,
) -> (f64, bool) {
    if let Some(price) = pricing.get(&data.model.id) {
        let t = &summary.total_tokens;
        let has_tokens =
            t.input > 0 || t.output > 0 || t.cache_created > 0 || t.cache_read > 0;
        if has_tokens {
            let cost = price.input * t.input as f64
                + price.output * t.output as f64
                + price.cache_read * t.cache_read as f64
                + price.cache_creation * t.cache_created as f64;
            return (cost, true);
        }
    }
    (data.cost.total_cost_usd, false)
}

/// ⑲+② 实时路径成本：stdin 会话累计 token（input/output/cache_read/
/// cache_creation）× 单价。cache 字段为 0/缺失 → 结果与旧公式一致（回归
/// 不变）；命中返回 (估算值, true)；未命中（含内置表）→ 透传官方
/// total_cost_usd（含 cache，准）；命中但 token 全 0 → 无数据可算 → 透传。
pub fn realtime_cost(data: &SessionData, pricing: &PricingTable) -> (f64, bool) {
    if let Some(price) = pricing.get(&data.model.id) {
        let t_in = data.context_window.total_input_tokens as f64;
        let t_out = data.context_window.total_output_tokens as f64;
        let t_cr = data.context_window.current_usage.cache_read_input_tokens as f64;
        let t_cc = data.context_window.current_usage.cache_creation_input_tokens as f64;
        if t_in > 0.0 || t_out > 0.0 || t_cr > 0.0 || t_cc > 0.0 {
            return (
                price.input * t_in
                    + price.output * t_out
                    + price.cache_read * t_cr
                    + price.cache_creation * t_cc,
                true,
            );
        }
    }
    (data.cost.total_cost_usd, false)
}

/// ⑦ 工具级成本归因排行（估算路径：无逐工具 token）——
/// per_call = (input×in_p + output×out_p + cache_read×cr_p + cache_creation×cc_p)
/// ÷ 总调用数；tool[t] = per_call × calls[t]，成本降序。
/// 模型未命中定价 → None（该段 `—`）；零调用或零 token → Some(空)。
pub fn tool_cost_ranking(
    summary: &TranscriptSummary,
    pricing: &PricingTable,
    model_id: &str,
) -> Option<Vec<(String, usize, f64)>> {
    let price = pricing.get(model_id)?;
    let total_calls: usize = summary.tool_counts.values().sum();
    if total_calls == 0 {
        return Some(vec![]);
    }
    let t = &summary.total_tokens;
    let total_cost = price.input * t.input as f64
        + price.output * t.output as f64
        + price.cache_read * t.cache_read as f64
        + price.cache_creation * t.cache_created as f64;
    if total_cost <= 0.0 {
        return Some(vec![]);
    }
    let per_call = total_cost / total_calls as f64;
    let mut rows: Vec<(String, usize, f64)> = summary
        .tool_counts
        .iter()
        .map(|(tool, calls)| (tool.clone(), *calls, per_call * *calls as f64))
        .collect();
    rows.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    Some(rows)
}

/// 把 effective cost / 估算标记 / 币种注入 WidgetConfig。
/// compact.rs 与 dashboard.rs 两条管线共用（widget 签名零改动）。
pub fn inject_cost(
    data: &SessionData,
    summary: Option<&TranscriptSummary>,
    config: &AppConfig,
    widget_config: &mut WidgetConfig,
) {
    let merged = merged_pricing(config);
    if let Some(summary) = summary {
        let (cost, estimated) = effective_cost(data, summary, &merged);
        widget_config
            .values
            .insert("effective_cost".into(), cost.to_string());
        widget_config
            .values
            .insert("cost_estimated".into(), estimated.to_string());
    }
    widget_config
        .values
        .insert("currency_symbol".into(), config.currency_symbol.clone());
    widget_config
        .values
        .insert(
            "pricing_configured".into(),
            merged.contains_key(&data.model.id).to_string(),
        );
}

/// ⑲ 实时注入（compact/render 路径）：与 inject_cost 同组键，widget 签名零改动。
pub fn inject_cost_realtime(
    data: &SessionData,
    config: &AppConfig,
    widget_config: &mut WidgetConfig,
) {
    let merged = merged_pricing(config);
    let (cost, estimated) = realtime_cost(data, &merged);
    widget_config
        .values
        .insert("effective_cost".into(), cost.to_string());
    widget_config
        .values
        .insert("cost_estimated".into(), estimated.to_string());
    widget_config
        .values
        .insert("currency_symbol".into(), config.currency_symbol.clone());
    widget_config
        .values
        .insert(
            "pricing_configured".into(),
            merged.contains_key(&data.model.id).to_string(),
        );
    // ⑳ 预算档位（cost_display 组尾显示占比）：cap 为 0 时前端隐藏。
    widget_config
        .values
        .insert("budget_cap_usd".into(), config.budget.cap_usd.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transcript::TokenTotal;

    fn session(model: &str, official_cost: f64) -> SessionData {
        let json = format!(
            r#"{{"model":{{"id":"{model}","display_name":"{model}"}},
                "context_window":{{"used_percentage":1,"total_input_tokens":1,
                "context_window_size":200000}},
                "cost":{{"total_cost_usd":{official_cost},"total_duration_ms":1}}}}"#
        );
        SessionData::from_stdin_json(&json).unwrap()
    }

    fn summary_with_tokens(
        input: u64,
        output: u64,
        cache_read: u64,
        cache_created: u64,
    ) -> TranscriptSummary {
        let mut s = TranscriptSummary::default();
        s.total_tokens = TokenTotal {
            input,
            output,
            cache_created,
            cache_read,
        };
        s
    }

    #[test]
    fn hit_with_tokens_recomputes_and_marks_estimated() {
        let data = session("m1", 9.99);
        let summary = summary_with_tokens(1_000_000, 500_000, 100_000, 10_000);
        let mut pricing = PricingTable::new();
        pricing.insert(
            "m1".into(),
            PriceEntry {
                input: 1e-6,
                output: 2e-6,
                cache_read: 0.5e-6,
                cache_creation: 2.5e-6,
            },
        );
        let (cost, estimated) = effective_cost(&data, &summary, &pricing);
        // 1.0 + 1.0 + 0.05 + 0.025
        assert!((cost - 2.075).abs() < 1e-9);
        assert!(estimated);
    }

    #[test]
    fn miss_passthroughs_official_cost() {
        let data = session("m2", 0.034);
        let summary = summary_with_tokens(100, 100, 0, 0);
        let pricing = PricingTable::new();
        let (cost, estimated) = effective_cost(&data, &summary, &pricing);
        assert_eq!(cost, 0.034);
        assert!(!estimated);
    }

    #[test]
    fn hit_without_tokens_passthroughs() {
        let data = session("m1", 0.034);
        let summary = TranscriptSummary::default(); // 零 token
        let mut pricing = PricingTable::new();
        pricing.insert(
            "m1".into(),
            PriceEntry {
                input: 1e-6,
                ..Default::default()
            },
        );
        let (cost, estimated) = effective_cost(&data, &summary, &pricing);
        assert_eq!(cost, 0.034);
        assert!(!estimated);
    }

    #[test]
    fn partial_prices_count_missing_as_zero() {
        let data = session("m1", 9.99);
        let summary = summary_with_tokens(1000, 0, 0, 0);
        let mut pricing = PricingTable::new();
        pricing.insert(
            "m1".into(),
            PriceEntry {
                input: 1e-3,
                ..Default::default()
            },
        );
        let (cost, estimated) = effective_cost(&data, &summary, &pricing);
        assert!((cost - 1.0).abs() < 1e-12);
        assert!(estimated); // 部分单价缺失 → 值偏小但仍标 ≈（诚实）
    }

    #[test]
    fn inject_cost_adds_keys() {
        let data = session("m1", 0.5);
        let summary = summary_with_tokens(1000, 0, 0, 0);
        let mut pricing = PricingTable::new();
        pricing.insert(
            "m1".into(),
            PriceEntry {
                input: 1e-3,
                ..Default::default()
            },
        );
        let mut config = AppConfig::default();
        config.currency_symbol = "¥".into();
        config.pricing = pricing;
        let mut wc = WidgetConfig::default();
        inject_cost(&data, Some(&summary), &config, &mut wc);
        assert_eq!(wc.get_str("currency_symbol", ""), "¥");
        assert_eq!(wc.get_f64("effective_cost", -1.0), 1.0);
        assert!(wc.get_bool("cost_estimated", false));
        // 无 summary → 只注入币种，不注入成本键
        let mut wc2 = WidgetConfig::default();
        inject_cost(&data, None, &config, &mut wc2);
        assert_eq!(wc2.get_str("currency_symbol", ""), "¥");
        assert_eq!(wc2.get_f64("effective_cost", -1.0), -1.0);
    }

    fn session_with_tokens(model: &str, t_in: u64, t_out: u64, official: f64) -> SessionData {
        let json = format!(
            r#"{{"model":{{"id":"{model}","display_name":"{model}"}},
                "context_window":{{"used_percentage":1,"total_input_tokens":{t_in},
                "total_output_tokens":{t_out},"context_window_size":200000}},
                "cost":{{"total_cost_usd":{official},"total_duration_ms":1}}}}"#
        );
        SessionData::from_stdin_json(&json).unwrap()
    }

    #[test]
    fn realtime_hit_recomputes_and_marks_estimated() {
        let data = session_with_tokens("m1", 1_000_000, 500_000, 9.99);
        let mut pricing = PricingTable::new();
        pricing.insert(
            "m1".into(),
            PriceEntry { input: 1e-6, output: 2e-6, ..Default::default() },
        );
        let (cost, estimated) = realtime_cost(&data, &pricing);
        // 1.0 + 1.0（无 cache 项）
        assert!((cost - 2.0).abs() < 1e-9);
        assert!(estimated);
    }

    #[test]
    fn realtime_miss_passthroughs_official_cost() {
        let data = session_with_tokens("m2", 100, 100, 0.034);
        let (cost, estimated) = realtime_cost(&data, &PricingTable::new());
        assert_eq!(cost, 0.034);
        assert!(!estimated);
    }

    #[test]
    fn realtime_hit_without_tokens_passthroughs() {
        let data = session_with_tokens("m1", 0, 0, 0.034);
        let mut pricing = PricingTable::new();
        pricing.insert("m1".into(), PriceEntry { input: 1e-6, ..Default::default() });
        let (cost, estimated) = realtime_cost(&data, &pricing);
        assert_eq!(cost, 0.034);
        assert!(!estimated);
    }

    #[test]
    fn realtime_partial_prices_count_missing_as_zero() {
        let data = session_with_tokens("m1", 1000, 0, 9.99);
        let mut pricing = PricingTable::new();
        pricing.insert("m1".into(), PriceEntry { input: 1e-3, ..Default::default() });
        let (cost, estimated) = realtime_cost(&data, &pricing);
        assert!((cost - 1.0).abs() < 1e-12);
        assert!(estimated);
    }

    #[test]
    fn builtin_covers_mainstream_models() {
        let b = builtin_pricing();
        let sonnet = b.get("claude-sonnet-4-6").expect("sonnet-4-6 in builtin");
        assert!((sonnet.input - 3e-6).abs() < 1e-12);
        assert!((sonnet.output - 15e-6).abs() < 1e-12);
        assert!((sonnet.cache_read - 0.3e-6).abs() < 1e-12);
        assert!((sonnet.cache_creation - 3.75e-6).abs() < 1e-12);
        let haiku = b.get("claude-haiku-4-5-20251001").expect("haiku dated id");
        assert!((haiku.input - 1e-6).abs() < 1e-12);
        assert!((haiku.output - 5e-6).abs() < 1e-12);
        let opus = b.get("claude-opus-4-7").expect("opus-4-7 in builtin");
        assert!((opus.input - 5e-6).abs() < 1e-12);
        assert!((opus.output - 25e-6).abs() < 1e-12);
    }

    #[test]
    fn merged_pricing_user_overrides_builtin() {
        let mut config = AppConfig::default();
        let mut user = PricingTable::new();
        user.insert(
            "claude-sonnet-4-6".into(),
            PriceEntry {
                input: 9e-6,
                ..Default::default()
            },
        );
        config.pricing = user;
        let m = merged_pricing(&config);
        // 用户值优先
        assert!((m.get("claude-sonnet-4-6").unwrap().input - 9e-6).abs() < 1e-12);
        // 内置补齐其他模型
        assert!(m.get("claude-opus-4-7").is_some());
        // 用户新增模型保留
        assert!(m.get("deepseek-v4-flash").is_none());
    }

    #[test]
    fn builtin_hit_marks_estimated_in_realtime() {
        let data = session_with_tokens("claude-sonnet-4-6", 100_000, 50_000, 9.99);
        let (cost, estimated) = realtime_cost(&data, &builtin_pricing());
        // 0.30 + 0.75（无 cache 字段）
        assert!((cost - 1.05).abs() < 1e-9);
        assert!(estimated);
    }

    #[test]
    fn unknown_model_still_passthroughs() {
        let data = session_with_tokens("deepseek-v4-flash", 100_000, 50_000, 9.99);
        let (cost, estimated) = realtime_cost(&data, &builtin_pricing());
        assert_eq!(cost, 9.99);
        assert!(!estimated);
    }

    #[test]
    fn realtime_cache_fields_weighted_by_prices() {
        // 带 cache 字段的 stdin：input 100k / output 50k / cache_read 20k / cache_creation 30k
        let json = r#"{"model":{"id":"m","display_name":"m"},
            "context_window":{"used_percentage":1,"total_input_tokens":100000,
            "total_output_tokens":50000,"context_window_size":200000,
            "current_usage":{"input_tokens":100000,"output_tokens":50000,
            "cache_read_input_tokens":20000,"cache_creation_input_tokens":30000}},
            "cost":{"total_cost_usd":9.99,"total_duration_ms":1}}"#;
        let data = SessionData::from_stdin_json(json).unwrap();
        let mut pricing = PricingTable::new();
        pricing.insert(
            "m".into(),
            PriceEntry {
                input: 1e-6,
                output: 2e-6,
                cache_read: 0.1e-6,
                cache_creation: 2.5e-6,
            },
        );
        let (cost, estimated) = realtime_cost(&data, &pricing);
        // 0.10 + 0.10 + 0.002 + 0.075
        assert!((cost - 0.277).abs() < 1e-9);
        assert!(estimated);
    }

    #[test]
    fn realtime_no_cache_fields_regression_unchanged() {
        let data = session_with_tokens("m1", 1_000_000, 500_000, 9.99);
        let mut pricing = PricingTable::new();
        pricing.insert(
            "m1".into(),
            PriceEntry {
                input: 1e-6,
                output: 2e-6,
                ..Default::default()
            },
        );
        let (cost, estimated) = realtime_cost(&data, &pricing);
        assert!((cost - 2.0).abs() < 1e-9);
        assert!(estimated);
    }

    #[test]
    fn inject_cost_adds_pricing_configured_flag() {
        let data = session("m1", 0.5);
        let mut pricing = PricingTable::new();
        pricing.insert("m1".into(), PriceEntry::default());
        let mut config = AppConfig::default();
        config.pricing = pricing;
        let mut wc = WidgetConfig::default();
        inject_cost(&data, None, &config, &mut wc);
        assert!(wc.get_bool("pricing_configured", false));
        let mut wc2 = WidgetConfig::default();
        inject_cost(&data, None, &AppConfig::default(), &mut wc2);
        assert!(!wc2.get_bool("pricing_configured", true));
    }

    fn summary_with_tools(tools: &[(&str, usize)], input: u64, output: u64) -> TranscriptSummary {
        let mut s = TranscriptSummary::default();
        s.tool_counts = tools
            .iter()
            .map(|(t, n)| (t.to_string(), *n))
            .collect();
        s.total_tokens = super::super::transcript::TokenTotal {
            input,
            output,
            cache_read: 0,
            cache_created: 0,
        };
        s
    }

    #[test]
    fn ranking_sorts_desc_and_estimates() {
        // Bash 3 + Read 2 + Skill 1 = 6 calls；input 600k output 300k；
        // sonnet 价（3e-6/15e-6）→ 总成本 1.8 + 4.5 = 6.3 → per_call 1.05
        let s = summary_with_tools(&[("Bash", 3), ("Read", 2), ("Skill", 1)], 600_000, 300_000);
        let mut pricing = PricingTable::new();
        pricing.insert(
            "claude-sonnet-4-6".into(),
            PriceEntry {
                input: 3e-6,
                output: 15e-6,
                ..Default::default()
            },
        );
        let rows = tool_cost_ranking(&s, &pricing, "claude-sonnet-4-6")
            .expect("model priced");
        assert_eq!(rows.len(), 3);
        // 降序：Bash 3.15 > Read 2.10 > Skill 1.05
        assert_eq!(rows[0].0, "Bash");
        assert_eq!(rows[0].1, 3);
        assert!((rows[0].2 - 3.15).abs() < 1e-9);
        assert_eq!(rows[1].0, "Read");
        assert!((rows[1].2 - 2.10).abs() < 1e-9);
        assert_eq!(rows[2].0, "Skill");
        assert!((rows[2].2 - 1.05).abs() < 1e-9);
    }

    #[test]
    fn ranking_unknown_model_returns_none() {
        let s = summary_with_tools(&[("Bash", 3)], 600_000, 300_000);
        let rows = tool_cost_ranking(&s, &PricingTable::new(), "deepseek-v4-flash");
        assert!(rows.is_none());
    }

    #[test]
    fn ranking_zero_calls_returns_empty() {
        let s = summary_with_tools(&[], 600_000, 300_000);
        let rows = tool_cost_ranking(&s, &builtin_pricing(), "claude-sonnet-4-6");
        assert_eq!(rows, Some(vec![]));
    }

    #[test]
    fn ranking_zero_tokens_returns_empty() {
        let s = summary_with_tools(&[("Bash", 3)], 0, 0);
        let rows = tool_cost_ranking(&s, &builtin_pricing(), "claude-sonnet-4-6");
        assert_eq!(rows, Some(vec![]));
    }
}
