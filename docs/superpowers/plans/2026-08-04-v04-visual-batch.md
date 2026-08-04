# v0.4 视觉批次实施计划（动画时间相位重建 + tabbed 布局）

> **For agentic workers:** 按本计划任务顺序执行（T1-T11）。步骤用 checkbox（`- [ ]`）追踪。
> 批次约定：不逐任务 commit，批次末尾统一询问用户后由用户授权一次性提交（与 v0.3 批次一致）。

**Goal:** animation.rs 重建为时间相位纯函数，6 效果接线（渐变进度条/呼吸/缓动计数器/CRT 扫描线/伪 3D 面板/盲文频谱），tabbed 布局补全。

**Architecture:** 墙钟相位（`CLAUDE_HUD_PHASE` env 可覆盖，黑盒确定性）驱动纯函数；删除 frame 计数器 `AnimationState`；新 widget `token_rate`（紧凑速率文本 + 仪表盘盲文条）；tabbed = 四态布局循环 + tab 条 + `←`/`→` 切换。

**Tech Stack:** Rust 2021 · ratatui 0.29 · 黑盒套件（scripts/hudlib + test_hud.py）

**规格:** `docs/superpowers/specs/2026-08-04-v04-visual-batch-design.md`

**前置事实（已核实）：**
- `fit_line`（compact.rs:207）剥 ANSI 后测 unicode 宽度 → 逐 cell 渐变不影响宽度口径
- 默认主题（preset `"full"` → load_preset 返回 None → `Theme::default()`）nord：success `#a3be8c`(163,190,140)、danger `#bf616a`(191,97,106)、accent `#88c0d0`、border `#434c5e`、muted `#5e81ac`
- `Theme::parse_hex(hex) -> Option<(u8,u8,u8)>`（theme.rs:215，pub）
- `TranscriptSummary.token_timeline: Vec<TokenSnapshot>`，桶 = 60s epoch 对齐（transcript.rs:438），每桶 `total_tokens` 为累计值
- 黑盒 harness：`env_extra` 已支持（test_hud.py:221 传给 pre_cmds 与主运行）；`transcript_copy` 复制 fixture 并改写 stdin 的 transcript_path
- ratatui 0.29 `Block::border_style` 只有单样式（无按侧）→ 伪 3D 用"accent 边框 + 右下偏移 1 格阴影块"实现
- cargo 不在 PATH：所有 cargo 命令前缀 `export PATH="$HOME/.cargo/bin:$PATH" &&`
- 禁止运行 `cargo fmt`

---

### Task 1: animation.rs 时间相位纯函数重建

**Files:**
- Rewrite: `src/core/animation.rs`
- Test: `src/core/animation.rs`（模块内 `#[cfg(test)]`）

- [ ] **Step 1: 重写 animation.rs（删 AnimationState，加 5 个纯函数 + 测试）**

```rust
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
```

- [ ] **Step 2: 验证测试通过 + 无警告**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test animation 2>&1 | tail -15`
Expected: 6 个 animation 测试全 PASS

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo check 2>&1 | grep -c warning`
Expected: `0`

- [ ] **Step 3: 后续接线任务（T2-T5）完成后回来跑全量 `cargo test`**（本任务不单独 commit，批次末尾统一提交）

---

### Task 2: 呼吸接线（alerts + agent_detail 替换 frame 制）

**Files:**
- Modify: `src/widgets/alerts.rs:31-43, 108-126`
- Modify: `src/widgets/agent_detail.rs:39-51, 107-115, 64-93`

- [ ] **Step 1: alerts.rs 硬切换 → 明暗呼吸**

alerts.rs 顶部 import 追加：

```rust
use crate::core::animation;
```

`render_compact` critical 分支（原 :38-40）改为：

```rust
if pct >= critical {
    let (r, g, b) = animation::breathe(&theme.danger, animation::now_phase(4.0));
    let hex = format!("#{:02x}{:02x}{:02x}", r, g, b);
    alerts.push(ansi::ansi_fg(&format!("⚠ ctx {:.0}%", pct), &hex));
}
```

删除文件底部 `time_phase` 函数与 `time_phase_is_periodic` 测试（:108-126 整块）：

```rust
/// Seconds-based phase ... （整块删除）
fn time_phase(period: u64) -> u64 { ... }
```

- [ ] **Step 2: agent_detail.rs 删 AnimationState，改 breathe**

struct 与 new()（:39-51）改为：

```rust
pub struct AgentDetail {
    summary: Mutex<Option<TranscriptSummary>>,
}

impl AgentDetail {
    pub fn new() -> Self {
        Self { summary: Mutex::new(None) }
    }
}
```

删除 import `use crate::core::animation::AnimationState;`，改为 `use crate::core::animation;`。

render_dashboard 动画段（:107-114）改为：

```rust
let is_stalled_anim = {
    let (r, g, b) = animation::breathe(&theme.danger, animation::now_phase(4.0));
    Color::Rgb(r, g, b)
};
```

render_compact 卡顿指示（:75-79）改为：

