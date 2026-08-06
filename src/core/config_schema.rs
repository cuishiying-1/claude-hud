use super::config::AppConfig;
use super::theme::Theme;
use super::widget::WidgetRegistry;

/// 分组：TUI 左栏 Tab / Web section。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    General,
    Display,
    Alerts,
    Budget,
}

impl Group {
    pub fn all() -> [Group; 4] {
        [Group::General, Group::Display, Group::Alerts, Group::Budget]
    }

    pub fn name(self) -> &'static str {
        match self {
            Group::General => "config.group_general",
            Group::Display => "config.group_display",
            Group::Alerts => "config.group_alerts",
            Group::Budget => "config.group_budget",
        }
    }
}

/// 字段类型（选项由 options_for 动态提供）。
#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    Text,
    Number,
    Bool,
    Choice,
    Multi,
    NumberList,
}

/// 可编辑字段定义；label 为 i18n key。
#[derive(Debug, Clone)]
pub struct FieldDef {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub group: Group,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// 全部可编辑字段（唯一事实源；const 数组使 find() 可返回 &'static）。
const FIELDS: [FieldDef; 17] = [
    FieldDef { key: "language", label: "config.f_language", kind: FieldKind::Choice, group: Group::General, min: None, max: None },
    FieldDef { key: "active_mod", label: "config.f_active_mod", kind: FieldKind::Choice, group: Group::General, min: None, max: None },
    FieldDef { key: "currency_symbol", label: "config.f_currency", kind: FieldKind::Text, group: Group::General, min: None, max: None },
    FieldDef { key: "preset", label: "config.f_preset", kind: FieldKind::Choice, group: Group::Display, min: None, max: None },
    FieldDef { key: "separator", label: "config.f_separator", kind: FieldKind::Text, group: Group::Display, min: None, max: None },
    FieldDef { key: "compact_layout", label: "config.f_layout", kind: FieldKind::Multi, group: Group::Display, min: None, max: None },
    FieldDef { key: "theme", label: "config.f_theme", kind: FieldKind::Choice, group: Group::Display, min: None, max: None },
    FieldDef { key: "dashboard.refresh_interval_ms", label: "config.f_refresh", kind: FieldKind::Number, group: Group::Display, min: Some(0.0), max: None },
    FieldDef { key: "dashboard.default_layout", label: "config.f_default_layout", kind: FieldKind::Choice, group: Group::Display, min: None, max: None },
    FieldDef { key: "dashboard.scanlines", label: "config.f_scanlines", kind: FieldKind::Bool, group: Group::Display, min: None, max: None },
    FieldDef { key: "alerts.context_critical_pct", label: "config.f_ctx_critical", kind: FieldKind::Number, group: Group::Alerts, min: Some(0.0), max: Some(100.0) },
    FieldDef { key: "alerts.cost_threshold_usd", label: "config.f_cost_threshold", kind: FieldKind::Number, group: Group::Alerts, min: Some(0.0), max: None },
    FieldDef { key: "alerts.rate_limit_pct", label: "config.f_rate_limit", kind: FieldKind::Number, group: Group::Alerts, min: Some(0.0), max: Some(100.0) },
    FieldDef { key: "alerts.cooldown_minutes", label: "config.f_cooldown", kind: FieldKind::Number, group: Group::Alerts, min: Some(0.0), max: None },
    FieldDef { key: "alerts.compaction_eta_minutes", label: "config.f_compaction_eta", kind: FieldKind::Number, group: Group::Alerts, min: Some(0.0), max: None },
    FieldDef { key: "budget.cap_usd", label: "config.f_budget_cap", kind: FieldKind::Number, group: Group::Budget, min: Some(0.0), max: None },
    FieldDef { key: "budget.warn_pcts", label: "config.f_warn_pcts", kind: FieldKind::NumberList, group: Group::Budget, min: None, max: None },
];

pub fn fields() -> Vec<FieldDef> {
    FIELDS.to_vec()
}

pub fn find(key: &str) -> Option<&'static FieldDef> {
    FIELDS.iter().find(|f| f.key == key)
}

/// choice 字段选项；空字符串 = 不设/未选择。
pub fn options_for(f: &FieldDef, registry: &WidgetRegistry) -> Vec<String> {
    match f.key {
        "language" => vec!["en".into(), "zh".into()],
        "active_mod" => {
            let mut out: Vec<String> = [
                "glacier-workstation", "obsidian-command", "ember-night",
                "matrix-surveillance", "noir-precision", "noir-tabbed",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            if let Ok(dir) = AppConfig::mods_dir() {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for e in entries.filter_map(|e| e.ok()) {
                        let name = e.file_name().to_string_lossy().to_string();
                        if let Some(stripped) = name.strip_suffix(".toml") {
                            if !out.iter().any(|s| s == stripped) {
                                out.push(stripped.to_string());
                            }
                        }
                    }
                }
            }
            out
        }
        "preset" | "theme" => {
            let mut v: Vec<String> =
                Theme::preset_names().iter().map(|s| s.to_string()).collect();
            v.insert(0, String::new());
            v
        }
        "dashboard.default_layout" => vec![
            "grid-2x2".into(), "sidebar".into(), "focus".into(),
            "tabbed".into(), "windows".into(),
        ],
        "compact_layout" => {
            registry.widgets.iter().map(|w| w.id().to_string()).collect()
        }
        _ => vec![],
    }
}

