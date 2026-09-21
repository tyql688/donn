//! 集成测试：tempdir 上的全真实文件操作，驱动 `Donn` facade 的完整生命周期。
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use serde_json::{Value, json};
use tempfile::TempDir;

use donn_core::keys::ModelSlot;
use donn_core::{
    ConfigChange, Donn, DonnHome, DriftKind, Error, KeyState, ProfileDraft, Secret, SpecChange,
    doctor,
};

fn open(root: &Path) -> Donn {
    Donn::with_home(DonnHome::for_test(root)).unwrap()
}

fn draft(name: &str, preset: &str, key: Option<&str>) -> ProfileDraft {
    ProfileDraft {
        name: name.into(),
        preset: preset.into(),
        key: key.and_then(Secret::new),
        ..Default::default()
    }
}

fn read_settings(root: &Path, name: &str) -> Value {
    let home = DonnHome::for_test(root);
    let text = std::fs::read_to_string(home.settings_file(name)).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn env_of<'a>(settings: &'a Value, key: &str) -> Option<&'a str> {
    settings.get("env")?.get(key)?.as_str()
}

#[test]
fn create_writes_full_profile_layout() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    let receipt = donn
        .create(&draft("zai", "zai", Some("sk-ant-0123456789abcdefghij")))
        .unwrap();

    let home = DonnHome::for_test(tmp.path());
    assert!(home.spec_file("zai").is_file());
    assert!(home.settings_file("zai").is_file());
    assert!(home.claude_json_file("zai").is_file());
    assert_eq!(receipt.wrapper_paths.len(), 1);
    assert!(receipt.wrapper_paths[0].is_file());

    let settings = read_settings(tmp.path(), "zai");
    assert_eq!(
        env_of(&settings, "ANTHROPIC_BASE_URL"),
        Some("https://api.z.ai/api/anthropic")
    );
    assert_eq!(
        env_of(&settings, "ANTHROPIC_AUTH_TOKEN"),
        Some("sk-ant-0123456789abcdefghij")
    );
    assert_eq!(env_of(&settings, "DISABLE_AUTOUPDATER"), Some("1"));

    let claude_json: Value =
        serde_json::from_str(&std::fs::read_to_string(home.claude_json_file("zai")).unwrap())
            .unwrap();
    assert_eq!(claude_json["hasCompletedOnboarding"], true);

    // spec 落盘且不含 key
    let spec_text = std::fs::read_to_string(home.spec_file("zai")).unwrap();
    assert!(!spec_text.contains("sk-ant"), "{spec_text}");
    let spec = donn.spec("zai").unwrap();
    assert!(
        spec.footprint
            .settings_env
            .contains(&"ANTHROPIC_AUTH_TOKEN".to_string())
    );
}

#[test]
fn create_rejects_duplicate_and_invalid_names() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    donn.create(&draft("zai", "zai", Some("sk-1234567890")))
        .unwrap();
    assert!(matches!(
        donn.create(&draft("zai", "zai", Some("sk-1234567890"))),
        Err(Error::ProfileExists(_))
    ));
    assert!(matches!(
        donn.create(&draft("Bad_Name", "zai", None)),
        Err(Error::InvalidName(_))
    ));
    assert!(matches!(
        donn.create(&draft("x", "no-such-preset", None)),
        Err(Error::PresetNotFound(_))
    ));
}

#[test]
fn invalid_environment_entries_fail_without_leaking_or_writing() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    let mut invalid = draft("bad-env", "zai", Some("sk-1234567890"));
    invalid
        .env
        .push(("BAD=NAME".into(), "must-not-leak".into()));

    let error = donn.create(&invalid).unwrap_err();
    assert!(matches!(error, Error::InvalidInput(_)));
    assert!(!error.to_string().contains("must-not-leak"));
    assert!(!donn.exists("bad-env"));

    donn.create(&draft("zai", "zai", Some("sk-1234567890")))
        .unwrap();
    assert!(
        donn.edit(
            "zai",
            SpecChange::SetEnv("VALID".into(), "bad\0value".into())
        )
        .is_err()
    );
    assert!(!donn.spec("zai").unwrap().intent.env.contains_key("VALID"));
}

