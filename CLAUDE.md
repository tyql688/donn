# donn

Claude Code profile 管理器：Rust 单二进制 + Ratatui TUI。

## 文档分工

每个事实只写一个地方。下表说明每份文档负责什么，需要细节时直接看对应文件，不要在别处再抄一份。

| 文档 | 谁读，写什么 |
|---|---|
| [README.md](README.md) | 用户读的：donn 是什么、怎么安装、怎么用、数据放哪 |
| 本文 | 改代码的人读的：文档分工、验证方式、代码地图、新增配置该放哪 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 改代码的人读的：配置从意图到落盘的链路、渲染优先级、两种 isolation、启动流程、写文件的安全保证、依赖选型 |
| [docs/RULES.md](docs/RULES.md) | 改代码的人读的：不能破的边界，以及什么算验收通过 |
| [docs/GLOBAL-SETTINGS.md](docs/GLOBAL-SETTINGS.md) | 加 knob 或改 knob 语义的人读的：每个全局设置写哪个 Claude Code key、默认值、写入条件 |
| `config.rs` 的 `CONFIG_DOC` | 用户读的：`~/.donn/config.toml` 模板里的字段注释 |
| `keys.rs` 的常量注释 | 改 key 相关逻辑的人读的：每个 Claude Code key 的含义、取值、是否在官方文档在册 |
| `.claude/skills/` | 维护流程：`update-claude-catalog` 刷新 key、官方 model 和 provider preset（按版本核对的日期只记在它的 `references/ledger.md`）；`add-global-knob` 新增 knob |

## 构建与验证

`make check` 是唯一的提交前门槛，依次跑格式检查、Clippy（warning 即 error）和 release 构建。不通过就不能提交。

构建通过只是最低要求。涉及落盘逻辑的改动，要对照真实生成的文件看一眼；涉及 TUI 的改动，要在真实 PTY 里打开对应的搜索框、选择框、输入框和确认框走一遍；涉及启动流程的改动，要用一个假的 Claude 程序验证 env、命令行参数和退出码都正确传递。验收细节见 [docs/RULES.md](docs/RULES.md)。

## 架构

`profile.toml`（意图，不含 secret）→ `render` 纯函数算出应该写哪些 key → `reconcile` 按 footprint 精确增删，其余字段不碰。`donn run` 和 TUI 启动前会尽力 sync 一次；没有变化时，文件字节、mtime 和 `updated_at` 都不动。细节见 [ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 代码地图

- `crates/donn-core/`：无 UI 的业务层
  - `spec.rs` / `render.rs` / `reconcile.rs`：domain model 主干
  - `keys.rs`：Claude Code key、permission/effort 枚举、model slot
  - `knobs.rs`：全局 knob，`Knobs` 类型化字段与 `BOOL_KNOBS` / `VALUE_KNOBS` 表
  - `config.rs`：`GlobalConfig` 读写与 `config.toml` 模板
  - `ops/`：UI 和 CLI 所有写操作经过的 `Donn` facade
  - `preset/mod.rs` + `preset/presets/*.toml` + `build.rs`：内置 provider 数据、清单生成与 model package
  - `home.rs` / `claude.rs`：`~/.donn` 磁盘布局；Claude 配置 JSON 读写与 `settings.json` 只读视图
  - `fsx.rs`：原子写、0600、跨进程 lock
  - `launch.rs` / `wrapper.rs` / `session.rs` / `doctor.rs` / `proc.rs`：启动、alias、session 探测、体检、带超时的子进程
  - `secret.rs`：类型级 secret 防泄漏
- `crates/donn-cli/`：Clap 命令与 Ratatui dashboard
  - `commands/`：`run`、`list`、`doctor`
  - `tui/app.rs` / `tui/keymap.rs`：dashboard 状态与 action 分发；按键到 `Action` 的映射
  - `tui/components/`：输入框、单选/确认/提示 modal、状态栏、列表
  - `tui/panes/`：profiles、detail、add、settings、doctor
  - `tui/modals/`：provider 和 model 搜索选择器（`nucleo-matcher`）、帮助页

## 新增配置该放哪

| 类型 | 归属 |
|---|---|
| 取值有限的全局开关/选项 | 一个 key 对应一个值走 `knobs::BOOL_KNOBS` / `VALUE_KNOBS` 表；带条件或要成对写 key 的走 `knobs::Knobs` 类型化字段。步骤见 `add-global-knob` |
| provider 特有的 endpoint/model/认证 | preset TOML |
| 对所有 provider 生效的个人偏好 | `config.toml [defaults.env]` / `[defaults.settings]`（`env`、`permissions` 是保留 key，不能写在 `[defaults.settings]`） |
| 单个 profile 的差异 | `profile.toml [intent]` |
| 大块机器/账号配置 | 用户自己放进 profile 的 `claude/`，donn 不代管 |
| donn 不可妥协的行为 | `render.rs` 常量层 |
