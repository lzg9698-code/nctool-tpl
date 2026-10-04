# 2.0 开发与验收

修改前确认 `git status`，不要丢弃用户未提交改动。此次升级前的工作树快照位于本机忽略目录 `.nctool-backups/2026-10-01-v2`，并非发行资产。

## 构建与测试

```bash
npm ci --prefix ui
npm run --prefix ui test
node scripts/build_ui.mjs
python3 scripts/check_v2.py
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --workspace --all-features --doc --locked
cargo +1.89.0 check --workspace --all-features --locked
```

外部协议测试需要 Python 3，会使用可用的 python3/python 命令。真实进程测试覆盖握手、逆向服务调用、精度检查、错误输出、消息限制、崩溃、堵塞 stdin、超时、取消与下一次调用恢复。

`python3 scripts/check_v2.py` 验证纯模板依赖树、GUI 排除、引擎领域注册隔离、Python 示例适配器一致性、UI 生成物与 JavaScript 语法、基础包内容。

## 修改边界

- 引擎保持仅 MiniJinja 依赖；领域规则移入插件。
- 服务实现者和消费者共同遵守服务版本；改变接口语义需升级服务版本。
- CLI/WebUI 不导入领域模型；内置构造器只出现在 composition root。
- WebUI 源码是 `ui/src` 的 React/TypeScript/CSS。首次执行 `npm ci --prefix ui`；`node scripts/build_ui.mjs` 构建 `ui/dist` 并生成 `cli/ui` 的静态资源及 Rust 嵌入表。不要直接编辑生成文件。
- 新动作提供 JSON Schema、可运行示例和插件贡献的页面描述。
- 资产必须经 JsonStore/WriteKernel；不跳过指纹或静默替换文件。
- GUI 为冻结的旧版代码，不纳入 2.0 功能改动。

## 浏览器验收

默认组合：无 NC/机床/工序导航；编辑嵌套参数、检查变量、渲染、保存/重新读取、陈旧保存冲突、下载产物。

NC 组合：用专用表单新建机床、选择加工模板、编辑参数与行号；添加、复制、排序工序并生成连续编号；任一工序失败突出失败工序，不显示/下载半成品。

历史与迁移：比较两次成功输出，重新执行固定输入的历史任务；导出集合并在新工作区校验、导入，验证引用、预设、机床和元数据，重复导入拒绝覆盖。

历史环境：验证新记录的插件版本和服务绑定，环境一致、已变化及旧记录信息不完整三种提示。重执行必须记录来源与实际环境检查，不改写原历史文件。`scripts/browser_v2.cjs` 在临时工作区模拟旧版本记录；`cli/tests/platform.rs` 同时覆盖宿主、插件和服务绑定差异，兼容旧记录及不可比较的格式版本。

外部插件：通过页面检查本地目录、安装、检查依赖、保存启用配置，重启后出现数据报告表单，可调用模板服务生成结果。配置页面显示当前状态与下次启动状态。

同时检查 HTTP 的 Host/Origin/Content-Type/体积限制，任务运行时能查询能力与取消。平台发布需要 Linux/Windows/macOS CI 成功；测试输出不作为机床工艺验收。

自动浏览器验收：先构建 `cargo build -p nctool-cli --features nc-bundle`，安装 Playwright 1.62.1 和 Chromium，再执行 `node scripts/browser_v2.cjs`。可用 `NCTOOL_PLAYWRIGHT` 指定独立安装目录，`NCTOOL_CHROMIUM` 指定浏览器路径。脚本使用临时 home/workspace，覆盖两种组合与 Python 插件，不修改用户配置。


## 审查缺陷回归

运行 `node scripts/browser_review_fixes.cjs`（与既有浏览器验收使用相同的 Playwright/Chromium 环境）。这个脚本使用临时工作区和一次性外部提供方，覆盖领域能力缺失、显式动作保存、错误 JSON 不提交、草稿跨页/刷新恢复、动作切换、取消/失败、过期导出以及真实 HTTP 数值入口。Linux 浏览器 CI 同时运行两个验收脚本。

`cli/tests/platform.rs` 的 `review_r*` 回归覆盖 CLI 原子输出不覆盖、直接动作、后台任务、工序、历史快照、动态模板依赖、参数 Schema 与 specs、机床直接输入及外部编辑、整数边界和集合导入。新增功能要保持这些边界；不要仅用单一路径通过来代替全部入口的约束。

并行测试时不要让 template-only 和 nc-bundle 构建覆盖正在使用的同一个 `target/debug/nctool`。浏览器可通过 `NCTOOL_BINARY` 使用事先复制好的固定二进制；前端有改动后先运行 `node scripts/build_ui.mjs` 并重新编译宿主。

## 源码基线与发布门槛

使用 Python 3.9 或更新版本，为完整工作树建立本地恢复快照：

```bash
python3 scripts/source_baseline.py capture .nctool-backups/2026-10-04-architecture-next
python3 scripts/source_baseline.py verify .nctool-backups/2026-10-04-architecture-next
```

目录必须尚不存在；仓库内的目标必须被 Git 忽略。快照包括现存已跟踪文件和未忽略的新增文件，保存 HEAD、暂存/未暂存二进制补丁、删除状态和逐文件 SHA-256。归档与清单使用同一份读取字节，生成期间检测到源码或 Git 状态变化时拒绝签收，不生成完成清单。工具不修改索引，不执行提交，也不包含忽略的运行资产、依赖、密钥配置或历史备份；这些数据需独立备份。符号链接和子模块等非普通文件拒绝归档，需要单独处理。`verify` 检查归档及补丁完整性，不表示当前工作树仍与该快照相同。

恢复时先在新的空目录解压 `sources.tar.gz`，核对 `manifest.json`；补丁和 `status.txt` 用于重建 Git 暂存状态与删除记录。不要直接覆盖正在使用的工作区。

标签发布现在必须先通过复用的完整 CI：三平台 Rust/前端/架构检查、Linux 两组 Chromium 验收及 MSRV 检查。任何验证作业失败，二进制构建和发布不会执行。发行包按明确的 Rust target 编译，并附带每个二进制的 SHA-256 文件。工作流配置与本机通过不能替代 GitHub Actions 的实际三平台结果，也不替代真实控制器验收。