#[test]
fn create_into_kept_dir_merges_existing_settings_and_rollback_spares_them() {
    // 「保留会话」删除后目录还在；用户放在里面的 settings.json 不归 donn，创建只能并进去
    let tmp = TempDir::new().unwrap();
    let home = DonnHome::for_test(tmp.path());
    std::fs::create_dir_all(home.claude_config_dir("kept")).unwrap();
    std::fs::write(home.settings_file("kept"), r#"{"hooks": {"Stop": []}}"#).unwrap();
    let donn = open(tmp.path());

    // 写 wrapper 时才失败（bin 目录只读）：此时 settings.json 已并入 donn 的键，
    // 回滚必须把它恢复成创建前的原字节，其余身份文件删掉
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let original = std::fs::read(home.settings_file("kept")).unwrap();
        let bin = tmp.path().join("ro-bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o555)).unwrap();
        let writable = std::fs::write(bin.join(".probe"), "").is_ok(); // root 下只读目录拦不住
        std::fs::write(
            home.config_file(),
            format!("bin_dir = \"{}\"\n", bin.display()),
        )
        .unwrap();
        if !writable {
            assert!(
                donn.create(&draft("kept", "zai", Some("sk-1234567890")))
                    .is_err()
            );
            assert_eq!(std::fs::read(home.settings_file("kept")).unwrap(), original);
            assert!(!home.spec_file("kept").exists());
        }
        std::fs::remove_file(home.config_file()).unwrap();
    }

    donn.create(&draft("kept", "zai", Some("sk-1234567890")))
        .unwrap();
    let settings = read_settings(tmp.path(), "kept");
    assert_eq!(settings["hooks"], json!({"Stop": []}));
    assert_eq!(
        env_of(&settings, "ANTHROPIC_AUTH_TOKEN"),
        Some("sk-1234567890")
    );
}

#[test]
fn a_custom_models_window_belongs_to_the_model_id() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    donn.create(&draft("zai", "zai", Some("sk-1234567890")))
        .unwrap();
    let custom = || SpecChange::Model(ModelSlot::Sonnet, Some("my-gateway/model-x".into()));
    donn.edit_many(
        "zai",
        [
            custom(),
            SpecChange::ModelWindow("my-gateway/model-x".into(), Some(262_144)),
        ],
    )
    .unwrap();
    let window = |_: &Donn| {
        env_of(
            &read_settings(tmp.path(), "zai"),
            "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
        )
        .map(str::to_string)
    };
    assert_eq!(window(&donn).as_deref(), Some("262144"));

    // 换到 preset 候选：用候选自带的窗口；自定义模型的条目留着
    donn.edit(
        "zai",
        SpecChange::Model(ModelSlot::Sonnet, Some("glm-5.3[1m]".into())),
    )
    .unwrap();
    assert_eq!(window(&donn).as_deref(), Some("1000000"));
    assert_eq!(
        donn.spec("zai").unwrap().intent.model_windows["my-gateway/model-x"],
        262_144
    );

    // 换回来：窗口跟着模型回来，不用重填
    donn.edit("zai", custom()).unwrap();
    assert_eq!(window(&donn).as_deref(), Some("262144"));

    // 删掉条目：交还给 Claude Code
    donn.edit(
        "zai",
        SpecChange::ModelWindow("my-gateway/model-x".into(), None),
    )
    .unwrap();
    assert_eq!(window(&donn), None);
}

#[test]
fn failed_create_rolls_back_completely() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    donn.create(&draft("a", "zai", Some("sk-1234567890")))
        .unwrap();
    // 别名被 a 占用 → b 创建失败，不留残骸
    let mut d = draft("b", "zai", Some("sk-1234567890"));
    d.aliases = vec!["a".into()];
    assert!(donn.create(&d).is_err());
    assert!(!donn.exists("b"));
    assert!(!DonnHome::for_test(tmp.path()).profile_dir("b").exists());
}

#[test]
fn edit_updates_settings_via_spec() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    donn.create(&draft("zai", "zai", Some("sk-1234567890")))
        .unwrap();

    donn.edit(
        "zai",
        SpecChange::Model(ModelSlot::Sonnet, Some("glm-x".into())),
    )
    .unwrap();
    donn.edit(
        "zai",
        SpecChange::BaseUrl(Some("https://relay.example".into())),
    )
    .unwrap();
    donn.edit("zai", SpecChange::SetEnv("MY_FLAG".into(), "1".into()))
        .unwrap();

    let settings = read_settings(tmp.path(), "zai");
    assert_eq!(
        env_of(&settings, "ANTHROPIC_DEFAULT_SONNET_MODEL"),
        Some("glm-x")
    );
    assert_eq!(
        env_of(&settings, "ANTHROPIC_BASE_URL"),
        Some("https://relay.example")
    );
    assert_eq!(env_of(&settings, "MY_FLAG"), Some("1"));
    // key 在编辑过程中自动搬迁保留
    assert_eq!(
        env_of(&settings, "ANTHROPIC_AUTH_TOKEN"),
        Some("sk-1234567890")
    );

    // 清除覆盖 → 回到 preset 值；RemoveEnv 删除键
    donn.edit("zai", SpecChange::BaseUrl(None)).unwrap();
    donn.edit("zai", SpecChange::RemoveEnv("MY_FLAG".into()))
        .unwrap();
    let settings = read_settings(tmp.path(), "zai");
    assert_eq!(
        env_of(&settings, "ANTHROPIC_BASE_URL"),
        Some("https://api.z.ai/api/anthropic")
    );
    assert_eq!(env_of(&settings, "MY_FLAG"), None);
}

#[test]
fn set_key_replaces_secret() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    donn.create(&draft("zai", "zai", Some("sk-old-key-1234")))
        .unwrap();
    donn.set_key("zai", Secret::new("sk-new-key-5678").unwrap())
        .unwrap();
    let settings = read_settings(tmp.path(), "zai");
    assert_eq!(
        env_of(&settings, "ANTHROPIC_AUTH_TOKEN"),
        Some("sk-new-key-5678")
    );
}

