# NCtool 项目审查计划

日期：2026-10-15。状态：计划已编制，正式审查及运行验证待执行。

## 1. 目标与范围

判断当前 Rust 通用模板平台在加工能力退役与 WebUI 重构后，是否仍保持正确的输入校验、资产保护、执行与恢复语义，并识别可复现、可行动的缺陷。

- 审查对象：当前完整工作树，包括已跟踪改动、新增文件、删除记录、生成资源和恢复包；沿调用链检查必要的未改动代码。
- 对照基准：`e8fafd5e47a032d52f9f7a6adfbb384d454e0e15`（当前 HEAD）。该提交用于理解差异，当前产品契约以源码、AGENTS.md 和现行组件清单为准，不要求恢复已退役功能。
- 初始范围：已跟踪 diff 涉及 166 个文件，约 6,200 行新增、48,793 行删除，另有未跟踪新增文件。数量仅描述编制计划时的状态。
- 版本：CLI/WebUI 2.2.0、核心组件 2.0.0；不要求统一版本数字。
- 当前任务仅编制计划。后续审查默认只读取证、输出报告；修复、提交、推送、评论、合并、打标签及发布另按用户授权执行。
- 仓库外产品、研究原型及机床工艺验收不在范围内。

项目启动简报未能在 `D:/ai-brain/memory/projects/nctool/_brief.md` 读取到，且未检索到其他 `_brief.md`。本计划依据仓库规则、当前 diff、贡献指南和项目状态编制，不推断缺失的历史决策。

## 2. 审查顺序与重点

P0/P1/P2 在本节表示审查先后次序，不预先判定存在相应级别缺陷。

| 顺序 | 主题与主要入口 | 核心问题 | 必备场景及证据 |
| --- | --- | --- | --- |
| P0-1 | 退役边界：`runtime/src/config.rs`、`cli/src/composition.rs`、`cli/src/workbench.rs`、`cli/tests/retirement.rs` | 旧配置是否只读预览、显式备份并按 expected 写入；退役保留名能否被外部接管；删除活动源码后是否仍有可执行旁路 | 旧 nc profile、enabled/disabled/provider/plugins；有效与陈旧 expected；同名外部插件；迁移失败前后配置与资产字节对比 |
| P0-2 | 渲染与历史：`plugins/template`、`plugins/parameters`、CLI/API 调用链、`ui/src/lib/documentExecution.ts`、HistoryPage | 旧加工标记能否降级为通用 render；旧历史是否只允许查看原 artifact；重执行是否先检查环境、冻结输入并记录来源 | metadata.nc 缺失、null、空对象；退役动作与普通动作；旧历史恢复/重跑；插件版本及服务绑定变化；原历史不变 |
| P0-3 | 数值与通用输入：`assets/src/json_num.rs`、Schema、SDK/RPC、HTTP、表单解析 | 数值实现大幅删减后是否仍拒绝整数精度损失、非有限数及下溢；嵌套 JSON/null 是否跨入口一致 | 安全整数边界、超大整数、指数/下溢、负零、嵌套对象和数组/null；非法输入拒绝且无资产或产物副作用 |
| P0-4 | 资产与集合：JsonStore/WriteKernel、受管依赖、导入/导出及保存调用者 | 指纹、原子写入、快照与回滚是否仍完整；混合加工集合是否整体拒绝 | 陈旧保存、同名资产、动态引用变化、部分失败、合法与退役集合混包；失败后无部分写入，用户原资产不删除 |
| P0-5 | 任务与产物：任务服务、外部 RPC、CLI 输出、`ResultPane` 与 controller | 失败、取消、中断或输入变更后是否仍能导出旧结果；多文件是否完整成功后一次交付 | 普通成功、部分产物后失败、超时、取消、动作切换、取消后再次调用；任务状态、子进程终止、下载内容及文件系统证据 |
| P1-1 | 架构与 math：Cargo manifests、composition root、runtime、template/parameters/math | 引擎是否只依赖 MiniJinja；内置构造器是否局限于 composition root；math 编译/配置/请求三条件是否独立 | 默认构建、math 编译但未 enabled、enabled 但无 extensions、显式请求；普通与受管渲染及冻结重跑；依赖树与运行结果 |
| P1-2 | WebUI 状态：`ui/src/features/workbench/`、`managedDependencies.ts`、WorkbenchUi、PluginSettings、ParameterViews | 拆分 App 后草稿、作用域 store、异步响应、结果失效与恢复是否仍一致 | 多文档切换、切页/刷新恢复、错误 JSON、保存冲突、引用变更、旧请求晚返回；插件当前状态与下次启动状态 |
| P1-3 | HTTP 与外部协议：CLI 路由、runtime/SDK、Python 示例适配器 | Host/Origin/Content-Type/体积限制及 RPC 消息边界是否有效；协议与服务版本是否对应实际行为 | 合法/非法请求、畸形与超大消息、崩溃、stdin 堵塞、超时/取消与恢复；不把进程隔离视为 OS 沙箱 |
| P2-1 | 交互与布局：Editor、Radix 菜单/对话框、主题及 `ui/src/styles/` | 焦点、键盘、编辑状态与不同窗口尺寸是否可用 | Tab/Esc 焦点恢复、快捷键、选区/撤销、主题恢复、移动端无横向溢出；截图和实际交互断言分别记录 |
| P2-2 | 工程交付：锁文件、CI/release、构建脚本、生成资源、归档与文档 | 检查是否覆盖实际源码；默认/math 构建是否隔离；恢复包与文档是否完整、现行说明是否过时 | 生成物一致性、包内容、归档清单/SHA-256、三组浏览器是否纳入 CI、发行门槛、有效链接与可运行示例、依赖审计报告 |

