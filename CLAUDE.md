# donn

Claude Code profile 管理器：Rust 单二进制 + Ratatui TUI。

## 文档分工

一件事只写在一个地方，其它地方链接过去。改东西时改它的归属文档，不要在别处再抄一份。

| 文档 | 写什么 |
|---|---|
| [README.md](README.md) | 给用户：是什么、怎么装、怎么用、数据放哪 |
| 本文 | 给改代码的人：文档分工、验证命令、代码地图、新配置该放哪 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 怎么运转：Spec → Render → Reconcile、渲染层序（唯一的优先级定义）、隔离模式、启动链路、文件安全、依赖选型 |
| [docs/RULES.md](docs/RULES.md) | 不能破的边界和验收标准 |
| [docs/GLOBAL-SETTINGS.md](docs/GLOBAL-SETTINGS.md) | 每个全局旋钮写哪个键、默认值、写入规则 |
| `config.rs` 的 `CONFIG_DOC` | `~/.donn/config.toml` 模板注释，就是用户看的配置字段文档 |
| `keys.rs` 的注释 | 每个 Claude Code 键的含义、取值、出处 |
| `.claude/skills/` | 维护流程：`update-claude-catalog` 刷新键、官方模型、渠道 preset（核对日期只记在它的 `references/ledger.md`）；`add-global-knob` 加旋钮 |

## 构建与验证

```bash
make check   # = 下面四条
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --locked
```

测试过了不算完，落盘、启动、TUI 的改动还要按 [docs/RULES.md](docs/RULES.md) 的验收一节做真实检查。

## 一句话架构

`profile.toml`（意图，不含 secret）→ `render` 纯函数算出该写的键 → `reconcile` 按 footprint 精确增删，其余字段不碰。`donn run` 和 TUI 启动前会尽力 sync 一次；没变化时文件字节、mtime、`updated_at` 都不动。细节见 ARCHITECTURE.md。

## 代码地图

- `crates/donn-core/`：无 UI 的业务层
  - `spec.rs` / `render.rs` / `reconcile.rs`：领域模型主干
  - `keys.rs`：Claude Code 键、权限/effort 枚举、模型槽位
  - `knobs.rs`：全局旋钮——`Knobs` 类型化字段与旋钮表 `BOOL_KNOBS` / `VALUE_KNOBS`
  - `config.rs`：`GlobalConfig` 的读写与 `config.toml` 模板
  - `ops/`：UI/CLI 所有写操作走的 `Donn` facade
  - `preset/mod.rs` + `preset/presets/*.toml`：内置渠道数据与模型套餐
  - `fsx.rs`：原子写、0600、跨进程锁
  - `launch.rs` / `wrapper.rs` / `session.rs` / `doctor.rs` / `proc.rs`：启动、别名、会话探测、体检、带超时的子进程
  - `secret.rs`：类型级 secret 防泄露
- `crates/donn-cli/`：Clap 命令与 Ratatui dashboard
  - `commands/`：`run`、`list`、`doctor`
  - `tui/components/`：输入框、单选/确认/提示模态、状态栏、列表
  - `tui/panes/`：profiles、detail、add、settings、doctor
  - `tui/modals/`：渠道与模型的搜索选择器（`nucleo-matcher`）、帮助页
- `crates/donn-core/tests/integration.rs`：临时 HOME 的真实文件集成测试

## 新配置该放哪

| 类型 | 归属 |
|---|---|
| 取值有限的全局开关/选项 | 一键一值的走 `knobs::BOOL_KNOBS` / `VALUE_KNOBS` 表；带条件或要成对写键的走 `knobs::Knobs` 类型化字段。步骤见 `add-global-knob` |
| 渠道特有的端点/模型/认证 | preset TOML |
| 对所有渠道生效的个人偏好 | `config.toml [defaults.env]` / `[defaults.settings]`（`env`、`permissions` 是保留键，不能写在 `[defaults.settings]`） |
| 单个 profile 的差异 | `profile.toml [intent]` |
| 大块机器/账号配置 | 用户自己放进 profile 的 `claude/`，donn 不代管 |
| donn 不可妥协的行为 | `render.rs` 常量层 |
