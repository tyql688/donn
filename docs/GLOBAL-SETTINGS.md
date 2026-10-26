# 全局设置一览

donn 的「旋钮」是一次配置、对所有 profile 生效的 Claude Code 开关，比如权限模式、思考强度、回复语言。它们存在 `~/.donn/config.toml` 的 `[defaults.knobs]` 段，在 TUI 里按 `S` 打开设置面板修改，改完立即写入并同步到每个 profile。

本文是每个旋钮的对照表：面板里叫什么、写进 Claude Code 的哪个键、默认值是什么、什么条件下才写。用户读到的字段说明是 `config.toml` 模板里的注释，那部分是给安装后直接改文件的人看的，和本文内容一致但更简短。

## 改代码时要知道的

- 旋钮的字段定义、分组、目标键都在 `crates/donn-core/src/knobs.rs`。
- 面板每行的英文名在 `crates/donn-cli/src/tui/i18n.rs` 的 `knob_label`。
- 每个 Claude Code 键的含义和取值在 `crates/donn-core/src/keys.rs`。
- `crates/donn-core/src/config.rs` 有测试断言本文列全了所有字段，所以新增旋钮必须在这里补一行，否则测试失败。

旋钮按实现分三类。一键一值、无条件生效的放 `BOOL_KNOBS` / `VALUE_KNOBS` 表，加一行就生效；需要条件判断或成对写键的（权限模式、超时、agent teams、tool search）是 `Knobs` 上的类型化字段；只在该字段被显式设置过才写、且大多是布尔开关的，用 `knobs::TypedToggle` 表统一管理，面板和渲染都遍历它，不必各自列举字段名。

## 通用规则

- 字段不写，就不写对应的键，Claude Code 用它自己的默认行为。
- 已知字段的值不合法时，`config.toml` 在加载阶段就报错，不会带着错误的配置跑起来。
- 表驱动和 `TypedToggle` 管理的旋钮：生效值恰好等于 Claude Code 的默认值时，donn 不写这个键；如果用户显式把它设成默认值，donn 会删掉 preset 写过的同名 env 键，保证用户的显式选择和默认值表现一致。枚举类字段如果存了不在枚举里的旧值，donn 不写键，面板上那一行标成 invalid。
- 改任何一个旋钮，donn 都会立刻重新同步所有 profile。
- 「Claude Code 键」一列里带 `env` 前缀的，写进 `settings.json` 的 `env` 对象；不带前缀的，写在 `settings.json` 顶层。两种隔离模式下这些值怎么进入会话：`full` 模式直接由 profile 自己的 `settings.json` 生效；`shared` 模式把它们写进 `shared-settings.json`，启动时用 `--settings` 传给 Claude，从而压过 `~/.claude` 里的同名设置。

## session & models

| 字段 | 面板行名 | Claude Code 键 | 类型 / 取值 | Claude 默认 | 写入规则 |
| --- | --- | --- | --- | --- | --- |
| `permission_mode` | default permission mode | `permissions.defaultMode` + `skipDangerousModePermissionPrompt` | default / acceptEdits / plan / dontAsk / auto / bypassPermissions | 未设 | donn 默认 `bypassPermissions`；非 `default` 写 `defaultMode`，`bypassPermissions` 同时写确认标记 `true`；`default` 不管理该键 |
| `agent_teams` | agent teams (experimental) | env `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` | bool | 关 | donn 默认开，写 `"1"`；显式 `false` 删键（压过 preset） |
| `tool_search` | MCP tool search | env `ENABLE_TOOL_SEARCH` | 不设 / true / false（面板三选一） | 不设时按需加载；`ANTHROPIC_BASE_URL` 在网关后时 v2.1.304+ 也是按需加载，更早的版本一次全加载 | 不设不写键；`true` 写 `"true"`，强制按需加载并发 beta 头，代理不支持 `tool_reference` 会请求失败；`false` 写 `"false"`，一次全加载 |
| `effort` | default thinking effort | `effortLevel` | low / medium / high / xhigh | 未设 | 有值才写 |
| `max_effort` | effort cap | `maxEffortLevel` | low / medium / high / xhigh / max | 未设（不封顶） | 有值才写；v2.1.267+ |
| `language` | response language | `language` | 任意语言名 | 未设 | 有值才写，原样进系统提示 |
| `max_output_tokens` | max output tokens | env `CLAUDE_CODE_MAX_OUTPUT_TOKENS` | 非负整数 | 模型默认；未知 id 32000 | 有值才写 |
| `thinking` | extended thinking | `alwaysThinkingEnabled` | bool | 开 | `false` 才写 `false`；Fable 等恒思考模型忽略 |
| `auto_compact` | auto-compact | `autoCompactEnabled` | bool | 开 | `false` 才写 |
| `model_fallback` | automatic model fallback | env `CLAUDE_CODE_NO_MODEL_FALLBACK` | bool | 开 | `false` 才写 `"1"`；键在二进制中，官方文档未列 |
| `subagent_model_force` | force subagent model | env `CLAUDE_CODE_SUBAGENT_MODEL_FORCE` | bool | 关 | `true` 才写 `"1"`；v2.1.257+ |

## third-party endpoint

只对有生效 `base_url` 的 profile 注入。

| 字段 | 面板行名 | Claude Code 键 | 类型 / 取值 | Claude 默认 | 写入规则 |
| --- | --- | --- | --- | --- | --- |
| `api_timeout_ms` | request timeout (ms) | env `API_TIMEOUT_MS` | 毫秒 | 600000 | 恒写生效值；显式值压过 preset；上限 2147483647，超过会让上游计时器溢出、请求立刻失败，所以加载时就报错 |
| `disable_nonessential_traffic` | disable nonessential traffic | env `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | bool | 关 | `true` 写 `"1"`；`false` 删键（该键设任何值都为禁） |
| `disable_experimental_betas` | strip beta headers (proxy) | env `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS` | bool | 关 | `true` 才写 `"1"`；代理网关拒绝 `anthropic-beta` 头时用 |
| `disable_unknown_model_window_enforcement` | no early compact (unknown id) | env `CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT` | bool | 关 | `true` 才写 `"1"`。对 Claude Code 不认识的模型 id（网关别名）只在 API 报 too-long 后才压缩；preset 套餐给了 `max_context` 时 Claude 按声明窗口压缩，不需要开 |

## attribution & privacy

| 字段 | 面板行名 | Claude Code 键 | 类型 / 取值 | Claude 默认 | 写入规则 |
| --- | --- | --- | --- | --- | --- |
| `hide_attribution` | hide commit/PR attribution | `attribution` | bool | 关 | `true` 写 `{"commit": "", "pr": "", "sessionUrl": false}` |
| `disable_connectors` | disable claude.ai connectors | `disableClaudeAiConnectors` | bool | 关 | `true` 才写 |

## 旋钮之外

- `[defaults.env]`：任意 env 键值，注入每个 profile 的 `settings.json env`，覆盖旋钮同名键。面板 "env +"。
- `[defaults.settings]`：任意 `settings.json` 顶层字段（含嵌套表）；`env` / `permissions` 为保留键会被拒绝。面板 "settings +"。
- `[defaults.knobs]` 里未知的键原样保留并回写。
- donn 常量，不能配置：`DISABLE_AUTOUPDATER=1` 恒写；shared 模式 `ENABLE_CLAUDEAI_MCP_SERVERS=0`；`hasCompletedOnboarding=true` 与 api_key 模式的 `customApiKeyResponses` 写入 `.claude.json`。
