use super::theme::Theme;

/// Animation frame counter — incremented each dashboard tick.
#[derive(Debug, Clone)]
pub struct AnimationState {
    pub frame: u64,
    pub enabled: bool,
}

impl AnimationState {
    pub fn new(enabled: bool) -> Self {
        Self { frame: 0, enabled }
    }

    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    // === Pre-built animation helpers ===

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

    /// RGB spectrum cycle: rotates hue by frame count.
    pub fn spectrum_cycle(&self) -> (u8, u8, u8) {
        let h = (self.frame % 360) as f64;
        hsl_to_rgb(h, 0.8, 0.6)
    }

    /// Eased value: smoothly approach target from current display value.
    pub fn eased_value(current: f64, target: f64, speed: f64) -> f64 {
        current + (target - current) * speed
    }

    /// Barber pole offset: which column the stripe is at.
    pub fn barber_offset(&self, width: usize) -> usize {
        (self.frame as usize) % width
    }

    /// Spark trail particles for value changes.
    pub fn spark_frame(&self, since_change: u64) -> Vec<Spark> {
        if since_change > 10 {
            return vec![];
        }
        let remaining = 10 - since_change;
        (0..remaining)
            .map(|i| {
                let dist = i as f64 * 2.0;
                let alpha = 1.0 - (i as f64 / 10.0);
                Spark { distance: dist, alpha }
            })
            .collect()
    }

    /// Glitch: randomly shift a character position.
    pub fn glitch_offset(&self) -> Option<usize> {
        if self.frame % 3 == 0 {
            Some((self.frame as usize * 7 + 3) % 5)
        } else {
            None
        }
    }

    /// Marquee scroll position.
    pub fn marquee_offset(&self, text_len: usize, visible: usize) -> usize {
        if text_len <= visible {
            return 0;
        }
        let period = text_len + 10; // 10 frame pause at end
        ((self.frame as usize) % period).min(text_len - visible)
    }

    /// Wave distortion Y offset for a given column.
    pub fn wave_offset(&self, col: usize, amplitude: f64, frequency: f64) -> isize {
        let phase = self.frame as f64 * 0.3;
        (amplitude * ((col as f64 * frequency + phase).sin())) as isize
    }

    /// Liquid fill wave height at a given column.
    pub fn liquid_height(&self, col: usize, base_pct: f64) -> f64 {
        let wave = ((col as f64 * 0.5 + self.frame as f64 * 0.2).sin()) * 0.05;
        base_pct + wave
    }

    /// Scanline row opacity.
    pub fn scanline_alpha(&self, row: usize) -> f64 {
        let scan_pos = (self.frame as usize % 60) as f64 / 60.0;
        let row_f = row as f64;
        if (row_f / 40.0 - scan_pos).abs() < 0.05 {
            0.15
        } else if row % 2 == 0 {
            0.02
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct Spark {
    pub distance: f64,
    pub alpha: f64,
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h as u32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}