```rust
let status = if is_stalled {
    let (r, g, b) = animation::breathe(&theme.danger, animation::now_phase(4.0));
    ansi::ansi_fg("◐", &format!("#{:02x}{:02x}{:02x}", r, g, b))
} else {
    ansi::ansi_fg("◐", &theme.success)
};
```

- [ ] **Step 3: 验证**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test widgets::alerts widgets::agent_detail 2>&1 | tail -8`
Expected: 全部 PASS（alerts 的 time_phase 测试已删；agent_detail 3 测试不受影响）

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo check 2>&1 | grep -c warning`
Expected: `0`

---

### Task 3: context_bar 渐变进度条（接线 `gradient` 配置键）

**Files:**
- Modify: `src/widgets/context_bar.rs:17-35`

- [ ] **Step 1: render_compact 逐 cell 渐变**

`render_compact` 的 filled/empty 段（原 :19-27）改为：

```rust
        let warn = config.get_f64("warn_threshold", 80.0);
        let critical = config.get_f64("critical_threshold", 95.0);
        let gradient_on = config.get_bool("gradient", true);
        let filled_str = if gradient_on && filled > 0 {
            let mut s = String::new();
            for i in 0..filled {
                let t = i as f64 / (bar_width.saturating_sub(1) as f64).max(1.0);
                let (r, g, b) = crate::core::animation::gradient(&theme.success, &theme.danger, t);
                s.push_str(&ansi::ansi_fg(
                    &theme.bar_filled.to_string(),
                    &format!("#{:02x}{:02x}{:02x}", r, g, b),
                ));
            }
            s
        } else {
            let color = if pct >= critical {
                &theme.danger
            } else if pct >= warn {
                &theme.warning
            } else {
                &theme.success
            };
            ansi::ansi_fg(&theme.bar_filled.to_string().repeat(filled), color)
        };
```

`format!` 中 `filled_str` 直接使用（删除原来 `let filled_str = ...color...` 行与 `let color = ...` 行——`color` 移入 else 分支，避免未用变量警告）。

- [ ] **Step 2: 追加测试模块**

context_bar.rs 文件末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::session::SessionData;
    use crate::core::widget::{Widget, WidgetConfig};

    fn session_data(pct: f64) -> SessionData {
        SessionData::from_stdin_json(
            &format!(
                r#"{{"model":{{"id":"m","display_name":"M"}},
                    "context_window":{{"used_percentage":{},"total_input_tokens":1000,
                                     "total_output_tokens":2000,"context_window_size":200000}},
                    "cost":{{"total_cost_usd":0.0,"total_duration_ms":0}}}}"#,
                pct
            ),
        )
        .unwrap()
    }

    fn cfg(gradient: bool) -> WidgetConfig {
        WidgetConfig {
            values: [
                ("bar_width".to_string(), "4".to_string()),
                ("gradient".to_string(), gradient.to_string()),
            ]
            .into_iter()
            .collect(),
        }
    }

    /// 统计输出中不同的 truecolor 色码（38;2;R;G;B）。
    fn distinct_colors(out: &str) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        for part in out.split("\x1b[") {
            if let Some(code) = part.strip_prefix("38;2;") {
                let end = code.find('m').unwrap_or(code.len());
                let c = &code[..end];
                if !v.contains(&c) {
                    v.push(c);
                }
            }
        }
        v
    }

    #[test]
    fn gradient_on_produces_multiple_colors() {
        let data = session_data(90.0);
        let out = ContextBar.render_compact(&data, &Theme::default(), &cfg(true));
        let colors = distinct_colors(&out);
        assert!(
            colors.len() >= 3,
            "gradient on must yield >=3 distinct colors (cells + border), got {:?}: {}",
            colors, out
        );
        assert!(colors.contains(&"163;190;140"), "start cell = success: {}", out);
        assert!(colors.contains(&"191;97;106"), "end cell = danger: {}", out);
    }

    #[test]
    fn gradient_off_uses_single_filled_color() {
        let data = session_data(90.0);
        let out = ContextBar.render_compact(&data, &Theme::default(), &cfg(false));
        let colors = distinct_colors(&out);
        assert!(
            colors.len() <= 2,
            "gradient off must yield at most 2 colors (filled + border), got {:?}: {}",
            colors, out
        );
        assert!(colors.contains(&"191;97;106"), "pct 90 >= warn 80 → danger: {}", out);
    }

    #[test]
    fn gradient_empty_bar_no_crash() {
        let data = session_data(3.4); // filled = round(3.4/100*4) = 0
        let out = ContextBar.render_compact(&data, &Theme::default(), &cfg(true));
        assert!(out.contains("ctx "), "empty bar still renders: {}", out);
    }
}
```

- [ ] **Step 3: 验证**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test widgets::context_bar 2>&1 | tail -8`
Expected: 3 个测试全 PASS（90/100*4 = 3.6 → round = 4 cell 全 filled）

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo check 2>&1 | grep -c warning`
Expected: `0`

- [ ] **Step 4: 全量单测确认无既有回归**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -4`
Expected: `test result: ok. 126 passed`（120 + 6 animation + 3 context_bar - 1 alerts time_phase 删除 + 6 agent_detail 无变化…按实际数字核对，全 PASS 即可）

---

### Task 4: cost_display 缓动计数器（仪表盘进程内状态）

