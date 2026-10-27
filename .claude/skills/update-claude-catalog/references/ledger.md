# 核对台账

核对版本和日期只记在这里，不写进 TOML、代码注释或其它文档。每次刷新完更新；只写实际重读过的来源。

| 对象 | 来源 | 上次核对 |
| --- | --- | --- |
| Claude Code 键（`keys.go`） | claude 2.1.309 二进制 + env-vars / settings-reference / model-config / mcp 文档 | 2026-10-26 |
| 官方模型（`custom.toml`） | platform.claude.com 模型总览 + claude 2.1.309 二进制 | 2026-10-26 |
| `deepseek` | api-docs.deepseek.com/quick_start/pricing + pi 目录 | 2026-09-21 |
| `zai` / `zai-cn` | docs.z.ai/devpack/overview、devpack/tool/claude + pi 目录 | 2026-09-21 |
| `kimi-cn` | platform.kimi.ai/docs/models、guide/claude-code-kimi + pi 目录 | 2026-10-26 |
| `kimi-plan` | kimi.com/code/docs/en/kimi-code/models、third-party-tools/claude-code.html + pi 目录；api.kimi.ai/coding 的 Anthropic Messages 路由 | 2026-10-26 |
| `minimax` / `minimax-cn` | platform.minimax.io 文本生成指南 + pi 目录 | 2026-10-26 |
| `qwen-plan` | help.aliyun.com 接入页与 Token Plan 总览 + pi 目录 | 2026-09-21 |
| `ant-ling` | developer.ant-ling.com 接入页（页面日期 2026-09-18） | 2026-09-21 |
| `openrouter` | openrouter.ai/api/v1/models live 目录 + pi 目录 | 2026-10-26 |
| `huggingface` | router.huggingface.co/v1/models live 目录 + pi 目录 | 2026-10-26 |
| `vercel` | pi 目录里的 anthropic-messages 模型 | 2026-10-19 |
| `fireworks` | pi 目录里的 anthropic-messages 模型 + fireworks.ai/models/fireworks/muse-spark-open（serverless 限量预览至 2026-10-29） | 2026-10-26 |
| `opencode` / `opencode-go` | opencode.ai/v2/docs/console/models/、console/go/ + pi 目录；模型表的 Anthropic Messages 端点 | 2026-10-26 |
| `xai` | docs.x.ai/developers/models（.md 全文 + grok-4.7 模型页）+ pi 目录 | 2026-09-22 |
| `mimo` / `mimo-plan` | mimo.mi.com/docs/en-US/quick-start/summary/model + pi 目录；V2.5 自 2026-10-14 自动路由至 V2.6，旧 ID 于 2026-10-21 停用 | 2026-10-19 |
| `siliconflow` | www.siliconflow.cn/models 的公开目录（92 个模型，modelName / contextLen / status）+ docs.siliconflow.cn/docs/api/messages-post | 2026-10-19 |
| 各渠道 `/v1/messages` 端点 | `scripts/probe-endpoint.mjs` | 2026-09-14 |
| `opencode` / `opencode-go` / `kimi-plan` 端点 | 无鉴权探测：Messages 路由 400/401、对照路径 404；Go 需要客户端会话头 | 2026-10-19 |
| pi 目录快照（`pi-models.json`） | pi.dev/api/models + npm `@earendil-works/pi-ai`，42 个 provider；网关采用精选候选 | 2026-10-26 |
| `meta`（待定） | api.meta.ai/v1/messages 回 Anthropic 错误信封，与对照路径不同；缺厂商接入文档 | 2026-09-21 |
| `typesafe`（待定） | pi 目录只有 provider 名，没有模型数据 | 2026-10-19 |
