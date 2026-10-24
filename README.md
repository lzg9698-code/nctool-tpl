# NCtool 通用模板工作台

通用 **Jinja 模板平台**：编辑模板、定义参数、输入 JSON 数据并生成文本。内置 NC 能力（G 代码生成、机床配置、加工工序、同步粗车、越程槽、轨迹）已移除；领域能力通过开放的通用插件机制提供。

模板引擎基于 MiniJinja，支持其 Jinja 兼容语法；不声称与 Python Jinja 的全部扩展等价。默认构建提供通用模板与共享参数能力，不注入机床上下文或注册数学过滤器。数学是唯一可选扩展，以独立 `math` feature 编译，启用与单次渲染的扩展选择都显式。

## 快速开始

源码构建需要 Rust **1.89+**、Node.js **22.12+** 和 npm。完整发行二进制包含 WebUI，运行时不需要 Node.js。Python 外部插件示例需要 Python 3，主程序和纯模板模式不依赖 Python。

在仓库根目录执行：

```bash
npm ci --prefix ui
node scripts/build_ui.mjs

# 默认通用模板工作台（template + parameters），浏览器访问 http://127.0.0.1:8788
cargo run -p nctool-cli -- ui

# 查看动作及其 Schema，运行普通文本示例
cargo run -p nctool-cli -- actions
cargo run -p nctool-cli -- run template.render --input examples/v2/render.json

# 数学扩展：以 math feature 编译，启动后再启用并在每次渲染显式选择 extensions
cargo run -p nctool-cli --features math -- ui
cargo run -p nctool-cli --features math -- run template.render --input examples/v2/render.json
```

已下载完整二进制时，使用 `nctool ui`。默认组合始终为 `template` + `parameters`；数学扩展需以 `--features math` 构建（完整二进制已包含），在 `home/config.json` 的 `enabled` 中启用，并在每次渲染请求的 `extensions` 中显式选择，否则不注册任何数学过滤器。

全局参数：`--home <插件配置目录>`、`--workspace <资产目录>`、`--profile <组合>`。旧 `nc` profile 不再支持，会以 `invalid_profile` 拒绝；历史配置请用 `nctool config check` 只读预检、`nctool config migrate --expected <指纹>` 显式备份并迁移，启动不会自动改写配置。工作区默认是当前目录；配置目录优先使用 `NCTOOL_HOME`，Windows 默认 `%LOCALAPPDATA%/nctool`，其他平台使用 `$XDG_CONFIG_HOME/nctool` 或 `~/.config/nctool`。目录与配置详见[系统设计](docs/SYSTEM_DESIGN.md)。

`run --input -` 从标准输入读取 JSON；`--out <文件>` 只在动作完整成功后原子写入文本；多文件使用 `--out-dir <新目录>`。CLI 的结构化结果包含 `data`、`artifacts`、`diagnostics`，诊断与生成正文分离。

共享参数库支持共享定义、预置与模板绑定，也用于通用受管渲染；接口见[参数管理指南](docs/PARAMETER_MANAGEMENT.md)。旧加工模板与旧加工历史不会被恢复为可执行文档。

## 日常使用

- 在模板库选择模板，通过表单填写参数；切换“编辑模板”可修改源码、参数定义和元数据。
- 工作台明确显示“生成方式”，使用当前源码草稿执行。带旧加工元数据（`metadata.nc` 或加工 `execution.action`）的模板会被拒绝，不会降级为普通渲染。
- 草稿按工作区恢复，保存冲突可比较、加载磁盘版本或另存副本；输入变化后旧结果标为过期。
- 执行记录支持输出比较、输入恢复和重执行环境提示；工作区资产包支持预检查、导入和导出。旧加工历史只可查看、比较和导出原产物，不能重跑或恢复为可执行文档。

完整步骤见 [WebUI 使用指南](docs/WEBUI_USER_GUIDE.md)，模板作者从[模板编写指南](docs/TEMPLATE_WRITING_GUIDE.md)开始。

## 外部插件

示例插件通过 Python 计算合计，再调用宿主模板服务生成报告。在仓库根目录执行：

```bash
cargo run -p nctool-cli -- plugins install examples/plugins/python-report
cargo run -p nctool-cli -- plugins enable python-report
cargo run -p nctool-cli -- run report.compute --input examples/v2/report.json
```

安装不会自动启用。安装、启停和配置修改在下次启动生效，已运行的 WebUI 需要重启。Windows 没有 `python3` 时，安装前将示例 `plugin.json` 的 `command[0]` 改为 `python` 或实际 Python 可执行文件路径。协议与生命周期见[插件协议](docs/PLUGIN_PROTOCOL.md)，结构化诊断与跨语言验收见[宿主与插件契约](docs/CONTRACTS.md)。协议 1 插件须更新 SDK、描述与握手后重新安装。

## 文档导航

[文档总览](docs/README.md)提供使用、开发、发布和历史资料入口。

| 目标 | 文档 |
| --- | --- |
| 编写模板与参数 | [模板编写](docs/TEMPLATE_WRITING_GUIDE.md) · [参数管理](docs/PARAMETER_MANAGEMENT.md) |
| 理解组件与接口 | [架构](docs/ARCHITECTURE.md) · [系统设计](docs/SYSTEM_DESIGN.md) · [插件协议](docs/PLUGIN_PROTOCOL.md) |
| 开发与验收 | [贡献指南](docs/CONTRIBUTING.md) · [WebUI 验收清单](docs/UI_ACCEPTANCE_CHECKLIST.md) |
| 查看进展与发布流程 | [项目状态](docs/PROJECT_STATUS.md) · [路线图](docs/ROADMAP.md) · [发布说明](docs/RELEASE.md) · [变更记录](CHANGELOG.md) |
| 查阅旧版与验证记录 | [历史归档](docs/archive/README.md) |

CLI/WebUI 当前清单版本为 2.3.0（2026-10-24 发布），外部进程协议为 2；引擎、SDK、运行时和内置插件等组件为 2.0.0。旧 Tauri 桌面 GUI 源码及启动入口已移除，当前界面入口为浏览器 WebUI。旧 CLI、`/api` 路由、旧加工工序格式及业务模板不自动迁移；发行包不包含旧业务模板或内置机床方案。

示例仅用于软件行为验证。实际使用需要创建并核对自己的模板与参数资产。
