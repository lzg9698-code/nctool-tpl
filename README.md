# NCtool 2.2 工作台

通用 **Jinja 模板平台**：编辑模板、定义参数、输入 JSON 数据并生成文本。G 代码生成、机床配置和顺序多工序由可选插件提供。

模板引擎基于 MiniJinja，支持其 Jinja 兼容语法；不声称与 Python Jinja 的全部扩展等价。默认构建仅提供通用模板能力，不注入机床上下文或注册数学、NC 过滤器。

## 快速开始

源码构建需要 Rust **1.89+**、Node.js **22.12+** 和 npm。完整发行二进制包含 WebUI，运行时不需要 Node.js。Python 外部插件示例需要 Python 3，主程序和纯模板模式不依赖 Python。

在仓库根目录执行：

```bash
npm ci --prefix ui
node scripts/build_ui.mjs

# 默认通用模板工作台，浏览器访问 http://127.0.0.1:8788
cargo run -p nctool-cli -- ui

# 查看动作及其 Schema，运行普通文本示例
cargo run -p nctool-cli -- actions
cargo run -p nctool-cli -- run template.render --input examples/v2/render.json

# 构建并启用 NC、数学和多工序插件
cargo run -p nctool-cli --features nc-bundle -- --profile nc ui
cargo run -p nctool-cli --features nc-bundle -- --profile nc run nc.generate --input examples/v2/nc.json
cargo run -p nctool-cli --features nc-bundle -- --profile nc run process.generate --input examples/v2/process.json
```

已下载完整二进制时，使用 `nctool ui` 或 `nctool --profile nc ui`。完整构建包含可选领域插件，未设置配置时仍默认启动 `template` 组合。已有保存配置时，以配置和显式 `--profile` 为准。

全局参数：`--home <插件配置目录>`、`--workspace <资产目录>`、`--profile template|nc`。工作区默认是当前目录；配置目录优先使用 `NCTOOL_HOME`，Windows 默认 `%LOCALAPPDATA%/nctool`，其他平台使用 `$XDG_CONFIG_HOME/nctool` 或 `~/.config/nctool`。目录与配置详见[系统设计](docs/SYSTEM_DESIGN.md)。

`run --input -` 从标准输入读取 JSON；`--out <文件>` 只在动作完整成功后原子写入文本。CLI 的结构化结果包含 `data`、`artifacts`、`diagnostics`，诊断与生成正文分离。

独立参数库支持共享定义和预置；NC 扩展支持刀具加工推荐、原始业务输入计算及完整组合交付。多文件使用 `--out-dir <新目录>`。接口见[参数管理指南](docs/PARAMETER_MANAGEMENT.md)，V3 的 28 份模板以[可导入资产包](examples/v3-managed/README.md)提供，正式生成不依赖 Python。

## 日常使用

- 在模板库选择模板，通过表单填写参数；切换“编辑模板”可修改源码、参数定义和元数据。
- 工作台明确显示“生成方式”，使用当前源码草稿执行；带 NC 元数据的模板默认走 NC 校验与格式化。
- 草稿按工作区恢复，保存冲突可比较、加载磁盘版本或另存副本；输入变化后旧结果标为过期。
- 执行记录支持输出比较、输入恢复和重执行环境提示；工作区资产包支持预检查、导入和导出。

完整步骤见 [WebUI 使用指南](docs/WEBUI_USER_GUIDE.md)，模板作者从[模板编写指南](docs/TEMPLATE_WRITING_GUIDE.md)开始。

## 外部插件

示例插件通过 Python 计算合计，再调用宿主模板服务生成报告。在仓库根目录执行：

```bash
cargo run -p nctool-cli -- plugins install examples/plugins/python-report
cargo run -p nctool-cli -- plugins enable python-report
cargo run -p nctool-cli -- run report.compute --input examples/v2/report.json
```

安装不会自动启用。安装、启停和配置修改在下次启动生效，已运行的 WebUI 需要重启。Windows 没有 `python3` 时，安装前将示例 `plugin.json` 的 `command[0]` 改为 `python` 或实际 Python 可执行文件路径。协议与生命周期见[插件协议](docs/PLUGIN_PROTOCOL.md)。

## 文档导航

[文档总览](docs/README.md)提供使用、开发、发布和历史资料入口。

| 目标 | 文档 |
| --- | --- |
| 编写模板、配置 NC | [模板编写](docs/TEMPLATE_WRITING_GUIDE.md) · [机床配置](docs/MACHINE_CONFIG_GUIDE.md) · [工艺核对](docs/PROCESS_CHECKLIST.md) |
| 理解组件与接口 | [架构](docs/ARCHITECTURE.md) · [系统设计](docs/SYSTEM_DESIGN.md) · [插件协议](docs/PLUGIN_PROTOCOL.md) |
| 开发与验收 | [贡献指南](docs/CONTRIBUTING.md) · [WebUI 验收清单](docs/UI_ACCEPTANCE_CHECKLIST.md) |
| 查看进展与发布流程 | [项目状态](docs/PROJECT_STATUS.md) · [路线图](docs/ROADMAP.md) · [发布说明](docs/RELEASE.md) · [变更记录](CHANGELOG.md) |
| 查阅旧版与验证记录 | [历史归档](docs/archive/README.md) |

CLI/WebUI 当前版本为 2.2.0；引擎、SDK、运行时和内置插件等组件为 2.0.0。旧 Tauri 桌面 GUI 源码及启动入口已移除，当前界面入口为浏览器 WebUI。旧 CLI、`/api` 路由、工序格式及业务模板不自动迁移；发行包不包含旧业务模板或内置机床方案。

当前 NC 示例用于软件行为验证。实际使用需要创建并核对自己的模板和机床资产，软件测试通过不代表真实机床工艺验收。项目能力、验证边界和已知限制见[项目状态](docs/PROJECT_STATUS.md)。
