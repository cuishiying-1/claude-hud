use std::sync::Mutex;
use std::time::{Duration, Instant};
use tiny_http::{Response, Server};

use serde_json::Value;
use serde_json::json;

use crate::core::ansi;
use crate::core::config::AppConfig;
use crate::core::data_source::DataSource;
use crate::core::history::{HistoryStore, SessionRecord, WeekAgg};
use crate::core::scanner::{default_projects_root, model_windows_from_config, WindowsScanner};
use crate::core::windows::WindowInfo;
use crate::core::session::{ContextWindow, CostInfo, CurrentUsage, ModelInfo, RateLimits, SessionData};
use crate::core::transcript::TranscriptSummary;
use crate::core::i18n::tr;
use crate::core::state;
use crate::core::theme::Theme;
use crate::core::widget::WidgetRegistry;

const PORT: u16 = 9527;

/// ⑨+㉑ 历史聚合缓存：前端 2s 轮询 /api/data，weekly/trend 是分钟级统计，
/// 每请求重开 SQLite 纯属空转。30s TTL 内命中缓存不重查。
const HISTORY_TTL: Duration = Duration::from_secs(30);
static HISTORY_CACHE: Mutex<Option<(Instant, Value, Value, Value)>> = Mutex::new(None);
// (fetched_at, weekly_json, trend_json, week_compare_json)

fn ttl_fresh(fetched_at: Instant, now: Instant, ttl: Duration) -> bool {
    now.duration_since(fetched_at) < ttl
}

fn cached_history() -> (Value, Value, Value) {
    let mut guard = HISTORY_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if let Some((at, weekly, trend, wc)) = guard.as_ref() {
        if ttl_fresh(*at, now, HISTORY_TTL) {
            return (weekly.clone(), trend.clone(), wc.clone());
        }
    }
    let weekly = weekly_json_inner();
    let trend = trend_json_inner();
    let wc = week_compare_json_inner();
    *guard = Some((now, weekly.clone(), trend.clone(), wc.clone()));
    (weekly, trend, wc)
}

/// Launch the web dashboard HTTP server.
pub fn run(
    registry: &'static WidgetRegistry,
    config: &'static AppConfig,
    theme: &'static Theme,
) -> Result<(), String> {
    let addr = format!("127.0.0.1:{}", PORT);
    let server =
        Server::http(&addr).map_err(|e| format!("start server on {}: {}", addr, e))?;
    let lang = config.language();

    println!(
        "{}",
        tr(lang, "runtime.serve_start").replace("{port}", &PORT.to_string())
    );
    println!("{}", tr(lang, "runtime.serve_stop"));

    for request in server.incoming_requests() {
        let url = request.url().to_string();
        // ⑬ 路由：按 '?' 拆分 query（serve 路径带参数）
        let (path, query) = match url.split_once('?') {
            Some((p, q)) => (p, Some(q)),
            None => (url.as_str(), None),
        };
        match path {
            "/" | "/index.html" => {
                let html = build_dashboard_html(registry, config, theme);
                let header = "Content-Type: text/html; charset=utf-8"
                    .parse::<tiny_http::Header>()
                    .unwrap();
                let response = Response::from_string(html).with_header(header);
                let _ = request.respond(response);
            }
            "/api/data" => {
                let session_id = query_param(query, "session_id")
                    .and_then(|v| v.parse::<i64>().ok());
                let json =
                    build_api_json(registry, config, theme, session_id, current_session(config));
                let header = "Content-Type: application/json"
                    .parse::<tiny_http::Header>()
                    .unwrap();
                let response = Response::from_string(json).with_header(header);
                let _ = request.respond(response);
            }
            "/api/sessions" => {
                let limit = query_param(query, "limit")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(10);
                let offset = query_param(query, "offset")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0);
                let body = match HistoryStore::open() {
                    Ok(h) => {
                        let rows = h.sessions_page(limit, offset, None).unwrap_or_default();
                        // 前端 loadSessions 读 data.currency_symbol 显示成本列币种；
                        // 缺失时回退 '$'（与 totals 同款注入）。
                        let mut v = sessions_list_json(&rows);
                        v["currency_symbol"] = json!(config.currency());
                        v.to_string()
                    }
                    Err(_) => json!({"available": false, "sessions": []}).to_string(),
                };
                let header = "Content-Type: application/json"
                    .parse::<tiny_http::Header>()
                    .unwrap();
                let _ = request.respond(Response::from_string(body).with_header(header));
            }
            _ if path.starts_with("/api/sessions/") => {
                let id_str = &path["/api/sessions/".len()..];
                let detail = id_str
                    .parse::<i64>()
                    .ok()
                    .and_then(|id| session_detail_body(id, config).ok());
                match detail {
                    Some(body) => {
                        let header = "Content-Type: application/json"
                            .parse::<tiny_http::Header>()
                            .unwrap();
                        let _ = request.respond(Response::from_string(body).with_header(header));
                    }
                    None => {
                        let body = Response::from_string(tr(lang, "web.not_found"))
                            .with_status_code(404);
                        let _ = request.respond(body);
                    }
                }
            }
            "/api/windows" => {
                let wins = scanned_windows(config);
                let mut v = windows_json(&wins);
                v["currency_symbol"] = json!(config.currency());
                let body = v.to_string();
                let header = "Content-Type: application/json"
                    .parse::<tiny_http::Header>()
                    .unwrap();
                let _ = request.respond(Response::from_string(body).with_header(header));
            }
            "/api/totals" => {
                let body = match HistoryStore::open() {
                    Ok(h) => match h.totals() {
                        Ok(t) => {
                            let mut v = totals_json(&t);
                            v["currency_symbol"] = json!(config.currency());
                            v.to_string()
                        }
                        Err(_) => json!({"available": false}).to_string(),
                    },
                    Err(_) => json!({"available": false}).to_string(),
                };
                let header = "Content-Type: application/json"
                    .parse::<tiny_http::Header>()
                    .unwrap();
                let _ = request.respond(Response::from_string(body).with_header(header));
            }
            "/api/config" => {
                if request.method() == &tiny_http::Method::Post {
                    handle_config_post(request, config, lang);
                } else {
                    handle_config_get(request, registry, lang);
                }
            }
            "/config" => {
                let html = build_config_html(registry, config, lang);
                let header = "Content-Type: text/html; charset=utf-8"
                    .parse::<tiny_http::Header>()
                    .unwrap();
                let _ = request.respond(Response::from_string(html).with_header(header));
            }
            "/api/health" => {
                let _ = request.respond(Response::from_string("OK"));
            }
            _ => {
                let body = Response::from_string(tr(lang, "web.not_found"))
                    .with_status_code(404);
                let _ = request.respond(body);
            }
        }
    }

    Ok(())
}

/// GET /api/config：磁盘重读（权威）+ schema fields + current + 只读价格表。
fn handle_config_get(
    request: tiny_http::Request,
    registry: &WidgetRegistry,
    lang: crate::core::i18n::Language,
) {
    use crate::core::config_schema;
    let config = AppConfig::load().unwrap_or_default();
    let mut fields_json: Vec<Value> = Vec::new();
    for f in config_schema::fields() {
        let mut v = json!({
            "key": f.key,
            "label": tr(lang, f.label),
            "kind": match f.kind {
                config_schema::FieldKind::Text => "text",
                config_schema::FieldKind::Number => "number",
                config_schema::FieldKind::Bool => "bool",
                config_schema::FieldKind::Choice => "choice",
                config_schema::FieldKind::Multi => "multi",
                config_schema::FieldKind::NumberList => "list",
            },
            "group": f.group.name(),
            "options": config_schema::options_for(&f, registry),
        });
        if let Some(min) = f.min {
            v["min"] = json!(min);
        }
        if let Some(max) = f.max {
            v["max"] = json!(max);
        }
        fields_json.push(v);
    }
    let mut current = serde_json::Map::new();
    for f in config_schema::fields() {
        if let Some(v) = config_schema::get_value(&config, f.key) {
            current.insert(f.key.to_string(), json!(v));
        }
    }
    let body = json!({
        "fields": fields_json,
        "current": current,
        "readonly": readonly_pricing_json(&config),
    })
    .to_string();
    let header = "Content-Type: application/json"
        .parse::<tiny_http::Header>()
        .unwrap();
    let _ = request.respond(Response::from_string(body).with_header(header));
}

/// 只读价格表：合并 builtin + 用户 [models]/[pricing] 覆盖。
fn readonly_pricing_json(config: &AppConfig) -> Value {
    use crate::core::pricing;
    let mut models = pricing::builtin_models();
    for (id, m) in &config.models {
        models.insert(id.clone(), m.clone());
    }
    let pricing_table = pricing::merged_pricing(config);
    let mut rows: Vec<Value> = models
        .iter()
        .filter(|(_, m)| m.price_usd.is_some())
        .map(|(id, m)| {
            let p = pricing_table.get(id);
            json!({
                "id": id,
                "window": m.context_window.map(|w| w.to_string()).unwrap_or_else(|| "-".into()),
                "usd_in": p.map(|e| e.input).unwrap_or(0.0),
                "usd_out": p.map(|e| e.output).unwrap_or(0.0),
                "cny_in": m.price_cny.as_ref().map(|e| e.input).unwrap_or(0.0),
                "cny_out": m.price_cny.as_ref().map(|e| e.output).unwrap_or(0.0),
            })
        })
        .collect();
    rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    json!({ "models": rows })
}

/// POST /api/config：JSON → 递归展开点路径 → 克隆修改 → 校验保存。
fn handle_config_post(
    mut request: tiny_http::Request,
    _config: &AppConfig,
    _lang: crate::core::i18n::Language,
) {
    use crate::core::config_schema;
    let mut raw = String::new();
    let _ = request.as_reader().read_to_string(&mut raw);
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            respond_json(request, 400, json!({"ok": false, "error": format!("bad json: {e}")}));
            return;
        }
    };
    let mut edits: Vec<(String, String)> = Vec::new();
    flatten_json("", &v, &mut edits);
    let mut next = AppConfig::load().unwrap_or_default();
    for (key, raw) in &edits {
        if let Err(e) = config_schema::set_value(&mut next, key, raw) {
            respond_json(request, 400, json!({"ok": false, "error": e, "field": key}));
            return;
        }
    }
    let path = match AppConfig::config_path() {
        Ok(p) => p,
        Err(e) => {
            respond_json(request, 500, json!({"ok": false, "error": e}));
            return;
        }
    };
    match next.save(&path) {
        Ok(()) => {
            respond_json(request, 200, json!({"ok": true, "backup": "config.toml.bak"}));
        }
        Err(e) => {
            respond_json(request, 500, json!({"ok": false, "error": e}));
        }
    }
}

