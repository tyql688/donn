//! Secret：API key 的类型级防泄露封装（内部为 `secrecy::SecretString`：
//! drop 时内存清零）。Debug/Display 恒为打码形式；raw 值只能经
//! [`Secret::reveal`] 显式取出，且只应流向 settings.json 的生成逻辑——
//! 绝不进入日志、错误信息或 profile.toml。

use secrecy::{ExposeSecret, SecretString};

/// API key / auth token。不实现 Serialize：编译期杜绝意外序列化。
#[derive(Clone)]
pub struct Secret(SecretString);

impl Secret {
    /// 空字符串视为无 key，返回 None。
    pub fn new(raw: impl Into<String>) -> Option<Self> {
        let raw = raw.into();
        if raw.is_empty() {
            None
        } else {
            Some(Self(SecretString::from(raw)))
        }
    }

    /// 显式取出原文。调用点应仅限 render/写盘路径。
    pub fn reveal(&self) -> &str {
        self.0.expose_secret()
    }

    /// 打码显示：`sk-***后4位`。
    pub fn masked(&self) -> String {
        mask(self.reveal())
    }

    /// 后 4 位（UI 徽标用）。
    pub fn tail4(&self) -> String {
        self.suffix(4)
    }

    /// 后 20 位——Claude Code 确认屏白名单比对的形式。
    pub fn suffix20(&self) -> String {
        self.suffix(20)
    }

    /// 按字符取后 n 位（不足 n 则整串返回）。
    fn suffix(&self, n: usize) -> String {
        let chars: Vec<char> = self.reveal().chars().collect();
        chars[chars.len().saturating_sub(n)..].iter().collect()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret({})", self.masked())
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.masked())
    }
}

/// 打码：保留后 4 位。
fn mask(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 4 {
        return "***".to_string();
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("sk-***{tail}")
}

/// profile 的 key 状态——UI 只见这个，raw key 不跨 API 边界。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyState {
    /// 已配置；携带后 4 位供展示。
    Present { tail4: String },
    /// 该 preset 需要 key 但尚未配置。
    Absent,
    /// 该 preset 不需要 key（官方 OAuth）。
    NotNeeded,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_table() {
        let cases = [
            ("sk-abcdef1234567890", "sk-***7890"),
            ("abcd", "***"),
            ("", "***"),
            ("xy", "***"),
            ("longer-secret-value", "sk-***alue"),
        ];
        for (input, expect) in cases {
            assert_eq!(mask(input), expect, "input={input:?}");
        }
    }

    #[test]
    fn secret_never_leaks_via_debug_or_display() {
        let s = Secret::new("sk-ant-0123456789abcdefghij").unwrap();
        assert!(!format!("{s:?}").contains("0123456789abcdef"));
        assert!(!format!("{s}").contains("0123456789abcdef"));
        assert_eq!(format!("{s}"), "sk-***ghij");
        assert_eq!(s.tail4(), "ghij");
        assert_eq!(s.suffix20(), "0123456789abcdefghij");
        assert_eq!(s.reveal(), "sk-ant-0123456789abcdefghij");
    }

    #[test]
    fn empty_is_none_and_short_suffixes() {
        assert!(Secret::new("").is_none());
        let s = Secret::new("abc").unwrap();
        assert_eq!(s.suffix20(), "abc");
        assert_eq!(s.tail4(), "abc");
    }
}
