# donn 架构

> 本文讲 donn 怎么运转：数据流、渲染层序、隔离模式、启动链路、文件安全、依赖选型。不能破的规矩在 [RULES.md](RULES.md)，每个全局旋钮写什么键在 [GLOBAL-SETTINGS.md](GLOBAL-SETTINGS.md)，代码在哪在 [CLAUDE.md](../CLAUDE.md)。

## 进程形态

donn 是一个 Rust 单二进制：无参数进入 Ratatui dashboard；`run`、`list`、`doctor` 是同一二进制的脚本接口。

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
settings.json / .claude.json            精确清旧键、写新键、未知字段穿透
```

三个角色：

- `ProfileSpec`（`profile.toml`，schema v2）记 donn 的意图：preset、覆盖、附加 env、隔离模式、别名、footprint。不含 secret——secret 只在 `settings.json`；认证模式变了，sync 会从旧认证键里把它取出来搬到新键。
- `render` 是纯函数，算出 donn 这次应该写的全部 env、settings 顶层键和 `.claude.json` 键。
- `reconcile` 拿旧 footprint 精确删掉上次写过、这次不再需要的键，写入新值，并报告被覆盖的手改。不是 donn 写的字段一个不碰。

`footprint` 只是「上次写了哪些键」的清单，用来清理，不是审计日志。手改 donn 拥有的键会形成漂移，Doctor 和详情页看得见，sync 时以 spec 为准盖回去。

### 渲染层序

后写的盖先写的。这是全项目唯一的优先级定义，展示层的「生效值」也用同一组函数（`effective_base_url` / `effective_models`）算，不另写一套。

1. 端点，和类型化旋钮的默认值（超时、agent teams）。
2. 认证键。
3. preset：模型槽 → `preset.env` → 按 preset 的 sonnet 匹配的模型套餐。
4. 用户显式拨过的全局旋钮（类型化字段，然后是表驱动旋钮）。
5. `[defaults.env]`，以及它换了 sonnet 时对应的套餐。
6. profile 的 `[intent]`：模型槽 → 套餐 → `model_windows`（用户给模型 id 定义的窗口，生效 sonnet 命中时按套餐同一套规则展开，压过 preset 的数字）→ `intent.env`。
7. donn 常量（`DISABLE_AUTOUPDATER=1`，shared 模式的 `ENABLE_CLAUDEAI_MCP_SERVERS=0`）。

模型套餐（`max_context` 等）总是跟着最终生效的 sonnet 走：高层换了 sonnet，低层套餐写的窗口键不会残留。settings 顶层键同理：旋钮 < `[defaults.settings]`。

全局旋钮在代码里分两种：一键一值的放 `BOOL_KNOBS` / `VALUE_KNOBS` 表，加一行就生效；带条件或要成对写键的（权限模式、只对有 `base_url` 的渠道注入的超时与非必要流量、agent teams、tool search）是 `knobs::Knobs` 的类型化字段。

## 隔离模式

| | `full`（默认） | `shared` |
|---|---|---|
| `CLAUDE_CONFIG_DIR` | `~/.donn/profiles/<name>/claude/` | 不设置，Claude 用 `~/.claude` |
| 会话、CLAUDE.md、agents | 每个 profile 一套 | 共用 `~/.claude` 的 |
| 渠道 env | 写进 profile 的 `settings.json`，启动时也注入进程 | 同样写进 profile 的 `settings.json`，但只为了启动时读出来注入进程 |
| settings 类旋钮、权限模式、模型选择 | 写进 profile 的 `settings.json` | sync 生成 `shared-settings.json`，启动时用 `--settings` 带进会话，压过 `~/.claude` 里的同名配置 |
| connectors | profile 自己配 | 恒注入 `ENABLE_CLAUDEAI_MCP_SERVERS=0` |
| 删除 profile | 可以保留 `claude/` 会话目录 | 会话不归 donn，只删 profile 目录 |

两种模式 donn 都不写 `~/.claude`。

## 启动链路

wrapper（`~/.local/bin/<alias>`，只做一件事：`exec donn run <profile> -- "$@"`；记下的 donn 路径失效时退回 PATH 里的 `donn`）→ `donn run` → 尽力 sync 一次 → `launch::prepare` → exec `claude`。Unix 用 `exec` 替换进程，信号和退出码直接透传；Windows 是 spawn 后等待再透传退出码。

进程环境分三步拼：

1. 继承当前 shell 的环境，但先剥掉 donn 会写的全部 env 键和 `keys::SHELL_OVERRIDE_KEYS`（`ANTHROPIC_MODEL`、`CLAUDE_CODE_USE_BEDROCK` 这类会改道或盖掉模型映射的键）。不剥的话，shell 里 export 过的同名变量会让「旋钮关掉」失效。
2. 注入 profile `settings.json` 里的 `env`。用户自己写进 `[defaults.env]` / `[intent.env]` 的键在这一步进来，不受第 1 步影响。
3. `full` 模式最后强制写 `CLAUDE_CONFIG_DIR`。

`shared` 模式唯一追加的命令行参数是 `--settings <shared-settings.json>`。Claude 的 `--settings` 只认一个值，所以用户自己传了 `--settings` 时以用户的为准，donn 的不再追加，并在 stderr 提示这次会话没带上 profile 的模型、权限模式和全局设置。

## Preset 加载

内置 preset 编译进二进制（`preset/presets/*.toml`），启动时再读 `~/.donn/presets.d/*.toml`，同 key 覆盖内置，其余追加；全部按 key 字典序排列。解析失败的用户 preset 记入 `load_errors`，Doctor 报告，不影响其它渠道。

## Doctor 与会话探测

Doctor 逐项检查：claude 二进制可执行且 `--version` 在 10 秒内返回、wrapper 目录在 PATH、`[defaults.knobs]` 里没有 donn 不认识的键、用户 preset 可解析、每个 profile 的 `profile.toml` 可读、`settings.json` 可解析且与渲染值无漂移、认证键存在、preset 存在、每个 wrapper 存在且指向本 profile。单项失败不中断。

删除 profile 前用 `lsof +D <config_dir>`（3 秒超时）探测运行中会话；不可用或超时按 `Unavailable` 处理，TUI 改用二次确认文案。`shared` 模式的会话跑在 `~/.claude`，不探测。

## 并发与文件安全

- 所有写操作用 `~/.donn/.write.lock` 跨进程串行；标准库 `File::try_lock` 最多等待约 5 秒，失败返回明确错误。
- JSON/TOML 经临时文件 + rename 原子替换（rename 后尽力 fsync 目录项）；新建含 secret 的 JSON 权限为 `0600`，已有文件 mode 保留；Windows 无 POSIX mode，依赖 `~/.donn` 目录权限。
- 全局配置每次调用现场重读，长期运行 TUI 不缓存旧配置；配置写锁内再次合并，避免另一进程的独立改动丢失。
- 没有变化就不写盘：生成结果和现有文件深比较，`profile.toml` 的比较只忽略 `updated_at`。`shared-settings.json` 整份归 donn，读不出来就直接重写。
- 已知限制：sync 对 `.claude.json` 是整文件读、改、写。锁只管得住 donn 自己的进程，管不住运行中的 Claude 写同一个文件，极小的时间窗口里 Claude 刚写的内容可能被盖掉。会话进行中尽量别对同一个 profile 手动 sync。

## TUI 交互层

Ratatui/crossterm 负责终端底座；`tui-input` 负责 Unicode 输入编辑；`nucleo-matcher` 负责 provider/model 模糊搜索。每帧画完后 `tui/links.rs` 扫一遍屏幕缓冲区，记下完整显示的 http(s) 链接占哪些格子：右键复制、左键打开都查这张表，面板不用各自登记链接；被省略号截断的不算。左键先按普通点击处理，这一下没有改变选中或焦点才打开链接，所以点一行来选中它不会顺手弹出浏览器。有限选项统一走 `Select`，破坏操作统一走 `Confirm` 或带明确语义的 `Select`，所有弹窗在单一 modal 栈里独占按键。

模型 picker 的候选来自 `model_choices`、preset 各槽默认值和当前值。去重后按列显示短 label 与真实 ID，当前项的套餐/context 单独放在底部详情行；输入未命中时作为自由模型 ID。provider 概览中的候选模型逐行显示 ID，不拼成长段文字。单槽编辑只修改目标槽，套餐 env 由 render 根据最终 Sonnet ID 推导。

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
| JSON | `serde_json` `preserve_order` | 未知字段与稳定键序 |
| secret | `secrecy` | 明文暴露点显式、drop 清零 |
| URL | `open` | 使用系统默认应用，不手写平台分支 |
| 找链接 | `linkify` | 从屏幕文本里找 URL，边界（括号、标点）处理正确，不自写正则 |
| 文本剪贴板 | `arboard` | 跨平台文本复制；关闭图片 feature，Linux 启用 Wayland data-control |
| 进程超时 | `wait-timeout` | `lsof`/`claude --version` 不无限挂起 |
| shell 参数 | `shlex` | wrapper 与 `$EDITOR` 参数安全解析 |

Release 使用 LTO、单 codegen unit、strip 与 panic abort。
