# donn 架构

donn 管理一个 Claude Code 二进制和多套配置。Bubble Tea TUI、Cobra CLI 和 alias 都通过 `internal/core.Donn` 访问同一份磁盘状态。

## Spec → Render → Reconcile

`profile.toml` 是 schema v2 的配置意图，包含 preset、isolation、model/env 覆盖、alias 与 footprint；API key 只存在于 profile 的 `settings.json`。`Render` 纯函数计算应有配置，`reconcile` 按上次的 footprint 清理拥有的 key，再写入新值，保留未知 JSON 字段。认证模式变化时，sync 从 settings 中恢复 secret 并写入当前认证 key。

footprint 记录上次管理的 key。用户修改这些 key 会产生 drift，详情和 Doctor 会报告；sync 恢复 spec 定义的值。生效值和来源由 core 统一计算，TUI 直接展示。

## 渲染顺序

后面的层覆盖前面的层：

1. endpoint 与需要始终参与的 knob 默认值。
2. 认证 key。
3. preset 的 model slot、env、Sonnet model package。
4. 用户显式配置的全局 knob。
5. `[defaults.env]`，以及它选中的 Sonnet package。
6. profile 的 model slot、Sonnet package、自定义 model window、`intent.env`。
7. donn 常量：`DISABLE_AUTOUPDATER=1`；shared 下的 `ENABLE_CLAUDEAI_MCP_SERVERS=0`。

model slot 依次由 preset 的 models、preset 的 env、`defaults.env`、`intent.models`、`intent.env` 覆盖；显式空 env 值也参与覆盖。列表、TUI 和 shared overlay 使用同一组生效值与来源。model package 只展开最终 Sonnet 的候选配置，放在选中该模型的配置层；全局与 profile 的显式 env 覆盖同层 package。package 的窗口、自动压缩窗口和 subagent model 由同一个函数展开，用户的 `model_windows` 使用同一规则。settings 顶层值按 knob、`[defaults.settings]` 的顺序覆盖。

`KnobCatalog` 统一定义字段、行名、类型、默认值和校验。单 key 映射由表驱动，有条件或配对写入的 `TargetSpecial` 在 render 中处理；显式 env knob 在 preset 后再应用一次。具体写入规则见 [GLOBAL-SETTINGS.md](GLOBAL-SETTINGS.md)。

## Isolation

| | full（默认） | shared |
|---|---|---|
| Claude 配置目录 | profile 的 `claude/`，通过 `CLAUDE_CONFIG_DIR` 指定 | Claude 的默认目录 |
| session、agents、CLAUDE.md | 每个 profile 独立 | 使用 `~/.claude` 中的内容 |
| provider env | settings 文件与启动进程 | 从 profile settings 注入启动进程 |
| settings、permission mode、model | profile 的 settings.json | `shared-settings.json` 通过 `--settings` 注入 |
| 删除 | 可保留 session 后同名重建 | 删除 profile，公共 session 不受影响 |

两种模式都不写 `~/.claude`。overlay 整份归 donn 所有。

## 启动

alias 记录 donn 路径并执行 `donn run <profile> -- <args>`；路径失效时在 PATH 中查找 donn。CLI 和 TUI 通过 `PrepareLaunch` 同步配置、准备 `exec.Cmd` 并获取启动提示；sync 失败会给出警告，并尝试使用现有文件启动。

启动环境先继承 shell，再剥掉 donn 管理的 env 和切换云厂商、覆盖 model 的 key，随后注入 profile settings 中的 env；full 最后强制设置 `CLAUDE_CONFIG_DIR`。shared 默认追加 `--settings <overlay>`。用户自己传入 `--settings` 时使用用户参数，并明确提示此次没有加载 donn overlay。

CLI 在 Unix 上用 exec 替换进程，Windows 上等待子进程并返回其退出码。TUI 使用 Bubble Tea 的 Exec 暂停界面，将标准输入输出交给 Claude 或编辑器，进程结束后恢复终端和界面。

