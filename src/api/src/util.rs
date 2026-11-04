use chrono::Utc;
use data_encoding::HEXLOWER;
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

/// Unix seconds (UTC).
pub fn now() -> i64 {
    Utc::now().timestamp()
}

/// Unix milliseconds (UTC); the player clock uses these.
pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buf);
    HEXLOWER.encode(&buf)
}

pub fn sha256_hex(input: &str) -> String {
    HEXLOWER.encode(&Sha256::digest(input.as_bytes()))
}

/// Compare two secrets without leaking where they differ.
pub fn same_secret(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Truncate by characters (never splits a CJK character), adding "…" when cut.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Seconds as a short Chinese duration: 75 → "1分15秒", 3725 → "1時2分5秒".
/// This matches the wording v1 used in chat replies.
pub fn format_wait(secs: i64) -> String {
    let secs = secs.max(0);
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{h}時{m}分{s}秒")
    } else {
        format!("{m}分{s}秒")
    }
}

/// Seconds as a long Chinese duration for statistics: 90061 → "1 天 1 時 1 分 1 秒".
pub fn format_total(secs: i64) -> String {
    let secs = secs.max(0);
    let parts = [
        (secs / 86400, "天"),
        (secs / 3600 % 24, "時"),
        (secs / 60 % 60, "分"),
        (secs % 60, "秒"),
    ];
    let text: Vec<String> = parts
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, unit)| format!("{n} {unit}"))
        .collect();
    if text.is_empty() {
        "0 秒".into()
    } else {
        text.join(" ")
    }
}

/// `m:ss` / `h:mm:ss` for the UI-facing API.
pub fn format_clock(secs: i64) -> String {
    let secs = secs.max(0);
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_matches_v1_wording() {
        assert_eq!(format_wait(0), "0分0秒");
        assert_eq!(format_wait(75), "1分15秒");
        assert_eq!(format_wait(3725), "1時2分5秒");
    }

    #[test]
    fn total_skips_zero_units() {
        assert_eq!(format_total(0), "0 秒");
        assert_eq!(format_total(61), "1 分 1 秒");
        assert_eq!(format_total(90061), "1 天 1 時 1 分 1 秒");
        assert_eq!(format_total(3600), "1 時");
    }

    #[test]
    fn clock_format() {
        assert_eq!(format_clock(5), "0:05");
        assert_eq!(format_clock(3725), "1:02:05");
    }

    #[test]
    fn truncate_keeps_cjk_intact() {
        assert_eq!(truncate_chars("點歌播放器", 10), "點歌播放器");
        assert_eq!(truncate_chars("點歌播放器", 3), "點歌…");
    }

    #[test]
    fn secret_compare() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("abc", "abcd"));
    }
}
