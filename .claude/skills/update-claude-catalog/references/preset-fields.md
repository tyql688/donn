# preset TOML 字段

`[preset.models]` 是各槽位默认值（`sonnet` / `opus` / `fable` / `haiku`），槽位名拼错会静默无效。`[[preset.model_choices]]` 是选择器候选：

| 字段 | 注入 |
| --- | --- |
| `id` | 对应槽位键 |
| `label` | 仅显示 |
| `max_context` | `CLAUDE_CODE_MAX_CONTEXT_TOKENS` |
| `auto_compact` | `CLAUDE_CODE_AUTO_COMPACT_WINDOW`；省略 = `max_context`，封顶 `1000000` |
| `pin_subagent` | `CLAUDE_CODE_SUBAGENT_MODEL`；省略且设了 `max_context` 时为 true |
| `env` | 原样写入 |

套餐按生效的 `sonnet` id 匹配。只有 `id` + `label` 的候选不注入任何键。`[preset.models]` 的改动在用户下次 sync 时生效；用户在 `[intent.models]` 钉住的槽位不受影响。兼容性怪癖用 `[preset.flags]`。

## `max_context` 怎么填

填裸 token 数，非 Claude 的候选一律要有。数字用 `scripts/pi-sync.mjs --emit` 生成的，不手抄、不估。

| 情况 | 值 |
| --- | --- |
| 默认 | pi 目录的 `contextWindow` |
| 厂商文档公布了精确数字（MiniMax、Z.ai、Qwen 的 `1000000`，Kimi 套餐档位） | 厂商数字，并登记到 `pi-providers.json` 的 `vendor_windows` |
| pi 里没有这个 id | 厂商文档的数字；厂商也没有就不加这个候选 |
| id 带 `claude-` | 不设 |

`auto_compact` 省略即可，落盘时自动封顶 `1000000`。
