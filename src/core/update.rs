use std::cmp::Ordering;

/// 发布仓库，与 install.sh / install.ps1 / Cargo.toml repository 同源。
pub const UPDATE_REPO: &str = "cuishiying-1/claude-hud";

/// 版本比较：按 `.` 分段逐段数字比较；段数不同时缺段视为 0；前缀 v 忽略。
pub fn cmp_versions(a: &str, b: &str) -> Ordering {
    let a_nums: Vec<u64> = a
        .trim_start_matches('v')
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    let b_nums: Vec<u64> = b
        .trim_start_matches('v')
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    let max_len = a_nums.len().max(b_nums.len());
    for i in 0..max_len {
        let av = a_nums.get(i).copied().unwrap_or(0);
        let bv = b_nums.get(i).copied().unwrap_or(0);
        if av != bv {
            return av.cmp(&bv);
        }
    }
    Ordering::Equal
}

/// 升级检查结果。
pub enum UpdateStatus {
    /// 已是最新：携带当前版本号。
    UpToDate(String),
    /// 有新版本：携带最新版本号。
    Available(String),
    /// 仓库无 release（API 404）。
    NotPublished,
    /// 网络/其他错误。
    Unavailable,
}

/// 查询 GitHub latest release 与本地版本比较。
pub fn check_update() -> UpdateStatus {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        UPDATE_REPO
    );
    let resp = ureq::get(&url)
        .set("User-Agent", "claude-hud")
        .timeout(std::time::Duration::from_secs(10))
        .call();
    let body = match resp {
        Ok(r) => r.into_string().unwrap_or_default(),
        Err(ureq::Error::Status(404, _)) => return UpdateStatus::NotPublished,
        Err(_) => return UpdateStatus::Unavailable,
    };
    let Some(tag) = extract_tag_name(&body) else {
        return UpdateStatus::Unavailable;
    };
    let latest = tag.trim_start_matches('v').to_string();
    let current = env!("CARGO_PKG_VERSION").to_string();
    if cmp_versions(&current, &latest) != Ordering::Less {
        UpdateStatus::UpToDate(current)
    } else {
        UpdateStatus::Available(latest)
    }
}

/// 从 GitHub release JSON 中提取 tag_name。
fn extract_tag_name(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// 用户可读的检查结果（update check / doctor 共用；exit 0 恒定）。
pub fn describe(status: &UpdateStatus, lang: crate::core::i18n::Language) -> String {
    use crate::core::i18n::tr;
    match status {
        UpdateStatus::UpToDate(v) => {
            format!("{} (v{})", tr(lang, "runtime.up_to_date"), v)
        }
        UpdateStatus::Available(v) => {
            tr(lang, "runtime.update_available").replace("{version}", v)
        }
        UpdateStatus::NotPublished => tr(lang, "runtime.not_published").to_string(),
        UpdateStatus::Unavailable => tr(lang, "runtime.update_unavailable").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmp_versions_equal() {
        assert_eq!(cmp_versions("1.2.3", "1.2.3"), Ordering::Equal);
        assert_eq!(cmp_versions("v1.2.3", "1.2.3"), Ordering::Equal);
    }

    #[test]
    fn cmp_versions_newer_and_older() {
        assert_eq!(cmp_versions("1.2.3", "1.2.4"), Ordering::Less);
        assert_eq!(cmp_versions("1.2.4", "1.2.3"), Ordering::Greater);
    }

    #[test]
    fn cmp_versions_missing_segment_is_zero() {
        assert_eq!(cmp_versions("1.2", "1.2.3"), Ordering::Less);
        assert_eq!(cmp_versions("1.2.3", "1.2"), Ordering::Greater);
    }

    #[test]
    fn describe_matches_spec_wording() {
        let en = crate::core::i18n::Language::En;
        assert_eq!(describe(&UpdateStatus::NotPublished, en), "not published yet");
        assert_eq!(describe(&UpdateStatus::Unavailable, en), "update check unavailable");
        assert_eq!(describe(&UpdateStatus::UpToDate("0.2.0".into()), en), "✓ up to date (v0.2.0)");
        assert_eq!(
            describe(&UpdateStatus::Available("0.3.0".into()), en),
            "↗ update available: v0.3.0 — re-run the install script to upgrade"
        );
    }
}