/// 当前显示值（编辑缓冲 / Web 回填）。
pub fn get_value(config: &AppConfig, key: &str) -> Option<String> {
    match key {
        "language" => Some(config.language.clone()),
        "active_mod" => Some(config.active_mod.clone()),
        "currency_symbol" => Some(config.currency_symbol.clone().unwrap_or_default()),
        "preset" => Some(config.preset.clone().unwrap_or_default()),
        "separator" => Some(config.separator.clone()),
        "compact_layout" => Some(config.compact_layout.join(",")),
        "theme" => config.theme.as_ref().map(|t| match t {
            super::theme::ThemeRef::Preset(s) => s.clone(),
            super::theme::ThemeRef::Table(_) => "(custom)".into(),
        }),
        "dashboard.refresh_interval_ms" => {
            Some(config.dashboard.refresh_interval_ms.to_string())
        }
        "dashboard.default_layout" => Some(config.dashboard.default_layout.clone()),
        "dashboard.scanlines" => Some(config.dashboard.scanlines.to_string()),
        "alerts.context_critical_pct" => {
            Some(config.alerts.context_critical_pct.to_string())
        }
        "alerts.cost_threshold_usd" => {
            Some(config.alerts.cost_threshold_usd.to_string())
        }
        "alerts.rate_limit_pct" => Some(config.alerts.rate_limit_pct.to_string()),
        "alerts.cooldown_minutes" => Some(config.alerts.cooldown_minutes.to_string()),
        "alerts.compaction_eta_minutes" => {
            Some(config.alerts.compaction_eta_minutes.to_string())
        }
        "budget.cap_usd" => Some(config.budget.cap_usd.to_string()),
        "budget.warn_pcts" => Some(
            config
                .budget
                .warn_pcts
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(","),
        ),
        _ => None,
    }
}

fn parse_bool(key: &str, raw: &str) -> Result<bool, String> {
    match raw {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{key}: must be true or false")),
    }
}

fn parse_num(f: &FieldDef, raw: &str) -> Result<u64, String> {
    let v: u64 = raw
        .parse()
        .map_err(|_| format!("{}: invalid number '{}'", f.key, raw))?;
    check_range(f, v as f64)?;
    Ok(v)
}

fn parse_float(f: &FieldDef, raw: &str) -> Result<f64, String> {
    let v: f64 = raw
        .parse()
        .map_err(|_| format!("{}: invalid number '{}'", f.key, raw))?;
    check_range(f, v)?;
    Ok(v)
}

fn check_range(f: &FieldDef, v: f64) -> Result<(), String> {
    if let Some(min) = f.min {
        if v < min {
            return Err(format!("{}: below minimum {}", f.key, min));
        }
    }
    if let Some(max) = f.max {
        if v > max {
            return Err(format!("{}: above maximum {}", f.key, max));
        }
    }
    Ok(())
}

/// 单字段写入（解析 + 校验 + 落内存）。UI 与 Web POST 共用。
pub fn set_value(config: &mut AppConfig, key: &str, raw: &str) -> Result<(), String> {
    let f = find(key).ok_or_else(|| format!("unknown field: {key}"))?;
    let raw = raw.trim();
    match key {
        "language" => {
            if raw != "en" && raw != "zh" {
                return Err("language must be 'en' or 'zh'".into());
            }
            config.language = raw.into();
        }
        "active_mod" => config.active_mod = raw.into(),
        "currency_symbol" => {
            config.currency_symbol = if raw.is_empty() { None } else { Some(raw.into()) };
        }
        "preset" => {
            config.preset = if raw.is_empty() { None } else { Some(raw.into()) };
        }
        "separator" => config.separator = raw.into(),
        "compact_layout" => {
            config.compact_layout = raw
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string())
                .collect();
        }
        "theme" => {
            if raw == "(custom)" {
                return Err(
                    "theme uses table form; edit config.toml manually".into()
                );
            }
            config.theme = if raw.is_empty() {
                None
            } else {
                Some(super::theme::ThemeRef::Preset(raw.into()))
            };
        }
        "dashboard.refresh_interval_ms" => {
            config.dashboard.refresh_interval_ms = parse_num(f, raw)?;
        }
        "dashboard.default_layout" => config.dashboard.default_layout = raw.into(),
        "dashboard.scanlines" => config.dashboard.scanlines = parse_bool(key, raw)?,
        "alerts.context_critical_pct" => {
            config.alerts.context_critical_pct = parse_float(f, raw)?;
        }
        "alerts.cost_threshold_usd" => {
            config.alerts.cost_threshold_usd = parse_float(f, raw)?;
        }
        "alerts.rate_limit_pct" => config.alerts.rate_limit_pct = parse_float(f, raw)?,
        "alerts.cooldown_minutes" => config.alerts.cooldown_minutes = parse_num(f, raw)?,
        "alerts.compaction_eta_minutes" => {
            config.alerts.compaction_eta_minutes = parse_num(f, raw)?;
        }
        "budget.cap_usd" => config.budget.cap_usd = parse_float(f, raw)?,
        "budget.warn_pcts" => {
            let vals: Vec<f64> = if raw.is_empty() {
                vec![]
            } else {
                raw.split(',')
                    .map(|s| {
                        s.trim().parse::<f64>().map_err(|_| {
                            format!("{}: invalid number '{}'", key, s)
                        })
                    })
                    .collect::<Result<_, _>>()?
            };
            if !vals.windows(2).all(|w| w[0] < w[1]) {
                return Err(format!("{key}: must be strictly increasing"));
            }
            if vals.iter().any(|v| *v < 0.0 || *v > 100.0) {
                return Err(format!("{key}: values must be in 0..=100"));
            }
            config.budget.warn_pcts = vals;
        }
        _ => return Err(format!("unknown field: {key}")),
    }
    Ok(())
}

