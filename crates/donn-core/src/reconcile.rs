//! reconcile：把 [`Rendered`] 合并进现有 JSON `Value`。「保留未知字段」和「只动 footprint 里的键」在这里落实。
//!
//! 对 settings.json 与 .claude.json 同一形状的三步合并：
//! 1. 删除 `旧 footprint − 本次键集` —— donn 不再拥有的键；
//! 2. 写入本次渲染值，记录被覆盖的用户手改（漂移以 donn 为准，覆盖并提示）；
//! 3. 其余一切字段原样穿透，永不触碰。
//!
//! 不可变优先：输入输出为新 Value，不原地改传入值。

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::keys;
use crate::render::Rendered;
use crate::spec::Footprint;

/// 取出应为 object 的容器；类型不符 fail-fast——静默当空 object 会丢用户数据。
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

/// 手改 = 现有值既不是本次要写的，也不是 donn 上次写的。donn 只写字符串的键上出现
/// 别的类型，自然两者都不等，同样算手改。
fn hand_edited(current: Option<&Value>, new: &Value, donn_wrote: Option<&Value>) -> bool {
    current.is_some_and(|current| current != new && Some(current) != donn_wrote)
}

/// 合并结果：新 Value + 被覆盖的用户手改键列表。两个文件同一形状。
#[derive(Debug, Clone)]
pub struct Reconciled {
    pub value: Value,
    /// donn 拥有的键上检出的手改（值既非旧渲染值也非本次渲染值）——已覆盖，调用方提示。
    pub overwritten: Vec<String>,
}

