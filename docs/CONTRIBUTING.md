# 开发与验收

适用于 2.2 CLI/WebUI 与 2.0 核心组件。修改前确认 `git status`，不要丢弃用户未提交改动。历史升级快照与验证记录见[归档索引](archive/README.md)，它们不是当前验收或发行资产。

## 工具链与本地开发

使用 Codex 桌面开发时先阅读仓库 [AGENTS.md](../AGENTS.md)；专用 Skills、GitHub/浏览器/Context7 使用方法及依赖审计见 [Codex 开发指南](CODEX_DEVELOPMENT.md)。

Rust 1.89+，前端使用 Node.js 22.12+ 和 npm。Python 插件与验证脚本需要 Python 3；源码快照工具需要 Python 3.9+。Windows 可将下列 `python3` 改为可用的 `python`，不要依赖不可用的 WindowsApps 命令别名。

先在仓库根目录执行 `npm ci --prefix ui` 和 `node scripts/build_ui.mjs`，再运行 `cargo run -p nctool-cli -- ui`。默认 WebUI 地址为 `http://127.0.0.1:8788`。数学扩展使用 `cargo run -p nctool-cli --features math -- ui`，并需在配置 `enabled` 中启用、在请求 `extensions` 中显式选择。

前端开发可执行 `npm run --prefix ui dev`；Vite 将 `/api` 转发到 `127.0.0.1:8788`，同时运行 Rust 宿主。前端源码变更后重新生成嵌入资源并编译宿主，才能在发行式 WebUI 中看到更新。

## 构建与测试

