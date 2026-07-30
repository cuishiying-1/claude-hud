use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Text;

use crate::core::ansi;
use crate::core::session::SessionData;
use crate::core::theme::{IconSet, Theme};
use crate::core::widget::{Widget, WidgetConfig};

pub struct ModelDisplay;

impl Widget for ModelDisplay {
    fn id(&self) -> &str { "model_display" }
    fn display_name(&self) -> &str { "Model Display" }

    fn render_compact(&self, data: &SessionData, theme: &Theme, _config: &WidgetConfig) -> String {
        let name = &data.model.display_name;
        let (icon, suffix) = match theme.icon_set {
            IconSet::Nerd | IconSet::Minimal => ("▸ ", ""),
            IconSet::Ascii => ("[", "]"),
        };
        format!("{}{}{}{}",
            ansi::ansi_fg(icon, &theme.muted),
            ansi::ansi_fg(name, &theme.model_color),
            ansi::ansi_fg(suffix, &theme.muted),
            ansi::ansi_reset())
    }

    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, theme: &Theme, _config: &WidgetConfig) {
        let text = format!("Model: {} ({})", data.model.display_name, data.model.id);
        frame.render_widget(Text::from(text), area);
    }
}