**Files:**
- Modify: `src/widgets/cost_display.rs:10, 45-54, 68-143`
- Modify: `src/widgets/mod.rs:23`

- [ ] **Step 1: 加 EasedValue + struct 改造**

cost_display.rs 顶部（import 区）追加：

```rust
use std::sync::Mutex;
```

`pub struct CostDisplay;`（:10）改为：

```rust
const EASE_DURATION: f64 = 0.8;

/// 仪表盘缓动计数器（唯一进程内动画状态）：target 变化重置锚点，
/// ease_out 曲线 0.8s 内从当前显示值滚到新值。
struct EasedValue {
    target: f64,
    start: f64,
    elapsed: f64,
}

impl EasedValue {
    fn new() -> Self {
        Self { target: 0.0, start: 0.0, elapsed: 0.0 }
    }

    /// 帧推进：delta = 距上帧秒数；target 变化 → 以当前显示值为锚点重置。
    fn tick(&mut self, target: f64, delta: f64) -> f64 {
        if self.target != target {
            self.start = self.value();
            self.target = target;
            self.elapsed = 0.0;
        }
        self.elapsed = (self.elapsed + delta.max(0.0)).min(EASE_DURATION);
        self.value()
    }

    fn value(&self) -> f64 {
        self.start + (self.target - self.start) * crate::core::animation::ease_out(self.elapsed / EASE_DURATION)
    }
}

pub struct CostDisplay {
    eased: Mutex<EasedValue>,
    last_frame: Mutex<std::time::Instant>,
}

impl CostDisplay {
    pub fn new() -> Self {
        Self {
            eased: Mutex::new(EasedValue::new()),
            last_frame: Mutex::new(std::time::Instant::now()),
        }
    }
}
```

- [ ] **Step 2: render_dashboard 用缓动值**

`render_dashboard`（:45-47）开头改为：

```rust
    fn render_dashboard(&self, data: &SessionData, area: Rect, frame: &mut Frame, _theme: &Theme, config: &WidgetConfig) {
        let now = std::time::Instant::now();
        let delta = now
            .duration_since(*self.last_frame.lock().expect("frame clock"))
            .as_secs_f64();
        *self.last_frame.lock().expect("frame clock") = now;
        let display_cost = self.eased.lock().expect("eased value").tick(data.cost.total_cost_usd, delta);
        let dur = data.cost.total_duration_ms / 1000;
        let mut text = format!("Cost: ${:.4} | {}m {}s | +{}/-{} lines",
            display_cost, dur / 60, dur % 60, data.cost.total_lines_added, data.cost.total_lines_removed);
```

- [ ] **Step 3: widgets/mod.rs 注册改 new()**

`src/widgets/mod.rs:23`：

```rust
    registry.register(Box::new(cost_display::CostDisplay::new()));
```

- [ ] **Step 4: 追加缓动测试**

cost_display.rs 测试模块（`#[cfg(test)]` 内）追加：

```rust
    #[test]
    fn ease_reaches_target_after_duration() {
        let mut v = EasedValue::new();
        assert_eq!(v.tick(100.0, 0.0), 0.0);
        assert!((v.tick(100.0, 0.4) - 75.0).abs() < 0.001, "t=0.5 → ease 0.75");
        assert_eq!(v.tick(100.0, 0.4), 100.0); // elapsed clamp 0.8 → 1.0
    }

    #[test]
    fn target_change_resets_anchor_to_current_display() {
        let mut v = EasedValue::new();
        v.tick(100.0, 0.8); // settle at 100
        assert_eq!(v.tick(50.0, 0.0), 100.0); // 锚点 = 当前显示值，未开始移动
        assert!((v.tick(50.0, 0.4) - 75.0).abs() < 0.001); // 100→50 半程 = 75
        assert_eq!(v.tick(50.0, 0.4), 50.0);
    }

    #[test]
    fn negative_delta_clamped() {
        let mut v = EasedValue::new();
        v.tick(100.0, -1.0);
        assert_eq!(v.tick(100.0, 0.0), 0.0); // elapsed 不倒退
    }
```

- [ ] **Step 5: 验证**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test widgets::cost_display 2>&1 | tail -8`
Expected: 6 个测试全 PASS（原 3 + 新 3）
Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo check 2>&1 | grep -c warning`
Expected: `0`

---

### Task 5: token_rate widget（盲文频谱 + 紧凑速率文本）

**Files:**
- Create: `src/widgets/token_rate.rs`
- Modify: `src/widgets/mod.rs:13`（mod 声明）+ `:36` 后（注册）
- Modify: `src/core/config.rs:327-334`（默认 compact_layout）

- [ ] **Step 1: 新建 token_rate.rs**

