# 全局设置一览

`~/.donn/config.toml [defaults.knobs]` 每个字段的对照表：TUI 设置面板（`S`）里叫什么、写到 Claude Code 的哪个键、什么时候写。每个旋钮的默认值和写入规则只在这里写，别处不重复。给用户看的字段说明是 `config.toml` 模板里的注释。

代码位置：字段在 `crates/donn-core/src/knobs.rs`（`Knobs` 类型化字段、`BOOL_KNOBS`、`VALUE_KNOBS`），行名在 `crates/donn-cli/src/tui/i18n.rs` 的 `knob_label`，键的含义在 `crates/donn-core/src/keys.rs`。`config.rs` 有测试断言本文列全了字段：加旋钮必须在这里加一行。

没做成旋钮的键（遥测、错误上报、问卷、外观、auto memory、git 指令、会话清理天数等）直接写进 `[defaults.settings]` / `[defaults.env]`，效果一样。

通用规则：

- 字段不写 = 跟 Claude Code 默认走，donn 不写键。
- 已知字段的值不合法，`config.toml` 加载就报错。
- 表驱动旋钮（`BOOL_KNOBS` / `VALUE_KNOBS`）：生效值等于 Claude 默认时不写键；显式设成默认值会删掉 preset 写的同名 env 键；枚举表以外的旧值不写键，面板标 invalid。
- 改任何旋钮都会立刻 sync 全部 profile。
- 「Claude Code 键」一列带 `env` 前缀的写进 `settings.json` 的 `env`，不带的写到 `settings.json` 顶层。两种隔离模式怎么把它们送进会话，见 [ARCHITECTURE.md 的隔离模式](ARCHITECTURE.md#隔离模式)。

## session & models

| 字段 | 面板行名 | Claude Code 键 | 类型 / 取值 | Claude 默认 | 写入规则 |
| --- | --- | --- | --- | --- | --- |
| `permission_mode` | default permission mode | `permissions.defaultMode` + `skipDangerousModePermissionPrompt` | default / acceptEdits / plan / dontAsk / auto / bypassPermissions | 未设 | donn 默认 `bypassPermissions`；非 `default` 写 `defaultMode`，`bypassPermissions` 同时写确认标记 `true`；`default` 不管理该键 |
| `agent_teams` | agent teams (experimental) | env `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` | bool | 关 | donn 默认开，写 `"1"`；显式 `false` 删键（压过 preset） |
| `tool_search` | MCP tool search | env `ENABLE_TOOL_SEARCH` | 不设 / true / false（面板三选一） | 不设时：官方主机按需加载工具（defer），第三方端点一次全加载（upfront） | 不设不写键；`true` 写 `"true"`，强制按需加载并发 beta 头，代理不支持 `tool_reference` 会请求失败；`false` 写 `"false"`，一次全加载 |
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
