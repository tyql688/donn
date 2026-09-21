---
name: add-global-knob
description: 给 donn 的全局设置面板新增或修改一个 `~/.donn/config.toml [defaults.knobs]` 旋钮。
when_to_use: 触发词 - 全局设置、全局配置、旋钮、设置面板、加个开关、knob、settings panel。
---

# 加一个全局设置旋钮

## 归属

旋钮只收全局、类型化、取值有界的设置。自由值写 `[defaults.env]` / `[defaults.settings]`；渠道差异写 preset；单 profile 差异写 intent；donn 不可妥协行为写 `render.rs` 常量。`env` 和 `permissions` 是保留顶层键，走专用通路。

底层键必须先按 `update-claude-catalog` 第 1 节入册 `keys.rs`（文档与二进制都核对，标来源）。说不清取值范围就不暴露。

## 简单旋钮：走表

一个布尔或一个标量映射一个键、没有优先级条件、没有配对键的旋钮：

1. `crates/donn-core/src/knobs.rs` 的 `BOOL_KNOBS` / `VALUE_KNOBS` 加一行：`field`（config.toml 键名）、`group`（面板分组）、`target`（`Env` 或 `Setting`）、布尔的 `claude_default`（键不存在时 Claude Code 的行为）或标量的 `kind`（`Text` / `Number` / `Enum(&[...])`）。
2. `config.rs` 的 `CONFIG_DOC` 加一行 `# field = 默认值  # 说明`。
3. `crates/donn-cli/src/tui/i18n.rs` 的 `knob_label` 加行名（宽度小于 30）。
4. `docs/GLOBAL-SETTINGS.md` 对应分组加一行（字段、行名、键、取值、Claude 默认、写入规则）。

render、合并、校验、TUI 行与测试按表生效。表驱动旋钮什么时候写键、什么时候删键，见 `docs/GLOBAL-SETTINGS.md` 的通用规则。

## 类型化字段

需要两遍优先级、只在有 `base_url` 时注入、或配对写多个键的旋钮：

- `knobs.rs`：`Knobs` 加 `Option<T>` 字段与生效值方法（派生反序列化，不做旧字段名迁移：改名的旧键会落到 `extra` 并由 doctor 报为 unknown）；`Knobs::validate` 加取值校验。
- `config.rs`：`CONFIG_DOC` 加行。
- `render.rs`：env 类显式值在 `preset.env` 之后再压一次；依赖 `base_url` 的两遍都守条件；settings 自由默认可覆盖 settings 类旋钮，donn 常量最后写。
- `ops/config_ops.rs`：`apply_knob_changes` 只合并与 `base` 不同的字段；值变化时才校验。
- `tui/panes/settings.rs`：加 `Row`、分组位置、编辑与渲染；拨回默认存 `None`；有限枚举用 `Select`，数字用 `Prompt`；显式值与 `(default)` 区分显示。
- `i18n.rs`：`knob_label` 加行名。
- `docs/GLOBAL-SETTINGS.md`：加一行。

## 语义

- 先查清这个键「不设 / 真 / 假」各是什么行为，再定旋钮形态。有的键设任何值都算开，「关」只能删键（`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`）；有的键不设时上游另有判定，两态开关表达不了，要做成三选一（`ENABLE_TOOL_SEARCH`）。
- 每个旋钮的写入规则只记在 `docs/GLOBAL-SETTINGS.md` 的表里，别处不重复。
- 模型槽位与套餐有专用逻辑，旋钮不写模型槽位键。
- 保存配置立即 sync 全部 profile；失败逐 profile 汇报，不回滚已保存的配置。

## 验收

`make check`，再按 `docs/RULES.md` 的验收一节做真实检查。

测试覆盖默认值、显式值、拨回默认、模板行、config 往返与 render 结果。再用 `/tmp` 独立 `DONN_HOME` 打开真实 TUI：改该行后 `config.toml` 出现字段、所有 profile 同步；拨回默认后字段消失。
