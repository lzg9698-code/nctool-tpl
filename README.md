# NCtool 2.2 工作台

通用 **Jinja 模板平台**：编辑模板、检查变量、输入 JSON 数据并生成任意文本。G 代码生成、机床配置和顺序多工序能力通过可选插件提供。

模板引擎基于 MiniJinja，支持其 Jinja 兼容语法；不声称与 Python Jinja 的所有扩展完全等价。引擎默认不注册数学或 NC 过滤器，不注入机床上下文。

## WebUI 的日常操作

- **使用模板**：在模板库选择模板，通过表单填写对象、数组、数值、开关等参数，再生成并导出文件。
- 模板工作台明确显示生成方式。普通模板走通用文本渲染，带 NC 元数据的模板默认走 NC 插件校验、刀具联动与格式化；生成使用当前源码草稿。
- **编辑模板**：使用带行号和语法高亮的 Jinja 编辑器，识别变量后明确选择参数类型，配置分组、数组元素、默认值、单位及范围。
- **模板语法**：左侧导航或源码栏的“语法提示”可查看分类速查、搜索与复制片段，并用示例数据试跑；返回时保留编辑草稿。插件语法标明启用条件。
- 草稿、参数和未填写完整的数值会按工作区恢复；切换模板不会覆盖其他文档。
- 保存使用版本指纹。遇到其他编辑修改时，可比较、加载磁盘版本或另存副本，不静默覆盖。
- 支持复制、稳定标识下修改名称、删除、导入 `.j2` / 模板 JSON、导出模板与结果文件。仍被其他模板静态引用的模板不能直接删除。
- 数字使用无损 JSON 处理；大整数不会在表单切换时舍入，极小的非零数值会交给后端检查，而不是在浏览器中变成零。
- 执行在后台进行，可以切页和取消；执行记录保留输入、插件版本及结果。通用模板生成会保留完整模板快照，历史输入可恢复为新草稿。
- 修改输入后旧结果标为过期；当前任务失败或取消不会提供当前可交付文件。

WebUI 已接入双模式、后台任务、执行历史，以及以下日常操作：

- **NC 工作区**：保存机床配置、选择加工模板并填写参数；多工序页面支持添加、复制、排序、删除工序，以及公共参数、工序覆盖和机床切换。
- **插件与设置**：从本地目录安装插件，查看声明和依赖，通过表单配置，检查启动顺序后保存；当前状态与下次启动状态分别显示。
- **执行记录**：比较两次成功结果的行差异，使用历史输入重新执行。内置 NC 与多工序记录模板快照、机床及输出选项；插件版本仍以当前运行版本为准。
- **工作区迁移**：导出当前启用插件的资产集合，保留模板引用、规格、默认值、元数据、预设和机床。导入先显示映射并检查冲突，已有同名资产不会被覆盖。

外部插件使用 Schema 表单；专用页面由内置插件的视图贡献注册。默认通用模板组合不加载 NC 页面代码。

## 快速开始

需要 Rust **1.89+**；WebUI 开发和构建需要 Node.js **22.12+**。下载后的完整二进制已包含 WebUI 静态资源，运行时不需要 Node.js。Python 外部插件示例需要 Python 3，主程序和纯模板模式不依赖 Python。

```bash
# 首次从源码构建：安装前端依赖并生成嵌入资源。
npm ci --prefix ui
node scripts/build_ui.mjs

# 默认构建只有通用模板能力；启动本地 WebUI。
cargo run -p nctool-cli -- ui
# 浏览器访问 http://127.0.0.1:8788

# 查看动作及输入/输出规格，渲染一个普通模板。
cargo run -p nctool-cli -- actions
cargo run -p nctool-cli -- run template.render --input examples/v2/render.json

# 完整构建包含可选 NC 插件；默认启动组合仍然是 template。
cargo run -p nctool-cli --features nc-bundle -- --profile nc ui
cargo run -p nctool-cli --features nc-bundle -- --profile nc run process.generate --input examples/v2/process.json
```

