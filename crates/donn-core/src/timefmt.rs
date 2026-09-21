//! 时间格式化（RFC3339），基于 `time` crate。

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// 当前 UTC 时间，RFC3339（秒精度），如 `2026-07-14T10:00:00Z`。
pub fn now_rfc3339() -> String {
    let now = OffsetDateTime::now_utc();
    // 0 纳秒恒合法；万一失败保留纳秒（RFC3339 仍合法），绝不回退到假时间戳
    let now = now.replace_nanosecond(0).unwrap_or(now);
    #[allow(clippy::expect_used)] // 不变量：合法 OffsetDateTime 的 RFC3339 格式化不会失败
    now.format(&Rfc3339)
        .expect("RFC3339 formatting of a valid timestamp cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_shape() {
        let s = now_rfc3339();
        assert_eq!(s.len(), "2026-07-14T10:00:00Z".len(), "{s}");
        assert!(s.ends_with('Z'));
    }
}