#[test]
fn secret_migrates_across_auth_mode_change() {
    let tmp = TempDir::new().unwrap();
    let home = DonnHome::for_test(tmp.path());
    let donn = open(tmp.path());
    donn.create(&draft("zai", "zai", Some("sk-migrate-me-0001")))
        .unwrap();

    // preset 数据更新：同 key 换成 api_key 模式
    std::fs::create_dir_all(home.presets_dir()).unwrap();
    std::fs::write(
        home.presets_dir().join("zai.toml"),
        r#"[preset]
key = "zai"
label = "zai v2"
description = "auth mode changed upstream"
base_url = "https://api.z.ai/api/anthropic"
auth_mode = "api_key"
"#,
    )
    .unwrap();

    donn.sync("zai").unwrap();
    let settings = read_settings(tmp.path(), "zai");
    // key 自动从 AUTH_TOKEN 搬到 API_KEY，无需重输
    assert_eq!(
        env_of(&settings, "ANTHROPIC_API_KEY"),
        Some("sk-migrate-me-0001")
    );
    assert_eq!(env_of(&settings, "ANTHROPIC_AUTH_TOKEN"), None);
    // spec 跟随了新 auth 模式
    assert_eq!(
        donn.spec("zai").unwrap().auth.mode,
        donn_core::AuthMode::ApiKey
    );
    // api_key 模式补写确认屏白名单
    let claude_json: Value =
        serde_json::from_str(&std::fs::read_to_string(home.claude_json_file("zai")).unwrap())
            .unwrap();
    let approved = claude_json["customApiKeyResponses"]["approved"]
        .as_array()
        .unwrap();
    assert!(!approved.is_empty());
}

#[test]
fn hand_edits_to_owned_keys_are_drift_and_sync_overwrites() {
    let tmp = TempDir::new().unwrap();
    let home = DonnHome::for_test(tmp.path());
    let donn = open(tmp.path());
    donn.create(&draft("zai", "zai", Some("sk-1234567890")))
        .unwrap();

    // 手改 donn 拥有的键 + 加一个用户自己的键
    let mut settings = read_settings(tmp.path(), "zai");
    settings["env"]["ANTHROPIC_BASE_URL"] = json!("https://hand-edited.example");
    settings["env"]["USER_KEY"] = json!("mine");
    settings["hooks"] = json!({"PostToolUse": []});
    std::fs::write(
        home.settings_file("zai"),
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();

    // audit 检出漂移
    let drift = donn.audit_settings("zai").unwrap();
    assert_eq!(drift.len(), 1);
    assert_eq!(drift[0].key, "ANTHROPIC_BASE_URL");
    assert_eq!(drift[0].kind, DriftKind::Modified);
    assert_eq!(
        donn.inspect("zai").unwrap().drift,
        drift,
        "详情视图复用同一份漂移计算"
    );

    // sync 以 donn 为准覆盖，并报告被覆盖键；用户键与未知字段原样保留
    let report = donn.sync("zai").unwrap();
    assert_eq!(report.overwritten, vec!["ANTHROPIC_BASE_URL"]);
    let settings = read_settings(tmp.path(), "zai");
    assert_eq!(
        env_of(&settings, "ANTHROPIC_BASE_URL"),
        Some("https://api.z.ai/api/anthropic")
    );
    assert_eq!(env_of(&settings, "USER_KEY"), Some("mine"));
    assert_eq!(settings["hooks"], json!({"PostToolUse": []}));
    assert!(
        donn.audit_settings("zai").unwrap().is_empty(),
        "sync 后无漂移"
    );
    assert!(donn.inspect("zai").unwrap().drift.is_empty());
}

#[test]
fn steady_state_sync_does_not_touch_bytes_mtime_or_updated_at() {
    let tmp = TempDir::new().unwrap();
    let home = DonnHome::for_test(tmp.path());
    let donn = open(tmp.path());
    donn.create(&draft("zai", "zai", Some("sk-1234567890")))
        .unwrap();

    let settings_path = home.settings_file("zai");
    let spec_path = home.spec_file("zai");
    let before_settings = std::fs::read(&settings_path).unwrap();
    let before_spec = std::fs::read(&spec_path).unwrap();
    let before_settings_mtime = std::fs::metadata(&settings_path)
        .unwrap()
        .modified()
        .unwrap();
    let before_spec_mtime = std::fs::metadata(&spec_path).unwrap().modified().unwrap();
    let before_updated_at = donn.spec("zai").unwrap().updated_at;

    let report = donn.sync("zai").unwrap();

    assert!(report.overwritten.is_empty());
    assert_eq!(std::fs::read(&settings_path).unwrap(), before_settings);
    assert_eq!(std::fs::read(&spec_path).unwrap(), before_spec);
    assert_eq!(
        std::fs::metadata(&settings_path)
            .unwrap()
            .modified()
            .unwrap(),
        before_settings_mtime
    );
    assert_eq!(
        std::fs::metadata(&spec_path).unwrap().modified().unwrap(),
        before_spec_mtime
    );
    assert_eq!(donn.spec("zai").unwrap().updated_at, before_updated_at);
}

#[test]
fn alias_lifecycle_and_wrapper_delegation() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    donn.create(&draft("zai", "zai", Some("sk-1234567890")))
        .unwrap();

    let path = donn.add_alias("zai", "z").unwrap();
    assert!(path.is_file());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("run zai"), "{text}");
    assert!(
        text.lines()
            .take(2)
            .any(|l| l.contains("generated by donn"))
    );
    assert_eq!(donn.spec("zai").unwrap().wrapper.aliases, vec!["zai", "z"]);

    // 另一 profile 抢占已用别名 → 拒绝（名字避开真机 PATH 上可能存在的命令）
    donn.create(&draft("donn-test-b", "kimi-cn", Some("sk-1234567890")))
        .unwrap();
    assert!(donn.add_alias("donn-test-b", "z").is_err());
    assert!(donn.remove_alias("donn-test-b", "z").is_err());
    assert!(path.exists(), "one profile must not remove another's alias");

    donn.remove_alias("zai", "z").unwrap();
    assert!(!path.exists());
    assert_eq!(donn.spec("zai").unwrap().wrapper.aliases, vec!["zai"]);
}

