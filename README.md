# donn

管理 Claude Code profile 的 Go 单二进制工具，使用 Bubble Tea TUI。每个 profile 有自己的配置目录、model 映射和 API key，可以在官方 Claude 与 Kimi、MiniMax、Z.ai、OpenRouter、DeepSeek 等 Anthropic 兼容 provider 之间切换。

```bash
donn        # 打开 TUI dashboard
kimi        # 用创建 profile 时生成的 alias 启动 Claude
```

## 功能

- TUI 覆盖 profile 列表和详情、provider 搜索、新建表单、可搜索的 model picker、全局设置和 Doctor 体检。有限选项用选择框，破坏性操作用确认框。
- 内置 23 个 provider。要加 provider 或改内置数据，往 `~/.donn/presets.d/` 放一个 TOML 文件。
- 全局设置包含 permission mode、effort、thinking、response language、max output tokens、第三方 endpoint 兼容和 model fallback 等。改一次对所有 profile 生效。大多数 knob 的值等于 Claude Code 默认值时不写对应 key；个别例外（例如 api_timeout_ms 恒写生效值）见 docs/GLOBAL-SETTINGS.md。
- 两种 isolation：`full`（默认）给每个 profile 独立的 Claude 配置目录和 session；`shared` 继续用 `~/.claude` 里的 session 和文件，只在启动时换 provider、model 和全局设置。
- `full` 模式下删除 profile 时可以选择保留 session，以后同名重建还能接着用；也可以整个删掉。
- API key 只放在 profile 自己的 `settings.json`，新文件权限 `0600`，不会出现在日志、`profile.toml` 或错误信息里。
- 手改 donn 管理的 key 会形成 drift。donn 能检出，下次启动前按 `profile.toml` 改回去。
- 除 TUI 外，还有 `run`、`list`、`doctor` 三个命令，适合脚本调用。

## 从当前源码安装

需要 Go 1.26 或更新版本。

```bash
go install ./cmd/donn
```

本地开发直接运行 `go run ./cmd/donn`。`make build` 生成不依赖 C 运行时的 `bin/donn`，可直接复制到目标机器；交叉编译通过 `GOOS` 和 `GOARCH` 指定目标系统与架构。

Makefile 目标：

```bash
make install                 # 从当前源码安装 donn
make run                     # 不安装，直接打开 TUI
make run ARGS="doctor"       # 从源码运行子命令
make check                   # gofmt、go vet、二进制构建
make clean                   # 删除 bin/ 下的构建产物
```

## 快速上手

1. 运行 `donn`。
2. 按 `a`，搜索 provider，按 Enter 选中。
3. 填 profile 名和 API key。provider、model、effort 和 isolation 都有选择弹窗；permission 在全局设置中修改；model 也可以直接输入任意 ID。
4. 创建后运行生成的 alias，或者用 `donn run <name>` 启动。

常用按键：

| 按键 | 作用 |
|---|---|
| `a` | 新建 profile |
| `e` / `Tab` | 聚焦 profile 详情 |
| `Enter` | 列表中启动；详情或设置中编辑当前行 |
| `S` | 全局设置 |
| `D` | Doctor 体检 |
| `s` | 同步当前 profile |
| `d` | 删除，并选择是否保留 session |
| `o` | 打开 provider 的取 key 页面 |
| `y` | 复制启动命令、路径或 profile env 值 |
| 鼠标点链接 | 右键复制；左键用浏览器打开。链接在列表行里时，第一次点击只选中该行，再点才打开。屏幕上完整显示的 http(s) 链接都可以点 |
| `Ctrl-t` | 显示或隐藏 API key（详情焦点下） |
| `?` | 完整快捷键帮助 |

## 命令

```bash
donn                         # 交互式 dashboard
donn run <name> -- <args>    # 先 sync 再 exec Claude；参数和退出码透传
donn list                    # 紧凑 profile 表格
donn doctor                  # 有任一失败项就返回非 0
```

## 数据与配置

磁盘布局：

```text
~/.donn/
├── config.toml
├── presets.d/
└── profiles/<name>/
    ├── profile.toml
    ├── shared-settings.json   # 仅 shared isolation：--settings overlay
    └── claude/
        ├── settings.json
        └── .claude.json
```

`~/.donn/config.toml` 自带说明。第一次打开 TUI 会生成带注释的模板，在 TUI 里改设置会保留模板注释和未知顶层配置。全局设置一改，立刻同步到所有 profile。同一项在多处设置时，优先级是 profile 自己的设置 > 全局设置 > provider preset。

两种 isolation 下，donn 都不会写你的 `~/.claude/`。

## 开发

改代码先看 [CLAUDE.md](CLAUDE.md)。里面有代码地图、验证命令，也写明了每份文档负责什么：[架构](docs/ARCHITECTURE.md)、[硬规则](docs/RULES.md)、[全局设置对照表](docs/GLOBAL-SETTINGS.md)。

License: MIT
