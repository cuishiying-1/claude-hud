use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Text;

use crate::core::ansi;
use crate::core::i18n::tr;
use crate::core::session::SessionData;
use crate::core::theme::{IconSet, Theme};
use crate::core::widget::{Widget, WidgetConfig};

pub struct ModelDisplay;

impl Widget for ModelDisplay {
    fn id(&self) -> &str { "model_display" }
    fn display_name(&self) -> &str { "Model Display" }

    fn render_compact(&self, data: &SessionData, theme: &Theme, _config: &WidgetConfig) -> String {
        let name = ansi::truncate(&data.model.display_name, 24);
        let (icon, suffix) = match theme.icon_set {
            IconSet::Auto => ("> ", ""),   // 防御分支：所有渲染入口均已先决议 icon_set，此处正常不可达
            IconSet::Nerd | IconSet::Minimal => ("▸ ", ""),
            IconSet::Ascii => ("[", "]"),
        };
        format!("{}{}{}{}",
            ansi::ansi_fg(icon, &theme.muted),
            ansi::ansi_fg(&name, &theme.model_color),
            ansi::ansi_fg(suffix, &theme.muted),
            ansi::ansi_reset())
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, _theme: &Theme, config: &WidgetConfig) {
        let text = model_label_text(
            &data.model.display_name,
            &data.model.id,
            tr(config.lang, "runtime.model_label"),
        );
        frame.render_widget(Text::from(text), area);
    }
}

/// 面板标题行：display_name 与 id 相同（内置注册表兜底）时只显示一个，
/// 避免 `deepseek-v4-flash (deepseek-v4-flash)` 式冗余。
fn model_label_text(display_name: &str, id: &str, label: &str) -> String {
    if display_name == id {
        format!("{label}: {display_name}")
    } else {
        format!("{label}: {display_name} ({id})")
    }
}

#[cfg(test)]
mod tests {
    use super::model_label_text;

    #[test]
    fn model_label_text_dedups_when_display_name_equals_id() {
        assert_eq!(
            model_label_text("deepseek-v4-flash", "deepseek-v4-flash", "模型"),
            "模型: deepseek-v4-flash"
        );
    }

    #[test]
    fn model_label_text_keeps_id_when_distinct() {
        assert_eq!(
            model_label_text("DeepSeek V4 Flash", "deepseek-v4-flash", "Model"),
            "Model: DeepSeek V4 Flash (deepseek-v4-flash)"
        );
    }
}
