use super::theme::Theme;

/// Animation frame counter — incremented each dashboard tick.
#[derive(Debug, Clone)]
pub struct AnimationState {
    pub frame: u64,
}

impl AnimationState {
    pub fn new() -> Self {
        Self { frame: 0 }
    }

    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// Neon breathing: sin-based opacity for a color.
    /// Returns (r, g, b) with brightness modulated.
    pub fn neon_breathing(&self, hex: &str) -> Option<(u8, u8, u8)> {
        let (r, g, b) = Theme::parse_hex(hex)?;
        let factor = 0.5 + 0.5 * ((self.frame as f64 * 0.3).sin());
        Some((
            (r as f64 * factor) as u8,
            (g as f64 * factor) as u8,
            (b as f64 * factor) as u8,
        ))
    }
}
