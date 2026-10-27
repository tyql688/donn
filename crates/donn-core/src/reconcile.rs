//! reconcile：把 [`Rendered`] 合并进现有 JSON `Value`，在这里落实「保留未知字段」和「只动 footprint 里的 key」。
//!
//! settings.json 和 .claude.json 用同一套三步合并：
//! 1. 删除 ` 旧 footprint − 本次 key 集 `，也就是 donn 不再拥有的 key；
//! 2. 写入本次渲染值，记录被覆盖的用户手改（drift 以 donn 为准，覆盖并提示）；
//! 3. 其余字段原样穿透，永不触碰。
//!
//! 输入输出都是新的 `Value`，不原地修改传入值。

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::keys;
use crate::render::Rendered;
use crate::spec::Footprint;

/// 取出应该是 object 的容器；类型不符就直接报错。静默当成空 object 会丢用户数据。
fn as_object(value: Option<&Value>, what: &str) -> Result<Map<String, Value>> {
    match value {
        None => Ok(Map::new()),
        Some(Value::Object(map)) => Ok(map.clone()),
        Some(other) => Err(Error::NotMergeable {
            what: what.to_string(),
            expected: "a JSON object",
            found: json_type(other),
        }),
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// 手改 = 现有值既不是本次要写的值，也不是 donn 上次写的值。donn 只写字符串的 key 上出现
/// 别的类型时，两个条件都不满足，同样算手改。
fn hand_edited(current: Option<&Value>, new: &Value, donn_wrote: Option<&Value>) -> bool {
    current.is_some_and(|current| current != new && Some(current) != donn_wrote)
}

/// 合并结果：新的 `Value`，加上被覆盖的用户手改 key 列表。两个文件用同一个形状。
#[derive(Debug, Clone)]
pub struct Reconciled {
    pub value: Value,
    /// donn 拥有的 key 上检出的手改（值既不是旧渲染值，也不是本次渲染值），已经覆盖，调用方负责提示。
    pub overwritten: Vec<String>,
}

/// 合并 settings.json。
///
/// `old_rendered` = 变更前 spec 的渲染结果，用来区分「donn 自己上次写的值」（正常更新，
/// 不提示）和「用户手改的值」（覆盖并提示）。create 等没有前状态的场景传 `Rendered::default()`。
pub fn settings(
    existing: &Value,
    old: &Footprint,
    new: &Rendered,
    old_rendered: &Rendered,
) -> Result<Reconciled> {
    let mut root = as_object(Some(existing), "settings.json")?;
    let mut overwritten = Vec::new();

    // ---- 顶层字段 ----
    let new_top: Vec<&str> = new.settings_top.iter().map(|(k, _)| k.as_str()).collect();
    for old_key in &old.settings_top {
        if !new_top.contains(&old_key.as_str()) {
            root.shift_remove(old_key);
        }
    }
    for (key, value) in &new.settings_top {
        let was = old_rendered.settings_top.iter().find(|(k, _)| k == key);
        if hand_edited(root.get(key), value, was.map(|(_, v)| v)) {
            overwritten.push(key.clone());
        }
        root.insert(key.clone(), value.clone());
    }

    // ---- env ----
    let mut env = as_object(root.get("env"), "settings.json `env`")?;
    let new_keys: Vec<&str> = new.env.iter().map(|(k, _)| k.as_str()).collect();
    for old_key in &old.settings_env {
        if !new_keys.contains(&old_key.as_str()) {
            env.shift_remove(old_key);
        }
    }
    for (key, value) in &new.env {
        let value = Value::String(value.clone());
        let was = old_rendered.env.iter().find(|(k, _)| k == key);
        let was = was.map(|(_, v)| Value::String(v.clone()));
        if hand_edited(env.get(key), &value, was.as_ref()) {
            overwritten.push(key.clone());
        }
        env.insert(key.clone(), value);
    }
    root.insert("env".into(), Value::Object(env));

    // ---- permissions（只管理 defaultMode；deny 等其它子 key 原样穿透）----
    if root.contains_key("permissions") || new.permissions_default_mode.is_some() {
        let mut permissions = as_object(root.get("permissions"), "settings.json `permissions`")?;
        let dm = keys::PERMISSIONS_DEFAULT_MODE;
        if old.permissions_top.iter().any(|k| k == dm) && new.permissions_default_mode.is_none() {
            permissions.shift_remove(dm);
        }
        if let Some(mode) = &new.permissions_default_mode {
            let value = Value::String(mode.clone());
            let was = old_rendered
                .permissions_default_mode
                .clone()
                .map(Value::String);
            if hand_edited(permissions.get(dm), &value, was.as_ref()) {
                overwritten.push(format!("permissions.{dm}"));
            }
            permissions.insert(dm.into(), value);
        }
        // 删除最后一个 managed 子 key 后也必须回写空 object，否则 root 仍保留旧值。
        root.insert("permissions".into(), Value::Object(permissions));
    }

    Ok(Reconciled {
        value: Value::Object(root),
        overwritten,
    })
}

/// 合并 .claude.json（mcpServers 等原样穿透）。
pub fn claude_json(existing: &Value, old: &Footprint, new: &Rendered) -> Result<Reconciled> {
    let mut root = as_object(Some(existing), ".claude.json")?;

    for old_key in &old.claude_json {
        if !new.claude_json.iter().any(|k| k == old_key) {
            root.shift_remove(old_key);
        }
    }

    // 跳过首次向导：恒写 true
    root.insert(keys::ONBOARDING.into(), Value::Bool(true));

    // 确认屏白名单：追加 key 的后 20 位（保留 object 内其它子 key 和已有元素，幂等）
    if let Some(suffix) = &new.approved_key_suffix {
        let mut responses = as_object(
            root.get(keys::API_RESPONSES),
            ".claude.json `customApiKeyResponses`",
        )?;
        let mut approved: Vec<Value> = match responses.get("approved") {
            None => Vec::new(),
            Some(Value::Array(items)) => items.clone(),
            Some(other) => {
                return Err(Error::NotMergeable {
                    what: ".claude.json `customApiKeyResponses.approved`".into(),
                    expected: "a JSON array",
                    found: json_type(other),
                });
            }
        };
        if !approved.iter().any(|v| v.as_str() == Some(suffix)) {
            approved.push(Value::String(suffix.clone()));
        }
        responses.insert("approved".into(), Value::Array(approved));
        root.insert(keys::API_RESPONSES.into(), Value::Object(responses));
    }

    Ok(Reconciled {
        value: Value::Object(root),
        overwritten: Vec::new(),
    })
}
