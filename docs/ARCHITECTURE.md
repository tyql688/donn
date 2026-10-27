# donn 架构

donn 管理同一个 Claude Code 二进制和多套互相隔离的配置。它不复制 Claude Code，也不改官方客户端，只维护每个 profile 该有哪些设置，并在启动前把它们写进 Claude Code 会读的文件。

本文讲它怎么运转：进程怎么组织、配置从意图到落盘经过哪几层、两种 isolation 有什么区别、启动时发生什么，以及写文件怎么保证安全。

## 进程形态

donn 是一个 Rust 单二进制。无参数进入 Ratatui dashboard；`run`、`list`、`doctor` 是同一二进制的脚本接口。

```text
Ratatui TUI ─┐
Clap CLI ────┼──> donn_core::ops::Donn ──> ~/.donn
wrappers ────┘              │
                            └──> exec Claude
```

## Spec → Render → Reconcile

```text
profile.toml (ProfileSpec)             意图唯一来源；永不含 secret
        │
        ▼ render(spec, preset, secret, defaults)
Rendered                                有序的 donn 应然配置
        │
        ▼ reconcile(existing, old footprint, rendered)
settings.json / .claude.json            精确清旧 key、写新 key、未知字段穿透
```

三个角色：

- `ProfileSpec`（`profile.toml`，schema v2）记录 donn 的意图：preset、覆盖、附加 env、isolation、alias、footprint。它不含 secret；secret 只在 `settings.json`。认证模式变了，sync 会从旧认证 key 里把 secret 取出来搬到新 key。
- `render` 是纯函数，算出 donn 这次应该写的全部 env、settings 顶层 key 和 `.claude.json` key。
- `reconcile` 拿旧 footprint 精确删掉上次写过、这次不再需要的 key，写入新值，并报告被覆盖的手改。不是 donn 写的字段不碰。

`footprint` 只是「上次写了哪些 key」的清单，用来清理，不是审计日志。手改 donn 拥有的 key 会形成 drift，Doctor 和详情页能看见，sync 时以 spec 为准盖回去。

### 渲染层序

后写的盖先写的。这是全项目唯一的优先级定义，展示层的「生效值」也用同一组函数（`effective_base_url` / `effective_models`）算，不另写一套。

1. endpoint，以及类型化 knob 的默认值（超时、agent teams）。
2. 认证 key。
3. preset：model slot → `preset.env` → 按 preset 的 sonnet 匹配的 model package。
4. 用户显式拨过的全局 knob（类型化字段，然后是表驱动 knob）。
5. `[defaults.env]`，以及它换了 sonnet 时对应的 package。
6. profile 的 `[intent]`：model slot → package → `model_windows`（用户给 model id 定义的窗口，生效 sonnet 命中时按 package 同一套规则展开，压过 preset 的数字）→ `intent.env`。
7. donn 常量（`DISABLE_AUTOUPDATER=1`，shared 模式的 `ENABLE_CLAUDEAI_MCP_SERVERS=0`）。

model package（`max_context` 等）总是跟着最终生效的 sonnet 走：高层换了 sonnet，低层 package 写的窗口 key 不会残留。settings 顶层 key 同理：knob < `[defaults.settings]`。

全局 knob 在代码里分两种。一个 key 对应一个值的放 `BOOL_KNOBS` / `VALUE_KNOBS` 表，加一行就生效；带条件或要成对写 key 的（permission mode、只对有 `base_url` 的 provider 注入的超时与非必要流量、agent teams、tool search）是 `knobs::Knobs` 的类型化字段。类型化字段由 `knobs::TypedKnob::ALL` 统一描述字段名、分组、默认值和适用条件，设置面板、合并、校验与两遍渲染都遍历这张表，不各自列举字段名。

## isolation

