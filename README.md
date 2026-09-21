# donn

单二进制的 Claude Code profile 管理器，带完整终端 UI。它为官方 Claude 与 Kimi、MiniMax、Z.ai、OpenRouter、DeepSeek 等 Anthropic 兼容渠道维护相互隔离的配置。

```bash
donn        # 打开终端 dashboard
kimi        # 用创建时生成的 profile 别名启动 Claude
```

## 特性

- 完整的终端界面：profile 列表和详情、搜渠道、新建表单、可搜索的模型选择器、全局设置、Doctor 体检；该选的地方是选择框，该确认的地方有确认框
- 内置 23 个渠道；想加渠道或改内置渠道，往 `~/.donn/presets.d/` 放一个 TOML 就行
- 全局设置：权限、思考强度、thinking、回复语言、输出上限、第三方端点兼容、模型回退等，改一次对所有 profile 生效；值和 Claude Code 默认一样时什么都不写
- 两种隔离方式：`full`（默认）每个 profile 有自己的 Claude 配置目录和会话；`shared` 继续用你 `~/.claude` 里的会话和文件，只在这次启动里换渠道、模型和全局设置
- 删除 profile 时可以选：保留会话（以后同名重建还能接着用），或者整个删掉
- API key 只放在 profile 自己的 `settings.json` 里，新文件权限 `0600`；不进日志、不进 `profile.toml`、不进报错信息
- 有人手改了 donn 管的键，donn 看得出来，下次启动前自动改回去
- 除了界面，还有适合脚本用的 `run`、`list`、`doctor` 命令

## 从当前源码安装

需要 Rust 1.97 或更新版本。

```bash
cargo install --path crates/donn-cli --locked
```

本地开发可直接运行 `cargo run -p donn-cli --`。

Makefile 目标：

```bash
make install                 # 从当前源码安装 donn
make run                     # 不安装，直接打开 TUI
make run ARGS="doctor"       # 从源码运行子命令
make check                   # 格式、测试、Clippy、release 构建
make build                   # 构建 target/release/donn
```

## 快速上手

1. 运行 `donn`。
2. 按 `a`，搜索渠道并按 Enter。
3. 填 profile 名与 key。渠道、模型、effort、权限和隔离方式都有真正的选择/搜索弹窗；模型也允许直接输入任意 ID。
4. 创建后用生成的别名，或 `donn run <name>` 启动。

常用按键：

| 按键 | 作用 |
|---|---|
| `a` | 新建 profile |
| `e` / `Tab` | 聚焦 profile 详情 |
| `Enter` | 列表中启动；详情/设置中编辑当前行 |
| `S` | 全局设置 |
| `D` | Doctor 体检 |
| `s` | 同步当前 profile |
| `d` | 删除，并选择是否保留会话 |
| `o` | 打开渠道取 key 页面 |
| `y` | 复制启动命令、路径或 profile env 值 |
| 鼠标点链接 | 右键复制；左键用浏览器打开——链接在列表行里时，第一下只是选中该行，再点才打开。屏幕上完整显示的 http(s) 链接都可以点 |
| `Ctrl-t` | 显示/隐藏 API key（详情焦点下） |
| `?` | 完整快捷键帮助 |

## 命令

```bash
donn                         # 交互式 dashboard
donn run <name> -- <args>    # 先同步再 exec Claude；参数与退出码透传
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
    ├── shared-settings.json   # 仅 shared 隔离：--settings overlay
    └── claude/
        ├── settings.json
        └── .claude.json
```

`~/.donn/config.toml` 自带说明：第一次打开 TUI 会生成带注释的模板，在 TUI 里改设置不会弄丢这些注释。全局设置一改，立刻同步到所有 profile。同一项在多处都设了的话：profile 自己的设置优先于全局设置，全局设置优先于渠道 preset。

两种隔离方式下，donn 都不会写你的 `~/.claude/`。

## 开发

```bash
make check   # 格式、测试、Clippy、release 构建
```

改代码先看 [CLAUDE.md](CLAUDE.md)：里面有代码地图，也写明了每份文档各管什么（[架构](docs/ARCHITECTURE.md)、[硬规则](docs/RULES.md)、[全局设置对照表](docs/GLOBAL-SETTINGS.md)）。

License: MIT