```rust
use std::sync::Mutex;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::core::ansi;
use crate::core::session::SessionData;
use crate::core::theme::Theme;
use crate::core::transcript::TranscriptSummary;
use crate::core::widget::{Widget, WidgetConfig};
use crate::widgets::cost_display::format_tokens;

/// 8 级块条（0 级 = 空格），盲文频谱风格。
const SPECTRUM_LEVELS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// 仪表盘最多绘制最近 24 桶（24 分钟窗口）。
const SPECTRUM_BUCKETS: usize = 24;

pub struct TokenRate {
    summary: Mutex<Option<TranscriptSummary>>,
}

impl TokenRate {
    pub fn new() -> Self {
        Self { summary: Mutex::new(None) }
    }
}

/// 速率 = 尾桶 total_tokens / 60s（桶为 60s epoch，累计口径）。空 timeline → None。
pub fn rate_per_min(summary: &TranscriptSummary) -> Option<f64> {
    let last = summary.token_timeline.last()?;
    Some(last.total_tokens as f64 / 60.0)
}

/// 最近 max_buckets 桶归一化为 8 级块条；空 timeline → "—"。
pub fn spectrum_bars(timeline: &[crate::core::transcript::TokenSnapshot], max_buckets: usize) -> String {
    if timeline.is_empty() {
        return "—".to_string();
    }
    let start = timeline.len().saturating_sub(max_buckets);
    let buckets = &timeline[start..];
    let max = buckets.iter().map(|b| b.total_tokens).max().unwrap_or(1).max(1);
    buckets
        .iter()
        .map(|b| {
            let level = ((b.total_tokens as f64 / max as f64) * 8.0).round() as usize;
            SPECTRUM_LEVELS[level.min(8)]
        })
        .collect()
}

impl Widget for TokenRate {
    fn id(&self) -> &str { "token_rate" }

    fn display_name(&self) -> &str { "Token Rate" }

    fn render_compact(&self, _data: &SessionData, theme: &Theme, _config: &WidgetConfig) -> String {
        let guard = self.summary.lock().ok();
        let summary = guard.as_deref().flatten();
        let Some(rate) = summary.and_then(rate_per_min) else {
            return "—".to_string();
        };
        let rate_str = ansi::ansi_fg(&format!("{}/min", format_tokens(rate.round() as u64)), &theme.muted);
        format!("tok {}", rate_str)
    }

    fn render_dashboard(
        &self,
        _data: &SessionData,
        area: Rect,
        frame: &mut Frame,
        theme: &Theme,
        _config: &WidgetConfig,
    ) {
        let mut lines = vec![Line::from(Span::styled(
            "Token Rate",
            Style::default().fg(ansi::parse_ratatui_color(&theme.accent)),
        ))];
        let guard = self.summary.lock().ok();
        let summary = guard.as_deref().flatten();
        let bars = summary
            .map(|s| spectrum_bars(&s.token_timeline, SPECTRUM_BUCKETS))
            .unwrap_or_else(|| "—".to_string());
        lines.push(Line::from(bars));
        frame.render_widget(Paragraph::new(Text::from(lines)), area);
    }

    fn update_transcript(&self, summary: &TranscriptSummary) {
        if let Ok(ref mut guard) = self.summary.lock() {
            **guard = Some(summary.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transcript::TokenSnapshot;

    fn snapshot(total: u64) -> TokenSnapshot {
        TokenSnapshot {
            timestamp_secs: 0,
            input_tokens: total,
            output_tokens: 0,
            total_tokens: total,
        }
    }

    #[test]
    fn rate_from_last_bucket_per_minute() {
        let mut s = TranscriptSummary::default();
        s.token_timeline.push(snapshot(3000));
        assert_eq!(rate_per_min(&s), Some(3000.0 / 60.0));
        let mut s2 = TranscriptSummary::default();
        s2.token_timeline.push(snapshot(3000));
        s2.token_timeline.push(snapshot(3100));
        assert_eq!(rate_per_min(&s2), Some(3100.0 / 60.0)); // 尾桶累计口径
    }

    #[test]
    fn rate_none_on_empty_timeline() {
        assert_eq!(rate_per_min(&TranscriptSummary::default()), None);
    }

    #[test]
    fn spectrum_normalizes_to_max() {
        assert_eq!(spectrum_bars(&[], 24), "—");
        assert_eq!(spectrum_bars(&[snapshot(0), snapshot(0)], 24), "  ");
        assert_eq!(spectrum_bars(&[snapshot(0), snapshot(100)], 24), " █");
        assert_eq!(spectrum_bars(&[snapshot(100)], 24), "█");
        assert_eq!(spectrum_bars(&[snapshot(50)], 24), "█"); // 单桶自归一化为满
    }

    #[test]
    fn spectrum_keeps_last_buckets_only() {
        let timeline: Vec<TokenSnapshot> = (0..30).map(|i| snapshot((i % 3) as u64)).collect();
        let bars = spectrum_bars(&timeline, 24);
        assert_eq!(bars.chars().count(), 24);
    }
}
```

- [ ] **Step 2: 注册 + 默认布局**

`src/widgets/mod.rs`：`pub mod token_rate;` 加在 `pub mod token_attribution;` 后；注册加在 `alerts` 行后：

```rust
    registry.register(Box::new(token_rate::TokenRate::new()));
```

`src/core/config.rs` 默认 compact_layout（:327-334）在 `"alerts".into(),` 前插入：

```rust
                "token_rate".into(),
```

- [ ] **Step 3: 验证**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test widgets::token_rate 2>&1 | tail -8`
Expected: 4 个测试全 PASS

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo check 2>&1 | grep -c warning`
Expected: `0`

