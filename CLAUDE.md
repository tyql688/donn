# donn

Claude Code profile 管理器：Go 单二进制与 Bubble Tea TUI。

## 文档分工

| 文档 | 内容 |
|---|---|
| [README.md](README.md) | 安装、使用、快捷键和磁盘布局 |
| 本文 | 代码地图与开发入口 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 配置层序、isolation、启动与文件安全 |
| [docs/RULES.md](docs/RULES.md) | 代码边界与运行验收 |
| [docs/GLOBAL-SETTINGS.md](docs/GLOBAL-SETTINGS.md) | 各 knob 的默认值、目标 key 与写入条件 |
| `internal/core/config-template.toml` | 用户配置文件中的字段说明 |
| `.claude/skills/` | knob 与 provider 目录维护流程 |

## 构建与验证

需要 Go 1.26+。`make check` 检查 gofmt、运行 go vet 并构建 `bin/donn`。`make run` 打开 TUI，`make run ARGS="doctor"` 运行子命令。

运行验收使用临时目录中的独立 `HOME` 和 `DONN_HOME`。检查真实落盘文件、常规和窄尺寸 PTY，并用假的 Claude 可执行文件核对环境变量、参数和退出码。不要用真实用户配置或发出真实模型请求；不添加测试代码。

## 代码地图

| 位置 | 职责 |
|---|---|
| `cmd/donn/main.go` | 进程入口与退出码 |
| `internal/cli/cli.go` | Cobra 命令、list 表格、Doctor 输出 |
| `internal/core/types.go` | schema v2、model slot、Secret 与来源元数据 |
| `internal/core/keys.go` | Claude key、枚举与启动环境变量过滤 |
| `internal/core/knobs.go` | KnobCatalog：字段、行名、类型、默认值与校验 |
| `internal/core/render.go` | 纯配置渲染、生效值与来源 |
| `internal/core/reconcile.go` | 按 footprint 合并 JSON 与检测 drift |
| `internal/core/store.go` | Donn：创建、同步、编辑、alias、删除与全局更新 |
| `internal/core/config.go` | TOML 配置、profile 读写、自由 settings 值转换 |
| `internal/core/files.go` | Home 路径、原子写、锁、保留未知字段的 JSON/TOML 操作 |
| `internal/core/preset.go`、`presets/*.toml` | go:embed provider 数据、用户覆盖与 model package |
| `internal/core/wrapper.go` | alias 所有权识别与脚本生成 |
| `internal/core/launch.go`、`exec_*.go` | 启动准备、进程命令与平台执行方式 |
| `internal/core/doctor.go` | 体检与有超时的 session 探测 |
| `internal/tui/model.go` | Bubble Tea 状态、消息、按键分发与异步任务 |
| `internal/tui/view.go` | Lip Gloss 布局、尺寸、鼠标与链接 |
| `internal/tui/modal.go` | Bubbles 选择器、搜索、输入与滚动内容 |
| `internal/tui/actions.go`、`form.go` | 详情、全局设置、新建表单与操作 |
| `internal/tui/process.go` | TUI 挂起、外部进程与终端恢复 |

## 配置归属

取值有限的全局设置登记进 `KnobCatalog`。单 key 映射用 `TargetEnv` / `TargetSetting`，有条件或配对写入的用 `TargetSpecial` 并在 render 中实现。新增或修改 knob 时同步更新配置模板与全局设置表。

provider 差异放 preset TOML，个人全局偏好放 `[defaults.env]` / `[defaults.settings]`，单 profile 差异放 `[intent]`。`env`、`permissions` 是保留 settings key。donn 常量由 render 的最后一层写入。
