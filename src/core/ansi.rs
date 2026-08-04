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

/// Strip ANSI SGR escape sequences (e.g. \x1b[38;2;r;g;bm ... \x1b[0m).
/// Any ESC[ … letter sequence is removed; other text passes through.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_no_codes_passthrough() {
        assert_eq!(strip_ansi("plain text"), "plain text");
    }

    #[test]
    fn strip_ansi_single_segment() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi("\x1b[38;2;255;0;0mrgba\x1b[0m"), "rgba");
    }

    #[test]
    fn strip_ansi_adjacent_segments() {
        assert_eq!(
            strip_ansi("\x1b[38;2;255;0;0mA\x1b[0m\x1b[1mB\x1b[0m"),
            "AB"
        );
    }

    #[test]
    fn strip_ansi_mixed_plain_and_codes() {
        assert_eq!(strip_ansi("a\x1b[0mb\x1b[31mc"), "abc");
    }
}