| | `full`（默认） | `shared` |
|---|---|---|
| `CLAUDE_CONFIG_DIR` | `~/.donn/profiles/<name>/claude/` | 不设置，Claude 用 `~/.claude` |
| session、CLAUDE.md、agents | 每个 profile 一套 | 共用 `~/.claude` 的 |
| provider env | 写进 profile 的 `settings.json`，启动时也注入进程 | 同样写进 profile 的 `settings.json`，但只为了启动时读出来注入进程 |
| settings 类 knob、permission mode、model 选择 | 写进 profile 的 `settings.json` | sync 生成 `shared-settings.json`，启动时用 `--settings` 带进 session，压过 `~/.claude` 里的同名配置 |
| connectors | profile 自己配 | 恒注入 `ENABLE_CLAUDEAI_MCP_SERVERS=0` |
| 删除 profile | 可以保留 `claude/` session 目录 | session 不归 donn，只删 profile 目录 |

两种模式 donn 都不写 `~/.claude`。

## 启动链路

wrapper（`~/.local/bin/<alias>`，只做一件事：`exec donn run <profile> -- "$@"`；记下的 donn 路径失效时退回 PATH 里的 `donn`）→ `donn run` → 尽力 sync 一次 → `launch::prepare` → exec `claude`。Unix 用 `exec` 替换进程，信号和退出码直接透传；Windows 是 spawn 后等待再透传退出码。

进程 env 分三步拼出来：

1. 先继承当前 shell 的 env，然后剥掉 donn 会写的所有 env key，以及 `keys::SHELL_OVERRIDE_KEYS` 里那批会改道到别的云厂商或盖掉 model 映射的 key（`ANTHROPIC_MODEL`、`CLAUDE_CODE_USE_BEDROCK` 等）。不剥的话，shell 里 export 过的同名变量会让「knob 关掉」这个动作失效。
2. 注入 profile `settings.json` 里的 `env`。用户自己写进 `[defaults.env]` / `[intent.env]` 的 key 在这一步进来，不受第 1 步影响。
3. `full` 模式最后强制写入 `CLAUDE_CONFIG_DIR`。

`shared` 模式唯一追加的命令行参数是 `--settings <shared-settings.json>`。Claude 的 `--settings` 只认一个值，所以用户自己传了 `--settings` 时以用户的为准，donn 的不再追加，并在 stderr 提示这次 session 没带上 profile 的 model、permission mode 和全局设置。

## Preset 加载

内置 preset 编译进二进制；清单由 `build.rs` 从 `preset/presets/*.toml` 生成，文件名和 `key` 字段不一致时构建失败。启动时再读 `~/.donn/presets.d/*.toml`：key 相同的覆盖内置版本，其余追加，最后按 key 字典序排列。用户写的 preset 如果解析失败，会记进 `load_errors` 由 Doctor 报告，不影响其它 provider 正常使用。

## Doctor 与 session 探测

Doctor 逐项检查下面这些，单项失败不会中断其它检查：claude 二进制可执行、`--version` 在 10 秒内返回；wrapper 所在目录在 PATH 上；`[defaults.knobs]` 里没有 donn 不认识的 key；用户 preset 都能解析；每个 profile 的 `profile.toml` 可读；`settings.json` 可解析且与渲染结果无 drift；认证 key 存在；preset 存在；每个 wrapper 存在并指向本 profile。

删除 profile 之前，donn 用 `lsof +D <config_dir>`（3 秒超时）探测这个 profile 自己的 `claude/` 目录有没有正在运行的 session。lsof 不可用或超时，按 `Unavailable` 处理，`full` 模式在状态栏提示探测失败。`shared` 模式的 session 跑在 `~/.claude` 里，donn 不探测 profile 自己的 `claude/` 目录；`profile.toml` 读不出来时仍按保守策略探测。

## 并发与文件安全