每个主题沿“入口 → 校验 → 服务/执行 → 持久化/产物 → UI 展示”追踪。先检查高风险改动及调用者，再扩展到相关稳定代码；不机械逐行检查已退役的大量实现。

## 3. 执行阶段

### 阶段 A：固定证据基线

1. 重新记录 HEAD、分支、完整 status、暂存/未暂存 diff 和未跟踪清单；不要只依赖 `git diff`，它不包含新增文件正文。
2. 记录涉及源码及锁文件摘要、工具链版本、Windows 平台信息和 `cli/ui/source.sha256` 原始内容。审查期间若用户继续修改，注明受影响结论并重验相关部分。
3. 阅读专用审查/验证规则和实际测试入口，整理“契约—实现—已有测试—待补场景”矩阵。
4. 保留现有未提交内容。审查与运行证据写入独立的 Git 忽略目录，例如 `target/project-review-20261015/`。若增加源码快照，使用既有 source_baseline 工具，不把快照等同于用户资产备份。

### 阶段 B：静态审查与候选缺陷复核

1. 按 P0 → P1 → P2 顺序取证；可以分别检查互相独立的模块，跨层问题必须统一核对契约。
2. 每条候选问题先检查调用者、错误处理、已有测试及文档，排除受控兼容拒绝和有意退役行为。
3. 优先使用现有测试或最小隔离输入复现；记录具体输入、预期、实际行为及后果。
4. 审查阶段不顺带重构或自动修复；需要源码测试改动时，在获准修复阶段落实，或先用独立复现材料取证。

### 阶段 C：运行验证

先跑针对候选缺陷的最小必要验证，再进行平台整体检查。下列是待执行清单，不表示已经通过。

