use crate::core::i18n::{tr, Language};
use notify_rust::Notification;

/// Send an OS-native desktop notification.
pub fn send(title: &str, body: &str) {
    let result = Notification::new()
        .summary(title)
        .body(body)
        .appname("Claude HUD")
        .timeout(5000)
        .show();

    if let Err(e) = result {
        eprintln!("[claude-hud] notification failed: {}", e);
    }
}

/// Convenience: context critical alert.
pub fn context_critical(pct: f64, lang: Language) {
    send(
        tr(lang, "notify.context_critical"),
        &tr(lang, "notify.context_body").replace("{pct}", &format!("{:.0}", pct)),
    );
}

/// Convenience: all agents completed.
pub fn agents_complete(count: usize, lang: Language) {
    send(
        tr(lang, "notify.agents_complete"),
        &tr(lang, "notify.agents_body").replace("{n}", &count.to_string()),
    );
}

/// Convenience: cost threshold exceeded (symbol from config.currency_symbol).
pub fn cost_threshold(cost: f64, threshold: f64, symbol: &str, lang: Language) {
    send(
        tr(lang, "notify.cost_warning"),
        &tr(lang, "notify.cost_body")
            .replace("{sym}", symbol)
            .replace("{cost}", &format!("{:.2}", cost))
            .replace("{threshold}", &format!("{:.2}", threshold)),
    );
}

/// Convenience: rate limit warning.
pub fn rate_limit_warning(pct: f64, lang: Language) {
    send(
        tr(lang, "notify.rate_limit"),
        &tr(lang, "notify.rate_body").replace("{pct}", &format!("{:.0}", pct)),
    );
}

/// Convenience: stalled agent detected.
pub fn agent_stalled(name: &str, seconds: u64, lang: Language) {
    send(
        tr(lang, "notify.agent_stalled"),
        &tr(lang, "notify.stalled_body")
            .replace("{name}", name)
            .replace("{secs}", &seconds.to_string()),
    );
}

/// Convenience: compaction imminent (④; eta in minutes).
pub fn compaction(minutes: u64, lang: Language) {
    send(
        tr(lang, "notify.compaction"),
        &tr(lang, "notify.compaction_body").replace("{m}", &minutes.to_string()),
    );
}

/// Convenience: budget tier reached (⑳; cost is the realtime estimate).
pub fn budget(pct: f64, cap: f64, symbol: &str, lang: Language) {
    send(
        tr(lang, "notify.budget_warning"),
        &tr(lang, "notify.budget_body")
            .replace("{sym}", symbol)
            .replace("{cap}", &format!("{:.2}", cap))
            .replace("{pct}", &format!("{:.0}", pct)),
    );
}