- [ ] **Step 4: 全量单测**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -3`
Expected: 全 PASS（130 上下）

---

### Task 6: CRT 扫描线（dashboard 背景 + `[dashboard] scanlines` 配置）

**Files:**
- Modify: `src/core/config.rs:62-71`（DashboardConfig 加字段）
- Modify: `src/dashboard.rs:1-24, 205-265, 300-325`

- [ ] **Step 1: DashboardConfig 加 scanlines 字段（手动 Default 对齐）**

`src/core/config.rs:62-71` 替换为：

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DashboardConfig {
    #[serde(default = "default_refresh")]
    pub refresh_interval_ms: u64,
    #[serde(default = "default_dash_layout")]
    pub default_layout: String,
    #[serde(default = "default_scanlines")]
    pub scanlines: bool,
}

fn default_refresh() -> u64 { 500 }
fn default_dash_layout() -> String { "grid-2x2".into() }
fn default_scanlines() -> bool { true }

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            refresh_interval_ms: 500,
            default_layout: "grid-2x2".into(),
            scanlines: true,
        }
    }
}
```

> 注：手动 Default 使 Rust 默认（setup/mod reset 写出的 config.toml）与 serde 默认一致，且修正了此前 Rust 默认 refresh=0 的潜在忙轮询（DEPLOY.md 文档值即 500）。

- [ ] **Step 2: dashboard.rs 背景层**

dashboard.rs import 区追加：

```rust
use crate::core::animation;
```

`draw_dashboard` 在 `let main_area = areas[0];` 之后、布局分支之前插入：

```rust
    if config.dashboard.scanlines {
        render_scanlines(frame, main_area, theme);
    }
```

文件底部（build_single_panel 后）追加：

```rust
/// CRT 扫描线背景层：每 4 行一行 border 色 dim 行 + 1 行 accent 扫描带
/// （相位行进）。widget 渲染在其上，不遮挡内容。
fn render_scanlines(frame: &mut Frame, area: ratatui::layout::Rect, theme: &Theme) {
    let scan_row = animation::scanline_offset(animation::now_phase(8.0), area.height);
    let mut lines: Vec<Line> = Vec::new();
    for y in 0..area.height {
        let color = if y == scan_row {
            Some(&theme.accent)
        } else if y % 4 == 0 {
            Some(&theme.border)
        } else {
            None
        };
        let line = match color {
            Some(c) => Line::styled(
                " ".repeat(area.width as usize),
                Style::default().fg(ansi::parse_ratatui_color(c)),
            ),
            None => Line::raw(" ".repeat(area.width as usize)),
        };
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}
```

- [ ] **Step 3: 验证编译**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo check 2>&1 | tail -5`
Expected: 无错误、0 warnings
Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test dashboard 2>&1 | tail -5`
Expected: dashboard 既有测试 PASS

---

### Task 7: 伪 3D 面板（focus 布局）

**Files:**
- Modify: `src/dashboard.rs`（draw_dashboard 面板循环 + 新函数）

- [ ] **Step 1: focus 面板外包 3D 边框**

`draw_dashboard` 布局匹配（:237-241）保持；面板循环（:248-255）改为：

```rust
    for (i, panel_area) in layout.iter().enumerate() {
        let widget_id = widget_ids.get(i).copied().unwrap_or("context_bar");
        let render_area = if layout_name == "focus" {
            render_pseudo3d(*panel_area, frame, theme)
        } else {
            *panel_area
        };
        if let Some(widget) = registry.get(widget_id) {
            let mut widget_config = config.widget_config(widget_id);
            pricing::inject_cost(data, summary, config, &mut widget_config);
            widget.render_dashboard(data, render_area, frame, theme, &widget_config);
        }
    }
```

`build_single_panel` 函数后追加：

```rust
/// 伪 3D 面板：accent 边框（光源）+ 右下偏移 1 格 border 色阴影块
/// （ratatui 0.29 无按侧边框样式，用偏移阴影实现 bevel 立体感）。
/// 返回内边距 1 的内容区。
fn render_pseudo3d(area: ratatui::layout::Rect, frame: &mut Frame, theme: &Theme) -> ratatui::layout::Rect {
    use ratatui::widgets::{Block, Borders};
    use ratatui::layout::Margin;
    if area.width < 3 || area.height < 3 {
        return area;
    }
    let panel = ratatui::layout::Rect::new(area.x, area.y, area.width - 1, area.height - 1);
    let shadow = ratatui::layout::Rect::new(area.x + 1, area.y + 1, panel.width, panel.height);
    frame.render_widget(
        Block::bordered()
            .border_style(Style::default().fg(ansi::parse_ratatui_color(&theme.border))),
        shadow,
    );
    frame.render_widget(
        Block::bordered().borders(Borders::ALL)
            .border_style(Style::default().fg(ansi::parse_ratatui_color(&theme.accent))),
        panel,
    );
    panel.inner(Margin::new(1, 1))
}
```

> `Block::bordered()` 已含 `Borders::ALL`，第二处 `borders(Borders::ALL)` 可省略——保留仅作显式意图。imports 用函数内 use 避免文件头改动（或提到文件头，二选一，保持一致即可）。