#[test]
fn broken_profile_stays_visible_and_removable_with_its_wrapper() {
    let tmp = TempDir::new().unwrap();
    let home = DonnHome::for_test(tmp.path());
    let donn = open(tmp.path());
    let receipt = donn
        .create(&draft("broken", "zai", Some("sk-1234567890")))
        .unwrap();
    std::fs::write(home.spec_file("broken"), "not [valid toml").unwrap();

    let card = donn
        .cards()
        .unwrap()
        .into_iter()
        .find(|card| card.name == "broken")
        .unwrap();
    assert!(card.broken.is_some());
    assert!(donn.inspect("broken").is_err());

    donn.remove("broken", true, false).unwrap();
    assert!(!home.profile_dir("broken").exists());
    assert!(!receipt.wrapper_paths[0].exists());
}

#[test]
fn cards_and_inspect_reflect_effective_values() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    let mut d = draft("zai", "zai", Some("sk-1234567890"));
    d.models.set(ModelSlot::Sonnet, Some("my-sonnet".into()));
    d.aliases = vec!["zai".into(), "z".into()];
    donn.create(&d).unwrap();
    donn.create(&draft("official", "official", None)).unwrap();

    let cards = donn.cards().unwrap();
    assert_eq!(cards.len(), 2);
    let zai = cards.iter().find(|c| c.name == "zai").unwrap();
    assert_eq!(
        zai.base_url.as_deref(),
        Some("https://api.z.ai/api/anthropic")
    );
    assert_eq!(
        zai.models.get(ModelSlot::Sonnet),
        Some("my-sonnet"),
        "覆盖生效"
    );
    assert!(
        zai.models.get(ModelSlot::Haiku).is_some(),
        "未覆盖槽位跟随 preset"
    );
    assert_eq!(
        zai.key,
        KeyState::Present {
            tail4: "7890".into()
        }
    );
    assert_eq!(zai.aliases, vec!["zai", "z"]);
    let official = cards.iter().find(|c| c.name == "official").unwrap();
    assert_eq!(official.key, KeyState::NotNeeded);

    let view = donn.inspect("zai").unwrap();
    assert_eq!(view.preset.key, "zai");
    assert_eq!(view.models.get(ModelSlot::Sonnet), Some("my-sonnet"));
    assert!(view.drift.is_empty());
    assert_eq!(
        view.spec.intent.models.get(ModelSlot::Sonnet),
        Some("my-sonnet")
    );
    assert_eq!(
        view.spec.intent.models.get(ModelSlot::Haiku),
        None,
        "spec 只存差异"
    );
}

#[test]
fn official_profile_is_pure_sandbox() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    donn.create(&draft("official", "official", None)).unwrap();
    let settings = read_settings(tmp.path(), "official");
    let env = settings["env"].as_object().unwrap();
    assert_eq!(env.len(), 2, "只有 donn 默认键: {env:?}");
    assert_eq!(env_of(&settings, "DISABLE_AUTOUPDATER"), Some("1"));
    assert_eq!(
        env_of(&settings, "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"),
        Some("1"),
        "agent teams donn 默认开"
    );
    assert_eq!(
        env_of(&settings, "ENABLE_TOOL_SEARCH"),
        None,
        "默认不写键：由上游按端点主机判定（true 会强发 beta 头）"
    );
    assert_eq!(
        settings["permissions"]["defaultMode"],
        json!("bypassPermissions"),
        "权限直通 donn 默认开"
    );
    assert_eq!(settings["skipDangerousModePermissionPrompt"], json!(true));
    let view = donn.inspect("official").unwrap();
    assert_eq!(view.key, KeyState::NotNeeded);
}

#[test]
fn remove_deletes_profile_and_wrappers_only() {
    let tmp = TempDir::new().unwrap();
    let home = DonnHome::for_test(tmp.path());
    let donn = open(tmp.path());
    donn.create(&draft("zai", "zai", Some("sk-1234567890")))
        .unwrap();
    // 用户自己的文件不删
    let bin = donn.bin_dir().unwrap();
    std::fs::write(bin.join("keepme"), "#!/bin/sh\nuser file\n").unwrap();

    donn.remove("zai", true, false).unwrap();
    assert!(!donn.exists("zai"));
    assert!(!home.profile_dir("zai").exists());
    assert!(!bin.join("zai").exists());
    assert!(bin.join("keepme").exists());

    assert!(matches!(
        donn.remove("ghost", true, false),
        Err(Error::ProfileNotFound { .. })
    ));
}

