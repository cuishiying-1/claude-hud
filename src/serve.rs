use std::io::Read;
use tiny_http::{Response, Server};

use crate::core::config::AppConfig;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::widget::WidgetRegistry;

const PORT: u16 = 9527;

/// Launch the web dashboard HTTP server.
pub fn run(
    registry: &'static WidgetRegistry,
    config: &'static AppConfig,
    theme: &'static Theme,
) -> Result<(), String> {
    let addr = format!("127.0.0.1:{}", PORT);
    let server =
        Server::http(&addr).map_err(|e| format!("start server on {}: {}", addr, e))?;

    println!("Web dashboard: http://localhost:{}", PORT);
    println!("Press Ctrl+C to stop");

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
                let _ = request.respond(Response::from_string("Not Found").with_status_code(404));
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
    let data = read_current_data().unwrap_or_default();
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

    format!(
        r#"{{"model":"{}","context_pct":{},"cost_usd":{},"duration_ms":{},"widgets":[{}]}}"#,
        data.model.display_name,
        data.context_window.used_percentage,
        data.cost.total_cost_usd,
        data.cost.total_duration_ms,
        widgets_json.join(","),
    )
}

fn build_dashboard_html(
    _registry: &WidgetRegistry,
    _config: &AppConfig,
    _theme: &Theme,
) -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Claude HUD — Dashboard</title>
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
  <h1>Claude HUD Dashboard</h1>
  <div class="status">● Live · <span id="update-time">--</span></div>
</div>

<div class="grid" id="dashboard-grid">
  <div class="card">
    <div class="card-title">Model</div>
    <div class="card-value" id="val-model">--</div>
  </div>
  <div class="card">
    <div class="card-title">Context Window</div>
    <div class="metric-big" id="val-ctx">--</div>
    <div class="metric-label">used</div>
    <div class="bar"><div class="bar-fill" id="bar-ctx" style="width:0%;background:linear-gradient(90deg,#7ee787,#f0883e,#ff7b72);"></div></div>
  </div>
  <div class="card">
    <div class="card-title">Session Cost</div>
    <div class="metric-big" id="val-cost">--</div>
    <div class="metric-label">USD</div>
  </div>
  <div class="card">
    <div class="card-title">Duration</div>
    <div class="metric-big" id="val-dur">--</div>
    <div class="metric-label">active</div>
  </div>
</div>

<div id="widgets-area" style="margin-top:24px;"></div>

<div class="realtime">Refreshing every 2s · http://localhost:9527</div>

<script>
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
</html>"#.to_string()
}

fn read_current_data() -> Option<SessionData> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).ok()?;
    SessionData::from_stdin_json(&buf).ok()
}
