use std::f64::consts::TAU;

use super::theme::Theme;

/// 墙钟相位 [0,1)：period 秒内的位置。CLAUDE_HUD_PHASE 环境变量覆盖
/// （黑盒确定性，COLUMNS 先例）：合法 f64 ∈ [0,1) 直接返回，非法回退墙钟。
pub fn now_phase(period_secs: f64) -> f64 {
    if let Ok(v) = std::env::var("CLAUDE_HUD_PHASE") {
        if let Ok(p) = v.parse::<f64>() {
            if (0.0..1.0).contains(&p) {
                return p;
            }
        }
    }
    let period_ms = (period_secs * 1000.0).max(1.0);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64 % period_ms)
        .unwrap_or(0.0);
    ms / period_ms
}

/// 亮度呼吸：hex 与 hex×0.45 之间正弦脉动（k = 0.5+0.5·sin(2π·phase)）。
/// phase 0 → 亮度 0.725；0.25 → 1.0（全亮）；0.75 → 0.45（最暗）。
/// 相位 0 与 0.5 同为 0.725（正弦对称）。
pub fn breathe(hex: &str, phase: f64) -> (u8, u8, u8) {
    let (r, g, b) = Theme::parse_hex(hex).unwrap_or((255, 255, 255));
    let k = 0.5 + 0.5 * (TAU * phase).sin();
    let dim = 0.45 + 0.55 * k;
    (
        (r as f64 * dim) as u8,
        (g as f64 * dim) as u8,
        (b as f64 * dim) as u8,
    )
}

/// 线性 RGB 插值，t 钳制 [0,1]。t=0 → a 色；t=1 → b 色。
pub fn gradient(hex_a: &str, hex_b: &str, t: f64) -> (u8, u8, u8) {
    let (ar, ag, ab) = Theme::parse_hex(hex_a).unwrap_or((255, 255, 255));
    let (br, bg, bb) = Theme::parse_hex(hex_b).unwrap_or((255, 255, 255));
    let t = t.clamp(0.0, 1.0);
    (
        (ar as f64 + (br as f64 - ar as f64) * t) as u8,
        (ag as f64 + (bg as f64 - ag as f64) * t) as u8,
        (ab as f64 + (bb as f64 - ab as f64) * t) as u8,
    )
}

/// ease-out：1 - (1-t)²。
pub fn ease_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

/// 扫描线行号：phase 行进覆盖 [0, height)。
pub fn scanline_offset(phase: f64, height: u16) -> u16 {
    if height == 0 {
        return 0;
    }
    ((phase.clamp(0.0, 1.0) * height as f64) as u16).min(height - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_phase_env_override() {
        std::env::set_var("CLAUDE_HUD_PHASE", "0.25");
        assert_eq!(now_phase(8.0), 0.25);
        std::env::set_var("CLAUDE_HUD_PHASE", "0.0");
        assert_eq!(now_phase(1.0), 0.0);
        std::env::remove_var("CLAUDE_HUD_PHASE");
    }

    #[test]
    fn now_phase_invalid_env_falls_back_to_wall_clock() {
        std::env::set_var("CLAUDE_HUD_PHASE", "abc");
        assert!((0.0..1.0).contains(&now_phase(4.0)));
        std::env::set_var("CLAUDE_HUD_PHASE", "1.5");
        assert!((0.0..1.0).contains(&now_phase(4.0)));
        std::env::remove_var("CLAUDE_HUD_PHASE");
        assert!((0.0..1.0).contains(&now_phase(4.0)));
    }

    #[test]
    fn breathe_brightness_extremes() {
        assert_eq!(breathe("#00ff00", 0.25), (0, 255, 0));
        assert_eq!(breathe("#00ff00", 0.75), (0, (255.0 * 0.45) as u8, 0));
        assert_eq!(breathe("#00ff00", 0.0), (0, (255.0 * 0.725) as u8, 0));
        // 正弦对称：相位 0 与 0.5 亮度相同
        assert_eq!(breathe("#00ff00", 0.0), breathe("#00ff00", 0.5));
    }

    #[test]
    fn gradient_endpoints_and_midpoint_exact() {
        assert_eq!(gradient("#ff0000", "#0000ff", 0.0), (255, 0, 0));
        assert_eq!(gradient("#ff0000", "#0000ff", 1.0), (0, 0, 255));
        assert_eq!(gradient("#ff0000", "#0000ff", 0.5), (127, 0, 127));
        assert_eq!(gradient("#ff0000", "#0000ff", 2.0), (0, 0, 255)); // clamp
        assert_eq!(gradient("#ff0000", "#0000ff", -1.0), (255, 0, 0)); // clamp
    }

    #[test]
    fn ease_out_endpoints_monotone() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_out(0.5), 0.75);
        assert!(ease_out(0.2) > 0.2);
        assert_eq!(ease_out(1.5), 1.0); // clamp
    }

    #[test]
    fn scanline_offset_boundaries() {
        assert_eq!(scanline_offset(0.0, 10), 0);
        assert_eq!(scanline_offset(0.5, 10), 5);
        assert_eq!(scanline_offset(0.999, 10), 9);
        assert_eq!(scanline_offset(0.5, 0), 0);
        assert_eq!(scanline_offset(1.5, 10), 9); // clamp
    }
}