- [ ] **Step 2: 验证编译**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo check 2>&1 | tail -5`
Expected: 无错误、0 warnings

---

### Task 8: tabbed 布局（四态循环 + tab 条 + `←`/`→` 切换）

**Files:**
- Modify: `src/dashboard.rs:49-60, 162-203, 237-265, 305-325, 368-392`

- [ ] **Step 1: next_layout 四态 + next_tab 纯函数**

`next_layout`（:195-203）改为：

```rust
/// ⑯ 'l' 键布局循环：grid-2x2 → sidebar → focus → tabbed → grid-2x2；未知值从 grid-2x2 起步。
pub fn next_layout(cur: &str) -> String {
    match cur {
        "grid-2x2" => "sidebar".to_string(),
        "sidebar" => "focus".to_string(),
        "focus" => "tabbed".to_string(),
        "tabbed" => "grid-2x2".to_string(),
        _ => "grid-2x2".to_string(),
    }
}

/// tab 切换（wrap）：dir>0 右移，dir<0 左移；len=0 → 0。
pub fn next_tab(cur: usize, len: usize, dir: i8) -> usize {
    if len == 0 {
        return 0;
    }
    let d = if dir > 0 { 1 } else { len - 1 };
    (cur + d) % len
}
```

- [ ] **Step 2: run_loop 状态与按键**

`run_loop` 状态变量（:58 后）加：

```rust
    let mut tab_idx: usize = 0;
```

按键匹配（:164-180）`KeyCode::Char('?')` 分支后追加：

```rust
                    KeyCode::Left | KeyCode::Right => {
                        if layout_name == "tabbed" {
                            let len = config.compact_layout.len();
                            tab_idx = next_tab(
                                tab_idx,
                                len,
                                if key.code == KeyCode::Left { -1 } else { 1 },
                            );
                        }
                    }
```

`draw_dashboard` 调用（:153-159）传 `tab_idx`：

```rust
                draw_dashboard(
                    frame, registry, &data, theme, config, summary.as_ref(),
                    &layout_name, tab_idx, show_help,
                );
```

- [ ] **Step 3: draw_dashboard 拆分 tabbed**

函数签名（:205-214）加 `tab_idx: usize`：

```rust
fn draw_dashboard(
    frame: &mut Frame,
    registry: &WidgetRegistry,
    data: &SessionData,
    theme: &Theme,
    config: &AppConfig,
    summary: Option<&TranscriptSummary>,
    layout_name: &str,
    tab_idx: usize,
    show_help: bool,
) {
```

布局分支（:237-241）与面板循环改为：

```rust
    if layout_name == "tabbed" {
        draw_tabbed(
            frame, registry, data, theme, config, summary, main_area, tab_idx,
        );
    } else {
        let layout = match layout_name {
            "sidebar" => build_sidebar(main_area),
            "focus" => vec![main_area],
            _ => build_grid_2x2(main_area),
        };
        // Map widgets to panels (use compact_layout order as panel assignment)
        let widget_ids: Vec<&str> = config.compact_layout.iter()
            .map(|s| s.as_str())
            .collect();
        for (i, panel_area) in layout.iter().enumerate() {
            let widget_id = widget_ids.get(i).copied().unwrap_or("context_bar");
            let render_area = if layout_name == "focus" {
                render_pseudo3d(*panel_area, frame, theme)
            } else {
                *panel_area
            };
            if let Some(widget) = registry.get(widget_id) {
                let mut widget_config = config.widget_config(widget_id);
                pricing::inject_cost(data, summary, config, &mut widget_config);
                widget.render_dashboard(data, render_area, frame, theme, &widget_config);
            }
        }
    }
```

`build_single_panel` 删除（无调用者）。`draw_tabbed` 函数追加（render_pseudo3d 后）：

```rust
/// tabbed 布局：顶部 1 行 tab 条（compact_layout 各 widget 名，激活项 accent）
/// + 下方伪 3D 内容面板（当前 tab 的 widget）。
fn draw_tabbed(
    frame: &mut Frame,
    registry: &WidgetRegistry,
    data: &SessionData,
    theme: &Theme,
    config: &AppConfig,
    summary: Option<&TranscriptSummary>,
    area: ratatui::layout::Rect,
    tab_idx: usize,
) {
    let tab_bar = ratatui::layout::Rect::new(area.x, area.y, area.width, 1);
    let mut spans: Vec<Span> = Vec::new();
    for (i, id) in config.compact_layout.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        let name = registry
            .get(id)
            .map(|w| w.display_name().to_string())
            .unwrap_or_else(|| id.clone());
        let color = if i == tab_idx {
            ansi::parse_ratatui_color(&theme.accent)
        } else {
            ansi::parse_ratatui_color(&theme.muted)
        };
        spans.push(Span::styled(name, Style::default().fg(color)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), tab_bar);

    let content = ratatui::layout::Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(1));
    let inner = render_pseudo3d(content, frame, theme);
    let widget_id = config
        .compact_layout
        .get(tab_idx)
        .cloned()
        .unwrap_or_else(|| "context_bar".to_string());
    if let Some(widget) = registry.get(&widget_id) {
        let mut widget_config = config.widget_config(&widget_id);
        pricing::inject_cost(data, summary, config, &mut widget_config);
        widget.render_dashboard(data, inner, frame, theme, &widget_config);
    }
}
```

`render_help`（:308-325）：第 2 行改为 `l        cycle layout (grid-2x2 → sidebar → focus → tabbed)`，追加一行 `←/→     switch tab (tabbed)`，`HELP_PANEL_HEIGHT` 8 → 9（:305）。

- [ ] **Step 4: 更新测试**

dashboard.rs 测试模块（:368-392）：

```rust
    #[test]
    fn next_layout_cycles_four_layouts() {
        assert_eq!(next_layout("grid-2x2"), "sidebar");
        assert_eq!(next_layout("sidebar"), "focus");
        assert_eq!(next_layout("focus"), "tabbed");
        assert_eq!(next_layout("tabbed"), "grid-2x2");
    }

    #[test]
    fn next_layout_unknown_starts_from_grid() {
        assert_eq!(next_layout(""), "grid-2x2");
        assert_eq!(next_layout("weird"), "grid-2x2");
    }

    #[test]
    fn next_tab_wraps_both_directions() {
        assert_eq!(next_tab(0, 4, 1), 1);
        assert_eq!(next_tab(3, 4, 1), 0);
        assert_eq!(next_tab(0, 4, -1), 3);
        assert_eq!(next_tab(2, 4, -1), 1);
        assert_eq!(next_tab(0, 0, 1), 0);
    }
