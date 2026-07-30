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
pub fn context_critical(pct: f64) {
    send(
        "⚠ Context Critical",
        &format!("Context window at {:.0}%. Compaction imminent.", pct),
    );
}

/// Convenience: all agents completed.
pub fn agents_complete(count: usize) {
    send(
        "✓ Agents Complete",
        &format!("All {} agents have finished.", count),
    );
}

/// Convenience: cost threshold exceeded.
pub fn cost_threshold(cost: f64, threshold: f64) {
    send(
        "¥ Cost Warning",
        &format!("Session cost ¥{:.2} exceeded threshold ¥{:.2}.", cost, threshold),
    );
}

/// Convenience: rate limit warning.
pub fn rate_limit_warning(pct: f64) {
    send(
        "⚠ Rate Limit",
        &format!("Rate limit at {:.0}%. Consider pacing requests.", pct),
    );
}

/// Convenience: stalled agent detected.
pub fn agent_stalled(name: &str, seconds: u64) {
    send(
        "⚠ Agent Stalled",
        &format!("Agent '{}' has had no tool call for {}s.", name, seconds),
    );
}