- 所有写操作都在 `~/.donn/.write.lock` 这把跨进程 lock 里串行执行。lock 用标准库的 `File::try_lock`，最多等约 5 秒，拿不到就报明确的错误，不会静默继续。
- JSON 和 TOML 都先写临时文件再 rename 替换，读的人永远看到完整文件；rename 之后尽力 fsync 目录项。新建的、含 secret 的 JSON 权限是 `0600`，已存在的文件保持原 mode。Windows 没有 POSIX mode，靠 `~/.donn` 目录本身的权限兜底。
- 全局配置每次调用都重新读盘，长期开着的 TUI 不会缓存旧配置。写 lock 内会再做一次合并，避免另一个进程的独立改动被覆盖。
- 没有变化就不写盘：生成结果先和现有文件深比较，`profile.toml` 的比较只忽略 `updated_at`。`shared-settings.json` 整份归 donn 所有，读不出来就直接重写。
- 已知限制：sync 对 `.claude.json` 是整文件读、改、写。lock 只能管住 donn 自己的进程，管不住正在运行的 Claude 同时写这个文件，极小的时间窗口里 Claude 刚写进去的内容可能被盖掉。session 进行中尽量别对同一个 profile 手动 sync。

## TUI 交互层

Ratatui 和 crossterm 负责终端底座，`tui-input` 负责 Unicode 输入编辑，`nucleo-matcher` 负责 provider 和 model 的模糊搜索。每帧画完之后，`tui/links.rs` 会扫一遍屏幕缓冲区，记下完整显示出来的 http(s) 链接各占哪些格子；右键复制和左键打开都查这张表，各个面板不用自己登记链接，被省略号截断的链接不算。左键先按普通点击处理，只有这一下没有改变选中或焦点时才打开链接，所以点一行来选中它不会顺手弹出浏览器。有限选项统一用 `Select` 控件，破坏性操作统一用 `Confirm` 或语义明确的 `Select`，所有弹窗挂在同一个 modal 栈上独占键盘输入。

model picker 的候选来自 `model_choices`、preset 各 slot 的默认值、用户给自定义 model 定义过窗口的条目，以及当前覆盖值。去重后按列显示短 label 和真实 ID，当前选中项的 package 和上下文窗口单独放在底部详情行。当前值不在目录里时也会作为候选项出现并保持选中，重新打开不会跳回 Follow。输入没有命中任何候选时，输入的文本直接当作自由 model ID 使用。provider 概览里的候选 model 逐行显示 ID，不拼成长段文字。编辑单个 slot 只改那一个 slot，package 相关的 env 由 render 根据最终生效的 Sonnet ID 推导出来。

## 主要依赖

| 用途 | 依赖 | 选择理由 |
|---|---|---|
| TUI | `ratatui` + `crossterm` | 成熟、跨平台终端生态 |
| 宽度截断/补齐 | `unicode-truncate` + `unicode-width` | 按显示宽度处理 CJK，不自写字符循环 |
| 输入 | `tui-input` | Unicode 光标/编辑，不自造文本框 |
| 模糊搜索 | `nucleo-matcher` | Helix 同源的成熟 matcher |
| CLI | `clap` | 子命令、帮助与透传边界清晰 |
| 表格 | `comfy-table` | `donn list` 非 TTY 输出 |
| TOML | `toml_edit` | serde 映射并保留未知顶层字段 |
| JSON | `serde_json` `preserve_order` | 未知字段与稳定 key 序 |
| secret | `secrecy` | 明文暴露点显式、drop 时清零 |
| URL | `open` | 使用系统默认应用，不手写平台分支 |
| 找链接 | `linkify` | 从屏幕文本里找 URL，边界（括号、标点）处理正确，不自写正则 |
| 文本剪贴板 | `arboard` | 跨平台文本复制；关闭图片 feature，Linux 启用 Wayland data-control |
| 进程超时 | `wait-timeout` | `lsof` / `claude --version` 不无限挂起 |
| shell 参数 | `shlex` | wrapper 与 `$EDITOR` 参数安全解析 |

release 构建使用 LTO、单 codegen unit、strip 与 panic abort。