## Preset

`go:embed` 将 `internal/core/presets/*.toml` 编入二进制。加载时检查文件名与 key 一致，再读 `~/.donn/presets.d/*.toml`：同 key 覆盖，其余追加，按 key 排序。用户 preset 错误单独收集，由 Doctor 报告，不阻塞其它 provider。

## 文件安全与并发

- 写操作持有 gofrs/flock 跨进程锁，最长等 5 秒。全局编辑在锁内重新读配置，只修改指定字段，然后逐 profile 同步并汇报失败。
- natefinch/atomic 写临时文件并替换目标，已存在文件保持权限，新文件使用 0600，目录使用 0700；Unix 上替换后尽力同步父目录。Windows 权限由目录 ACL 决定。
- JSON 用通用对象与 json.Number 保留未知字段和大整数。TOML 使用 go-toml 做类型映射、tomledit 保留未知顶层字段及关联注释。全局配置的模板注释在启动和保存时刷新。
- 内容相同就不写。profile 比较只忽略 updated_at，稳态 sync 保持文件字节和 mtime。
- 创建和 alias 修改失败会恢复涉及的原文件；保留 session 的目录不会被整体回滚删除。wrapper 只在内容严格匹配所有权和目标 profile 时才可删除。
- Secret 默认格式化脱敏并拒绝通用序列化，明文读取必须显式调用 Reveal；Go 字符串不承诺内存清零。

锁只约束 donn 进程。Claude 同时写 `.claude.json` 时，整文件读改写仍可能覆盖其刚完成的写入，因此 session 运行期间应避免主动同步同一个 profile。

## TUI

Bubble Tea 管理事件循环与外部进程，Bubbles 提供 list、fuzzy filter、textinput 和 viewport，Lip Gloss 负责布局与显示宽度。磁盘操作通过 Cmd 执行，返回消息后更新界面；modal 独占输入，有限选项用 picker，删除与丢弃用确认选择。

每次列表刷新从磁盘读取一次全局配置和 preset catalog，逐 profile 检查时复用本次读取的值。Doctor 同样在一次检查内复用配置和 catalog。

model picker 合并 preset 候选、slot 默认、自定义窗口和当前值。无匹配的搜索文本可作为任意 model ID；第三方 Sonnet 缺少窗口时继续询问 token 数，取消不会提前保存。provider 列表展示名称和 endpoint，model 列表展示 ID 和上下文窗口。小于 60 列时切为单面板。

鼠标选择按屏幕坐标定位 Bubbles 列表。完整显示的 URL 用 xurls 识别，左键在焦点和选中项未变化时打开，右键复制；省略的 URL 不打开。浏览器使用 cli/browser，剪贴板通过终端 OSC 52，支持情况取决于终端设置。

## Doctor 与 session

Doctor 检查 Claude 可执行文件及 10 秒内的版本响应、PATH、未知 knob、preset 解析、profile 状态、认证、drift、wrapper 与文件权限。单项错误不终止其它检查。

删除 full profile 前用 `lsof +D` 检查活动 session，3 秒超时，发现活动 session 就拒绝删除。shared 不探测公共目录；损坏 profile 按 full 保守处理。探测不可用时界面会提示，默认选择保留 session。

## 主要依赖

| 用途 | 依赖 |
|---|---|
| TUI 与组件 | Bubble Tea v2、Bubbles v2、Lip Gloss v2 |
| CLI | Cobra |
| TOML | go-toml/v2、tomledit |
| 原子文件与锁 | natefinch/atomic、gofrs/flock |
| shell quoting 与编辑器参数 | mvdan.cc/sh/v3 |
| URL 与浏览器 | xurls/v2、cli/browser |
| JSON、进程超时、嵌入资源 | Go 标准库 |

`make build` 使用 `CGO_ENABLED=0`、trimpath 和去符号参数生成可独立分发的二进制。