```bash
npm ci --prefix ui
npm run --prefix ui lint
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

已有生产构建时可用 `node scripts/build_ui.mjs --check` 只检查嵌入资源一致性。按改动选择检查：纯文档改动核对导航、链接、接口和采用的命令示例；业务/前端改动运行相关测试及发布所需完整检查，不用旧测试数量代替当前结果。

外部协议测试需要 Python 3，会使用可用的 python3/python 命令。真实进程测试覆盖握手、逆向服务调用、精度检查、错误输出、消息限制、崩溃、堵塞 stdin、超时、取消与下一次调用恢复。

`python3 scripts/check_v2.py` 验证默认 template 不包含 NC，`math` 为唯一可选 feature，领域中立组件只有白名单生产依赖，旧桌面入口移除、Tauri 隔离、引擎领域注册隔离、Python 适配器、UI 资源与 JavaScript 语法、基础包内容均无加工实现。数学由独立 `math` feature 提供，运行扩展仍需显式选择。

`npm run --prefix ui lint` 执行 TypeScript 类型及未使用声明检查，不是 ESLint。前端依赖与测试环境必须声明在清单并锁定；不要依赖已有 node_modules 中未声明的包。当前范围与验收门槛见[路线图](ROADMAP.md)与[项目状态](PROJECT_STATUS.md)；2026-10-16 的 P0 方案已归档，仅作历史参考。

## 修改边界

- 引擎保持仅 MiniJinja 依赖；领域规则移入插件。
- 服务实现者和消费者共同遵守服务版本；改变接口语义需升级服务版本。
- CLI/WebUI 不导入领域模型；内置构造器只出现在 composition root。
- WebUI 源码是 `ui/src` 的 React/TypeScript/CSS。首次执行 `npm ci --prefix ui`；`node scripts/build_ui.mjs` 构建 `ui/dist` 并生成 `cli/ui` 的静态资源及 Rust 嵌入表。不要直接编辑生成文件。`ui/dist` 是构建中间产物且被 Git 忽略，受版本控制的是 `cli/ui`；新克隆必须先构建一次，`node scripts/build_ui.mjs --check` 与 `python scripts/check_v2.py` 才有可比对的产物。
- 新动作提供 JSON Schema、可运行示例和插件贡献的页面描述。
- 资产必须经 JsonStore/WriteKernel；不跳过指纹或静默替换文件。
- 旧 Tauri 桌面 GUI 已移除；界面开发使用当前 WebUI，旧 GUI 资料仅作历史参考。

## 浏览器验收

默认组合：无 NC/机床/工序导航；编辑嵌套参数、检查变量、渲染、保存/重新读取、陈旧保存冲突、下载产物。带旧加工元数据的模板被拒绝，不降级为普通渲染。

数学扩展：默认构建未启用时不注册数学过滤器；显式编译、配置启用并在请求 `extensions` 选择后可渲染，历史保存扩展选择。

历史与迁移：比较两次成功输出，重新执行固定输入的历史任务；导出集合并在新工作区校验、导入，验证引用、预设和元数据，重复导入拒绝覆盖。含退役加工集合的资产包整体拒绝。

历史环境：验证新记录的插件版本和服务绑定，环境一致、已变化及旧记录信息不完整三种提示。重执行必须记录来源与实际环境检查，不改写原历史文件。`scripts/browser_v2.cjs` 在临时工作区模拟旧版本记录；`cli/tests/platform.rs` 同时覆盖宿主、插件和服务绑定差异，兼容旧记录及不可比较的格式版本。

外部插件：通过页面检查本地目录、安装、检查依赖、保存启用配置，重启后出现数据报告表单，可调用模板服务生成结果。配置页面显示当前状态与下次启动状态。

同时检查 HTTP 的 Host/Origin/Content-Type/体积限制，任务运行时能查询能力与取消。平台发布需要 Linux/Windows/macOS CI 成功。

自动浏览器验收：先构建 `cargo build -p nctool-cli --features math`，安装 Playwright 1.62.1 和 Chromium，再执行 `node scripts/browser_v2.cjs`。可用 `NCTOOL_PLAYWRIGHT` 指定独立安装目录，`NCTOOL_CHROMIUM` 指定浏览器路径。脚本使用临时 home/workspace，覆盖默认与数学组合、退役边界与 Python 插件，不修改用户配置。

`browser_v2.cjs` 还检查实际加载的编辑器样式：CSS 变量均有定义，关键字/数字/字符串/注释具有独立颜色，搜索面板和输入框背景不透明且文字可见，搜索命中高亮出现。报告目录保存 `editor-styles.json` 与 `editor-styles.png`，样式断言失败会阻断现有浏览器 CI。

手工逐项走查见[WebUI 验收清单](UI_ACCEPTANCE_CHECKLIST.md)，每次记录平台、源码版本和实际执行结果。


## 审查缺陷回归

运行 `node scripts/browser_review_fixes.cjs`（与既有浏览器验收使用相同的 Playwright/Chromium 环境）。这个脚本使用临时工作区和一次性外部提供方，覆盖领域能力缺失、显式动作保存、错误 JSON 不提交、草稿跨页/刷新恢复、动作切换、取消/失败、过期导出以及真实 HTTP 数值入口。Linux 浏览器 CI 同时运行三个验收脚本。

`cli/tests/platform.rs` 的 `review_r*` 回归覆盖 CLI 原子输出不覆盖、直接动作、后台任务、历史快照、动态模板依赖、参数 Schema 与整数边界、集合导入，以及退役领域身份不再被声明为可执行。新增功能要保持这些边界；不要仅用单一路径通过来代替全部入口的约束。

并行测试时不要让默认与 `math` 构建覆盖正在使用的同一个 `target/debug/nctool`。浏览器可通过 `NCTOOL_BINARY` 使用事先复制好的固定二进制；前端有改动后先运行 `node scripts/build_ui.mjs` 并重新编译宿主。

## 模板参数与数学扩展回归

运行 `node scripts/browser_generic_parameters.cjs`（与既有浏览器验收使用相同的 Playwright/Chromium 环境，`NCTOOL_BINARY` 指向固定程序）。这个脚本使用临时 home/workspace 覆盖模板内参数：参数定义与默认值、本次填写、恢复默认值、未完成数值与无效 JSON 阻止执行，以及修改参数使旧结果过期并禁止导出。二进制包含 `math` 扩展（`--features math`）时，脚本继续启用并重启临时服务，验证语法页试跑 math 示例、工作台生成携带 `extensions`、HTTP 无 `extensions` 报未知过滤器，以及后台 math 任务记录扩展且重执行不改写原历史。

## 源码基线与发布门槛

使用 Python 3.9 或更新版本，为完整工作树建立本地恢复快照：

```bash
python3 scripts/source_baseline.py capture .nctool-backups/my-source-baseline
python3 scripts/source_baseline.py verify .nctool-backups/my-source-baseline
```

目录必须尚不存在；仓库内的目标必须被 Git 忽略。快照包括现存已跟踪文件和未忽略的新增文件，保存 HEAD、暂存/未暂存二进制补丁、删除状态和逐文件 SHA-256。归档与清单使用同一份读取字节，生成期间检测到源码或 Git 状态变化时拒绝签收，不生成完成清单。工具不修改索引，不执行提交，也不包含忽略的运行资产、依赖、密钥配置或历史备份；这些数据需独立备份。符号链接和子模块等非普通文件拒绝归档，需要单独处理。`verify` 检查归档及补丁完整性，不表示当前工作树仍与该快照相同。

恢复时先在新的空目录解压 `sources.tar.gz`，核对 `manifest.json`；补丁和 `status.txt` 用于重建 Git 暂存状态与删除记录。不要直接覆盖正在使用的工作区。

标签发布现在必须先通过复用的完整 CI：三平台 Rust/前端/架构检查、Linux 三组 Chromium 验收及 MSRV 检查。任何验证作业失败，二进制构建和发布不会执行。发行包按明确的 Rust target 编译，并附带每个二进制的 SHA-256 文件。工作流配置与本机通过不能替代 GitHub Actions 的实际三平台结果；测试输出也不替代使用方对生成正文的最终核对。

标签、资产名称、签收和 crates.io 边界见[发布说明](RELEASE.md)。当前限制与待验证项见[项目状态](PROJECT_STATUS.md)。