#[test]
fn remove_keeping_sessions_leaves_data_and_revives_on_recreate() {
    let tmp = TempDir::new().unwrap();
    let home = DonnHome::for_test(tmp.path());
    let donn = open(tmp.path());
    donn.create(&draft("zai", "zai", Some("sk-1234567890")))
        .unwrap();
    // 伪造会话数据
    let sessions = home.claude_config_dir("zai").join("projects");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join("s1.jsonl"), "{}").unwrap();

    donn.remove("zai", true, true).unwrap();
    // 身份全清：spec、含 key 的两个 json、别名
    assert!(!donn.exists("zai"));
    assert!(donn.profile_names().unwrap().is_empty(), "无幽灵条目");
    assert!(!home.settings_file("zai").exists());
    assert!(!home.claude_json_file("zai").exists());
    assert!(!donn.bin_dir().unwrap().join("zai").exists());
    // 会话原地保留
    assert!(sessions.join("s1.jsonl").exists());

    // 同名重建 → 自动接上原会话
    donn.create(&draft("zai", "zai", Some("sk-new-key-5678")))
        .unwrap();
    assert!(sessions.join("s1.jsonl").exists());
    let settings = read_settings(tmp.path(), "zai");
    assert_eq!(
        env_of(&settings, "ANTHROPIC_AUTH_TOKEN"),
        Some("sk-new-key-5678")
    );

    // create 失败回滚不得殃及保留的会话：先删掉再用冲突别名重建失败
    donn.remove("zai", true, true).unwrap();
    let mut bad = draft("zai", "zai", Some("sk-1234567890"));
    std::fs::write(donn.bin_dir().unwrap().join("taken"), "user file").unwrap();
    bad.aliases = vec!["taken".into()];
    assert!(donn.create(&bad).is_err());
    assert!(sessions.join("s1.jsonl").exists(), "回滚不删残留会话");

    // shared 模式该参数无效果：目录整删
    let mut sh = draft("shzai", "zai", Some("sk-1234567890"));
    sh.isolation = donn_core::Isolation::Shared;
    donn.create(&sh).unwrap();
    donn.remove("shzai", true, true).unwrap();
    assert!(!home.profile_dir("shzai").exists());
}

#[test]
fn shared_isolation_launches_without_config_dir_override() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    let mut d = draft("mm", "minimax", Some("sk-1234567890"));
    d.isolation = donn_core::Isolation::Shared;
    donn.create(&d).unwrap();

    let fake = tmp.path().join("claude-bin");
    std::fs::write(&fake, "#!/bin/sh\n").unwrap();
    let config = donn_core::GlobalConfig {
        claude_bin: Some(fake.display().to_string()),
        ..Default::default()
    };
    config.save(donn.home()).unwrap();
    let donn = open(tmp.path());

    let plan = donn.launch_plan("mm").unwrap();
    // 渠道 env 照常注入；CLAUDE_CONFIG_DIR 不设 → session 走主 ~/.claude
    assert!(plan.env.iter().any(|(k, _)| k == "ANTHROPIC_AUTH_TOKEN"));
    // connectors 显式停用（静默分支，避免 env 认证优先横幅）
    assert!(
        plan.env
            .iter()
            .any(|(k, v)| k == "ENABLE_CLAUDEAI_MCP_SERVERS" && v == "0")
    );
    // shared 的模型/权限/旋钮全部经 --settings overlay 文件带入，命令行只指向文件本身
    let overlay = donn.home().shared_settings_file("mm");
    assert!(overlay.is_file(), "shared sync 生成 settings overlay");
    let overlay_value: Value =
        serde_json::from_str(&std::fs::read_to_string(&overlay).unwrap()).unwrap();
    assert_eq!(
        overlay_value,
        json!({
            "model": "MiniMax-M3[1m]",
            "permissions": {"defaultMode": "bypassPermissions"},
            "skipDangerousModePermissionPrompt": true
        }),
        "overlay = full 写盘内容 + 权限模式 + sonnet 槽（压 ~/.claude 持久化选择）"
    );
    assert_eq!(
        plan.extra_args,
        vec![("--settings".to_string(), overlay.display().to_string())]
    );
    assert!(
        !plan.env.iter().any(|(k, _)| k == "CLAUDE_CONFIG_DIR"),
        "shared mode must not override CLAUDE_CONFIG_DIR: {:?}",
        plan.env
    );
    // spec 记录了隔离模式
    let spec = donn.spec("mm").unwrap();
    assert_eq!(spec.isolation, donn_core::Isolation::Shared);
    // donn 依然没碰 ~/.claude
    assert!(!donn.home().main_claude_dir().exists());

    // 切回独立模式：停用键经 footprint diff 被清除；overlay 被清掉不留孤儿
    donn.edit("mm", SpecChange::Isolation(donn_core::Isolation::Full))
        .unwrap();
    let plan = donn.launch_plan("mm").unwrap();
    assert!(plan.env.iter().any(|(k, _)| k == "CLAUDE_CONFIG_DIR"));
    assert!(
        !plan
            .env
            .iter()
            .any(|(k, _)| k == "ENABLE_CLAUDEAI_MCP_SERVERS")
    );
    assert!(plan.extra_args.is_empty(), "独立模式不追加参数");
    assert!(!overlay.exists(), "full 模式不得残留 shared overlay");
}

