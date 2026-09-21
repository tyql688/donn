---
name: update-claude-catalog
description: 刷新 donn 的三份数据面：Claude Code 键注册表（keys.rs）、Anthropic 官方模型 id、第三方渠道 preset。Claude Code 升级、厂商出新模型、改 id/端点、新增渠道都走这里。
when_to_use: 触发词 - claude 升级、键漂移、keys.rs、CLAUDE_CODE_*、settings.json key、Claude 新模型、官方模型 id、ModelSlot、新增渠道、加 provider、模型 id 变了、渠道下架模型、preset、model_choices、base_url、context window、刷新模型目录、根据 skills 更新资源。
allowed-tools: Bash(claude --version), Bash(rg *), Bash(grep *), Bash(strings *), Bash(node *), Bash(curl -sL https://code.claude.com/*), Bash(curl -sL https://platform.claude.com/*), Bash(curl -s https://openrouter.ai/api/v1/models), Bash(curl -s https://router.huggingface.co/v1/models)
---

# 刷新 Claude 目录

按 1 → 2 → 3 执行，分别汇报。键先定基线，模型槽位和 preset 才知道该写什么。查不到出处的差异不改。做完更新 [references/ledger.md](references/ledger.md)——核对版本和日期只记在那里，不写进 TOML、代码注释或其它文档。

本目录的文件：

- [references/ledger.md](references/ledger.md)：每个对象上次核对的来源和日期。开工前读，收工后改。
- [references/pi-providers.json](references/pi-providers.json)：pi 的每个 provider 在 donn 这边怎么处置——接到哪个 preset、为什么跳过、还是待定；以及登记过的例外（`ignore` 不收的 id、`vendor_windows` 以厂商数字为准的窗口）。脚本读它，做了新决定就改它。
- `references/pi-models.json`：上次对账时录下的 pi 目录快照（provider → 模型 id → 协议、窗口）。只给脚本 diff 用，不用读。
- [references/preset-fields.md](references/preset-fields.md)：preset TOML 字段含义和 `max_context` 填法。增删候选或写新 preset 时读。
- `scripts/pi-sync.mjs`：pi 目录对账、生成候选 TOML、录快照。
- `scripts/key-context.mjs`：看键在 claude 二进制里的上下文。
- `scripts/probe-endpoint.mjs`：探测端点有没有 Anthropic Messages 路由。

下面的命令都从仓库根目录跑，`S=.claude/skills/update-claude-catalog`。

## 1. Claude Code 键（`crates/donn-core/src/keys.rs`）

键的用途、取值和出处只写在 `keys.rs` 常量注释。每个键标 `[官方]`、`[官方·未在册]`（二进制有、文档无）或 `[官方·二进制 schema]`；后两类升级时优先复查。用户自定义 env 不入册。

```bash
claude --version
strings -a "$(readlink -f "$(command -v claude)")" > /tmp/donn-claude-strings.txt
rg -cF 'CLAUDE_CODE_MAX_CONTEXT_TOKENS' /tmp/donn-claude-strings.txt   # keys.rs 里每个键都数一遍，0 必须解释
node $S/scripts/key-context.mjs effortLevel alwaysThinkingEnabled

# 官方文档抓 Markdown 原文再 grep，不用摘要
curl -sL https://code.claude.com/docs/en/env-vars.md -o /tmp/donn-envvars.md
curl -sL https://code.claude.com/docs/en/settings-reference.md -o /tmp/donn-settings-ref.md
curl -sL https://code.claude.com/docs/en/model-config.md -o /tmp/donn-model-config.md
grep -nF '`CLAUDE_CODE_AUTO_COMPACT_WINDOW`' /tmp/donn-envvars.md
n=$(grep -n '^### `alwaysThinkingEnabled`' /tmp/donn-settings-ref.md | cut -d: -f1); sed -n "$n,$((n+20))p" /tmp/donn-settings-ref.md
```

逐项复查：

- `effortLevel` 档位仍是 low/medium/high/xhigh；`maxEffortLevel` 仍含 `max`；`CLAUDE_CODE_EFFORT_LEVEL` 仍含 `max`
- `permissions.defaultMode` 枚举与别名
- 布尔键的默认值有没有翻转（`alwaysThinkingEnabled` 只有 `false` 有效）：键名不变，旋钮语义要跟着变
- `ENABLE_TOOL_SEARCH` 不设时的行为（官方主机 defer、第三方端点 upfront）和合法取值
- `CLAUDE_CODE_AUTO_COMPACT_WINDOW` 取值范围（`100000..=1000000`）、`API_TIMEOUT_MS` 上限
- 未在册键是否转正或消失
- `keys::SHELL_OVERRIDE_KEYS`：有没有新的「换云厂商 / 盖模型」类 env 要加进启动剥除名单

改动位置：常量与注释在 `keys.rs`；写入层在 `render.rs`（层序见 `docs/ARCHITECTURE.md`，新键插入哪层要用测试锁定）；新旋钮转 `add-global-knob`。

## 2. Anthropic 官方模型

来源：`https://platform.claude.com/docs/en/about-claude/models/overview.md` 与本地二进制，两者都有才写。

```bash
rg -o 'claude-(fable|mythos|opus|sonnet|haiku)-[0-9a-z.-]*' /tmp/donn-claude-strings.txt | sort | uniq -c | sort -rn
```

- `[1m]` / `[2m]` 是 Claude Code 客户端形式，不是 API 模型 id；官方候选不带后缀，也不设 `max_context`。
- `presets/custom.toml`：只维护 `[[preset.model_choices]]`，不设 `[preset.models]`。当代模型加总览页「Legacy models (still available)」里的主力。
- `presets/openrouter.toml`：`anthropic/*` 用 OpenRouter live 目录的 slug 与 context，过滤 `:batch` / `-fast` 变体。
- `presets/official.toml`：OAuth 直连，不写模型槽。
- 新档位：改 `keys.rs` 的 `ModelSlot`、`ALL`、`env_key`、`label` 与 `SlotMap` 字段，TUI 与 render 都迭代 `ModelSlot::ALL`；新 `ANTHROPIC_DEFAULT_<TIER>_MODEL` 先按第 1 节入册。
- 退役：只删候选，不改用户 `profile.toml [intent.models]`。

## 3. 第三方渠道 preset

只动 `crates/donn-core/src/preset/presets/<key>.toml`，一渠道一文件；新渠道 = 一个新 TOML，零行代码。用户可在 `~/.donn/presets.d/` 放同 key 文件覆盖内置。

模型 id 和窗口数字不手抄。来源是 pi 的模型目录：pi 自己刷新模型用的接口 `https://pi.dev/api/models`，加上刚进 npm 包、接口里还没有的 provider。脚本直接联网取，不依赖本机装没装 pi。

```bash
node $S/scripts/pi-sync.mjs                # 对账，只读
node $S/scripts/pi-sync.mjs vercel         # 网关渠道默认只报数，点名才展开
node $S/scripts/pi-sync.mjs --emit zai     # 打印 zai 还缺的候选：现成 TOML 块，id / label / max_context 全来自 pi
node $S/scripts/pi-sync.mjs --record       # 全部处理完，把线上目录录成新快照
```

对账输出三段，逐段清零：

1. **pi 目录相对快照的变化**：新 provider、新模型、下架的模型、窗口或协议变了的模型。这一段告诉你这次要看什么。某个 provider 新出现 `anthropic-messages` 协议 = 以前接不了现在能接了。
2. **没登记处置的 provider**：pi 新增了渠道。按下一小节判断能不能接，然后在 `references/pi-providers.json` 里登记 `preset`、`skip`（写理由）或 `pending`（写清还缺什么）。不登记脚本会一直报。
3. **已接渠道**：`+` 是 pi 有、候选里没有的模型；`≠` 是 preset 的 `max_context` 和 pi 不一致。

怎么处理第 3 段：

- 厂商直连和订阅渠道：`+` 的都加，用 `--emit` 的输出贴进 TOML。宁多勿少，选择器带搜索。确实不该收的（别名、没核实端点是否提供的）在 `pi-providers.json` 给该 provider 登记 `ignore` 正则和 `ignore_reason`，不要留着不管。
- 网关渠道（`pi-providers.json` 里 `gateway: true`）：目录上百个，只挑主力编码模型，其余用户自由输入。脚本只看 `api` 为 `anthropic-messages` 的模型，同一网关上走 OpenAI 协议的 Claude Code 接不了。`openrouter`、`huggingface` 再对一遍各自的 live 目录（`https://openrouter.ai/api/v1/models`、`https://router.huggingface.co/v1/models`），下架的 slug 要删。
- `≠`：厂商文档公布了精确窗口就以厂商为准，并登记到 `vendor_windows`（写出处）；厂商只说「1M」这类约数就改成 pi 的数字。
- 非 Claude 的候选一律要有 `max_context`（有测试把关）：Claude Code 不认识第三方 id，不给就按它猜的窗口压缩。id 里带 `claude-` 的不设，官方文档写明对能解析成 Claude 模型的 id 该变量不生效。

pi 答不了、只能看厂商文档的：`[preset.models]` 默认槽位该用哪个主力模型、`[1m]` 这类 Claude Code 专用 id（和裸 API id 不同时注释写清，如 Kimi `k3[1m]` / `k3`）、认证模式（`ANTHROPIC_AUTH_TOKEN` 与 `ANTHROPIC_API_KEY` 都要求时开 `auth_token_also_sets_api_key`）、`key_url`、flags。套餐总览页和 Claude Code 接入页都读，接入页可能落后一代。厂商退役 id 且自动路由时直接换新 id，用户钉住的旧 id 靠路由继续可用。preset 头部注释只留文档 URL 和不看就会踩的坑。

`pi-providers.json` 的 `presets_not_in_pi`（`siliconflow` 等）pi 里没有，只能对厂商目录。

### 新渠道能否接 Claude Code

Claude Code 只说 Anthropic Messages 协议。按序判定，查到即止：

1. pi 目录里该 provider 有 `api` 为 `anthropic-messages` 的模型：能接，`baseUrl` 在数据里。
2. pi 只记了 OpenAI 协议不代表不能接（deepseek 就是，厂商实有 `/anthropic` 端点）。实测：`node $S/scripts/probe-endpoint.mjs https://api.x.ai`，判读方法写在脚本头部。
3. 能接但缺厂商接入文档（认证头、`key_url` 不确定）：登记 `pending`，不凭猜测建 preset。
4. 原生没有就走网关：把它的主力模型加进 `vercel` / `openrouter` 候选。Cloudflare AI Gateway 和本地翻译代理用 `custom` preset，donn 不管理代理进程。

新建 preset 后：`preset/mod.rs` 的 `builtin_presets_parse_and_cover_launch_list` 加 key，`pi-providers.json` 登记映射，台账加一行。

## 验收

`make check`，再按 `docs/RULES.md` 的验收一节做真实落盘检查。本 skill 特有的：

- `node $S/scripts/pi-sync.mjs` 三段都清零（或只剩登记过的待定项），然后 `--record`。
- 改了键、端点或 auth 模式：用 `/tmp` 独立 `DONN_HOME` 生成真实 `settings.json` / `.claude.json`，检查存在/移除条件、footprint 与稳态 sync no-op。
- 改了模型候选：打开真实 TUI 看详情与新建表单的模型选择器。