```

- [ ] **Step 5: 验证**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test dashboard 2>&1 | tail -6`
Expected: 5 个测试全 PASS（4 布局循环 + 未知回退 + tab wrap）
Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo check 2>&1 | grep -c warning`
Expected: `0`

---

### Task 9: 黑盒用例（P5-12 渐变 / P5-13 呼吸 env / P5-14-15 token_rate）

**Files:**
- Create: `fixtures/transcript/token_rate.jsonl`
- Modify: `scripts/hudlib/cases.py`（P5 列表追加 6 例，计数 141 → 147）

- [ ] **Step 1: 新建 token_rate fixture**

`fixtures/transcript/token_rate.jsonl`：

```json
{"type":"assistant","message":{"usage":{"input_tokens":3000}},"timestamp":"2026-08-04T10:00:00Z"}
{"type":"assistant","message":{"usage":{"input_tokens":100}},"timestamp":"2026-08-04T10:01:00Z"}
```

> 尾桶累计 3100 tok / 60s → 3100/min → "3.1k/min"（format_tokens(3100) = "3.1k"）。

- [ ] **Step 2: P5 列表追加 6 例（`"P5-11"` 用例后、列表 `]` 前）**

```python
    render_case("P5-12a", "渐变进度条逐 cell 渐变（默认开）", "P5",
                {"exit": 0,
                 "stdout_contains": ["38;2;163;190;140m", "38;2;191;97;106m"]},
                stdin=j(full_dict(**{"context_window.used_percentage": 90})),
                config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"context_bar\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"
                    "[widgets.context_bar]\nbar_width = \"4\"\n"),
                note="v0.4：bar 4 cell 全 filled（90%），cell0=success #a3be8c、cell3=danger #bf616a → 两端 truecolor 色码同现"),
    render_case("P5-12b", "gradient=false 回退 3 档单色", "P5",
                {"exit": 0,
                 "stdout_contains": ["38;2;191;97;106m"],
                 "stdout_not_contains": ["38;2;163;190;140m"]},
                stdin=j(full_dict(**{"context_window.used_percentage": 90})),
                config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"context_bar\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"
                    "[widgets.context_bar]\nbar_width = \"4\"\ngradient = \"false\"\n"),
                note="v0.4：gradient=false → 90% ≥ warn 80 → 整段 danger 单色，success 色码缺席"),
    render_case("P5-13a", "呼吸 env 相位 0.25 全亮", "P5",
                {"exit": 0,
                 "stdout_contains": ["38;2;191;97;106m"]},
                stdin=j(full_dict(**{"context_window.used_percentage": 99})),
                config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"alerts\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                env_extra={"CLAUDE_HUD_PHASE": "0.25"},
                note="v0.4：⚠ ctx 99% critical 呼吸色，phase 0.25 → k=1 → danger 原色 #bf616a"),
    render_case("P5-13b", "呼吸 env 相位 0 变暗", "P5",
                {"exit": 0,
                 "stdout_contains": ["38;2;138;70;76m"]},
                stdin=j(full_dict(**{"context_window.used_percentage": 99})),
                config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"alerts\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                env_extra={"CLAUDE_HUD_PHASE": "0"},
                note="v0.4：phase 0 → 亮度 0.725 → 191/97/106 × 0.725 = 138/70/76"),
    render_case("P5-14", "token_rate 速率文本（transcript 尾桶）", "P5",
                {"exit": 0, "stdout_contains": ["tok 3.1k/min"]},
                stdin=j(full_dict()),
                config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"token_rate\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                transcript_copy="token_rate.jsonl",
                note="v0.4：尾桶累计 3100 tok / 60s = 3100/min → 3.1k/min"),
    render_case("P5-15", "token_rate 无数据降级 —", "P5",
                {"exit": 0, "stdout_contains": ["—"],
                 "stdout_not_contains": ["tok "]},
                stdin=j(full_dict()),
                config=(
                    "active_mod = \"\"\n"
                    "preset = \"full\"\n"
                    "separator = \" │ \"\n"
                    "compact_layout = [\"token_rate\"]\n"
                    "[dashboard]\nrefresh_interval_ms = 0\ndefault_layout = \"\"\n"
                    "[widgets]\n"),
                note="v0.4：无 transcript → timeline 空 → —（与成本组零数据降级同口径）"),