/// 保存前全量校验；错误信息含字段路径。
pub fn validate_config(config: &AppConfig) -> Result<(), String> {
    if config.language != "en" && config.language != "zh" {
        return Err("language must be 'en' or 'zh'".into());
    }
    for f in fields() {
        if matches!(f.kind, FieldKind::Number) {
            let raw = get_value(config, f.key).unwrap();
            let v: f64 = raw
                .parse()
                .map_err(|_| format!("{}: invalid number", f.key))?;
            check_range(&f, v)?;
        }
        if f.kind == FieldKind::NumberList {
            let vals: Vec<f64> = get_value(config, f.key)
                .unwrap()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| format!("{}: invalid number", f.key))?;
            if !vals.windows(2).all(|w| w[0] < w[1]) {
                return Err(format!("{}: must be strictly increasing", f.key));
            }
            if vals.iter().any(|v| *v < 0.0 || *v > 100.0) {
                return Err(format!("{}: values must be in 0..=100", f.key));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::AppConfig;
    use crate::core::widget::WidgetRegistry;

    fn cfg() -> AppConfig {
        AppConfig::load().unwrap_or_default()
    }

    #[test]
    fn fields_has_17_entries() {
        assert_eq!(fields().len(), 17);
    }

    #[test]
    fn get_set_round_trip() {
        let mut c = cfg();
        set_value(&mut c, "language", "zh").unwrap();
        assert_eq!(get_value(&c, "language").unwrap(), "zh");
        set_value(&mut c, "dashboard.refresh_interval_ms", "250").unwrap();
        assert_eq!(c.dashboard.refresh_interval_ms, 250);
        assert_eq!(get_value(&c, "dashboard.refresh_interval_ms").unwrap(), "250");
    }

    #[test]
    fn set_number_range_rejects_out_of_range() {
        let mut c = cfg();
        let err = set_value(&mut c, "alerts.rate_limit_pct", "150").unwrap_err();
        assert!(err.contains("rate_limit_pct"), "err = {err}");
    }

    #[test]
    fn set_warn_pcts_rejects_unsorted() {
        let mut c = cfg();
        assert!(set_value(&mut c, "budget.warn_pcts", "80,50").is_err());
        set_value(&mut c, "budget.warn_pcts", "50,80,100").unwrap();
        assert_eq!(c.budget.warn_pcts, vec![50.0, 80.0, 100.0]);
    }

    #[test]
    fn validate_rejects_bad_language() {
        let mut c = cfg();
        c.language = "xx".into();
        assert!(validate_config(&c).is_err());
    }

    #[test]
    fn validate_accepts_defaults() {
        assert!(validate_config(&cfg()).is_ok());
    }

    #[test]
    fn options_language_and_layouts() {
        let reg = WidgetRegistry::new();
        let lang = fields().into_iter().find(|f| f.key == "language").unwrap();
        assert_eq!(options_for(&lang, &reg), vec!["en", "zh"]);
        let lay = fields().into_iter().find(|f| f.key == "dashboard.default_layout").unwrap();
        assert_eq!(options_for(&lay, &reg),
                   vec!["grid-2x2", "sidebar", "focus", "tabbed", "windows"]);
    }

    #[test]
    fn options_active_mod_includes_builtin() {
        let reg = WidgetRegistry::new();
        let am = fields().into_iter().find(|f| f.key == "active_mod").unwrap();
        let opts = options_for(&am, &reg);
        assert!(opts.contains(&"glacier-workstation".to_string()));
        assert!(opts.contains(&"noir-tabbed".to_string()));
    }

    #[test]
    fn theme_table_form_is_protected() {
        let mut c = cfg();
        c.theme = Some(crate::core::theme::ThemeRef::Table(Default::default()));
        assert_eq!(get_value(&c, "theme").unwrap(), "(custom)");
        let err = set_value(&mut c, "theme", "(custom)").unwrap_err();
        assert!(err.contains("manually"), "err = {err}");
    }
}