全局参数：`--home <插件配置目录>`、`--workspace <资产目录>`、`--profile template|nc`。资产目录默认是当前目录，插件配置默认 `$NCTOOL_HOME`、`$XDG_CONFIG_HOME/nctool` 或 `~/.config/nctool`；Windows 默认使用 `%LOCALAPPDATA%/nctool`。

`run --input -` 从标准输入读取 JSON。`--out <文件>` 只在整个动作成功后原子写入文本产物。结果中的 `diagnostics` 与正文分离，错误不会混入生成文本。

## 外部插件：无需重新编译

```bash
cargo run -p nctool-cli -- plugins install examples/plugins/python-report
cargo run -p nctool-cli -- plugins enable python-report
cargo run -p nctool-cli -- run report.compute --input examples/v2/report.json
```

示例由 Python 计算合计，通过双向 JSON-RPC 调用宿主的 `template.render`，生成一个非加工领域报告。安装不自动启用；安装、启用、停用在下一次启动生效。已运行的 WebUI 需重启，新的 CLI 调用自动使用最新配置。

```bash
nctool plugins list
nctool plugins disable python-report
nctool plugins uninstall python-report
```

Windows 如没有 `python3` 命令，安装前将示例 `plugin.json` 的 `command[0]` 改为 `python` 或 Python 可执行文件路径。插件包不能包含符号链接，首次安装后不静默覆盖同名插件。首版支持本地目录安装，不包含插件市场、任意外部表达式过滤器或操作系统级沙箱。

## 模板与领域能力的边界

| 层 | 内容 |
| --- | --- |
| `nctool-tpl` | Jinja 解析、变量与模板引用提取、严格/宽松渲染、显式过滤器注册 |
| `nctool-assets` | 通用 JSON 资产、路径约束、原子写、跨进程锁、完整版本指纹、数值精度检查 |
| `nctool-plugin-sdk` | 插件、服务、动作、诊断、取消、渲染扩展和事件契约 |
| `nctool-runtime` | 依赖解析、注册与释放、提供方选择、外部进程协议 |
| `plugins/template` | 模板、参数规格、默认值、元数据、通用预设 |
| `plugins/math` | 可选数学/角度过滤器 |
| `plugins/nc` | 机床、参数派生与校验、NC 格式化、G 代码后处理 |
| `plugins/process` | 参数继承、工序覆盖、顺序执行、行号连续、整体交付 |
| `cli` / `ui` | 宿主入口、能力目录、插件贡献的页面与表单 |

模板服务接受嵌套对象、数组、字符串、数字、布尔和空值。普通渲染不经过 NC 生成管线。NC 的硬校验不会因模板宽松模式而跳过。多工序插件依赖版本化 `nc.generate` 服务，不导入 NC 的 Rust 模型。

WebUI 从插件贡献描述生成导航与动作表单；默认组合不会出现机床或工序页面。服务仅绑定回环地址，并限制请求体、跨站访问、消息大小与并发调用。

## 2.0 与旧版

这是允许接口不兼容的主版本升级：旧 CLI 命令、`/api` 路由、工序文件格式和业务模板不迁移。旧模板、V3 模板和内置机床方案不进入新发行包；新示例只用于架构与行为验证。实际 NC 使用需要创建并验证自己的领域资产。

桌面 GUI 保留为旧版代码，已退出 2.0 workspace 与发布检查。历史设计文档仍可查阅，当前规范以以下文档为准：

- [架构与依赖边界](docs/ARCHITECTURE.md)
- [插件协议与服务契约](docs/PLUGIN_PROTOCOL.md)
- [系统设计与配置](docs/SYSTEM_DESIGN.md)
- [开发与验收](docs/CONTRIBUTING.md)
- [升级状态及验证边界](docs/PROJECT_STATUS.md)

## 验证

```bash
npm run --prefix ui test
node scripts/build_ui.mjs --check
python3 scripts/check_v2.py
cargo test --workspace --all-targets --all-features --locked
cargo test --workspace --all-features --doc --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.89.0 check --workspace --all-features --locked
```

Linux、Windows、macOS 的构建、测试和外部协议检查由 CI 矩阵执行。本机通过的检查不等价于其他平台已实际执行，也不等价于工艺或机床验收。
