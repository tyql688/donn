# 核对台账

核对版本和日期只记在这里，不写进 TOML、代码注释或其它文档。每次刷新完更新；只写实际重读过的来源。

| 对象 | 来源 | 上次核对 |
| --- | --- | --- |
| Claude Code 键（`keys.rs`） | claude 2.1.280 二进制 + env-vars / settings-reference / model-config 文档 | 2026-09-23 |
| 官方模型（`custom.toml`） | platform.claude.com 模型总览 + 二进制 | 2026-09-23 |
| `deepseek` | api-docs.deepseek.com/quick_start/pricing + pi 目录 | 2026-09-21 |
| `zai` / `zai-cn` | docs.z.ai/devpack/overview、devpack/tool/claude + pi 目录 | 2026-09-21 |
| `kimi-cn` / `kimi-plan` | platform.kimi.ai/docs/models、kimi.com/code 模型页 + pi 目录 | 2026-09-21 |
| `minimax` / `minimax-cn` | platform.minimax.io 文本生成指南 + pi 目录 | 2026-09-21 |
| `qwen-plan` | help.aliyun.com 接入页与 Token Plan 总览 + pi 目录 | 2026-09-21 |
| `ant-ling` | developer.ant-ling.com 接入页（页面日期 2026-09-18） | 2026-09-21 |
| `openrouter` | live 目录 | 2026-09-23 |
| `huggingface` | live 目录 | 2026-09-23 |
| `vercel` | pi 目录里的 anthropic-messages 模型 | 2026-09-23 |
| `fireworks` / `opencode` / `opencode-go` | pi 目录里的 anthropic-messages 模型 | 2026-09-23 |
| `xai` | docs.x.ai/developers/models（.md 全文 + grok-4.7 模型页）+ pi 目录 | 2026-09-22 |
| `mimo` / `mimo-plan` | mimo.mi.com 模型总览、V2.6 发布说明、按量价格、Token Plan 订阅页、Claude Code 接入页（页面日期 2026-09-21/22）；pi 目录的 V2.6 窗口与 preset 一致（2026-09-23） | 2026-09-22 |
| `siliconflow` | siliconflow.cn/models（需登录，没重读） | 2026-09-14 |
| 各渠道 `/v1/messages` 端点 | `scripts/probe-endpoint.mjs` | 2026-09-14 |
| pi 目录快照（`pi-models.json`） | pi.dev/api/models + npm `@earendil-works/pi-ai` | 2026-09-23 |
| `meta`（待定） | api.meta.ai/v1/messages 回 Anthropic 错误信封，与对照路径不同；缺厂商接入文档 | 2026-09-21 |