```

计数断言（:1128）改：

```python
assert len(CASES) == 147, f"expected 147 cases, got {len(CASES)}"
```

- [ ] **Step 3: 构建 + 全量黑盒**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -2`
Expected: 编译成功

Run: `python scripts/test_hud.py 2>&1 | tail -15`
Expected: `147 passed`（如个别既有用例因默认布局加入 token_rate / 渐变默认开而失败，逐例确认是**预期行为变更**（更新期望）还是回归（修复代码）；记录到批次报告）

- [ ] **Step 4: 单跑新用例复核**

Run: `python scripts/test_hud.py --case P5-12a && python scripts/test_hud.py --case P5-13a`
Expected: 两例 PASS

---

### Task 10: 文档同步

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `COMPLETE.md`
- Modify: `DEPLOY.md`
- Modify: `TASKS.md`
- Modify: `docs/superpowers/specs/2026-08-04-v04-visual-batch-design.md`（验收回写 [x]）

- [ ] **Step 1: CHANGELOG.md 顶部新段**

```markdown
## [0.6.0] - 2026-08-04 (v0.4 视觉批次)

### Added
- 动画系统重建为时间相位纯函数（now_phase/breathe/gradient/ease_out/scanline_offset，`CLAUDE_HUD_PHASE` env 黑盒确定性）；删除 frame 制 AnimationState
- context_bar 渐变进度条：逐 cell truecolor 渐变替 3 档变色（接线既有 `gradient` 配置键，默认开）
- 新 widget `token_rate`：紧凑 `tok 3.1k/min` 速率文本 + 仪表盘最近 24 桶盲文频谱竖条（token_timeline 数据源；空数据 `—`）
- dashboard CRT 扫描线背景（`[dashboard] scanlines`，默认开）+ 伪 3D 面板（focus/tabbed accent 边框 + 偏移阴影）
- tabbed 布局补全：四态布局循环 + 顶部 tab 条 + `←`/`→` 切换（noir-tabbed mod 声明的 Tabbed 布局不再是 focus 别名）
- 缓动计数器（仪表盘 cost_display 0.8s ease-out；紧凑进程重生单帧不适用，拍板确认）
```

- [ ] **Step 2: COMPLETE.md**（§9 动画系统、§20 实现状态表、§21 路线图、文件树）
  - §9：动画系统描述改为时间相位纯函数架构 + 6 效果接线清单 + tabbed 布局
  - §20 完整实现段：追加 v0.4 段（147 例 + 6 效果 + tabbed）
  - §21：动画接入行 ⬜ → ✅（v0.4 批次，2026-08-04）；布局补全行 ⬜ → ✅
  - 文件树：animation.rs / token_rate.rs 行更新

- [ ] **Step 3: DEPLOY.md**
  - 配置示例：`[widgets.context_bar]` 段加 `gradient = "true"` 注释说明；`[dashboard]` 段加 `scanlines = true`
  - 默认 compact_layout 示例加 `"token_rate"`
  - 仪表盘快捷键表：`l` 说明改四布局循环；加 `←`/`→` 行
  - 状态栏宽度/渲染章节补 token_rate 与渐变说明

- [ ] **Step 4: TASKS.md 延期队列**：动画接入行 → `✅ 已完成（v0.4 批次，2026-08-04）`，附完成摘要（时间相位重建 + 6 效果 + tabbed 布局补全；黑盒 147 例）

- [ ] **Step 5: spec 验收回写**：按实际结果勾选/标注各节验收框（V1-V7 + 批次总验收）

---

### Task 11: 全量验证与提交询问

- [ ] **Step 1: 全量验证**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo check 2>&1 | grep -c warning`
Expected: `0`

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test 2>&1 | tail -3`
Expected: 全 PASS（130 上下）

Run: `python scripts/test_hud.py 2>&1 | tail -3`
Expected: `147 passed`

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1 | tail -1 && ./target/debug/claude-hud doctor`
Expected: doctor 全 [ok]

- [ ] **Step 2: 清理临时产物**（fixtures/reports 若生成新报告，确认归属；不暂存未跟踪的 fixtures/reports 内容）

- [ ] **Step 3: 提交询问**（与 v0.3 批次一致，AskUserQuestion 询问是否代提交；授权后暂存全部变更 + 新文件一次性 commit，消息参照仓库风格，不带 Co-Authored-By）

- [ ] **Step 4: 批次总结报告**（实现摘要 + 验证结果 + 注意事项；下一步候选：国际化或竞品功能吸收）
