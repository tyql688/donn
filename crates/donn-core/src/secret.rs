//! Secret：API key 的类型级防泄漏封装（内部是 `secrecy::SecretString`，
//! drop 时清零内存）。Debug 和 Display 永远是打码形式；raw 值只能通过
//! [`Secret::reveal`] 显式取出，而且只应该流向 settings.json 的生成逻辑。
//! 绝不进入日志、错误信息或 profile.toml。

use secrecy::{ExposeSecret, SecretString};

/// API key 或 auth token。不实现 Serialize，编译期杜绝意外序列化。
#[derive(Clone)]
pub struct Secret(SecretString);

impl Secret {
    /// 空字符串视为没有 key，返回 `None`。
    pub fn new(raw: impl Into<String>) -> Option<Self> {
        let raw = raw.into();
        if raw.is_empty() {
            None
        } else {
            Some(Self(SecretString::from(raw)))
        }
    }

    /// 显式取出原文。调用点只应出现在 render 和写盘路径。
    pub fn reveal(&self) -> &str {
        self.0.expose_secret()
    }

    /// 打码显示：`sk-***` 加后 4 位。
    pub fn masked(&self) -> String {
        mask(self.reveal())
    }

    /// 后 4 位（UI 徽标用）。key 本身不超过 4 个字符时返回 `None`，
    /// 避免把整把 key 当成「后 4 位」显示出来。
    pub fn tail4(&self) -> Option<String> {
        (self.reveal().chars().count() > 4).then(|| self.suffix(4))
    }

    /// 后 20 位，Claude Code 确认屏白名单用来比对的形式。
    /// key 短于 20 个字符时返回整串，这是白名单比对的语义；`.claude.json` 本身是 0600。
    pub fn suffix20(&self) -> String {
        self.suffix(20)
    }

    /// 按字符取后 n 位（不足 n 时返回整串）。
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

/// profile 的 key 状态。UI 只能看到这个，raw key 不跨 API 边界。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyState {
    /// 已配置；携带后 4 位供展示，短 key 没有可展示的尾部。
    Present { tail4: Option<String> },
    /// 该 preset 需要 key 但尚未配置。
    Absent,
    /// 该 preset 不需要 key（官方 OAuth）。
    NotNeeded,
}