fn respond_json(request: tiny_http::Request, status: u16, body: Value) {
    let header = "Content-Type: application/json"
        .parse::<tiny_http::Header>()
        .unwrap();
    let _ = request.respond(
        Response::from_string(body.to_string())
            .with_status_code(status)
            .with_header(header),
    );
}

/// 嵌套 JSON → 点路径叶键。数组 → 逗号连接（multi/list 语义）。
fn flatten_json(prefix: &str, v: &serde_json::Value, out: &mut Vec<(String, String)>) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_json(&key, val, out);
            }
        }
        serde_json::Value::Array(items) => {
            let joined = items
                .iter()
                .map(|i| match i {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join(",");
            out.push((prefix.to_string(), joined));
        }
        serde_json::Value::String(s) => out.push((prefix.to_string(), s.clone())),
        serde_json::Value::Number(n) => out.push((prefix.to_string(), n.to_string())),
        serde_json::Value::Bool(b) => out.push((prefix.to_string(), b.to_string())),
        serde_json::Value::Null => {}
    }
}

/// 5s 间隔全局扫描缓存：serve 进程内唯一 scanner。窗口视图与当前会话
/// 卡片的数据源（state.json 不再作为展示权威 → last-writer-wins 竞争消解）。
static SERVE_SCANNER: Mutex<Option<(Instant, WindowsScanner, Vec<WindowInfo>)>> =
    Mutex::new(None);

const SCAN_INTERVAL: Duration = Duration::from_secs(5);

fn scanned_windows(config: &AppConfig) -> Vec<WindowInfo> {
    let mut guard = SERVE_SCANNER.lock().unwrap();
    if guard.is_none() {
        // last 初始化为「已过期」→ 首次调用立即扫描（否则首 5s 内空视图）
        *guard = Some((
            Instant::now() - SCAN_INTERVAL,
            WindowsScanner::new(
                default_projects_root(),
                crate::core::pricing::merged_pricing(config),
                model_windows_from_config(config),
            ),
            Vec::new(),
        ));
    }
    let (last, scanner, cache) = guard.as_mut().expect("initialized above");
    if last.elapsed() >= SCAN_INTERVAL {
        *cache = scanner.scan();
        *last = Instant::now();
    }
    cache.clone()
}

/// 当前会话：scanner 最新活跃窗口（修复 #6 交付）；无活跃窗口 →
/// state 快照兜底（会话整体空闲时仍有数据可看）。
fn current_session(config: &AppConfig) -> Option<SessionData> {
    scanned_windows(config)
        .into_iter()
        .find(|w| w.status == crate::core::windows::WindowStatus::Active)
        .map(|w| session_data_from_window(&w, config))
        .or_else(|| state::read_current_data())
}

/// scanner 窗口 → SessionData（当前会话卡片）：模型 display = id，
/// usage 从 transcript 累计 token 填充，成本为单价计算值（≈ 口径）。
pub fn session_data_from_window(
    w: &WindowInfo,
    config: &AppConfig,
) -> SessionData {
    let window = crate::core::pricing::model_window(config, &w.model).unwrap_or(0);
    let pct = if window > 0 {
        ((w.tokens_in + w.tokens_out) as f64 / window as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    SessionData {
        model: ModelInfo {
            id: w.model.clone(),
            display_name: w.model.clone(),
        },
        context_window: ContextWindow {
            used_percentage: pct,
            total_input_tokens: w.tokens_in,
            total_output_tokens: w.tokens_out,
            context_window_size: window,
            current_usage: CurrentUsage {
                input_tokens: w.tokens_in,
                output_tokens: w.tokens_out,
                cache_creation_input_tokens: w.cache_created,
                cache_read_input_tokens: w.cache_read,
            },
        },
        cost: CostInfo {
            total_cost_usd: w.cost,
            total_duration_ms: 0,
            total_lines_added: 0,
            total_lines_removed: 0,
        },
        rate_limits: RateLimits::default(),
        transcript_path: None,
        subagent_status_line: None,
        data_source: DataSource::Reported,
    }
}

fn build_api_json(
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
    session_id: Option<i64>,
    current: Option<SessionData>,
) -> String {
    // ㉒ session_id → 历史会话数据（pct 为 total_tokens ÷ 窗口的估算值）；
    // None → 当前会话：scanner 最新活跃窗口优先，state 快照兜底。
    let mut data = match session_id {
        Some(id) => session_data_by_id(id, config).unwrap_or_default(),
        None => current.unwrap_or_else(|| state::read_current_data().unwrap_or_default()),
    };
    // v0.7 窗口单点解析：serve 的 /api/data pct 同样吃真实窗口（覆盖 200k 兜底）
    crate::core::pricing::resolve_context_window(&mut data, config);
    let layout = &config.compact_layout;

    let lang = config.language();
    let widgets_json: Vec<Value> = layout
        .iter()
        .filter_map(|id| {
            let widget = registry.get(id)?;
            let widget_config = config.widget_config(id);
            let rendered = widget.render_compact(&data, theme, &widget_config);
            // widget.<id> key 存在 → 翻译；否则回退原显示名（与 draw_tabbed 同口径）
            let translated = crate::core::i18n::tr_dyn(lang, id);
            let name = if translated == id.as_str() {
                widget.display_name().to_string()
            } else {
                translated.into_owned()
            };
            Some(json!({
                "id": id,
                "name": name,
                // web 显示纯文本：compact 输出的 ANSI 色码在浏览器里是乱码
                "output": ansi::strip_ansi(&rendered),
            }))
        })
        .collect();

    let pricing_configured = model_pricing_configured(config, &data.model.id);
    let (weekly, trend, wc) = cached_history();
    json!({
        "model": data.model.display_name,
        "model_id": data.model.id,
        "pricing_configured": pricing_configured,
        "context_pct": data.context_window.used_percentage,
        "currency_symbol": config.currency(),
        "cost_usd": data.cost.total_cost_usd,
        "duration_ms": data.cost.total_duration_ms,
        "total_input_tokens": data.context_window.total_input_tokens,
        "total_output_tokens": data.context_window.total_output_tokens,
        // ㉒ 当前查看目标：null = 实时会话；数字 = 历史会话（pct 为估算值）
        "session_id": session_id,
        "weekly": weekly,
        "trend": trend,
        "week_compare": wc,
        "widgets": widgets_json,
    })
    .to_string()
}

/// 模型是否有已知单价（内置注册表或用户 [pricing]/[models] 覆盖）。
/// 与注入路径（pricing::inject_cost 的 merged 口径）一致：v0.7 内置注册表
/// 后不得只查 [pricing] 段——内置模型未手写配置 ≠ 未配置单价。
fn model_pricing_configured(config: &AppConfig, model_id: &str) -> bool {
    crate::core::pricing::merged_pricing(config).contains_key(model_id)
}

/// ⑨ 本周聚合统计：open/query 失败 → available:false 全 0（前端显示 —）。
fn weekly_json_inner() -> Value {
    let weekly = HistoryStore::open()
        .ok()
        .and_then(|h| h.weekly_stats().ok());
    match weekly {
        Some(w) => json!({
            "available": true,
            "total_cost": w.total_cost,
            "total_sessions": w.total_sessions,
            "total_tokens": w.total_tokens,
            "avg_duration_min": w.avg_duration_min,
            "avg_agents_per_session": w.avg_agents_per_session,
        }),
        None => json!({
            "available": false,
            "total_cost": 0,
            "total_sessions": 0,
            "total_tokens": 0,
            "avg_duration_min": 0,
            "avg_agents_per_session": 0,
        }),
    }
}

/// ㉑ 近 7 天日成本趋势（供周曲线）：open/query 失败或无数据 → available:false。
fn trend_json_inner() -> Value {
    let trend = HistoryStore::open()
        .ok()
        .and_then(|h| h.daily_cost_trend().ok());
    match trend {
        Some(days) if !days.is_empty() => {
            let days_json: Vec<Value> = days
                .iter()
                .map(|(day, cost)| json!({"day": day, "cost": cost}))
                .collect();
            json!({"available": true, "days": days_json})
        }
        _ => json!({"available": false, "days": []}),
    }
}

/// ⑫ 服务端渲染 SVG 折线（零依赖）。数据点 <2 → None（调用方显示占位）。
/// 几何：viewBox 560x64，左右上边距 8/8/6，底部 16 留日期标签。
pub fn trend_svg(days: &[(String, f64)]) -> Option<String> {
    if days.len() < 2 {
        return None;
    }
    let (w, h) = (560.0_f64, 64.0_f64);
    // 顶边距 10 给数值标注留位（点上方 y-3，字号 7 需约 10px 净高）
    let (ml, mt, mr, mb) = (8.0_f64, 10.0_f64, 8.0_f64, 16.0_f64);
    let max = days.iter().map(|(_, c)| *c).fold(0.0, f64::max).max(0.0001);
    let inner_w = w - ml - mr;
    let inner_h = h - mt - mb;
    let n = days.len();
    let xy = |i: usize| {
        let x = ml + inner_w * i as f64 / (n - 1) as f64;
        let y = mt + inner_h * (1.0 - days[i].1 / max);
        (x, y)
    };
    let mut points = String::new();
    let mut circles = String::new();
    let mut values = String::new();
    for i in 0..n {
        let (x, y) = xy(i);
        points.push_str(&format!("{:.1},{:.1} ", x, y));
        circles.push_str(&format!(
            r##"<circle cx="{:.1}" cy="{:.1}" r="1.5" fill="#4c8dff"/>"##,
            x, y
        ));
        // 数据标注：点上方的数值（2 位小数），与折线同色系灰
        values.push_str(&format!(
            r##"<text x="{:.1}" y="{:.1}" font-size="7" fill="#8b949e" text-anchor="middle">{:.2}</text>"##,
            x,
            y - 3.0,
            days[i].1
        ));
    }
    let mut labels = String::new();
    let mut last: Option<usize> = None;
    for idx in [0usize, n / 2, n - 1] {
        if last == Some(idx) {
            continue;
        }
        last = Some(idx);
        let (x, _) = xy(idx);
        let short = days[idx].0.get(5..).unwrap_or(&days[idx].0);
        labels.push_str(&format!(
            r##"<text x="{:.1}" y="{:.1}" font-size="8" fill="#8b949e">{}</text>"##,
            x, h - 3.0, short
        ));
    }
    Some(format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:.0} {h:.0}" style="width:100%;height:64px;">
<polyline points="{points}" fill="none" stroke="#4c8dff" stroke-width="1.5"/>
{circles}
{values}
{labels}
</svg>"##,
        w = w,
        h = h,
        points = points.trim(),
        circles = circles,
        labels = labels,
    ))
}