/// 合并 settings.json。
///
/// `old_rendered` = 变更前 spec 的渲染结果：区分「donn 自己上次写的值」（正常更新，
/// 不提示）与「用户手改的值」（覆盖并提示）。create 等无前状态场景传 `Rendered::default()`。
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

    // ---- permissions（只管理 defaultMode；deny 等其余子键原样穿透）----
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
        // 删除最后一个 managed 子键后也必须回写空对象，否则 root 仍保留旧值。
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

    // 确认屏白名单：追加 key 后 20 位（保留对象内其他子键与既有元素，幂等）
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rendered(env: &[(&str, &str)], claude_keys: &[&str], suffix: Option<&str>) -> Rendered {
        Rendered {
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            claude_json: claude_keys.iter().map(|s| s.to_string()).collect(),
            approved_key_suffix: suffix.map(str::to_string),
            ..Default::default()
        }
    }

    fn fp(env: &[&str]) -> Footprint {
        Footprint {
            settings_env: env.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn non_object_containers_fail_fast() {
        // 顶层/env/permissions 被改成错误类型 → 报错修复，绝不静默清空重建
        let ok = rendered(&[], &[], None);
        for existing in [
            json!([1, 2]),
            json!({"env": "oops"}),
            json!({"permissions": ["not-an-object"]}),
        ] {
            let r = settings(&existing, &Footprint::default(), &ok, &Rendered::default());
            assert!(r.is_err(), "{existing}");
        }
        assert!(claude_json(&json!("str"), &Footprint::default(), &ok).is_err());
        let with_key = rendered(&[], &[keys::API_RESPONSES], Some("0123456789abcdefghij"));
        for existing in [
            json!({"customApiKeyResponses": "oops"}),
            json!({"customApiKeyResponses": {"approved": "not-a-list"}}),
        ] {
            assert!(
                claude_json(&existing, &Footprint::default(), &with_key).is_err(),
                "{existing}"
            );
        }
    }

    #[test]
    fn fresh_file_gets_rendered_env() {
        let r = settings(
            &json!({}),
            &Footprint::default(),
            &rendered(
                &[
                    ("ANTHROPIC_BASE_URL", "https://x"),
                    ("DISABLE_AUTOUPDATER", "1"),
                ],
                &[],
                None,
            ),
            &Rendered::default(),
        )
        .unwrap();
        assert_eq!(r.value["env"]["ANTHROPIC_BASE_URL"], "https://x");
        assert_eq!(r.value["env"]["DISABLE_AUTOUPDATER"], "1");
        assert!(r.overwritten.is_empty());
    }

    #[test]
    fn unknown_fields_pass_through_untouched() {
        let existing = json!({
            "env": {"USER_VAR": "keep", "ANTHROPIC_BASE_URL": "https://old"},
            "hooks": {"PostToolUse": [{"cmd": "gofmt"}]},
            "spinnerTipsEnabled": false,
            "futureField": [1, 2, 3]
        });
        let r = settings(
            &existing,
            &fp(&["ANTHROPIC_BASE_URL"]),
            &rendered(&[("ANTHROPIC_BASE_URL", "https://new")], &[], None),
            &Rendered::default(),
        )
        .unwrap();
        assert_eq!(r.value["env"]["USER_VAR"], "keep");
        assert_eq!(r.value["hooks"]["PostToolUse"][0]["cmd"], "gofmt");
        assert_eq!(r.value["spinnerTipsEnabled"], false);
        assert_eq!(r.value["futureField"], json!([1, 2, 3]));
        assert_eq!(r.value["env"]["ANTHROPIC_BASE_URL"], "https://new");
        assert_eq!(
            r.overwritten,
            vec!["ANTHROPIC_BASE_URL"],
            "手改被覆盖时提示"
        );
    }

    #[test]
    fn stale_owned_keys_removed_user_keys_stay() {
        let existing = json!({"env": {
            "ANTHROPIC_AUTH_TOKEN": "sk-old",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "old-opus",
            "USER_VAR": "keep"
        }});
        // 新一轮不再写 OPUS 槽位
        let r = settings(
            &existing,
            &fp(&["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_DEFAULT_OPUS_MODEL"]),
            &rendered(&[("ANTHROPIC_AUTH_TOKEN", "sk-new")], &[], None),
            &Rendered::default(),
        )
        .unwrap();
        let env = r.value["env"].as_object().unwrap();
        assert!(
            !env.contains_key("ANTHROPIC_DEFAULT_OPUS_MODEL"),
            "不再拥有的键被删除"
        );
        assert_eq!(env["USER_VAR"], "keep");
        assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk-new");
    }

    #[test]
    fn donn_own_update_is_not_reported_as_hand_edit() {
        // 场景：用户通过 donn 把 effort 从 low 改成 medium。
        // 现有值 "low" 是 donn 上次自己写的（old_env 佐证）→ 正常更新，不提示。
        let existing = json!({"env": {"CLAUDE_CODE_EFFORT_LEVEL": "low"}});
        let old_rendered = Rendered {
            env: vec![("CLAUDE_CODE_EFFORT_LEVEL".to_string(), "low".to_string())],
            ..Default::default()
        };
        let r = settings(
            &existing,
            &fp(&["CLAUDE_CODE_EFFORT_LEVEL"]),
            &rendered(&[("CLAUDE_CODE_EFFORT_LEVEL", "medium")], &[], None),
            &old_rendered,
        )
        .unwrap();
        assert_eq!(r.value["env"]["CLAUDE_CODE_EFFORT_LEVEL"], "medium");
        assert!(r.overwritten.is_empty(), "donn 自身变更不是手改");

        // 场景：用户手改成 "high"（既非旧渲染值也非新渲染值）→ 覆盖并提示。
        let hand_edited = json!({"env": {"CLAUDE_CODE_EFFORT_LEVEL": "high"}});
        let r = settings(
            &hand_edited,
            &fp(&["CLAUDE_CODE_EFFORT_LEVEL"]),
            &rendered(&[("CLAUDE_CODE_EFFORT_LEVEL", "medium")], &[], None),
            &old_rendered,
        )
        .unwrap();
        assert_eq!(r.overwritten, vec!["CLAUDE_CODE_EFFORT_LEVEL"]);
    }

    #[test]
    fn permissions_default_mode_write_and_cleanup() {
        // 写入：用户 permissions 其他子键（allow）原样保留
        let existing = json!({"permissions": {"allow": ["Bash(ls:*)"]}});
        let mut new = rendered(&[], &[], None);
        new.permissions_default_mode = Some("bypassPermissions".into());
        let r = settings(&existing, &Footprint::default(), &new, &Rendered::default()).unwrap();
        assert_eq!(r.value["permissions"]["defaultMode"], "bypassPermissions");
        assert_eq!(r.value["permissions"]["allow"], json!(["Bash(ls:*)"]));
        assert!(r.overwritten.is_empty());

        // 旋钮关闭：旧 footprint 声明的 defaultMode 被删，用户子键仍在
        let old_fp = Footprint {
            permissions_top: vec!["defaultMode".into()],
            ..Default::default()
        };
        let r = settings(
            &r.value,
            &old_fp,
            &rendered(&[], &[], None),
            &Rendered::default(),
        )
        .unwrap();
        assert!(r.value["permissions"].get("defaultMode").is_none());
        assert_eq!(r.value["permissions"]["allow"], json!(["Bash(ls:*)"]));

        // defaultMode 是唯一子键时，清理后仍需把空 permissions 对象写回。
        let r = settings(
            &json!({"permissions": {"defaultMode": "bypassPermissions"}}),
            &old_fp,
            &rendered(&[], &[], None),
            &Rendered::default(),
        )
        .unwrap();
        assert_eq!(r.value["permissions"], json!({}));

        // 用户手改 defaultMode → 覆盖并提示
        let mut new = rendered(&[], &[], None);
        new.permissions_default_mode = Some("bypassPermissions".into());
        let r = settings(
            &json!({"permissions": {"defaultMode": "plan"}}),
            &old_fp,
            &new,
            &Rendered::default(),
        )
        .unwrap();
        assert_eq!(r.value["permissions"]["defaultMode"], "bypassPermissions");
        assert_eq!(r.overwritten, vec!["permissions.defaultMode"]);
    }

    #[test]
    fn settings_top_write_update_and_cleanup() {
        // 写入顶层字段；用户自己的顶层字段（hooks）不受影响
        let existing = json!({"hooks": {"Stop": []}, "theme": "dark"});
        let mut new = rendered(&[], &[], None);
        new.settings_top = vec![("spinnerTipsEnabled".into(), json!(false))];
        let r = settings(&existing, &Footprint::default(), &new, &Rendered::default()).unwrap();
        assert_eq!(r.value["spinnerTipsEnabled"], false);
        assert_eq!(r.value["theme"], "dark");
        assert!(r.overwritten.is_empty(), "{:?}", r.overwritten);

        // donn 上次写的值更新 → 不算手改；用户手改的顶层值 → 提示
        let old_rendered = {
            let mut o = rendered(&[], &[], None);
            o.settings_top = vec![("effortLevel".into(), json!("low"))];
            o
        };
        let old_fp = Footprint {
            settings_top: vec!["effortLevel".into()],
            ..Default::default()
        };
        let mut new = rendered(&[], &[], None);
        new.settings_top = vec![("effortLevel".into(), json!("high"))];
        let r = settings(&json!({"effortLevel": "low"}), &old_fp, &new, &old_rendered).unwrap();
        assert!(r.overwritten.is_empty(), "donn 自身更新非手改");
        let r = settings(
            &json!({"effortLevel": "medium"}),
            &old_fp,
            &new,
            &old_rendered,
        )
        .unwrap();
        assert_eq!(r.overwritten, vec!["effortLevel"]);

        // 从 defaults 移除 → 下次 reconcile 按 footprint 清理
        let r = settings(
            &json!({"effortLevel": "high", "theme": "dark"}),
            &old_fp,
            &rendered(&[], &[], None),
            &old_rendered,
        )
        .unwrap();
        assert!(r.value.get("effortLevel").is_none(), "不再拥有的顶层键被删");
        assert_eq!(r.value["theme"], "dark");
    }

    #[test]
    fn nested_object_key_reordering_is_not_a_hand_edit() {
        let old_value = serde_json::from_str::<Value>(r#"{"commit":"","pr":""}"#).unwrap();
        let reordered = serde_json::from_str::<Value>(r#"{"pr":"","commit":""}"#).unwrap();
        let old_rendered = Rendered {
            settings_top: vec![("attribution".into(), old_value)],
            ..Default::default()
        };
        let new = old_rendered.clone();
        let old = Footprint {
            settings_top: vec!["attribution".into()],
            ..Default::default()
        };
        let result = settings(
            &json!({"attribution": reordered}),
            &old,
            &new,
            &old_rendered,
        )
        .unwrap();
        assert!(result.overwritten.is_empty());
    }

    #[test]
    fn reconcile_is_immutable() {
        let existing = json!({"env": {"A": "1"}});
        let before = existing.clone();
        let _ = settings(
            &existing,
            &fp(&["A"]),
            &rendered(&[("B", "2")], &[], None),
            &Rendered::default(),
        )
        .unwrap();
        assert_eq!(existing, before);
    }

    #[test]
    fn non_string_value_on_owned_key_is_reported_as_hand_edit() {
        // donn 只写字符串：owned 键上出现非字符串值（JSON 合法）同样是手改，覆盖并提示
        let existing =
            json!({"env": {"ANTHROPIC_BASE_URL": 123, "ANTHROPIC_AUTH_TOKEN": "sk-old"}});
        // 旧渲染佐证 AUTH_TOKEN 是 donn 自己上次写的 → 更新不算手改；BASE_URL 是非字符串手改 → 提示
        let old_rendered = Rendered {
            env: vec![("ANTHROPIC_AUTH_TOKEN".to_string(), "sk-old".to_string())],
            ..Default::default()
        };
        let r = settings(
            &existing,
            &fp(&["ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN"]),
            &rendered(
                &[
                    ("ANTHROPIC_BASE_URL", "https://new"),
                    ("ANTHROPIC_AUTH_TOKEN", "sk-new"),
                ],
                &[],
                None,
            ),
            &old_rendered,
        )
        .unwrap();
        assert!(
            r.overwritten.iter().any(|k| k == "ANTHROPIC_BASE_URL"),
            "{:?}",
            r.overwritten
        );
        assert!(
            !r.overwritten.iter().any(|k| k == "ANTHROPIC_AUTH_TOKEN"),
            "donn 写法的字符串值照旧不误报: {:?}",
            r.overwritten
        );
        assert_eq!(r.value["env"]["ANTHROPIC_BASE_URL"], "https://new");

        // permissions.defaultMode 同理
        let mut new = rendered(&[], &[], None);
        new.permissions_default_mode = Some("plan".into());
        let r = settings(
            &json!({"permissions": {"defaultMode": 42}}),
            &Footprint::default(),
            &new,
            &Rendered::default(),
        )
        .unwrap();
        assert_eq!(r.overwritten, vec!["permissions.defaultMode"]);
        assert_eq!(r.value["permissions"]["defaultMode"], "plan");
    }

    #[test]
    fn user_permissions_deny_passes_through_untouched() {
        let existing = json!({"permissions": {"allow": ["Bash"], "deny": ["user-rule"]}});
        let mut new = rendered(&[], &[], None);
        new.permissions_default_mode = Some("plan".into());
        let r = settings(&existing, &Footprint::default(), &new, &Rendered::default()).unwrap();
        assert_eq!(
            r.value["permissions"]["deny"],
            json!(["user-rule"]),
            "deny 完全由用户管理"
        );
        assert_eq!(r.value["permissions"]["allow"], json!(["Bash"]));
        assert_eq!(r.value["permissions"]["defaultMode"], "plan");
    }

    #[test]
    fn claude_json_onboarding_and_approved() {
        let existing = json!({
            "mcpServers": {"jadx": {"command": "jadx-mcp"}},
            "theme": "dark",
            "customApiKeyResponses": {"approved": ["existingsuffix0000000"], "rejected": ["r1"]}
        });
        let old = Footprint {
            claude_json: vec![keys::ONBOARDING.into(), keys::API_RESPONSES.into()],
            ..Default::default()
        };
        let new = rendered(
            &[],
            &[keys::ONBOARDING, keys::API_RESPONSES],
            Some("0123456789abcdefghij"),
        );
        let r = claude_json(&existing, &old, &new).unwrap();
        assert_eq!(r.value["hasCompletedOnboarding"], true);
        assert_eq!(
            r.value["mcpServers"]["jadx"]["command"], "jadx-mcp",
            "mcpServers 原样保留"
        );
        assert_eq!(r.value["theme"], "dark", "donn 不管理 theme，原样保留");
        let approved = r.value["customApiKeyResponses"]["approved"]
            .as_array()
            .unwrap();
        assert_eq!(approved.len(), 2, "追加不清空既有元素");
        assert_eq!(
            r.value["customApiKeyResponses"]["rejected"],
            json!(["r1"]),
            "其他子键保留"
        );
        // 幂等：重复合并不重复追加
        let r2 = claude_json(&r.value, &old, &new).unwrap();
        assert_eq!(
            r2.value["customApiKeyResponses"]["approved"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn claude_json_removes_stale_key_when_switching_to_token_mode() {
        let existing =
            json!({"hasCompletedOnboarding": true, "customApiKeyResponses": {"approved": ["x"]}});
        let old = Footprint {
            claude_json: vec![keys::ONBOARDING.into(), keys::API_RESPONSES.into()],
            ..Default::default()
        };
        // 切到 auth_token 模式：本次只拥有 onboarding
        let new = rendered(&[], &[keys::ONBOARDING], None);
        let r = claude_json(&existing, &old, &new).unwrap();
        assert_eq!(r.value["hasCompletedOnboarding"], true);
        assert!(r.value.get(keys::API_RESPONSES).is_none());
    }
}
