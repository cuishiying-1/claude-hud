//! Shared ANSI + color utility functions for all widgets.

use ratatui::style::Color;
use crate::core::theme::Theme;

/// Wrap text in a 24-bit true color ANSI foreground sequence.
pub fn ansi_fg(text: &str, hex: &str) -> String {
    if let Some((r, g, b)) = Theme::parse_hex(hex) {
        format!("\x1b[38;2;{};{};{}m{}\x1b[0m", r, g, b, text)
    } else {
        text.to_string()
    }
}

/// ANSI reset sequence.
pub fn ansi_reset() -> String {
    "\x1b[0m".to_string()
}

/// Parse a hex string to ratatui Color.
pub fn parse_ratatui_color(hex: &str) -> Color {
    if let Some((r, g, b)) = Theme::parse_hex(hex) {
        Color::Rgb(r, g, b)
    } else {
        Color::White
    }
}

/// Truncate a string to max chars with ellipsis.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated)
    } else {
        s.to_string()
    }
}