/// ⑫ 趋势卡片内容：/api/data 的 trend JSON → 内嵌 SVG；数据 <2 点 → i18n 占位。
pub fn trend_card_html(trend: &Value, lang: crate::core::i18n::Language) -> String {
    let days: Vec<(String, f64)> = trend["days"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    Some((
                        d.get("day")?.as_str()?.to_string(),
                        d.get("cost")?.as_f64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    match trend_svg(&days) {
        Some(svg) => svg,
        None => format!(r#"<div class="card-detail">{}</div>"#, tr(lang, "web.trend_no_data")),
    }
}

/// ⑬ query 串参数（"limit=1&offset=0" → 值）；缺失 → None。
pub fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    query?
        .split('&')
        .find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == key).then(|| v.to_string())
        })
}

/// ⑭ 环比百分比：last ≤ 0（无上周）→ None；否则 (cur-last)/last*100 四舍五入。
pub fn pct_change(cur: f64, last: f64) -> Option<i64> {
    if last <= 0.0 {
        return None;
    }
    Some(((cur - last) / last * 100.0).round() as i64)
}

/// ⑭ 周环比 JSON：this/last 聚合 → 卡片数据。
/// this_week/last_week 为 null 表示该周无会话；cost_pct 等 null = 无上周可比
/// （前端显示 —）。
pub fn week_compare_json(
    this: Option<&WeekAgg>,
    last: Option<&WeekAgg>,
) -> Value {
    let week = |w: &WeekAgg| json!({"cost": w.cost, "sessions": w.sessions, "tokens": w.tokens});
    json!({
        "available": true,
        "this_week": this.map(week),
        "last_week": last.map(week),
        "cost_pct": this.zip(last).and_then(|(a, b)| pct_change(a.cost, b.cost)),
        "session_pct": this.zip(last).and_then(|(a, b)| pct_change(a.sessions as f64, b.sessions as f64)),
        "token_pct": this.zip(last).and_then(|(a, b)| pct_change(a.tokens as f64, b.tokens as f64)),
    })
}

/// ⑭ 周环比接线：库不可开/查询失败 → 全 None（available true 全 null）。
fn week_compare_json_inner() -> Value {
    let (this, last) = HistoryStore::open()
        .ok()
        .and_then(|h| h.weekly_compare().ok())
        .unwrap_or((None, None));
    week_compare_json(this.as_ref(), last.as_ref())
}

/// ㉒ 历史会话 → SessionData（/api/data?session_id 用）：transcript 细分
/// in/out 优先，缺 transcript 则 in=total_tokens/out=0；pct = 总 token ÷
/// 模型窗口（v0.7 窗口单点，估算口径），窗口未知 → 0（前端显示 —）。
pub fn session_data_from_record(
    record: &SessionRecord,
    summary: Option<&TranscriptSummary>,
    config: &AppConfig,
) -> SessionData {
    let (t_in, t_out) = match summary {
        Some(s) => (s.total_tokens.input, s.total_tokens.output),
        None => (record.total_tokens, 0),
    };
    let window = crate::core::pricing::model_window(config, &record.model).unwrap_or(0);
    let pct = if window > 0 {
        ((t_in + t_out) as f64 / window as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    SessionData {
        model: ModelInfo {
            id: record.model.clone(),
            display_name: record.model.clone(),
        },
        context_window: ContextWindow {
            used_percentage: pct,
            total_input_tokens: t_in,
            total_output_tokens: t_out,
            context_window_size: window,
            current_usage: CurrentUsage::default(),
        },
        cost: CostInfo {
            total_cost_usd: record.total_cost_usd,
            total_duration_ms: record.duration_secs * 1000,
            total_lines_added: 0,
            total_lines_removed: 0,
        },
        rate_limits: RateLimits::default(),
        transcript_path: record.transcript_path.clone(),
        subagent_status_line: None,
        data_source: DataSource::Reported,
    }
}

/// ㉒ 按 id 解析历史会话数据：库不可用/未找到 → None（前端保持当前会话视图）。
fn session_data_by_id(id: i64, config: &AppConfig) -> Option<SessionData> {
    let store = HistoryStore::open().ok()?;
    let record = store.session_by_id(id).ok()??;
    let summary = record.transcript_path.as_deref().and_then(|p| {
        if std::path::Path::new(p).exists() {
            Some(crate::core::transcript::TranscriptReader::new(p.into()).read_updates())
        } else {
            None
        }
    });
    Some(session_data_from_record(&record, summary.as_ref(), config))
}

/// ⑬ 单会话详情接线：session_by_id → transcript 尾读 → 详情 JSON。
/// 未找到 / 库不可用 → Err（调用方 404）。
fn session_detail_body(id: i64, config: &AppConfig) -> Result<String, ()> {
    let store = HistoryStore::open().map_err(|_| ())?;
    let Some(record) = store.session_by_id(id).map_err(|_| ())? else {
        return Err(());
    };
    let summary = match record.transcript_path.as_deref() {
        Some(path) if std::path::Path::new(path).exists() => {
            Some(crate::core::transcript::TranscriptReader::new(path.into()).read_updates())
        }
        _ => None,
    };
    Ok(session_detail_json(&record, summary.as_ref(), config).to_string())
}

/// ⑬ 会话列表 JSON（分页行；字段与 sessions 表一一对应）。
pub fn sessions_list_json(rows: &[SessionRecord]) -> Value {
    json!({
        "available": true,
        "sessions": rows
            .iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "started_at": r.started_at,
                    "duration_secs": r.duration_secs,
                    "total_cost_usd": r.total_cost_usd,
                    "total_tokens": r.total_tokens,
                    "agent_count": r.agent_count,
                    "model": r.model,
                })
            })
            .collect::<Vec<_>>(),
    })
}

/// ⑬ 单会话详情 JSON：transcript 尾读（summary）+ 工具成本排行（前 5）。
/// 无 transcript → transcript_detail available false + tools 空（与 CLI ⑥ 一致）。
pub fn session_detail_json(
    record: &SessionRecord,
    summary: Option<&TranscriptSummary>,
    config: &AppConfig,
) -> Value {
    let tools: Vec<Value> = summary
        .as_ref()
        .and_then(|s| {
            crate::core::pricing::tool_cost_ranking(
                s,
                &crate::core::pricing::merged_pricing(config),
                &record.model,
            )
        })
        .unwrap_or_default()
        .iter()
        .take(5)
        .map(|(tool, calls, cost)| json!({"tool": tool, "calls": calls, "cost": cost}))
        .collect();
    json!({
        "id": record.id,
        "started_at": record.started_at,
        "model": record.model,
        "duration_secs": record.duration_secs,
        "total_cost_usd": record.total_cost_usd,
        "currency_symbol": config.currency(),
        "total_tokens": record.total_tokens,
        "agent_count": record.agent_count,
        "transcript_detail": match summary {
            Some(s) => json!({
                "available": true,
                "tokens_in": s.total_tokens.input,
                "tokens_out": s.total_tokens.output,
                "agents": s
                    .agents
                    .iter()
                    .map(|a| json!({"name": a.name, "tool_calls": a.tool_calls}))
                    .collect::<Vec<_>>(),
            }),
            None => json!({
                "available": false,
                "tokens_in": 0,
                "tokens_out": 0,
                "agents": [],
            }),
        },
        "tools": tools,
    })
}

/// /api/windows 响应:每窗口 dir/status/model/pct/cost/tokens/agents/corrupt。
fn windows_json(wins: &[crate::core::windows::WindowInfo]) -> Value {
    let arr: Vec<Value> = wins
        .iter()
        .map(|w| {
            json!({
                "dir": w.dir_name,
                "model": w.model,
                "pct": w.used_pct,
                "cost": w.cost,
                "tokens": w.tokens_in + w.tokens_out,
                "agents": w.agent_count,
                "corrupt": w.corrupt,
                "status": crate::core::windows::status_name(&w.status),
                "cache_read": w.cache_read,
                "cache_created": w.cache_created,
                "data_source": w.data_source,
            })
        })
        .collect();
    json!({ "windows": arr })
}

/// /api/totals 响应:全量 SUM + AVG(available=false 时前端显示占位)。
fn totals_json(t: &crate::core::history::Totals) -> Value {
    json!({
        "sessions": t.sessions,
        "total_cost": t.total_cost,
        "total_tokens": t.total_tokens,
        "total_duration_secs": t.total_duration_secs,
        "avg_duration_min": t.avg_duration_min,
    })
}

