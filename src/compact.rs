use std::path::PathBuf;

use crate::core::ansi;
use crate::core::config::AppConfig;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::transcript::TranscriptReader;
use crate::core::widget::WidgetRegistry;

/// Render the compact status bar from stdin JSON data.
pub fn render(
    registry: &WidgetRegistry,
    config: &AppConfig,
    theme: &Theme,
) -> Result<String, String> {
    let stdin_data = read_stdin()?;
    let data = SessionData::from_stdin_json(&stdin_data)
        .map_err(|e| format!("parse stdin JSON: {}", e))?;

    // Phase 2: parse transcript and push to all widgets
    parse_and_push_transcript(&data, registry);

    let layout = &config.compact_layout;
    if layout.is_empty() {
        return Ok(String::new());
    }

    let lines = config
        .runtime_overrides
        .as_ref()
        .and_then(|o| o.compact_lines)
        .unwrap_or(theme.compact_lines) as usize;

    let sep = &config.separator;
    let widgets_per_line = if lines == 1 {
        layout.len()
    } else {
        (layout.len() + lines - 1) / lines
    };

    let mut output = String::new();
    for line_idx in 0..lines {
        let start = line_idx * widgets_per_line;
        let end = (start + widgets_per_line).min(layout.len());
        if start >= end {
            break;
        }
        let line_widgets: Vec<String> = layout[start..end]
            .iter()
            .filter_map(|id| {
                let w = registry.get(id)?;
                let widget_config = config.widget_config(id);
                let rendered = w.render_compact(&data, theme, &widget_config);
                if rendered.is_empty() {
                    None
                } else {
                    Some(rendered)
                }
            })
            .collect();
        if !line_widgets.is_empty() {
            output.push_str(&line_widgets.join(sep));
            output.push('\n');
        }
    }

    Ok(output.trim_end().to_string())
}

/// Parse transcript and push summary to all widgets via trait method.
fn parse_and_push_transcript(data: &SessionData, registry: &WidgetRegistry) {
    let transcript_path = match &data.transcript_path {
        Some(p) => PathBuf::from(p),
        None => return,
    };

    if !transcript_path.exists() {
        return;
    }

    let mut reader = TranscriptReader::new(transcript_path);
    let summary = reader.read_updates();

    // Push to all widgets that accept transcript updates
    for widget in &registry.widgets {
        widget.update_transcript(&summary);
    }
}

fn read_stdin() -> Result<String, String> {
    use std::io::Read;
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|e| format!("read stdin: {}", e))?;
    Ok(buffer)
}