#[test]
fn shared_settings_overlay_carries_settings_class_knobs() {
    let tmp = TempDir::new().unwrap();
    let home = DonnHome::for_test(tmp.path());
    std::fs::create_dir_all(home.root()).unwrap();
    std::fs::write(
        home.config_file(),
        r#"
[defaults.knobs]
permission_mode = "plan"
effort = "medium"
max_effort = "high"
language = "中文"
thinking = false
auto_compact = false
hide_attribution = true
"#,
    )
    .unwrap();
    let donn = open(tmp.path());
    let mut d = draft("ov", "official", None);
    d.isolation = donn_core::Isolation::Shared;
    donn.create(&d).unwrap();

    let overlay: Value =
        serde_json::from_str(&std::fs::read_to_string(home.shared_settings_file("ov")).unwrap())
            .unwrap();
    assert_eq!(
        overlay,
        json!({
            "alwaysThinkingEnabled": false,
            "attribution": {"commit": "", "pr": "", "sessionUrl": false},
            "autoCompactEnabled": false,
            "effortLevel": "medium",
            "language": "中文",
            "maxEffortLevel": "high",
            "permissions": {"defaultMode": "plan"}
        }),
        "settings 类键与权限模式全部经 overlay 到达 shared 会话"
    );

    // 稳态 no-op：二次 sync overlay 字节不变
    let before = std::fs::read(home.shared_settings_file("ov")).unwrap();
    donn.sync("ov").unwrap();
    assert_eq!(
        std::fs::read(home.shared_settings_file("ov")).unwrap(),
        before
    );

    // overlay 整份归 donn：损坏了直接重写，不拦 sync
    std::fs::write(home.shared_settings_file("ov"), "not json").unwrap();
    donn.sync("ov").unwrap();
    assert_eq!(
        std::fs::read(home.shared_settings_file("ov")).unwrap(),
        before
    );

    // permission_mode = default 档：overlay 只剩其余键；确认标记随 bypass 一起消失
    std::fs::write(
        home.config_file(),
        "[defaults.knobs]\npermission_mode = \"default\"\neffort = \"medium\"\n",
    )
    .unwrap();
    let donn = open(tmp.path());
    donn.sync("ov").unwrap();
    let overlay: Value =
        serde_json::from_str(&std::fs::read_to_string(home.shared_settings_file("ov")).unwrap())
            .unwrap();
    assert_eq!(overlay, json!({"effortLevel": "medium"}));
}

#[test]
fn launch_plan_env_and_config_dir() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    donn.create(&draft("zai", "zai", Some("sk-1234567890")))
        .unwrap();
    // 伪 claude 二进制
    let fake = tmp.path().join("claude-bin");
    std::fs::write(&fake, "#!/bin/sh\n").unwrap();
    let config = donn_core::GlobalConfig {
        claude_bin: Some(fake.display().to_string()),
        ..Default::default()
    };
    config.save(donn.home()).unwrap();
    let donn = open(tmp.path()); // 重新加载 config

    let plan = donn.launch_plan("zai").unwrap();
    let get = |k: &str| {
        plan.env
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("ANTHROPIC_AUTH_TOKEN"), Some("sk-1234567890"));
    assert_eq!(plan.env.last().unwrap().0, "CLAUDE_CONFIG_DIR");
    assert!(
        get("CLAUDE_CONFIG_DIR")
            .unwrap()
            .ends_with("profiles/zai/claude")
    );
}

#[test]
fn doctor_flags_broken_profiles_with_fix_hints() {
    let tmp = TempDir::new().unwrap();
    let home = DonnHome::for_test(tmp.path());
    let donn = open(tmp.path());
    donn.create(&draft("zai", "zai", Some("sk-1234567890")))
        .unwrap();

    // 健康时：settings.json 检查通过
    let checks = doctor::run(&donn);
    assert!(
        checks
            .iter()
            .any(|c| c.ok && c.title.contains("settings.json"))
    );

    // 故意破坏：手改 donn 拥有的键 → drift 检出，fix 提示 sync
    let mut settings = read_settings(tmp.path(), "zai");
    settings["env"]["ANTHROPIC_BASE_URL"] = json!("https://evil.example");
    std::fs::write(
        home.settings_file("zai"),
        serde_json::to_string(&settings).unwrap(),
    )
    .unwrap();
    let checks = doctor::run(&donn);
    let drift_check = checks.iter().find(|c| c.title.contains("drift")).unwrap();
    assert!(!drift_check.ok);
    assert!(drift_check.detail.contains("ANTHROPIC_BASE_URL"));
    assert!(!doctor::all_ok(&checks));

    // 删掉 wrapper → 检出 missing
    std::fs::remove_file(donn.bin_dir().unwrap().join("zai")).unwrap();
    let checks = doctor::run(&donn);
    assert!(
        checks
            .iter()
            .any(|c| !c.ok && c.title.contains("wrapper") && c.detail.contains("missing"))
    );
}

