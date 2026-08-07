// src/totals_render.rs 行渲染：totals/daily/window/folded 四种单行 + 折叠统计。
use crate::core::ansi;
use crate::core::i18n::{tr, Language};
use crate::core::theme::Theme;
use crate::core::windows::{WindowInfo, WindowStatus};

/// 折叠行信息：已结束窗口计数 / 成本合计 / 最新时间戳。
pub struct EndedFold {
    pub count: usize,
    pub cost: f64,
    pub latest_ts: u64,
}

/// 已结束窗口折叠：保留活跃+空闲（顺序不变），已结束合成一行汇总。
/// 入参顺序由 scan_windows 保证（活跃→空闲→已结束）。损坏窗口 ts=0
/// → status Ended → 归入折叠组。
pub fn fold_ended(wins: &[WindowInfo]) -> (Vec<&WindowInfo>, Option<EndedFold>) {
    let mut kept: Vec<&WindowInfo> = Vec::new();
    let mut ended: Vec<&WindowInfo> = Vec::new();
    for w in wins {
        if w.status == WindowStatus::Ended {
            ended.push(w);
        } else {
            kept.push(w);
        }
    }
    if ended.is_empty() {
        return (kept, None);
    }
    let count = ended.len();
    let cost = ended.iter().map(|w| w.cost).sum();
    let latest_ts = ended.iter().map(|w| w.ts).max().unwrap_or(0);
    (kept, Some(EndedFold { count, cost, latest_ts }))
}

/// 左对齐列：超宽截断（ansi::truncate 语义，宽-3 + "..."）+ 右补空格。
pub fn col(s: &str, width: usize) -> String {
    let t = ansi::truncate(s, width);
    format!("{:<width$}", t, width = width)
}

/// 右对齐列：超宽截断 + 左补空格。
pub fn col_right(s: &str, width: usize) -> String {
    let t = ansi::truncate(s, width);
    format!("{:>width$}", t, width = width)
}

/// 千位缩写（与 main.rs format_history_tokens 同口径）：45000 → "45k"。
pub fn tokens_k(tokens: u64) -> String {
    if tokens >= 1000 {
        format!("{}k", (tokens as f64 / 1000.0).round() as u64)
    } else {
        tokens.to_string()
    }
}

/// 分区标题行：`━━ {title} ━━...`（accent 色；color=false 纯文本）。
pub fn section_title(title: &str, theme: &Theme, color: bool) -> String {
    let line = format!("━━ {} {}", title, "━".repeat(24));
    if color {
        ansi::ansi_fg(&line, &theme.accent)
    } else {
        line
    }
}

/// ① 全会话总计单行：`{n} sessions · $1.50 total · 288k tokens · 6071m · avg 83.2m`。
/// 金额段 success 色，其余 fg 色（分段着色，避免嵌套 ANSI 重置问题）。
pub fn totals_line(
    n: usize,
    cost: f64,
    tokens: u64,
    dur_secs: u64,
    avg_min: f64,
    sym: &str,
    theme: &Theme,
    lang: Language,
    color: bool,
) -> String {
    let pre = tr(lang, "runtime.t_totals_line_pre")
        .replace("{n}", &n.to_string());
    let mid = tr(lang, "runtime.t_totals_line_mid")
        .replace("{tok}", &tokens_k(tokens))
        .replace("{dur}", &format_dur(dur_secs))
        .replace("{avg}", &format!("{:.1}", avg_min));
    let cost_txt = format!("{}{:.2}", sym, cost);
    if color {
        format!(
            "{}{}{}",
            ansi::ansi_fg(&pre, &theme.fg),
            ansi::ansi_fg(&cost_txt, &theme.success),
            ansi::ansi_fg(&mid, &theme.fg)
        )
    } else {
        format!("{}{}{}", pre, cost_txt, mid)
    }
}

/// ② 每日行：`MM-DD  $1.40  253k`（day 截取后 5 字符；成本 success 色）。
pub fn daily_line(
    day: &str,
    cost: f64,
    tokens: u64,
    sym: &str,
    theme: &Theme,
    lang: Language,
    color: bool,
) -> String {
    let mmdd = day.get(5..).unwrap_or(day).to_string();
    let d = col(&mmdd, 5);
    let c = col_right(&format!("{}{:.2}", sym, cost), 7);
    let k = col_right(&tokens_k(tokens), 5);
    let _ = lang;
    if color {
        format!(
            "  {}{}{}",
            ansi::ansi_fg(&d, &theme.fg),
            ansi::ansi_fg(&c, &theme.success),
            ansi::ansi_fg(&k, &theme.fg)
        )
    } else {
        format!("  {}{}{}", d, c, k)
    }
}