| 检查组 | 命令/执行要求 | 判断依据 |
| --- | --- | --- |
| 前端 | `npm run --prefix ui lint`；`npm run --prefix ui test` | 类型/未使用项与测试分别记录；lint 当前不是 ESLint |
| 资源 | `node scripts/build_ui.mjs --check` | 优先只读检查已有产物一致性；需重新生成时，先保存 source.sha256 原值，再运行正式构建并核对全部生成差异 |
| 架构/Python | `python scripts/check_v2.py`；`python -m unittest discover -s scripts/tests -p 'test_*.py'` | 当前平台边界、默认依赖树、脚本和恢复包完整性 |
| Rust 格式 | `cargo fmt --all -- --check` | 格式结果 |
| Rust 静态检查 | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 使用本轮独立 target，保留完整诊断 |
| Rust 默认测试 | `cargo test --workspace --all-targets --locked --target-dir target/project-review-20261015/default` | 默认 feature 组合；workspace 中独立 math crate 的测试不等于 CLI 默认启用 math |
| Rust math 测试 | `cargo test --workspace --all-targets --all-features --locked --target-dir target/project-review-20261015/math` | 显式 math 组合；与默认构建隔离 |
| Rust 文档 | `cargo test --workspace --all-features --doc --locked` | 文档示例，以本轮独立 target 执行 |
| MSRV | `cargo +1.89.0 check --workspace --all-features --locked` | 实际 Rust 1.89.0 结果；工具链不可用时记未完成 |
| CLI 构建矩阵 | 分别构建 `nctool-cli --no-default-features` 与 `nctool-cli --features math`，带 `--locked` 和不同 `--target-dir` | 固定两份二进制，实际验证 math 编译/配置/输入三条件 |
| 依赖审计 | `python scripts/audit_dependencies.py` | 依赖已有变化，需保留当前报告；网络失败记未完成，不自动升级或忽略告警 |
| 浏览器 | `scripts/browser_v2.cjs`、`scripts/browser_review_fixes.cjs`、`scripts/browser_ui_refactor.cjs` | 三组分别记录；固定 NCTOOL_BINARY、独立 Playwright/Chromium 和报告目录 |

执行前核对依赖及工具链是否已安装；确需恢复前端依赖时按锁文件使用 `npm ci --prefix ui`。普通审查不触发不必要的资源重建。

浏览器及 CLI 使用临时 `--home`、`--workspace`、空闲 loopback 端口（`ui --port 0`）。手动交互通过桌面 preview 工具启动/附加本轮宿主，按观察到的控件操作；自动批量回归沿用现有脚本。只结束本轮创建的进程和标签页。

组件测试不等于浏览器验收；编辑器 DOM 保持不等于选区和撤销历史正确。上述缺口需专项交互证据。

### 阶段 D：报告与收口

1. 将每条发现对应到具体文件/行、触发输入或操作、实际行为、影响、证据和建议修复方向。
2. 严重度按实际影响划分：P0 为立即阻断的广泛严重故障；P1 为资产损坏、错误交付或关键兼容/执行边界失效；P2 为局部功能与恢复缺陷；P3 为影响有限的工程问题。避免仅凭代码外观判严重度。
3. 区分“已复现”“代码可推导”“待验证”；待验证项单列，不当作已确认缺陷。可维护性建议与正确性缺陷分开。
4. 附验证矩阵：命令、平台、源码基准及未提交状态、通过/失败/未执行、报告路径。无发现也记录覆盖限制。
5. 若后续用户要求发布准备，另核对拟发布提交 SHA 对应的 Linux/Windows/macOS 实际 CI 与发行资产。当前本地结果不能证明远端成功；只读查询远端前先确认 remote。

## 4. 完成标准与交付物

- 全部 P0/P1 主题已沿数据流审阅，正常、失败、边界场景都有本轮证据或明确的未完成原因。
- P2 主题已有结论；高风险疑点已复核，没有把有意退役行为误报为缺陷。
- 检查结果均绑定到本轮源码状态，不继承历史测试数量或成功结论。
- 工作树已有改动与用户运行资产保持完整；不擅自修复、提交或发布。
- 交付一份按影响排序的审查报告、一份覆盖/验证矩阵，以及必要的最小复现材料和浏览器截图/日志。

完整审查结束不自动等于可发布。若发现 P0/P1 问题，先输出阻断项及证据；后续修复完成后重验相关入口，再更新报告。

## 5. 编制本计划时实际完成的工作

已读取项目审查/验证规则、贡献指南、项目状态、文档入口与 workspace 清单；执行了 `git status --short`、`git diff --stat`、相关文件 diff/numstat 及 HEAD 查询；定位了 WebUI 新增工作台模块。

尚未执行正式代码审查、测试、构建、浏览器验收、依赖审计或远端 CI 查询。现有文档中的 2026-10-15 Windows 成功记录仅作背景，不计入本计划的后续执行结果。
