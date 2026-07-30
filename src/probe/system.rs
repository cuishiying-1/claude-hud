/// Get current process memory usage in MB (best-effort).
pub fn memory_mb() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        let data = fs::read_to_string("/proc/self/status").ok()?;
        for line in data.lines() {
            if line.starts_with("VmRSS:") {
                let kb: u64 = line
                    .split_whitespace()
                    .nth(1)?
                    .parse()
                    .ok()?;
                return Some(kb as f64 / 1024.0);
            }
        }
    }
    None
}

/// Current local time formatted as HH:MM:SS
pub fn time_now() -> String {
    let now = chrono_lite();
    now
}

/// Minimal time helper without pulling in the chrono crate.
fn chrono_lite() -> String {
    #[cfg(not(windows))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs();
        let h = (secs / 3600) % 24;
        let m = (secs / 60) % 60;
        let s = secs % 60;
        format!("{:02}:{:02}:{:02}", h, m, s)
    }
    #[cfg(windows)]
    {
        // Fallback for windows without chrono
        String::from("--:--:--")
    }
}
