use std::sync::Mutex;
use std::time::{Duration, Instant};
use tiny_http::{Response, Server};

use crate::core::config::AppConfig;
use crate::core::history::HistoryStore;
use crate::core::i18n::tr;
use crate::core::state;
use crate::core::theme::Theme;
use crate::core::widget::WidgetRegistry;

const PORT: u16 = 9527;

/// ⑨+㉑ 历史聚合缓存：前端 2s 轮询 /api/data，weekly/trend 是分钟级统计，
/// 每请求重开 SQLite 纯属空转。30s TTL 内命中缓存不重查。
const HISTORY_TTL: Duration = Duration::from_secs(30);
static HISTORY_CACHE: Mutex<Option<(Instant, String, String)>> = Mutex::new(None);
// (fetched_at, weekly_json, trend_json)

fn ttl_fresh(fetched_at: Instant, now: Instant, ttl: Duration) -> bool {
    now.duration_since(fetched_at) < ttl
}

fn cached_history() -> (String, String) {
    let mut guard = HISTORY_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if let Some((at, weekly, trend)) = guard.as_ref() {
        if ttl_fresh(*at, now, HISTORY_TTL) {
            return (weekly.clone(), trend.clone());
        }
    }
    let weekly = weekly_json_inner();
    let trend = trend_json_inner();
    *guard = Some((now, weekly.clone(), trend.clone()));
    (weekly, trend)
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
        match url.as_str() {
            "/" | "/index.html" => {
                let html = build_dashboard_html(registry, config, theme);
                let header = "Content-Type: text/html; charset=utf-8"
                    .parse::<tiny_http::Header>()
                    .unwrap();
                let response = Response::from_string(html).with_header(header);
                let _ = request.respond(response);
            }
            "/api/data" => {
                let json = build_api_json(registry, config, theme);
                let header = "Content-Type: application/json"
                    .parse::<tiny_http::Header>()
                    .unwrap();
                let response = Response::from_string(json).with_header(header);
                let _ = request.respond(response);
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

fn build_api_json(
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> String {
    let data = state::read_current_data().unwrap_or_default();
    let layout = &config.compact_layout;

    let widgets_json: Vec<String> = layout
        .iter()
        .filter_map(|id| {
            let widget = registry.get(id)?;
            let widget_config = config.widget_config(id);
            let rendered = widget.render_compact(&data, theme, &widget_config);
            Some(format!(
                r#"{{"id":"{}","name":"{}","output":"{}"}}"#,
                id,
                widget.display_name(),
                rendered.replace('"', "\\\"").replace('\n', "\\n"),
            ))
        })
        .collect();

    let pricing_configured = config.pricing.contains_key(&data.model.id);
    let (weekly, trend) = cached_history();
    format!(
        r#"{{"model":"{}","model_id":"{}","pricing_configured":{},"context_pct":{},"cost_usd":{},"duration_ms":{},"weekly":{},"trend":{},"widgets":[{}]}}"#,
        data.model.display_name,
        data.model.id,
        pricing_configured,
        data.context_window.used_percentage,
        data.cost.total_cost_usd,
        data.cost.total_duration_ms,
        weekly,
        trend,
        widgets_json.join(","),
    )
}

/// ⑨ 本周聚合统计：open/query 失败 → available:false 全 0（前端显示 —）。
fn weekly_json_inner() -> String {
    let weekly = HistoryStore::open()
        .ok()
        .and_then(|h| h.weekly_stats().ok());
    match weekly {
        Some(w) => format!(
            r#"{{"available":true,"total_cost":{},"total_sessions":{},"total_tokens":{},"avg_duration_min":{},"avg_agents_per_session":{}}}"#,
            w.total_cost, w.total_sessions, w.total_tokens, w.avg_duration_min,
            w.avg_agents_per_session,
        ),
        None => r#"{"available":false,"total_cost":0,"total_sessions":0,"total_tokens":0,"avg_duration_min":0,"avg_agents_per_session":0}"#
            .to_string(),
    }
}

/// ㉑ 近 7 天日成本趋势（供周曲线）：open/query 失败或无数据 → available:false。
fn trend_json_inner() -> String {
    let trend = HistoryStore::open()
        .ok()
        .and_then(|h| h.daily_cost_trend().ok());
    match trend {
        Some(days) if !days.is_empty() => {
            let days_json: Vec<String> = days
                .iter()
                .map(|(day, cost)| format!(r#"{{"day":"{}","cost":{}}}"#, day, cost))
                .collect();
            format!(r#"{{"available":true,"days":[{}]}}"#, days_json.join(","))
        }
        _ => r#"{"available":false,"days":[]}"#.to_string(),
    }
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
</div>

<div id="pricing-note" style="display:none;color:#d29922;font-size:11px;margin-bottom:12px;"></div>
<div class="grid" id="dashboard-grid">
  <div class="card">
    <div class="card-title">{web_model}</div>
    <div class="card-value" id="val-model">--</div>
  </div>
  <div class="card">
    <div class="card-title">{web_ctx_window}</div>
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
  </div>
</div>

  <div class="card" id="trend-card" style="display:none;">
    <div class="card-title">{web_cost_trend}</div>
    <div id="trend-bars" style="display:flex;align-items:flex-end;gap:6px;height:64px;margin-top:8px;"></div>
  </div>

<div id="widgets-area" style="margin-top:24px;"></div>

<div class="realtime">{web_realtime}</div>

<script>
const T = {pricing_note: "T_PRICING_NOTE", not_found: "T_NOT_FOUND"};
async function refresh() {
  try {
    const resp = await fetch('/api/data');
    const data = await resp.json();
    document.getElementById('val-model').textContent = data.model;
    document.getElementById('val-ctx').textContent = Math.round(data.context_pct) + '%';
    document.getElementById('bar-ctx').style.width = data.context_pct + '%';
    document.getElementById('val-cost').textContent = '$' + data.cost_usd.toFixed(4);
    const mins = Math.floor(data.duration_ms / 60000);
    const secs = Math.floor((data.duration_ms % 60000) / 1000);
    document.getElementById('val-dur').textContent = mins + 'm ' + secs + 's';
    const wk = data.weekly || {};
    if (wk.available) {
      document.getElementById('val-week-cost').textContent = '$' + wk.total_cost.toFixed(2);
      document.getElementById('val-week-sessions').textContent = wk.total_sessions;
    } else {
      document.getElementById('val-week-cost').textContent = '—';
      document.getElementById('val-week-sessions').textContent = '—';
    }
    const note = document.getElementById('pricing-note');
    if (data.pricing_configured) {
      note.style.display = 'none';
    } else {
      note.textContent = T.pricing_note.replace('{id}', data.model_id);
      note.style.display = 'block';
    }
    const trend = data.trend || {};
    const trendCard = document.getElementById('trend-card');
    if (trend.available && trend.days && trend.days.length) {
      const bars = document.getElementById('trend-bars');
      bars.innerHTML = '';
      const max = Math.max(...trend.days.map(d => d.cost), 0.0001);
      trend.days.forEach(d => {
        const bar = document.createElement('div');
        bar.style.width = '28px';
        bar.style.height = Math.max(2, Math.round(d.cost / max * 60)) + 'px';
        bar.style.background = '#4c8dff';
        bar.style.borderRadius = '2px';
        bar.title = d.day + ' $' + d.cost.toFixed(2);
        bars.appendChild(bar);
      });
      trendCard.style.display = 'block';
    } else {
      trendCard.style.display = 'none';
    }
    document.getElementById('update-time').textContent = new Date().toLocaleTimeString();

    const area = document.getElementById('widgets-area');
    area.innerHTML = '';
    if (data.widgets) {
      data.widgets.forEach(w => {
        const div = document.createElement('div');
        div.className = 'card';
        div.style.marginBottom = '8px';
        div.innerHTML = '<div class="card-title">' + w.name + '</div><div style="font-size:11px;color:#c9d1d9;white-space:pre-wrap;">' + w.output + '</div>';
        area.appendChild(div);
      });
    }
  } catch(e) {
    console.error('refresh error:', e);
  }
}
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
}


#[cfg(test)]
mod tests {
    use super::build_dashboard_html;
    use super::ttl_fresh;
    use crate::core::config::AppConfig;
    use crate::core::theme::Theme;
    use crate::core::widget::WidgetRegistry;
    use std::time::{Duration, Instant};

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
    }
}