/// 时长人类化（与 main.rs format_history_duration 同口径）：3600 → "60m"。
fn format_dur(secs: u64) -> String {
    if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

/// ③ 窗口单行：`{name:<28} {model:<20} {pct:>4} {sym}{cost:>8} {tok:>5} {agents:>10} [status]`。
/// 状态标签色：活跃 success / 空闲 fg / 已结束 muted / 损坏 danger；
/// 金额段 success 色（与 totals_line/daily_line/folded_line 分段着色一致）。
pub fn window_line(
    w: &WindowInfo,
    sym: &str,
    theme: &Theme,
    lang: Language,
    color: bool,
) -> String {
    let name = if w.corrupt {
        tr(lang, "runtime.t_win_corrupt").to_string()
    } else {
        w.dir_name.clone()
    };
    let model = if w.corrupt { String::new() } else { w.model.clone() };
    let pct = if w.corrupt {
        String::new()
    } else {
        format!("{:.0}%", w.used_pct)
    };
    let cost_txt = if w.corrupt {
        String::new()
    } else {
        format!("{}{:.2}", sym, w.cost)
    };
    let tok = if w.corrupt {
        String::new()
    } else {
        tokens_k(w.tokens_in + w.tokens_out)
    };
    let agents = if w.corrupt {
        String::new()
    } else {
        format!("{} {}", w.agent_count, tr(lang, "runtime.agents"))
    };
    let status_txt = match w.status {
        WindowStatus::Active => tr(lang, "runtime.t_win_active"),
        WindowStatus::Idle => tr(lang, "runtime.t_win_idle"),
        WindowStatus::Ended => tr(lang, "runtime.t_win_ended"),
    };
    let status_hex = match w.status {
        WindowStatus::Active => theme.success.clone(),
        WindowStatus::Idle => theme.fg.clone(),
        WindowStatus::Ended => theme.muted.clone(),
    };
    let status_hex = if w.corrupt { theme.danger.clone() } else { status_hex };
    let status_txt = if w.corrupt {
        tr(lang, "runtime.t_win_corrupt").to_string()
    } else {
        status_txt.to_string()
    };
    let cost_seg = col_right(&cost_txt, 8);
    let body = format!(
        "{} {} {} {} {} {}",
        col(&name, 28),
        col(&model, 20),
        col_right(&pct, 4),
        if color && !cost_txt.is_empty() {
            ansi::ansi_fg(&cost_seg, &theme.success)
        } else {
            cost_seg
        },
        col_right(&tok, 5),
        col_right(&agents, 10)
    );
    let tag = format!("[{}]", status_txt);
    if color {
        format!("  {} {}", ansi::ansi_fg(&body, &theme.fg), ansi::ansi_fg(&tag, &status_hex))
    } else {
        format!("  {} {}", body, tag)
    }
}

/// ④ 已结束折叠行：`（另有 {n} 个已结束会话 · 合计 $0.12 · 最新于 14:32 —— totals --all 展开）`。
/// muted 色 + 金额段 success。
pub fn folded_line(
    f: &EndedFold,
    sym: &str,
    theme: &Theme,
    lang: Language,
    color: bool,
) -> String {
    let pre = tr(lang, "runtime.t_ended_fold_pre").replace("{n}", &f.count.to_string());
    let mid = tr(lang, "runtime.t_ended_fold_mid")
        .replace("{time}", &format_time(f.latest_ts));
    let cost_txt = format!("{}{:.2}", sym, f.cost);
    if color {
        format!(
            "{}{}{}",
            ansi::ansi_fg(&pre, &theme.muted),
            ansi::ansi_fg(&cost_txt, &theme.success),
            ansi::ansi_fg(&mid, &theme.muted)
        )
    } else {
        format!("{}{}{}", pre, cost_txt, mid)
    }
}

/// 折叠行时间：UTC HH:MM（chrono 无 clock feature，用 from_timestamp 构造）。
fn format_time(ts: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts as i64, 0)
        .map(|d| d.format("%H:%M").to_string())
        .unwrap_or_else(|| "--:--".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(status: WindowStatus, ts: u64, cost: f64) -> WindowInfo {
        WindowInfo {
            dir_name: "proj".to_string(),
            status,
            ts,
            cost,
            ..Default::default()
        }
    }

    #[test]
    fn fold_ended_splits_kept_from_fold() {
        let wins = vec![
            win(WindowStatus::Active, 1000, 0.0),
            win(WindowStatus::Idle, 800, 0.0),
            win(WindowStatus::Ended, 500, 1.0),
            win(WindowStatus::Ended, 200, 2.0),
        ];
        let (kept, fold) = fold_ended(&wins);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].status, WindowStatus::Active);
        assert_eq!(kept[1].status, WindowStatus::Idle);
        let f = fold.unwrap();
        assert_eq!(f.count, 2);
        assert_eq!(f.cost, 3.0);
        assert_eq!(f.latest_ts, 500);
    }

    #[test]
    fn fold_ended_no_ended_is_none() {
        let wins = vec![win(WindowStatus::Active, 1000, 0.0)];
        let (kept, fold) = fold_ended(&wins);
        assert_eq!(kept.len(), 1);
        assert!(fold.is_none());
    }

    #[test]
    fn col_truncates_and_pads() {
        assert_eq!(col("abc", 5), "abc  ");
        assert_eq!(col(&"x".repeat(10), 5), "xx...");
        assert_eq!(col_right("4%", 4), "  4%");
        assert_eq!(col_right(&"x".repeat(10), 5), "xx...");
    }

    #[test]
    fn section_title_contains_title_and_rule() {
        let t = Theme::load_preset("dracula").unwrap();
        let s = section_title("Totals", &t, true);
        assert!(s.contains("Totals"));
        assert!(s.contains('━'));
        assert!(s.contains("38;2;189;147;249"), "accent #bd93f9: {s}");
        assert!(!section_title("Totals", &t, false).contains('\x1b'));
    }

    #[test]
    fn tokens_k_formats() {
        assert_eq!(tokens_k(45000), "45k");
        assert_eq!(tokens_k(999), "999");
    }

    #[test]
    fn totals_line_highlights_cost() {
        let t = Theme::load_preset("dracula").unwrap();
        let s = totals_line(73, 1.5, 288_000, 364_260, 83.2, "$", &t, Language::En, true);
        assert!(s.contains("73 sessions"));
        assert!(s.contains("288k tokens"));
        assert!(s.contains("38;2;80;250;123"), "success 绿: {s}");
        assert!(!totals_line(73, 1.5, 288_000, 364_260, 83.2, "$", &t, Language::En, false)
            .contains('\x1b'));
    }

    #[test]
    fn daily_line_uses_mm_dd() {
        let t = Theme::load_preset("dracula").unwrap();
        let s = daily_line("2026-08-06", 1.4, 253_000, "$", &t, Language::En, false);
        assert!(s.contains("08-06"));
        assert!(s.contains("1.40"));
        assert!(s.contains("253k"));
    }

    #[test]
    fn window_line_colors_and_truncates() {
        let t = Theme::load_preset("dracula").unwrap();
        let w = WindowInfo {
            dir_name: "D--workspace-claude-hud".to_string(),
            model: "deepseek-v4-flash".to_string(),
            status: WindowStatus::Idle,
            used_pct: 4.0,
            tokens_in: 39_000,
            tokens_out: 0,
            cost: 0.22,
            agent_count: 0,
            ts: 1000,
            ..Default::default()
        };
        let s = window_line(&w, "$", &t, Language::En, true);
        assert!(s.contains("D--workspace-claude-hud"));
        assert!(s.contains("deepseek-v4-flash"));
        assert!(s.contains("[idle]"));
        assert!(s.contains("38;2;80;250;123"), "success 绿: {s}");
        assert!(!window_line(&w, "$", &t, Language::En, false).contains('\x1b'));

        let mut long = w.clone();
        long.dir_name = "x".repeat(40);
        let s2 = window_line(&long, "$", &t, Language::En, false);
        assert!(s2.contains("..."), "超宽截断: {s2}");
    }

    #[test]
    fn window_line_corrupt_label() {
        let t = Theme::load_preset("dracula").unwrap();
        let mut w = WindowInfo { corrupt: true, ..Default::default() };
        w.status = WindowStatus::Ended;
        let s = window_line(&w, "$", &t, Language::En, false);
        assert!(s.contains("[corrupt]"));
    }

    #[test]
    fn folded_line_counts_cost_and_time() {
        let t = Theme::load_preset("dracula").unwrap();
        let f = EndedFold { count: 2, cost: 0.12, latest_ts: 1_700_000_000 };
        let s = folded_line(&f, "$", &t, Language::En, false);
        assert!(s.contains("2 ended sessions"));
        assert!(s.contains("0.12"));
        assert!(s.contains("totals --all"));
        assert!(s.contains(':'), "UTC HH:MM: {s}");
        assert!(folded_line(&f, "$", &t, Language::En, true).contains("38;2;80;250;123"));
    }
}