#[test]
fn concurrent_profile_edits_do_not_lose_updates() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    let donn = open(&root);
    donn.create(&draft("zai", "zai", Some("sk-1234567890")))
        .unwrap();

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(5));
    let handles: Vec<_> = (0..4)
        .map(|index| {
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let donn = open(&root);
                barrier.wait();
                donn.edit(
                    "zai",
                    SpecChange::SetEnv(format!("CONCURRENT_{index}"), index.to_string()),
                )
                .unwrap();
            })
        })
        .collect();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }

    let spec = open(&root).spec("zai").unwrap();
    for index in 0..4 {
        assert_eq!(
            spec.intent.env.get(&format!("CONCURRENT_{index}")),
            Some(&index.to_string())
        );
    }
}

#[test]
fn concurrent_config_edits_reload_inside_the_write_lock() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|index| {
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let donn = open(&root);
                barrier.wait();
                donn.edit_config(ConfigChange::SetDefaultEnv(
                    format!("DEFAULT_{index}"),
                    index.to_string(),
                ))
                .unwrap();
            })
        })
        .collect();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }

    let donn = open(&root);
    for index in 0..2 {
        assert_eq!(
            donn.config()
                .unwrap()
                .defaults
                .env
                .get(&format!("DEFAULT_{index}")),
            Some(&index.to_string())
        );
    }
}

#[test]
fn config_reads_are_stateless_and_see_manual_disk_edits() {
    let dir = TempDir::new().unwrap();
    let home = DonnHome::for_test(dir.path());
    let donn = open(dir.path());
    assert!(donn.config().unwrap().claude_bin.is_none());
    std::fs::create_dir_all(home.root()).unwrap();
    std::fs::write(home.config_file(), "claude_bin = \"/tmp/stub-claude\"\n").unwrap();
    assert_eq!(
        donn.config().unwrap().claude_bin.as_deref(),
        Some("/tmp/stub-claude")
    );
}

#[test]
fn profile_listing_ignores_junk_entries_and_not_found_lists_available() {
    let dir = TempDir::new().unwrap();
    let home = DonnHome::for_test(dir.path());
    let donn = open(dir.path());
    donn.create(&draft("valid", "official", None)).unwrap();
    std::fs::write(home.profiles_dir().join("plain-file"), "junk").unwrap();
    std::fs::create_dir_all(home.profiles_dir().join("empty-dir")).unwrap();
    assert_eq!(donn.profile_names().unwrap(), ["valid"]);
    let error = donn.spec("missing").unwrap_err().to_string();
    assert!(error.contains("available: valid"), "{error}");
}

#[test]
fn concurrent_knob_edits_merge_only_fields_changed_from_base() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|index| {
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let donn = open(&root);
                let base = donn.config().unwrap().defaults.knobs;
                let mut value = base.clone();
                if index == 0 {
                    value.agent_teams = Some(false);
                } else {
                    value.tool_search = Some(false);
                }
                barrier.wait();
                donn.edit_config(ConfigChange::Knobs { base, value })
                    .unwrap();
            })
        })
        .collect();
    barrier.wait();
    for handle in handles {
        handle.join().unwrap();
    }

    let donn = open(&root);
    let config = donn.config().unwrap();
    let knobs = &config.defaults.knobs;
    assert_eq!(knobs.agent_teams, Some(false));
    assert_eq!(knobs.tool_search, Some(false));
}

#[test]
fn extra_env_survives_key_rotation_and_edits() {
    // 回归：任何一次 regenerate 都不得丢失 intent 里的附加 env
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    let mut d = draft("zai", "zai", Some("sk-1234567890"));
    d.env = vec![("API_TIMEOUT_MS".into(), "9000".into())];
    donn.create(&d).unwrap();
    donn.set_key("zai", Secret::new("sk-rotated-9999").unwrap())
        .unwrap();
    donn.edit("zai", SpecChange::Model(ModelSlot::Opus, Some("x".into())))
        .unwrap();
    let settings = read_settings(tmp.path(), "zai");
    assert_eq!(env_of(&settings, "API_TIMEOUT_MS"), Some("9000"));
    assert_eq!(
        env_of(&settings, "ANTHROPIC_AUTH_TOKEN"),
        Some("sk-rotated-9999")
    );
}

#[test]
fn regeneration_is_deterministic() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    let mut d = draft("zai", "zai", Some("sk-1234567890"));
    d.env = vec![
        ("Z_LAST".into(), "1".into()),
        ("A_FIRST".into(), "2".into()),
    ];
    donn.create(&d).unwrap();
    let home = DonnHome::for_test(tmp.path());
    let first = std::fs::read(home.settings_file("zai")).unwrap();
    donn.sync("zai").unwrap();
    let second = std::fs::read(home.settings_file("zai")).unwrap();
    assert_eq!(first, second, "同一 spec 两次生成字节级一致");
}

