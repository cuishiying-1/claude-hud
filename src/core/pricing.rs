use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::config::AppConfig;
use super::session::SessionData;
use super::transcript::{TranscriptSummary, TokenTotal};
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

/// ⑲ 实时路径成本：stdin 会话累计 token（input/output）× 单价。
/// 实时路径无 cache 数据 → 必然低估 → 命中返回 (估算值, true)；
/// 未命中 [pricing] → 透传官方 total_cost_usd（含 cache，准）；
/// 命中但 token 全 0 → 无数据可算 → 透传。
pub fn realtime_cost(data: &SessionData, pricing: &PricingTable) -> (f64, bool) {
    if let Some(price) = pricing.get(&data.model.id) {
        let t_in = data.context_window.total_input_tokens as f64;
        let t_out = data.context_window.total_output_tokens as f64;
        if t_in > 0.0 || t_out > 0.0 {
            return (price.input * t_in + price.output * t_out, true);
        }
    }
    (data.cost.total_cost_usd, false)
}

/// 把 effective cost / 估算标记 / 币种注入 WidgetConfig。
/// compact.rs 与 dashboard.rs 两条管线共用（widget 签名零改动）。
pub fn inject_cost(
    data: &SessionData,
    summary: Option<&TranscriptSummary>,
    config: &AppConfig,
    widget_config: &mut WidgetConfig,
) {
    if let Some(summary) = summary {
        let (cost, estimated) = effective_cost(data, summary, &config.pricing);
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
            config.pricing.contains_key(&data.model.id).to_string(),
        );
}

/// ⑲ 实时注入（compact/render 路径）：与 inject_cost 同组键，widget 签名零改动。
pub fn inject_cost_realtime(
    data: &SessionData,
    config: &AppConfig,
    widget_config: &mut WidgetConfig,
) {
    let (cost, estimated) = realtime_cost(data, &config.pricing);
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
            config.pricing.contains_key(&data.model.id).to_string(),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
