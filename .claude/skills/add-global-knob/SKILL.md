---
name: add-global-knob
description: 给 donn 的全局设置面板新增或修改一个 config.toml defaults.knobs 选项。
---

# 添加全局设置

旋钮收取值有界的全局配置；自由值放 `[defaults.env]` / `[defaults.settings]`，provider 差异放 preset，单 profile 差异放 intent。`env` 和 `permissions` 是保留 settings key。

## 实现

1. 核对 Claude Code key 的行为与取值，在 `internal/core/keys.go` 登记 key；来源与核对时间记入 update-claude-catalog 的 ledger。
2. 在 `internal/core/knobs.go` 的 `KnobCatalog` 加入字段、行名、类型、默认值、目标与枚举。一个 key 的映射使用 `TargetEnv` / `TargetSetting`；有条件或配对写入使用 `TargetSpecial`。
3. 特殊语义在 `internal/core/render.go` 实现。env knob 默认值在 preset 前应用，显式值在 preset 后再应用；endpoint 条件两遍都要满足；settings 自由默认覆盖 settings knob；常量最后写。
4. 更新 `internal/core/config-template.toml` 的用户注释与 `docs/GLOBAL-SETTINGS.md` 的对应行。

面板、类型校验和配置编辑共享 KnobCatalog；有限选项使用 picker，数字和文本使用 prompt。恢复默认值删除显式字段。保存全局设置后同步所有 profile，按 profile 汇报失败，保留已保存的全局配置。

## 验证

运行 `make check`，然后使用临时 HOME 与 DONN_HOME 打开真实 TUI：修改设置，检查 config.toml、各 profile 的 JSON 和 footprint；恢复默认后检查 key 的删除条件；再次 sync 确认字节、mtime 和 updated_at 不变。不添加测试代码。