#[test]
fn donn_own_edits_never_report_hand_edit() {
    let tmp = TempDir::new().unwrap();
    let donn = open(tmp.path());
    donn.create(&draft("fx", "zai", Some("sk-ant-0123456789abcdefghij")))
        .unwrap();

    // 通过 donn 连续改同一个键：都不是手改，不该有 overwritten 提示
    let r = donn
        .edit(
            "fx",
            SpecChange::SetEnv("CLAUDE_CODE_EFFORT_LEVEL".into(), "low".into()),
        )
        .unwrap();
    assert!(r.overwritten.is_empty(), "{:?}", r.overwritten);
    let r = donn
        .edit(
            "fx",
            SpecChange::SetEnv("CLAUDE_CODE_EFFORT_LEVEL".into(), "medium".into()),
        )
        .unwrap();
    assert!(
        r.overwritten.is_empty(),
        "donn 自身变更误报手改: {:?}",
        r.overwritten
    );

    // set_key 换 key 也不是手改
    let r = donn
        .set_key("fx", Secret::new("sk-ant-zzzzzzzzzzzzzzzzzzzz").unwrap())
        .unwrap();
    assert!(r.overwritten.is_empty(), "{:?}", r.overwritten);

    // 真正手改 settings.json 后再 sync：必须提示
    let home = DonnHome::for_test(tmp.path());
    let mut settings = read_settings(tmp.path(), "fx");
    settings["env"]["CLAUDE_CODE_EFFORT_LEVEL"] = json!("xhigh");
    std::fs::write(
        home.settings_file("fx"),
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();
    let r = donn.sync("fx").unwrap();
    assert_eq!(r.overwritten, vec!["CLAUDE_CODE_EFFORT_LEVEL"]);
}

#[test]
fn global_defaults_flow_into_every_profile_and_clean_up() {
    let tmp = TempDir::new().unwrap();
    let home = DonnHome::for_test(tmp.path());
    std::fs::create_dir_all(home.root()).unwrap();
    std::fs::write(
        home.config_file(),
        r#"
[defaults.env]
CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = "1"

[defaults.settings]
spinnerTipsEnabled = false
"#,
    )
    .unwrap();

    let donn = open(tmp.path());
    donn.create(&draft("gd", "zai", Some("sk-ant-0123456789abcdefghij")))
        .unwrap();
    let settings = read_settings(tmp.path(), "gd");
    assert_eq!(
        env_of(&settings, "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"),
        Some("1")
    );
    assert_eq!(settings["spinnerTipsEnabled"], json!(false));
    // 第三方默认层同样在（来自 base_url 推导，非 preset 声明）
    assert_eq!(env_of(&settings, "API_TIMEOUT_MS"), Some("600000"));
    let spec = donn.spec("gd").unwrap();
    assert_eq!(
        spec.footprint.settings_top,
        vec!["skipDangerousModePermissionPrompt", "spinnerTipsEnabled"]
    );

    // 从全局配置移除 defaults → 重新打开 + sync → 用户自由键被清理
    std::fs::write(home.config_file(), "").unwrap();
    let donn = open(tmp.path());
    donn.sync("gd").unwrap();
    let settings = read_settings(tmp.path(), "gd");
    assert!(settings.get("spinnerTipsEnabled").is_none());
    assert_eq!(
        env_of(&settings, "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"),
        Some("1"),
        "agent teams 是 donn 默认，不随用户配置清空"
    );
    assert_eq!(
        env_of(&settings, "ANTHROPIC_AUTH_TOKEN"),
        Some("sk-ant-0123456789abcdefghij"),
        "key 不受影响"
    );
}

#[test]
fn broken_profile_stays_visible_in_cards_and_detail_errors() {
    let tmp = TempDir::new().unwrap();
    let home = DonnHome::for_test(tmp.path());
    let donn = open(tmp.path());
    donn.create(&draft("good", "zai", Some("sk-1234567890")))
        .unwrap();
    donn.create(&draft("bad", "zai", Some("sk-1234567890")))
        .unwrap();

    // 破坏 spec：坏 profile 不得从列表消失，必须以 broken 状态可见
    std::fs::write(home.spec_file("bad"), "schema_version = [broken").unwrap();
    let cards = donn.cards().unwrap();
    assert_eq!(cards.len(), 2, "坏 profile 不消失");
    let bad = cards.iter().find(|c| c.name == "bad").unwrap();
    let reason = bad.broken.as_deref().unwrap();
    assert!(reason.contains("invalid TOML"), "{reason}");
    assert!(
        cards
            .iter()
            .find(|c| c.name == "good")
            .unwrap()
            .broken
            .is_none()
    );

    // inspect 同一 profile：硬失败（与列表的软可见互补）
    assert!(donn.inspect("bad").is_err());

    // 非法 effort 档位写入即拒绝
    let err = donn
        .edit(
            "good",
            SpecChange::SetEnv("CLAUDE_CODE_EFFORT_LEVEL".into(), "ultra".into()),
        )
        .unwrap_err();
    assert!(err.to_string().contains("invalid effort"), "{err}");

    // settings.json 顶层被改成数组 → sync 报错修复，不静默清空重建
    std::fs::write(home.settings_file("good"), "[1, 2]").unwrap();
    let err = donn.sync("good").unwrap_err();
    assert!(err.to_string().contains("expected a JSON object"), "{err}");
}
