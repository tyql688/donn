# 硬规则

每条都是门禁，不是建议。本文只写不能破的边界；怎么运转看 [ARCHITECTURE.md](ARCHITECTURE.md)，每个旋钮的写入规则看 [GLOBAL-SETTINGS.md](GLOBAL-SETTINGS.md)，维护步骤在 `.claude/skills/`。

## 铁律

1. `donn-core` 不依赖 TUI/终端；CLI 与 TUI 只经 `Donn` facade 读写领域状态，不直接拼用户配置。
2. 写用户文件必须走 `fsx` 的原子写/权限通路；读 JSON 必须保留未知字段，禁止完整反序列化后重建用户文件。
3. 只增删 `profile.toml [footprint]` 声明的键，其余 settings/Claude 字段永不触碰。
4. 绝不写主 `~/.claude/`，绝不 patch Claude 二进制。测试和验收一律用 `/tmp` 下独立的 `HOME` 和 `DONN_HOME`，不得读写真实的 `~/.donn` 或 `~/.claude`。
5. API key 不入日志、不入 `profile.toml`、不入错误信息；只用 `Secret` 表示，明文只流向最终 settings 渲染和显式 TUI reveal。
6. 新 provider = `crates/donn-core/src/preset/presets/<key>.toml`；逻辑层不得按 provider 特判，兼容怪癖用 preset flags/data 表达。
7. `~/.donn` 磁盘格式是对外契约。schema 变更先设计迁移；存量文件必须可直接读取。
8. profile 坏文件不能让列表或删除成为死胡同：列表必须可见、Doctor 必须可诊断、删除走最保守的会话保留语义。
9. 启动前 sync 是 best-effort：失败要保留精确告警并继续尝试现有配置；稳态 sync 不得改 bytes、mtime 或 `updated_at`。
10. UI 的有限选项必须使用明确的 `Select`/搜索 picker；不能为了省代码退化成 Enter 循环切换。渠道和模型搜索使用 `nucleo-matcher`，输入编辑使用 `tui-input`，URL 使用 `open`，不重复造已有成熟轮子。

## 单一来源

- Claude Code 键、枚举、模型槽：`crates/donn-core/src/keys.rs`。
- 全局旋钮的定义与默认值：`crates/donn-core/src/knobs.rs`；`config.toml` 模板：`crates/donn-core/src/config.rs`。
- preset schema、内置清单与模型套餐：`crates/donn-core/src/preset/mod.rs` + `preset/presets/*.toml`。
- 按键到语义动作：`crates/donn-cli/src/tui/keymap.rs`。
- 通用选择/确认/输入行为：`crates/donn-cli/src/tui/components/modal.rs`。

## TUI 展示规范

- 表单展示生效值，不展示内部存储技巧。空 override 表示跟随 preset 时，仍需显示默认值并标注 `(default)`；提交时不得因此写成显式 override。
- 结构化集合按行、按列展示；禁止把候选项用分隔符拼成长段 prose。模型列表以短名称和真实 ID 为主，套餐/context 只放当前项详情。
- 列宽、补齐和省略按终端显示宽度计算，必须覆盖 CJK 与窄终端；内容不得越过 panel/modal 边界。
- 状态栏每个上下文最多展示 5 个当前步骤的主要操作。低频快捷键保留在 `?` 帮助页，不在多个分区重复陈列。
- action 行与输入字段使用不同视觉语义：action 只高亮实际文本，不得借用字段标签 padding 画出大块空白按钮。
- TUI 改动除单元测试外，必须在至少一个常规尺寸和一个窄尺寸真实 PTY 中走到对应状态；只看静态 buffer 不算验收。

## 变更边界

- 自由 env 只做操作系统无法表示内容的校验：名称为空/含 `=`/NUL 或值含 NUL时报错；错误不得包含可能是 secret 的 value。
- `auth_token` 写空 `ANTHROPIC_API_KEY`，屏蔽 shell 继承的游离 key；这是认证优先级边界。
- 启动时从继承的 shell 环境剥掉 donn 会写的 env 键和 `keys::SHELL_OVERRIDE_KEYS`：profile 的身份、路由和模型只来自 donn 的配置。
- 旋钮的写入语义以 GLOBAL-SETTINGS.md 的表为准，改语义要同时改表和测试。
- 模型套餐跟生效的 Sonnet 槽匹配；编辑单个槽只改该槽，不能把非统一 preset 扇出成全槽同值。
- wrapper 只删除严格识别为 donn 生成且实际指向目标 profile 的文件。

## 验收

提交前 `make check` 必须通过（包含哪几条命令见 [CLAUDE.md](../CLAUDE.md)）。

测试通过不等于验收。落盘改动要对照真实生成文件；TUI 改动要在真实 PTY 中打开对应搜索框、选择框、输入框和确认框；启动改动要用假 Claude 验证 env、参数与退出码透传。