fn build_dashboard_html(
    _registry: &WidgetRegistry,
    config: &AppConfig,
    _theme: &Theme,
) -> String {
    let html = r#"<!DOCTYPE html>
<html lang="{web_lang}">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{web_title}</title>
<style>
  @import url('https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500;600;700&display=swap');
  * { margin:0; padding:0; box-sizing:border-box; }
  body {
    background:#0a0a0a; color:#ccc;
    font-family:'JetBrains Mono',monospace;
    padding:32px; min-height:100vh;
  }
  .header {
    display:flex; justify-content:space-between; align-items:center;
    margin-bottom:32px; padding-bottom:16px;
    border-bottom:1px solid #1a1a1a;
  }
  .header h1 { font-size:20px; font-weight:300; color:#fff; letter-spacing:-1px; }
  .header .status { font-size:11px; color:#666; }
  .grid {
    display:grid;
    grid-template-columns:repeat(auto-fit,minmax(340px,1fr));
    gap:16px;
  }
  .card {
    background:#0d1117; border:1px solid #21262d;
    border-radius:8px; padding:16px;
    transition:border-color 0.3s;
  }
  .card:hover { border-color:#444; }
  .card-title {
    font-size:11px; font-weight:600; color:#58a6ff;
    text-transform:uppercase; letter-spacing:1px;
    margin-bottom:10px;
  }
  .card-value {
    font-size:14px; font-weight:500; color:#fff;
  }
  .card-detail {
    font-size:10px; color:#8b949e; margin-top:4px;
  }
  .metric-big {
    font-size:32px; font-weight:300; color:#fff;
  }
  .metric-label {
    font-size:10px; color:#666; text-transform:uppercase; letter-spacing:1px;
  }
  .bar {
    height:6px; border-radius:3px; background:#21262d;
    margin-top:6px; overflow:hidden;
  }
  .bar-fill {
    height:100%; border-radius:3px;
    transition:width 0.5s ease;
  }
  .alert { color:#ff7b72; font-size:10px; padding:4px 0; }
  .pulse { animation:pulse 2s ease-in-out infinite; }
  @keyframes pulse { 0%,100%{opacity:1;} 50%{opacity:0.4;} }
  .session-row { cursor:pointer; }
  .session-row:hover td { background:#161b22; }
  .session-detail td {
    background:#0d1117; font-size:10px; color:#8b949e;
    white-space:pre-wrap; border-top:1px dashed #21262d;
  }
  .realtime {
    font-size:9px; color:#333; margin-top:24px;
    text-align:center;
  }
</style>
</head>
<body>
<div class="header">
  <h1>{web_heading}</h1>
  <div class="status">● Live · <span id="update-time">--</span></div>
  <div style="margin-top:10px;">
    <select id="session-select" style="background:#0d1117;border:1px solid #30363d;color:#c9d1d9;border-radius:4px;padding:3px 8px;font-size:11px;max-width:480px;">
      <option value="">{web_current_session}</option>
    </select>
  </div>
</div>

<div id="pricing-note" style="display:none;color:#d29922;font-size:11px;margin-bottom:12px;"></div>
<div class="grid" id="dashboard-grid">
  <div class="card">
    <div class="card-title">{web_model}</div>
    <div class="card-value" id="val-model">--</div>
  </div>
  <div class="card">
    <div class="card-title">{web_ctx_window}<span id="ctx-est" style="display:none;color:#8b949e;font-size:9px;">{web_ctx_estimated}</span></div>
    <div class="metric-big" id="val-ctx">--</div>
    <div class="metric-label">{web_used}</div>
    <div class="bar"><div class="bar-fill" id="bar-ctx" style="width:0%;background:linear-gradient(90deg,#7ee787,#f0883e,#ff7b72);"></div></div>
  </div>
  <div class="card">
    <div class="card-title">{web_session_cost}</div>
    <div class="metric-big" id="val-cost">--</div>
    <div class="metric-label">{web_usd}</div>
  </div>
  <div class="card">
    <div class="card-title">{web_duration}</div>
    <div class="metric-big" id="val-dur">--</div>
    <div class="metric-label">{web_active}</div>
  </div>
  <div class="card">
    <div class="card-title">{web_this_week}</div>
    <div class="metric-big" id="val-week-cost">--</div>
    <div class="metric-label"><span id="val-week-sessions">--</span> {web_sessions}</div>
    <div class="card-detail" id="week-compare">--</div>
  </div>
</div>

<div class="card" id="windows-card">
  <div class="card-title">{web_windows_title}</div>
  <table style="width:100%;border-collapse:collapse;font-size:11px;">
    <thead>
      <tr style="color:#8b949e;text-align:left;">
        <th>{web_w_col_status}</th><th>{web_w_col_dir}</th>
        <th>{web_w_col_model}</th><th>{web_w_col_ctx}</th>
        <th>{web_w_col_cost}</th><th>{web_w_col_tokens}</th>
        <th>{web_w_col_agents}</th>
      </tr>
    </thead>
    <tbody id="windows-body"></tbody>
  </table>
  <div id="windows-empty" class="card-detail">{web_win_none}</div>
</div>

  <div class="card" id="trend-card">
    <div class="card-title">{web_cost_trend}</div>
    {web_trend_svg}
  </div>

  <div class="card" id="sessions-card">
    <div class="card-title">{web_sessions_title}</div>
    <table style="width:100%;border-collapse:collapse;font-size:11px;">
      <thead>
        <tr style="color:#8b949e;text-align:left;">
          <th>{web_col_time}</th><th>{web_col_cost}</th><th>{web_col_duration}</th>
          <th>{web_col_agents}</th><th>{web_col_tokens}</th>
        </tr>
      </thead>
      <tbody id="sessions-body"></tbody>
    </table>
    <button id="sessions-more" style="margin-top:8px;background:#21262d;border:1px solid #30363d;color:#c9d1d9;border-radius:4px;padding:4px 12px;font-size:11px;cursor:pointer;">{web_load_more}</button>
  </div>

<div id="widgets-area" style="margin-top:24px;"></div>

<div class="card" id="totals-card" style="margin-top:24px;">
  <div class="card-title">{web_totals_title}</div>
  <div class="card-value" id="totals-line">—</div>
</div>

<div class="realtime">{web_realtime}</div>

<script>
const T = {
  pricing_note: "T_PRICING_NOTE",
  not_found: "T_NOT_FOUND",
  load_more: "T_LOAD_MORE",
  h_model: "T_H_MODEL",
  h_tokens: "T_H_TOKENS",
  h_tokens_plain: "T_H_TOKENS_PLAIN",
  h_tools_title: "T_H_TOOLS_TITLE",
  h_tool_line: "T_H_TOOL_LINE",
  week_compare: "T_WEEK_COMPARE",
  totals_line: "T_TOTALS_LINE",
};
let selectedSession = '';
async function refresh() {
  try {
    const resp = await fetch('/api/data' + (selectedSession ? '?session_id=' + selectedSession : ''));
    const data = await resp.json();
    document.getElementById('val-model').textContent = data.model;
    // ㉒ 历史会话 pct 为 total_tokens ÷ 窗口估算值 → 标注（估算）；实时会话隐藏
    document.getElementById('ctx-est').style.display = data.session_id ? 'inline' : 'none';
    // 诚实降级：无成本/用量数据（上游不提供 usage）→ 「—」，不显示 0%/¥0.0000 假精确
    const noUsage = data.cost_usd === 0 && data.total_input_tokens === 0 && data.total_output_tokens === 0;
    document.getElementById('val-ctx').textContent = noUsage ? '—' : Math.round(data.context_pct) + '%';
    document.getElementById('bar-ctx').style.width = data.context_pct + '%';
    const cur = data.currency_symbol || '$';
    document.getElementById('val-cost').textContent = noUsage ? '—' : cur + data.cost_usd.toFixed(4);
    const mins = Math.floor(data.duration_ms / 60000);
    const secs = Math.floor((data.duration_ms % 60000) / 1000);
    document.getElementById('val-dur').textContent = mins + 'm ' + secs + 's';
    const wk = data.weekly || {};
    if (wk.available) {
      document.getElementById('val-week-cost').textContent = cur + wk.total_cost.toFixed(2);
      document.getElementById('val-week-sessions').textContent = wk.total_sessions;
    } else {
      document.getElementById('val-week-cost').textContent = '—';
      document.getElementById('val-week-sessions').textContent = '—';
    }
    const wc = data.week_compare || {};
    const cmp = document.getElementById('week-compare');
    if (wc.available && wc.this_week && wc.cost_pct !== null && wc.cost_pct !== undefined) {
      const f = p => (p > 0 ? '+' : '') + p + '%';
      cmp.textContent = T.week_compare
        .replace('{cost}', f(wc.cost_pct))
        .replace('{sessions}', f(wc.session_pct))
        .replace('{tokens}', f(wc.token_pct));
    } else {
      cmp.textContent = '—';
    }
    const note = document.getElementById('pricing-note');
    if (data.pricing_configured) {
      note.style.display = 'none';
    } else {
      note.textContent = T.pricing_note.replace('{id}', data.model_id);
      note.style.display = 'block';
    }
    document.getElementById('update-time').textContent = new Date().toLocaleTimeString();

    const area = document.getElementById('widgets-area');
    area.innerHTML = '';
    if (data.widgets) {
      data.widgets.forEach(w => {
        const div = document.createElement('div');
        div.className = 'card';
        div.style.marginBottom = '8px';
        const title = document.createElement('div');
        title.className = 'card-title';
        title.textContent = w.name;
        const body = document.createElement('div');
        body.style.cssText = 'font-size:11px;color:#c9d1d9;white-space:pre-wrap;';
        body.textContent = w.output || '—';
        div.appendChild(title);
        div.appendChild(body);
        area.appendChild(div);
      });
    }
  } catch(e) {
    console.error('refresh error:', e);
  }
}
let sessionOffset = 0;
function formatDur(secs) {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return (m > 0 ? m + 'm ' : '') + s + 's';
}
function formatTok(tok) {
  return tok >= 1000 ? (tok / 1000).toFixed(1) + 'k' : String(tok);
}
async function loadSessions() {
  try {
    const resp = await fetch('/api/sessions?limit=10&offset=' + sessionOffset);
    const data = await resp.json();
    const cur = data.currency_symbol || '$';
    const tbody = document.getElementById('sessions-body');
    if (!data.available || !data.sessions || !data.sessions.length) {
      document.getElementById('sessions-more').style.display = 'none';
      if (sessionOffset === 0) {
        const tr = document.createElement('tr');
        const td = document.createElement('td');
        td.colSpan = 5;
        td.style.color = '#8b949e';
        td.textContent = '—';
        tr.appendChild(td);
        tbody.appendChild(tr);
      }
      return;
    }
    data.sessions.forEach(s => {
      const tr = document.createElement('tr');
      tr.className = 'session-row';
      [s.started_at,
       cur + s.total_cost_usd.toFixed(2),
       formatDur(s.duration_secs),
       String(s.agent_count),
       formatTok(s.total_tokens)].forEach(text => {
        const td = document.createElement('td');
        td.textContent = text;
        tr.appendChild(td);
      });
      tr.addEventListener('click', () => toggleSessionDetail(tr, s.id));
      tbody.appendChild(tr);
    });
    sessionOffset += data.sessions.length;
  } catch(e) {
    console.error('sessions error:', e);
  }
}
async function toggleSessionDetail(tr, id) {
  const next = tr.nextElementSibling;
  if (next && next.className === 'session-detail') {
    next.remove();
    return;
  }
  const row = document.createElement('tr');
  row.className = 'session-detail';
  const td = document.createElement('td');
  td.colSpan = 5;
  td.textContent = '…';
  row.appendChild(td);
  tr.parentNode.insertBefore(row, tr.nextSibling);
  try {
    const resp = await fetch('/api/sessions/' + id);
    const d = await resp.json();
    const inout = (d.transcript_detail && d.transcript_detail.available)
      ? T.h_tokens
          .replace('{tok}', formatTok(d.total_tokens))
          .replace('{in}', d.transcript_detail.tokens_in)
          .replace('{out}', d.transcript_detail.tokens_out)
      : T.h_tokens_plain.replace('{tok}', formatTok(d.total_tokens));
    const tools = (d.tools || []).map(t => T.h_tool_line
      .replace('{tool}', t.tool)
      .replace('{n}', t.calls)
      .replace('{sym}', d.currency_symbol || '$')
      .replace('{cost}', t.cost.toFixed(2)));
    td.textContent = [T.h_model.replace('{model}', d.model), inout].join(' · ')
      + (tools.length ? '\n' + T.h_tools_title + ': ' + tools.join('; ') : '');
  } catch(e) {
    td.textContent = T.not_found;
  }
}
async function loadWindows() {
  try {
    const d = await (await fetch('/api/windows')).json();
    const rows = d.windows || [];
    const tbody = document.getElementById('windows-body');
    const empty = document.getElementById('windows-empty');
    if (rows.length === 0) { tbody.innerHTML = ''; empty.style.display = ''; return; }
    empty.style.display = 'none';
    const sym = d.currency_symbol || '$';
    tbody.innerHTML = rows.map(w =>
      '<tr><td>' + w.status + '</td><td>' + w.dir + '</td><td>' + w.model + '</td>' +
      '<td>' + Math.round(w.pct) + '%</td><td>' + sym + w.cost.toFixed(2) + '</td>' +
      '<td>' + w.tokens + '</td><td>' + w.agents + '</td></tr>'
    ).join('');
  } catch(e) { console.error('windows error:', e); }
}
async function loadTotals() {
  try {
    const d = await (await fetch('/api/totals')).json();
    if (d.available === false) { return; }
    const cur = d.currency_symbol || '$';
    document.getElementById('totals-line').textContent = T.totals_line
      .replace('{n}', d.sessions)
      .replace('{sym}', cur)
      .replace('{cost}', d.total_cost.toFixed(2))
      .replace('{tok}', formatTok(d.total_tokens))
      .replace('{avg}', d.avg_duration_min.toFixed(1));
  } catch(e) { console.error('totals error:', e); }
}
loadWindows();
loadTotals();
setInterval(() => { loadWindows(); loadTotals(); }, 2000);
loadSessions();
document.getElementById('sessions-more').addEventListener('click', loadSessions);
// ㉒ 会话选择器：填充历史会话下拉；切换 → 带 session_id 刷新 /api/data
async function loadSessionOptions() {
  try {
    const resp = await fetch('/api/sessions?limit=50&offset=0');
    const data = await resp.json();
    const sel = document.getElementById('session-select');
    if (data.available && data.sessions) {
      data.sessions.forEach(s => {
        const opt = document.createElement('option');
        opt.value = s.id;
        opt.textContent = '#' + s.id + '  ' + s.started_at + '  ' + s.model;
        sel.appendChild(opt);
      });
    }
  } catch(e) { console.error('session options error:', e); }
}
document.getElementById('session-select').addEventListener('change', e => {
  selectedSession = e.target.value;
  refresh();
});
loadSessionOptions();
refresh();
setInterval(refresh, 2000);
</script>
</body>
</html>"#;
    let lang = config.language();
    html.replace("{web_title}", tr(lang, "web.title"))
        .replace("{web_lang}", config.language().code())
        .replace("{web_heading}", tr(lang, "web.heading"))
        .replace("{web_model}", tr(lang, "web.model"))
        .replace("{web_ctx_window}", tr(lang, "web.ctx_window"))
        .replace("{web_used}", tr(lang, "web.used"))
        .replace("{web_session_cost}", tr(lang, "web.session_cost"))
        .replace("{web_usd}", tr(lang, "web.usd"))
        .replace("{web_duration}", tr(lang, "web.duration"))
        .replace("{web_active}", tr(lang, "web.active"))
        .replace("{web_this_week}", tr(lang, "web.this_week"))
        .replace("{web_sessions}", tr(lang, "web.sessions"))
        .replace("{web_cost_trend}", tr(lang, "web.cost_trend"))
        .replace("{web_realtime}", tr(lang, "web.realtime"))
        .replace("T_PRICING_NOTE", tr(lang, "web.pricing_note"))
        .replace("T_NOT_FOUND", tr(lang, "web.not_found"))
        .replace("{web_trend_svg}", &trend_card_html(&cached_history().1, lang))
        .replace("{web_sessions_title}", tr(lang, "web.sessions_title"))
        .replace("{web_col_time}", tr(lang, "web.col_time"))
        .replace("{web_col_cost}", tr(lang, "web.col_cost"))
        .replace("{web_col_duration}", tr(lang, "web.col_duration"))
        .replace("{web_col_agents}", tr(lang, "web.col_agents"))
        .replace("{web_col_tokens}", tr(lang, "web.col_tokens"))
        .replace("{web_load_more}", tr(lang, "web.load_more"))
        .replace("T_LOAD_MORE", tr(lang, "web.load_more"))
        .replace("T_H_MODEL", tr(lang, "runtime.h_session_model"))
        .replace("T_H_TOKENS", tr(lang, "runtime.h_session_tokens"))
        .replace("T_H_TOKENS_PLAIN", tr(lang, "runtime.h_session_tokens_plain"))
        .replace("T_H_TOOLS_TITLE", tr(lang, "runtime.h_tools_title"))
        .replace("T_H_TOOL_LINE", tr(lang, "runtime.h_tool_line"))
        .replace("T_WEEK_COMPARE", tr(lang, "web.week_compare"))
        .replace("{web_windows_title}", tr(lang, "web.windows_title"))
        .replace("{web_win_none}", tr(lang, "web.win_none"))
        .replace("{web_w_col_status}", tr(lang, "web.w_col_status"))
        .replace("{web_w_col_dir}", tr(lang, "web.w_col_dir"))
        .replace("{web_w_col_model}", tr(lang, "web.w_col_model"))
        .replace("{web_w_col_ctx}", tr(lang, "web.w_col_ctx"))
        .replace("{web_w_col_cost}", tr(lang, "web.w_col_cost"))
        .replace("{web_w_col_tokens}", tr(lang, "web.w_col_tokens"))
        .replace("{web_w_col_agents}", tr(lang, "web.w_col_agents"))
        .replace("{web_totals_title}", tr(lang, "web.totals_title"))
        .replace("T_TOTALS_LINE", tr(lang, "web.totals_line"))
        .replace("{web_current_session}", tr(lang, "web.current_session"))
        .replace("{web_ctx_estimated}", tr(lang, "web.ctx_estimated"))
}

/// /config 卡片分组表单页：服务端渲染字段控件 + 内联 JS
/// （读 /api/config 填表、脏标记、卡片级错误高亮、POST 提交）。
fn build_config_html(
    registry: &WidgetRegistry,
    _config: &AppConfig,
    lang: crate::core::i18n::Language,
) -> String {
    use crate::core::config_schema;
    let t = |k: &'static str| tr(lang, k);
    let is_new = |k: &str| {
        matches!(
            k,
            "runtime_overrides.compact_lines"
                | "runtime_overrides.animation.enabled"
                | "theme.icon_set"
        )
    };
    let fields = config_schema::fields();
    let mut cards_html = String::new();
    for g in config_schema::Group::all() {
        let gfields: Vec<&config_schema::FieldDef> =
            fields.iter().filter(|f| f.group == g).collect();
        let mut rows = String::new();
        for f in &gfields {
            let key = f.key;
            let label = t(f.label);
            let new_badge = if is_new(key) {
                "<span class=\"new\">NEW</span>"
            } else {
                ""
            };
            let control = match f.kind {
                config_schema::FieldKind::Choice => {
                    let opts = config_schema::options_for(f, registry);
                    let options: String = opts
                        .iter()
                        .map(|o| {
                            format!(
                                "<option value=\"{}\">{}</option>",
                                html_escape(o),
                                if o.is_empty() { t("config.none_option") } else { o }
                            )
                        })
                        .collect();
                    format!("<select id=\"{key}\" name=\"{key}\">{options}</select>")
                }
                config_schema::FieldKind::Bool => {
                    format!(
                        "<input type=\"checkbox\" id=\"{key}\" name=\"{key}\" value=\"true\">"
                    )
                }
                config_schema::FieldKind::Multi => {
                    let opts = config_schema::options_for(f, registry);
                    let boxes: String = opts
                        .iter()
                        .map(|o| {
                            format!(
                                "<label><input type=\"checkbox\" name=\"{key}\" value=\"{}\"> {}</label> ",
                                html_escape(o), o
                            )
                        })
                        .collect();
                    format!("<div id=\"{key}\">{boxes}</div>")
                }
                config_schema::FieldKind::NumberList => {
                    format!("<input type=\"text\" id=\"{key}\" name=\"{key}\" placeholder=\"50,80,100\">")
                }
                _ => {
                    // compact_lines 空值 = auto（跟随布局），占位提示
                    let ph = if key == "runtime_overrides.compact_lines" {
                        " placeholder=\"auto\""
                    } else {
                        ""
                    };
                    format!("<input type=\"text\" id=\"{key}\" name=\"{key}\"{ph}>")
                }
            };
            rows.push_str(&format!(
                "<div class=\"row\"><span class=\"label\">{new_badge} {label}</span>\
                 <span class=\"ctl\">{control}</span></div>"
            ));
        }
        let count = t("web.cfg_group_count").replace("{n}", &gfields.len().to_string());
        cards_html.push_str(&format!(
            "<div class=\"card\" data-group=\"{}\"><div class=\"card-title\">{}</div>{}</div>",
            g.name(),
            format!("{} · {}", t(g.name()), count),
            rows
        ));
    }
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>{cfg_title}</title>
<style>
body {{ font-family: sans-serif; margin: 0; background: #f6f7f9; color: #1a1a2e; }}
.savebar {{ position: sticky; top: 0; z-index: 10; display: flex; align-items: center;
  justify-content: space-between; gap: 1rem; background: #fff;
  border-bottom: 1px solid #e2e4e8; padding: 0.6rem 1.2rem; }}
.save-title {{ font-weight: 700; font-size: 14px; }}
.save-hint {{ font-weight: 400; color: #999; font-size: 12px; }}
.savebar-actions {{ display: flex; align-items: center; gap: 0.6rem; }}
.badge {{ display: none; color: #b45309; background: #fef3c7; border-radius: 6px;
  padding: 0.15rem 0.5rem; font-size: 12px; font-weight: 600; }}
button[type=submit] {{ background: #2563eb; color: #fff; border: 0;
  border-radius: 6px; padding: 0.3rem 1rem; font-weight: 600; cursor: pointer; }}
.wrap {{ max-width: 64rem; margin: 1.2rem auto; padding: 0 1.2rem; }}
.cards {{ display: grid; grid-template-columns: 1fr 1fr; gap: 0.9rem; }}
@media (max-width: 48rem) {{ .cards {{ grid-template-columns: 1fr; }} }}
.card {{ background: #fff; border: 1px solid #e2e4e8; border-radius: 10px;
  padding: 0.8rem 1rem; }}
.card-title {{ font-weight: 700; margin-bottom: 0.5rem; color: #1a1a2e; }}
.card .count {{ color: #999; font-weight: 400; font-size: 11px; }}
.card-error {{ border-color: #c00; }}
.row {{ display: flex; align-items: center; justify-content: space-between;
  padding: 0.28rem 0; }}
.row + .row {{ border-top: 1px solid #f0f1f4; }}
.label {{ color: #444; }}
.ctl select, .ctl input[type=text] {{ border: 1px solid #d0d3d9;
  border-radius: 6px; padding: 0.15rem 0.4rem; background: #fff; min-width: 12rem; }}
.ctl input[type=checkbox] {{ width: 16px; height: 16px; accent-color: #2563eb; }}
.new {{ background: #16a34a; color: #fff; border-radius: 4px; padding: 0 0.3rem;
  font-size: 10px; margin-right: 0.3rem; vertical-align: 1px; }}
.pricing {{ border: 1px dashed #c9cdd6; border-radius: 10px; padding: 0.8rem 1rem;
  margin-top: 0.9rem; background: #fff; }}
.pricing-title {{ font-weight: 700; color: #1a1a2e; }}
.pricing-readonly {{ color: #999; font-size: 11px; font-weight: 400; }}
.pricing-toggle {{ color: #2563eb; font-size: 12px; cursor: pointer;
  user-select: none; margin-left: 0.4rem; }}
.pricing-summary {{ color: #999; font-size: 12px; margin-top: 0.3rem; }}
#pricing-body {{ display: none; margin-top: 0.6rem; }}
#pricing-body table {{ border-collapse: collapse; font-size: 12px; width: 100%; }}
#pricing-body th, #pricing-body td {{ padding: 0.2rem 0.5rem;
  border-bottom: 1px solid #f0f1f4; text-align: left; }}
#status {{ font-weight: bold; font-size: 13px; }}
#status.ok {{ color: #16a34a; }}
#status.err {{ color: #c00; }}
</style></head>
<body>
<div class="savebar">
  <div class="save-title">{cfg_title}<span class="save-hint">{cfg_save_hint}</span></div>
  <div class="savebar-actions">
    <span class="badge" id="dirty">{cfg_dirty}</span>
    <button type="submit" form="cfg-form">{cfg_save}</button>
  </div>
</div>
<div class="wrap">
<form id="cfg-form" onsubmit="return submitCfg(event)">
<div class="cards">{cards_html}</div>
<div class="pricing">
  <div class="pricing-title">{cfg_models_title}
    <span class="pricing-readonly">· {cfg_readonly_note}</span>
    <span class="pricing-toggle" id="pricing-toggle" onclick="togglePricing()">{cfg_models_toggle} ▾</span>
  </div>
  <div class="pricing-summary" id="pricing-summary"></div>
  <div id="pricing-body"></div>
</div>
</form>
<p id="status"></p>
</div>
<script>
const CFG_KEYS = [FIELDS_JSON];
let dirty = false;
function markDirty() {{
  if (!dirty) {{
    dirty = true;
    document.getElementById('dirty').style.display = 'inline-block';
  }}
}}
document.querySelectorAll('#cfg-form input, #cfg-form select').forEach(el => {{
  ['input', 'change'].forEach(ev => el.addEventListener(ev, markDirty));
}});
async function loadCfg() {{
  const r = await fetch('/api/config');
  const d = await r.json();
  for (const [k, v] of Object.entries(d.current)) {{
    const el = document.querySelector(`[name="${{k}}"]`);
    if (!el) continue;
    if (el.type === 'checkbox') {{ el.checked = (v === 'true'); }}
    else if (el.tagName === 'SELECT') {{ el.value = v; }}
    else el.value = v;
  }}
  const ro = d.readonly.models || [];
  const n = ro.length;
  const first = ro.slice(0, 1).map(m =>
    `${{m.id}} · ${{m.window}} · $${{m.usd_in}} / $${{m.usd_out}}`).join('');
  document.getElementById('pricing-summary').textContent =
    (first ? first + ' …' : '') +
    (n ? ' · ' + '{cfg_group_count}'.replace('{{n}}', n) : '');
  document.getElementById('pricing-body').innerHTML =
    '<table><tr><th>model</th><th>window</th><th>USD in</th><th>USD out</th><th>CNY in</th><th>CNY out</th></tr>' +
    ro.map(m => `<tr><td>${{m.id}}</td><td>${{m.window}}</td><td>${{m.usd_in}}</td><td>${{m.usd_out}}</td><td>${{m.cny_in}}</td><td>${{m.cny_out}}</td></tr>`).join('') +
    '</table>';
}}
function togglePricing() {{
  const body = document.getElementById('pricing-body');
  const toggle = document.getElementById('pricing-toggle');
  const open = body.style.display !== 'block';
  body.style.display = open ? 'block' : 'none';
  toggle.textContent = '{cfg_models_toggle} ' + (open ? '▴' : '▾');
}}
function collectCfg() {{
  const out = {{}};
  for (const k of CFG_KEYS) {{
    const els = document.querySelectorAll(`[name="${{k}}"]`);
    if (!els.length) continue;
    // Table 形态主题（custom）跳过提交：服务端会报「手工编辑」误错
    if (k === 'theme' && els[0].value === '(custom)') continue;
    if (els[0].type === 'checkbox') {{ out[k] = els[0].checked ? 'true' : 'false'; continue; }}
    if (els.length > 1) {{ out[k] = [...els].filter(e => e.checked).map(e => e.value).join(','); continue; }}
    out[k] = els[0].value;
  }}
  return out;
}}
async function submitCfg(ev) {{
  ev.preventDefault();
  const status = document.getElementById('status');
  status.className = '';
  document.querySelectorAll('.card-error').forEach(c => c.classList.remove('card-error'));
  try {{
    const r = await fetch('/api/config', {{ method: 'POST',
      headers: {{'Content-Type': 'application/json'}}, body: JSON.stringify(collectCfg()) }});
    const d = await r.json();
    if (d.ok) {{
      status.textContent = '{cfg_saved}'; status.className = 'ok';
      dirty = false;
      document.getElementById('dirty').style.display = 'none';
    }} else {{
      status.textContent = '{cfg_error}'.replace('{{err}}', (d.field ? d.field + ': ' : '') + d.error);
      status.className = 'err';
      if (d.field) {{
        const el = document.querySelector(`[name="${{d.field}}"]`);
        const card = el && el.closest('.card');
        if (card) card.classList.add('card-error');
      }}
    }}
  }} catch (e) {{
    status.textContent = '{cfg_error}'.replace('{{err}}', e); status.className = 'err';
  }}
  return false;
}}
loadCfg();
</script>
</body></html>"#,
        cfg_title = t("web.cfg_title"),
        cfg_save_hint = t("web.cfg_save_hint"),
        cfg_dirty = t("web.cfg_dirty"),
        cfg_models_title = t("web.cfg_models_title"),
        cfg_readonly_note = t("web.cfg_readonly_note"),
        cfg_models_toggle = t("web.cfg_models_toggle"),
        cfg_save = t("web.cfg_save"),
        cfg_saved = t("web.cfg_saved"),
        cfg_error = t("web.cfg_error"),
        cfg_group_count = t("web.cfg_group_count"),
        cards_html = cards_html,
    )
    .replace("FIELDS_JSON", &{
        let keys: Vec<String> = config_schema::fields()
            .iter()
            .map(|f| format!("\"{}\"", f.key))
            .collect();
        keys.join(",")
    })
}

/// HTML 转义（字段选项为本地文件名/预设名，防御性转义双引号）。
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}


#[cfg(test)]
mod tests {
    use super::build_api_json;
    use super::build_dashboard_html;
    use super::model_pricing_configured;
    use super::pct_change;
    use super::query_param;
    use super::session_data_by_id;
    use super::session_data_from_record;
    use super::session_data_from_window;
    use super::session_detail_json;
    use super::sessions_list_json;
    use super::totals_json;
    use super::trend_card_html;
    use super::windows_json;
    use super::trend_svg;
    use super::ttl_fresh;
    use super::week_compare_json;
    use crate::core::config::AppConfig;
    use crate::core::history::SessionRecord;
    use crate::core::session::SessionData;
    use crate::core::transcript::{TokenTotal, TranscriptSummary};
    use crate::core::theme::Theme;
    use crate::core::widget::Widget;
    use crate::core::widget::WidgetConfig;
    use crate::core::widget::WidgetRegistry;
    use ratatui::layout::Rect;
    use ratatui::Frame;
    use serde_json::{Value, json};
    use std::time::{Duration, Instant};

    /// v0.7 内置注册表后：内置模型（deepseek-v4-flash 等）未手写 [pricing]
    /// 不算"未配置单价"；未知模型才算。曾只查 [pricing] 导致内置模型误报
    /// 页面警告"当前模型未配置单价"。
    #[test]
    fn pricing_configured_includes_builtin_registry() {
        let cfg = AppConfig::default();
        assert!(model_pricing_configured(&cfg, "deepseek-v4-flash"), "内置模型有价");
        assert!(model_pricing_configured(&cfg, "claude-sonnet-4-6"), "内置 claude 有价");
        assert!(!model_pricing_configured(&cfg, "no-such-model-xyz"), "未知模型无价");
        let cfg2: AppConfig =
            toml::from_str("[pricing]\ncustom-x = { input = 1.0, output = 2.0 }\n").unwrap();
        assert!(model_pricing_configured(&cfg2, "custom-x"), "用户 [pricing] 覆盖生效");
    }

    /// 前端无数据占位判定需要 token 总量（cost=0 && in+out=0 → 「—」，
    /// 与桌面 dashboard 诚实降级同口径）。
    #[test]
    fn api_json_includes_token_totals() {
        let cfg = AppConfig::default();
        let registry = WidgetRegistry::new();
        let out = build_api_json(&registry, &cfg, &Theme::default(), None, None);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v["total_input_tokens"].is_number(), "missing in tokens: {}", out);
        assert!(v["total_output_tokens"].is_number(), "missing out tokens: {}", out);
    }

    /// v0.7 币种透传：zh 配置 → currency_symbol 为 ¥。
    #[test]
    fn api_json_includes_currency_symbol() {
        let mut cfg = AppConfig::default();
        cfg.language = "zh".into();
        let registry = WidgetRegistry::new();
        let theme = Theme::default();
        let out = build_api_json(&registry, &cfg, &theme, None, None);
        assert!(out.contains("\"currency_symbol\":\"¥\""), "zh → ¥: {}", out);
    }

    /// 卡片标题 i18n（zh）：widgets[].name 必须翻译（"上下文栏" 而非 "Context Bar"），
    /// 与 draw_tabbed / widget list 的 tr_dyn 口径一致。
    #[test]
    fn api_json_widget_names_translated_for_zh() {
        let cfg: AppConfig = toml::from_str(
            "language = \"zh\"\ncompact_layout = [\"model_display\", \"context_bar\", \"agent_overview\", \"cost_display\", \"skills_mcp\", \"token_rate\", \"alerts\"]\n",
        )
        .unwrap();
        let mut registry = WidgetRegistry::new();
        crate::widgets::register_all(&mut registry, &cfg);
        let out = build_api_json(&registry, &cfg, &Theme::default(), None, None);
        let v: Value = serde_json::from_str(&out).unwrap();
        let widgets = v["widgets"].as_array().expect("widgets array");
        assert!(!widgets.is_empty(), "default layout has widgets");
        for w in widgets {
            let id = w["id"].as_str().unwrap();
            let name = w["name"].as_str().unwrap();
            assert_ne!(
                name, id,
                "widget '{}' name not translated (fell back to id)",
                id
            );
        }
        let names: Vec<&str> = widgets.iter().map(|w| w["name"].as_str().unwrap()).collect();
        assert!(
            names.iter().any(|n| *n == "上下文栏"),
            "context_bar translated: {:?}",
            names
        );
        assert!(
            names.iter().any(|n| *n == "代理总览"),
            "agent_overview translated: {:?}",
            names
        );
    }

    /// 卡片标题 i18n（en）：保持 display_name 原样（en 键存在时值与 display_name 一致）。
    #[test]
    fn api_json_widget_names_english_by_default() {
        let mut registry = WidgetRegistry::new();
        crate::widgets::register_all(&mut registry, &AppConfig::default());
        let out = build_api_json(&registry, &AppConfig::default(), &Theme::default(), None, None);
        let v: Value = serde_json::from_str(&out).unwrap();
        for w in v["widgets"].as_array().unwrap() {
            assert_eq!(
                w["name"].as_str().unwrap(),
                registry.get(w["id"].as_str().unwrap()).unwrap().display_name(),
                "en name = display_name"
            );
        }
    }

    /// ㉒ 历史会话数据：total_tokens ÷ [models] 窗口 = 估算 pct（30%）。
    #[test]
    fn session_data_from_record_pct_from_tokens_over_window() {
        let cfg: AppConfig = toml::from_str(
            "[models]\nm = { context_window = 200000 }\n",
        )
        .unwrap();
        let record = SessionRecord {
            id: 7,
            started_at: "2026-08-05T10:00:00".into(),
            duration_secs: 120,
            total_cost_usd: 0.55,
            total_tokens: 60_000,
            agent_count: 2,
            model: "m".into(),
            transcript_path: None,
        };
        let data = session_data_from_record(&record, None, &cfg);
        assert!((data.context_window.used_percentage - 30.0).abs() < 1e-9, "30%: {}", data.context_window.used_percentage);
        assert_eq!(data.context_window.context_window_size, 200_000);
        assert_eq!(data.context_window.total_input_tokens, 60_000, "无 transcript → in=total");
        assert_eq!(data.context_window.total_output_tokens, 0);
        assert_eq!(data.cost.total_duration_ms, 120_000, "secs→ms");
        assert_eq!(data.model.id, "m");
        assert_eq!(data.model.display_name, "m", "历史会话 display=id");
    }

    /// ㉒ transcript 细分 in/out 优先（有 transcript 时）。
    #[test]
    fn session_data_from_record_transcript_splits_in_out() {
        let cfg: AppConfig = toml::from_str(
            "[models]\nm = { context_window = 200000 }\n",
        )
        .unwrap();
        let record = SessionRecord {
            id: 1,
            started_at: "2026-08-05T10:00:00".into(),
            duration_secs: 60,
            total_cost_usd: 0.1,
            total_tokens: 999_999, // 故意与 transcript 不一致，验证 transcript 优先
            agent_count: 1,
            model: "m".into(),
            transcript_path: None,
        };
        let summary = TranscriptSummary {
            total_tokens: TokenTotal { input: 40_000, output: 20_000, ..Default::default() },
            ..Default::default()
        };
        let data = session_data_from_record(&record, Some(&summary), &cfg);
        assert_eq!(data.context_window.total_input_tokens, 40_000);
        assert_eq!(data.context_window.total_output_tokens, 20_000);
        assert!((data.context_window.used_percentage - 30.0).abs() < 1e-9, "30%: {}", data.context_window.used_percentage);
    }

    /// ㉒ 窗口未知 → pct 0（前端显示 —，不产生虚假百分比）；超窗口钳制 100。
    #[test]
    fn session_data_from_record_window_unknown_or_clamped() {
        let cfg = AppConfig::default(); // 无 [models] 窗口 → 0
        let record = SessionRecord {
            id: 2,
            started_at: "2026-08-05T10:00:00".into(),
            duration_secs: 60,
            total_cost_usd: 0.1,
            total_tokens: 50_000,
            agent_count: 1,
            model: "no-such-model".into(),
            transcript_path: None,
        };
        let data = session_data_from_record(&record, None, &cfg);
        assert_eq!(data.context_window.used_percentage, 0.0, "unknown window → 0");
        assert_eq!(data.context_window.context_window_size, 0);

        let cfg2: AppConfig = toml::from_str("[models]\nm = { context_window = 10000 }\n").unwrap();
        let mut rec2 = record.clone();
        rec2.model = "m".into();
        rec2.total_tokens = 50_000; // 5× 窗口
        let data2 = session_data_from_record(&rec2, None, &cfg2);
        assert_eq!(data2.context_window.used_percentage, 100.0, "clamped to 100");
    }

    /// ㉒ 响应带 session_id（live=null）；session_data_by_id 未找到 → 空数据。
    #[test]
    fn api_json_includes_session_id_field() {
        let registry = WidgetRegistry::new();
        let out = build_api_json(&registry, &AppConfig::default(), &Theme::default(), None, None);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v["session_id"].is_null(), "live view session_id null: {}", out);
        let data = session_data_by_id(999_999, &AppConfig::default());
        assert!(data.is_none(), "unknown id → none");
    }

    /// ㉒ 前端：header 会话下拉（默认「当前会话」）+ 估算标注挂点。
    #[test]
    fn dashboard_html_has_session_selector() {
        let cfg_zh: AppConfig = toml::from_str("language = \"zh\"\n").unwrap();
        let html_zh = build_dashboard_html(&WidgetRegistry::new(), &cfg_zh, &Theme::default());
        assert!(html_zh.contains("session-select"), "selector present");
        assert!(html_zh.contains("当前会话"), "zh default option");
        assert!(html_zh.contains("估算"), "zh estimate tag");
        let html_en = build_dashboard_html(
            &WidgetRegistry::new(),
            &AppConfig::default(),
            &Theme::default(),
        );
        assert!(html_en.contains("Current session"), "en default option");
        assert!(html_en.contains("est."), "en estimate tag");
    }

    /// 空输出占位：前端卡片 body 用 w.output || '—'（无数据 widget 显示 —）。
    #[test]
    fn dashboard_html_widget_empty_placeholder() {
        let html = build_dashboard_html(&WidgetRegistry::new(), &AppConfig::default(), &Theme::default());
        assert!(
            html.contains("textContent = w.output || '—'"),
            "html must render dash for empty widget output"
        );
    }

    /// Widget 输出含 ANSI 色码与 \r/\t/引号/反斜杠（compact 输出的真实形态）。
    /// build_api_json 必须产出合法 JSON，且 ANSI 被剥离（web 显示纯文本）。
    #[test]
    fn api_json_valid_with_ansi_and_control_chars() {
        struct NastyWidget;
        impl Widget for NastyWidget {
            fn id(&self) -> &str {
                "nasty"
            }
            fn display_name(&self) -> &str {
                "Nasty"
            }
            fn render_compact(
                &self,
                _data: &SessionData,
                _theme: &Theme,
                _config: &WidgetConfig,
            ) -> String {
                "\x1b[31mred\x1b[0m\r\n\t\"q\" \\s".to_string()
            }
            fn render_dashboard(
                &self,
                _data: &SessionData,
                _area: Rect,
                _frame: &mut Frame,
                _theme: &Theme,
                _config: &WidgetConfig,
            ) {
            }
        }
        let mut registry = WidgetRegistry::new();
        registry.register(Box::new(NastyWidget));
        let cfg: AppConfig = toml::from_str("compact_layout = [\"nasty\"]\n").unwrap();

        let json = build_api_json(&registry, &cfg, &Theme::default(), None, None);
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("api/data must be valid JSON");
        let out = value["widgets"][0]["output"]
            .as_str()
            .expect("widget output is a string");
        assert_eq!(out, "red\r\n\t\"q\" \\s");
        assert!(!json.contains('\x1b'), "no raw ESC in payload");
    }

    #[test]
    fn ttl_fresh_boundary() {
        let t0 = Instant::now();
        assert!(ttl_fresh(t0, t0 + Duration::from_secs(29), Duration::from_secs(30)));
        assert!(!ttl_fresh(t0, t0 + Duration::from_secs(30), Duration::from_secs(30)));
        assert!(!ttl_fresh(t0, t0 + Duration::from_secs(301), Duration::from_secs(300)));
    }

    #[test]
    fn dashboard_html_respects_language() {
        let cfg_zh: AppConfig = toml::from_str("language = \"zh\"\n").unwrap();
        let html_zh = build_dashboard_html(
            &WidgetRegistry::new(),
            &cfg_zh,
            &Theme::default(),
        );
        assert!(html_zh.contains("仪表盘"));
        assert!(html_zh.contains("模型"));
        let html_en = build_dashboard_html(
            &WidgetRegistry::new(),
            &AppConfig::default(),
            &Theme::default(),
        );
        assert!(html_en.contains("Claude HUD Dashboard"));
        assert!(html_en.contains("Model"));
        // 标记全部被替换（无残留 {web_）
        assert!(!html_en.contains("{web_"));
        assert!(!html_zh.contains("{web_"));
        // 分页按钮必须绑定 click（曾有按钮无事件、列表永远前 10 条的回归）
        assert!(html_en.contains("addEventListener('click', loadSessions)"));
    }

    #[test]
    fn trend_svg_two_points() {
        let days = vec![
            ("2026-08-01".to_string(), 1.0),
            ("2026-08-02".to_string(), 3.0),
        ];
        let svg = trend_svg(&days).expect("2+ points render");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("<polyline points="));
        assert!(svg.contains("<circle"));
        assert!(svg.contains("08-01"));
        assert!(svg.contains("08-02"));
    }

    /// 数据标注：每个数据点上方必须有数值文本（{:.2}），与日期标签共存。
    #[test]
    fn trend_svg_labels_values_above_points() {
        let days = vec![
            ("2026-08-01".to_string(), 1.0),
            ("2026-08-02".to_string(), 3.0),
        ];
        let svg = trend_svg(&days).expect("2+ points render");
        assert!(svg.contains(">1.00<"), "value label 1.0: {}", svg);
        assert!(svg.contains(">3.00<"), "value label 3.0: {}", svg);
        assert!(svg.contains(">08-01<"), "date label kept: {}", svg);
    }

    #[test]
    fn trend_svg_insufficient_points_none() {
        assert!(trend_svg(&[]).is_none());
        assert!(trend_svg(&[("2026-08-01".to_string(), 1.0)]).is_none());
    }

    #[test]
    fn trend_card_html_svg_or_placeholder() {
        use crate::core::i18n::Language;
        let trend = json!({"available": true, "days": [
            {"day": "2026-08-01", "cost": 1.0},
            {"day": "2026-08-02", "cost": 3.0},
        ]});
        let html = trend_card_html(&trend, Language::En);
        assert!(html.contains("<svg"));
        let empty = json!({"available": false, "days": []});
        let html2 = trend_card_html(&empty, Language::En);
        assert!(!html2.contains("<svg"));
        assert!(html2.contains("No trend data yet"));
    }

    #[test]
    fn query_param_parses_and_defaults() {
        assert_eq!(query_param(Some("limit=1&offset=2"), "limit"), Some("1".to_string()));
        assert_eq!(query_param(Some("limit=1&offset=2"), "offset"), Some("2".to_string()));
        assert_eq!(query_param(Some("limit=1"), "date"), None);
        assert_eq!(query_param(None, "limit"), None);
        assert_eq!(query_param(Some(""), "limit"), None);
    }

    #[test]
    fn sessions_list_json_fields() {
        use crate::core::history::SessionRecord;
        let rows = vec![SessionRecord {
            id: 2,
            started_at: "2026-08-02 10:00:00".to_string(),
            duration_secs: 60,
            total_cost_usd: 1.25,
            total_tokens: 5000,
            agent_count: 1,
            model: "claude-sonnet-4-6".to_string(),
            transcript_path: None,
        }];
        let v = sessions_list_json(&rows);
        assert_eq!(v["sessions"][0]["id"], json!(2));
        assert_eq!(v["sessions"][0]["model"], json!("claude-sonnet-4-6"));
        assert_eq!(v["sessions"][0]["total_cost_usd"], json!(1.25));
        assert_eq!(v["sessions"][0]["total_tokens"], json!(5000));
    }

    #[test]
    fn windows_json_maps_fields() {
        let mut w = crate::core::windows::WindowInfo {
            key: "k".to_string(),
            dir_name: "proj-a".to_string(),
            status: crate::core::windows::WindowStatus::Active,
            model: "op-us".to_string(),
            used_pct: 42.0,
            tokens_in: 1000,
            tokens_out: 500,
            cache_read: 0,
            cache_created: 0,
            cost: 1.25,
            agent_count: 2,
            ts: 0,
            corrupt: false,
            data_source: "reported".to_string(),
        };
        let v = windows_json(&[w.clone()]);
        assert_eq!(v["windows"][0]["dir"], json!("proj-a"));
        assert_eq!(v["windows"][0]["status"], json!("active"));
        assert_eq!(v["windows"][0]["tokens"], json!(1500));
        assert_eq!(v["windows"][0]["cache_read"], json!(0));
        assert_eq!(v["windows"][0]["cache_created"], json!(0));
        assert_eq!(v["windows"][0]["data_source"], json!("reported"));
        w.corrupt = true;
        let v2 = windows_json(&[w]);
        assert_eq!(v2["windows"][0]["corrupt"], json!(true));
    }

    #[test]
    fn session_data_from_window_maps_tokens_and_cost() {
        let cfg = AppConfig::default();
        let w = crate::core::windows::WindowInfo {
            key: "k".to_string(),
            dir_name: "proj-a".to_string(),
            status: crate::core::windows::WindowStatus::Active,
            model: "deepseek-v4-flash".to_string(),
            used_pct: 1.18,
            tokens_in: 1000,
            tokens_out: 500,
            cache_read: 200,
            cache_created: 100,
            cost: 0.0032,
            agent_count: 1,
            ts: 0,
            corrupt: false,
            data_source: "reported".to_string(),
        };
        let d = session_data_from_window(&w, &cfg);
        assert_eq!(d.model.id, "deepseek-v4-flash");
        assert_eq!(d.model.display_name, "deepseek-v4-flash", "scanner 窗口 display = id");
        assert_eq!(d.context_window.total_input_tokens, 1000);
        assert_eq!(d.context_window.total_output_tokens, 500);
        assert_eq!(d.context_window.current_usage.cache_read_input_tokens, 200);
        assert_eq!(d.context_window.current_usage.cache_creation_input_tokens, 100);
        assert_eq!(d.cost.total_cost_usd, 0.0032);
        // 内置 deepseek 1M 窗口 → pct = 1500/1e6*100 = 0.15
        assert!((d.context_window.used_percentage - 0.15).abs() < 1e-9);
        assert_eq!(d.context_window.context_window_size, 1_000_000);
        assert_eq!(
            d.data_source,
            crate::core::data_source::DataSource::Reported
        );
    }

    #[test]
    fn totals_json_maps_fields() {
        let t = crate::core::history::Totals {
            sessions: 3,
            total_cost: 4.5,
            total_tokens: 900,
            total_duration_secs: 600,
            avg_duration_min: 3.3,
        };
        let v = totals_json(&t);
        assert_eq!(v["sessions"], json!(3));
        assert_eq!(v["total_cost"], json!(4.5));
        assert_eq!(v["total_tokens"], json!(900));
        assert_eq!(v["total_duration_secs"], json!(600));
    }

    #[test]
    fn session_detail_json_shapes() {
        use crate::core::history::SessionRecord;
        use crate::core::transcript::{AgentRecord, TokenTotal, TranscriptSummary};
        let record = SessionRecord {
            id: 7,
            started_at: "2026-08-01 10:00:00".to_string(),
            duration_secs: 60,
            total_cost_usd: 1.25,
            total_tokens: 5000,
            agent_count: 1,
            model: "claude-sonnet-4-6".to_string(),
            transcript_path: None,
        };
        let cfg: AppConfig = toml::from_str("").unwrap();
        // 无 transcript：transcript_detail available false + tools 空
        let v = session_detail_json(&record, None, &cfg);
        assert_eq!(v["id"], json!(7));
        assert_eq!(v["transcript_detail"]["available"], json!(false));
        assert_eq!(v["tools"].as_array().map(Vec::len), Some(0));
        // 有 transcript：tokens 分解 + agents + 排行（sonnet 在表内）
        let mut s = TranscriptSummary::default();
        s.total_tokens = TokenTotal {
            input: 1000,
            output: 2000,
            cache_created: 0,
            cache_read: 0,
        };
        s.tool_counts.insert("Bash".to_string(), 3);
        s.agents.push(AgentRecord {
            name: "alpha".to_string(),
            tool_calls: 3,
            ..Default::default()
        });
        let v2 = session_detail_json(&record, Some(&s), &cfg);
        assert_eq!(v2["transcript_detail"]["tokens_in"], json!(1000));
        assert_eq!(v2["transcript_detail"]["agents"][0]["name"], json!("alpha"));
        assert_eq!(v2["tools"][0]["tool"], json!("Bash"));
        assert_eq!(v2["tools"][0]["calls"], json!(3));
    }

    #[test]
    fn pct_change_up_down_flat_and_no_last() {
        assert_eq!(pct_change(2.0, 1.0), Some(100));
        assert_eq!(pct_change(1.0, 2.0), Some(-50));
        assert_eq!(pct_change(2.0, 2.0), Some(0));
        assert_eq!(pct_change(2.0, 0.0), None);
    }

    #[test]
    fn week_compare_json_with_and_without_last() {
        use crate::core::history::WeekAgg;
        let this = WeekAgg { cost: 2.0, sessions: 2, tokens: 2000 };
        let last = WeekAgg { cost: 4.0, sessions: 4, tokens: 1000 };
        let v = week_compare_json(Some(&this), Some(&last));
        assert_eq!(v["available"], json!(true));
        assert_eq!(v["cost_pct"], json!(-50));
        assert_eq!(v["session_pct"], json!(-50));
        assert_eq!(v["token_pct"], json!(100));
        assert_eq!(v["this_week"]["cost"], json!(2.0));
        let v2 = week_compare_json(Some(&this), None);
        assert_eq!(v2["last_week"], Value::Null);
        assert_eq!(v2["cost_pct"], Value::Null);
        let v3 = week_compare_json(None, None);
        assert_eq!(v3["this_week"], Value::Null);
    }
}
