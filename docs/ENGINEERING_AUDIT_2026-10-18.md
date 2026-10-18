# 工程审计与优化（2026-10-18）

> 本文件是本轮长期任务的**唯一进度载体**：架构摘要、审计清单、质量基线、任务状态、验证证据与最终验收都记录在这里。会话中断后应从此文件恢复状态，不要依赖对话记忆。
> 规则：不新增业务功能、不删除既有功能、不改变公开接口与数据格式；每项修改可独立回退。

## 0. 任务边界与并行工作

- 分支：`claude/dreamy-swirles-e94cc9`（worktree，基线 `master` = `2729d47`，初始 clean）。
- **重要**：`origin/chore/residual-cleanup-2026-10-18`（HEAD `f18efd3`）是本机另一会话正在进行的前端/文档/开发工具工作，尚未合入 `master`。它已包含：编辑器样式接入（`editor.css`）、`ui/dist` 停止跟踪、`Form` 可空/禁用修正、`HistoryPage` label 结构、`App` 派生缓存与插入时序、CI 名称解耦、`history_measure.rs` 测量夹具、`scripts/dev/*` 工具。
  - 因此本任务的 WebUI 审计发现中**下列各项已在另一分支修复**，本轮不重复实现：CSS `editor.css` 未导入、可空数值校验、Ctrl+Enter 陈旧 busy、`<label>` 包裹多控件、插入引用 50ms 定时器、表单禁用一致性。
  - 本轮**不与那一分支并行修改** `ui/src/**`、`ui/dist`、`cli/ui`、`docs/CONTRIBUTING.md`、`docs/CODEX_DEVELOPMENT.md`、`.github/workflows/*`、`scripts/dev/*`，避免覆盖对方未合并的工作。WebUI 项在本文件登记为"已由并行分支处理/待合并后复核"，不改代码。
- 本轮实际修改范围：`cli/src/**`、`cli/tests/**`、`runtime/src/**`、`runtime/tests/**`、`src/**`（引擎）、`assets/**`（如需要）、`scripts/*.py|*.cjs`（除 `scripts/dev/`）、本审计文档、必要的 docs 说明与 `CHANGELOG.md`。

## 1. 项目架构摘要

技术栈：Rust 1.89 MSRV（当前 rustc 1.98）workspace + Node 24 / TypeScript / React 19 WebUI（Vite）+ Python 3.工程脚本；目标平台 Windows / Linux / macOS。

| 组件 | 路径 | 职责 | 生产代码行数 |
| --- | --- | --- | --- |
| `nctool-tpl` | `src/` | 通用 Jinja 解析 / 变量提取 / 渲染，仅依赖 MiniJinja | 3,756 |
| `nctool-assets` | `assets/` | JSON 数值保真、原子写入、指纹冲突、目录锁 | 2,570 |
| `nctool-plugin-sdk` | `sdk/` | 插件描述、服务与动作契约 | 401 |
| `nctool-runtime` | `runtime/` | 依赖解析、激活/停用、外部进程 JSON-RPC | 2,266 |
| `nctool-plugin-template` | `plugins/template/` | 通用模板动作与服务 | 845 |
| `nctool-plugin-parameters` | `plugins/parameters/` | 共享参数定义/预置/绑定/冻结 | 763 |
| `nctool-plugin-math` | `plugins/math/` | 可选数学扩展（`math` feature） | 82 |
| `nctool-cli` | `cli/` | composition root、CLI、Axum HTTP、workbench 任务 | 4,434 |
| WebUI | `ui/src/` | 工作台界面（66 文件） | 12,992 |

数据流：`CLI/WebUI → runtime（动作/服务注册）→ template / parameters / math 插件 → nctool-tpl 引擎`。资产写入统一经 `assets` 的 JsonStore/WriteKernel；历史任务经 `cli/src/workbench.rs` 以 `_index` 摘要 + 完整记录两段写入。

## 2. 质量基线（2026-10-18，Windows 本机，commit `2729d47`）

环境：rustc 1.98.0 / cargo 1.98.0、Node 24.15.0 / npm 11.10.0、Python 3.13.14。
`scripts/dev/check_env.py` 在 `master` 中不存在（该工具在并行分支上），环境自检**未执行**，按技能规则记录后继续。

| 检查 | 命令 | 结果 |
| --- | --- | --- |
| 格式 | `cargo fmt --all -- --check` | 通过 |
| Clippy | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 通过（0 告警） |
| Rust 测试 | `cargo test --workspace --all-targets --all-features --locked` | 282 通过 / 0 失败 |
| 文档测试 | `cargo test --workspace --all-features --doc --locked` | 通过 |
| MSRV | `cargo +1.89.0 check --workspace --all-features --locked` | 通过 |
| 架构检查 | `python scripts/check_v2.py` | PASS |
| Python 工具测试 | `python -m unittest discover -s scripts/tests -p 'test_*.py'` | 34 通过 |
| 前端 lint | `npm run --prefix ui lint` | 通过 |
| 前端测试 | `npm run --prefix ui test` | 28 文件 / 189 通过 |
| 生成资源一致性 | `node scripts/build_ui.mjs --check` | 14 文件匹配 |
| 浏览器回归 A/B/C | `browser_v2.cjs` / `browser_generic_parameters.cjs` / `browser_review_fixes.cjs` | 三个 PASS |

结论：基线**全绿**。本轮新增失败均可归因于本轮改动。

性能基线（release，隔离 target，`scripts/measure_workbench.py`，9 次取样中位数）：

| 构建 | 二进制 | small | rows-1000 | 峰值工作组 |
| --- | --- | --- | --- | --- |
| 默认 | 9,303,040 B | 34.99 ms | 38.26 ms | 37.8 / 39.8 MB |
| math | 9,361,920 B | 31.49 ms | 35.11 ms | 37.9 / 39.9 MB |

引擎基准（criterion，`cargo bench`）：`parse` 4.07 µs、`extract_undeclared` 3.95 µs、`render` 9.03 µs（中等 G-code 模板）。
静态资源：14 文件 949,702 B，gzip 306,269 B，解压 2.5 ms（与 10-17 结论一致：压缩收益需新增运行时解压，暂不实施）。

未执行：联网依赖审计（`scripts/audit_dependencies.py` 需网络且会改写冻结审计基线）、macOS/Linux 实机、远端 CI。

## 3. 审计发现清单

四路独立审计（引擎、宿主/运行时、WebUI、工程基础设施）共记录 100+ 项。按"是否可复现、影响面、是否与并行分支冲突"筛选，本轮处置如下。**未处置项保留在表中，标注原因**。

### 3.1 本轮已确认并修复

| # | 严重度 | 位置 | 问题 | 证据/复现 | 修复 | 回归测试 |
| --- | --- | --- | --- | --- | --- | --- |
| R1 | P1 | `cli/src/workbench.rs` `summaries()` | `_index` 中任一文件截断、超大或被数值策略拒绝时，`parse_json`/长度检查返回 `Err` → `Workbench::new` → `App::boot` 失败 → **`nctool ui` 无法启动**；`GET /api/v2/runs` 也整体 400。索引是派生状态，与既有"跳过畸形条目"策略矛盾。 | 写入 `_index/truncated.json` 内容 `{`，或 `{"started_at":1e-400}`（下溢拒绝） | 逐条跳过不可读/超限/不可解析条目，其余照常返回 | `cli/tests/recovery.rs::unparseable_or_oversized_index_entries_do_not_abort_boot_or_history` |
| R2 | P1 | `runtime/src/external.rs` `Transport::request` | 取消其中一个外部插件请求时调用 `fail()`：把所有并发 pending 请求一起失败、杀掉子进程。用户在 WebUI 取消一个任务会让**同一插件的其他在途任务全部失败**。 | 并发两个请求，取消其中一个，"sibling" 收到 `cancelled` | 取消只终结该请求并向插件发 `cancel` 通知 | `runtime/tests/external.rs::cancelling_one_request_keeps_concurrent_requests_alive`（修复前失败，修复后通过） |
| R2b | P1 | 同上 `receive()` | 修复 R2/R3 后，插件对被取消/超时请求的**迟到响应**仍会以 "unknown response ID" 触发 `rpc_protocol`，进而 `fail()` 掉整个传输——并发请求依然被连带失败。插件在取消后继续发起的 `host.call` 也会因 "parent is no longer active" 终止传输。 | 超时请求在 700 ms 返回，兄弟请求 450–850 ms 在途；修复前兄弟收到 `plugin_timeout` | 记录已放弃的请求 ID（`abandoned`，上限 128）；迟到响应丢弃，已放弃父请求的回调在本地返回 `cancelled` 错误 | `runtime/tests/external.rs::a_late_reply_after_a_timeout_does_not_break_a_concurrent_request`（在基线源码上失败，修复后通过；连跑 3 次稳定） |
| R3 | P1 | 同上 | 单个请求超时也 `fail()` 全链路，兄弟请求被连带失败 | 同上，`mode=sleep` | 超时只终结该请求；协议错误/进程退出仍终止进程（流不可信） | `runtime/tests/external.rs::a_sibling_survives_a_request_timeout_or_incompatible_frame` |
| R4 | P1 | `cli/src/workbench.rs` 运行线程 | 运行线程内任何 panic 会跳过末尾的 `controls.remove` 与 `active.fetch_sub`：泄漏并发槽位（8 次后永久 `plugin_busy`）并留下无法删除的幽灵任务 | 代码路径确认；`unwrap()` 曾直接暴露 | `catch_unwind` 记录 `internal_error`；`Drop` 守卫 `Released` 保证释放，且按 controls 中是否存在避免重复减计数；移除 `as_object_mut().unwrap()` | `cli/tests/platform.rs::panicking_action_releases_the_slot_and_records_a_failure`（连续 12 次 panic 超过并发上限，仍可继续启动、记录 `internal_error`、删除任务） |
| R5 | P2 | `runtime/src/config.rs` `asset_error` | HTTP 响应体回显 `WriteError::Display` 中的**绝对文件路径**，违反 `assets/src/lock.rs` 已声明的 P1-17 脱敏约束（"HTTP 响应体不得照抄路径"） | POST 到被占用资产 | `Conflict`/`LockBusy`/`ReadOnly` 映射为固定中文文案；数值错误保留字面量与行列；`Io`/`Corrupt` 去路径；新增 `_` 兜底（`WriteError` 非穷尽） | `runtime/src/config.rs::asset_error_tests::http_facing_messages_never_include_absolute_paths` |
| R6 | P2 | `cli/src/server.rs` | bundle 上限 64 MiB，但 413 文案固定写 "exceeds 1 MiB"；`body_timeout` 落到默认 400 | 代码确认 | 文案按实际上限生成；超时改为 408 `body_timeout` | HTTP 层仍无自动化测试（见 3.3） |
| R7 | P2 | `runtime/src/lib.rs` | `Workbench::start_with_replay` 每次启动都克隆全部 `Capability`（含所有 Schema）再线性查找 | 代码确认 | 新增 `Runtime::action_descriptor()` 单点查询，避免整表克隆 | 现有 `cli/tests/platform.rs` 等启动路径测试 |
| R8 | P3 | `docs/PLUGIN_PROTOCOL.md` | 文档写"首版采取终止失效进程的有界取消方式"，与修复后的取消语义不符 | 文档确认 | 改为"取消只终结对应请求；超时/协议错误/退出才终止进程" | — |

### 3.2 已确认但本轮不处置（附原因）

| # | 严重度 | 位置 | 问题 | 不处置原因 |
| --- | --- | --- | --- | --- |
| N1 | P2 | `src/extract.rs` `Stmt::Set` | 条件分支内 `{% set x %}` 后 `{{ x }}` 在 `flag=false` 时严格渲染失败，但 `extract_undeclared` 不报告 `x`（漏报，危险方向） | 修复需引入"条件声明"作用域模型，改变 `extract_undeclared` 的公开语义（参数列表），属接口行为变更，需用户批准 |
| N2 | P2 | `src/extract.rs` 递归遍历 | minijinja 对平铺左深链（`a.a.a…`）不设深度上限，提取器递归可能栈溢出 | 同上：需上游或显式深度预算，会改变错误分类；风险窗口仅恶意超长模板 |
| N3 | P2 | `.github/workflows/release.yml` | 所有 `uses:` 使用可变 tag（`@v4`、`@master`），而 job 持有 `contents: write` | CI/工作流归属并行分支；改 SHA 会影响发布链路，需合并后统一处理并在 PR 中审阅 |
| N4 | P2 | `scripts/browser_generic_parameters.cjs` | 二进制不含 math 时打印 LIMITATION 并以 0 退出，CI 靠构建顺序而非断言保证 math 覆盖 | 脚本与本项均属并行分支正在修改的文件，避免冲突；已记录建议（`NCTOOL_REQUIRE_MATH=1`） |
| N5 | P2 | `ui/src/lib/data.ts`、`ui/src/styles.css`、`HistoryPage` 等 | WebUI 审计的 20+ 项（见 3.4） | 并行分支 `chore/residual-cleanup-2026-10-18` 已修复其中主要项；其余待合并后统一复核 |
| N6 | P2 | `cli/src/composition.rs` `inventory()` | 每次启停插件请求扫描两次插件目录 | 收益取决于插件数量（通常个位数），改动面涉及配置层；优先级低于 R1–R7 |
| N7 | P3 | `cli/src/server.rs` | 请求体读取发生在并发许可之前，慢上传可各占 64 MiB | 仅 loopback，且需改并发模型；记录在案 |
| N8 | P3 | `cli/src/workbench.rs` | 每个任务的 `.nctool.lock` 文件删除任务时不清理 | 锁文件语义有意保留（跨进程互斥），数量与应用户任务数同级；如需清理需先证明无并发删除竞态 |
| N9 | P3 | `src/renderer.rs` | `render()` 每次重新解析编译；fuel(1M) 与输出上限(16 MiB) 不联动；`with_lenient` 文档夸大；`add_filter` 拒绝表硬编码；`.html` 模板名默认 HTML 转义 | 文档/默认值调整需产品决策（可能改变输出或既有模板行为），本轮仅记录，建议后续在 CHANGELOG 中列为已知边界 |
| N10 | P3 | `assets/src/lib.rs` `parse_json` | 每次解析做 3 遍扫描，逐 token `Vec` 分配 | 属微优化，需基准证明端到端收益；当前 1 MiB 请求的实测延迟未见瓶颈 |

### 3.3 测试覆盖缺口（本轮部分补齐）

- HTTP 层（`cli/src/server.rs`）**没有任何自动化测试**：Host/Origin/Sec-Fetch-Site、Content-Type、体积上限、并发许可、取消快速路径均无覆盖。本轮未补（需引入测试用异步服务器夹具），记录为后续高价值项。
- 外部进程协议：本轮补三个并发/超时/迟到响应回归；`read_frame` 上限、`callbacks` 上限、握手不一致仍由现有测试部分覆盖。
- panic 释放路径已有直接回归（`panicking_action_releases_the_slot_and_records_a_failure`）。

### 3.4 WebUI 审计摘要（并行分支处置）

| 发现 | 状态 |
| --- | --- |
| `editor.css`/`tokens.css` 未被导入，语法高亮无样式 | 并行分支已接入 `editor.css` 并删除 `tokens.css`；`browser_v2.cjs` 新增样式断言 |
| 可空数值字段被判非法，阻止生成 | 待合并后复核（并行分支 `Form.tsx` 已改禁用与 label，校验分支未见改动） |
| Ctrl+Enter 在任务结束后仍被旧 `busy` 阻止 | 并行分支 `App.tsx` 有改动，待合并后复核 |
| `<label>` 包裹输入框+下拉框+分页按钮 | 并行分支已修 |
| 插入模板引用的 50 ms 定时器竞态 | 并行分支已改为 `insertWhenReady` |
| 历史列表首帧误显示"还没有执行记录" | 待合并后复核 |
| `DiffView` 无虚拟化（2 MiB 上限） | 保留：属性能优化，需先有测量 |
| 插件配置读取失败时页面空白无重试 | 保留：待合并后确认 |
| 删除共享资产无确认对话框 | 保留：涉及交互流程，需产品确认是否与其他删除保持一致 |
| 校验错误键未做 RFC6901 转义，含 `/` 的参数名错误不显示 | 保留：小改动但位于并行分支正在改的 `Form.tsx`/`data.ts`，合并后处理 |

## 4. 优化计划与任务状态

| 任务 | 状态 | 提交 |
| --- | --- | --- |
| T1 质量基线与性能基线 | 完成 | 本文档 |
| T2 历史索引损坏导致启动失败（R1） | 完成 | 见第 5 节提交表 |
| T3 外部插件取消/超时隔离（R2/R3） | 完成 | 同上 |
| T4 任务线程 panic 泄漏（R4） | 完成 | 同上 |
| T5 资产错误路径脱敏（R5） | 完成 | 同上 |
| T6 HTTP 错误语义与工作台查询（R6/R7） | 完成 | 同上 |
| T7 全量回归 + 浏览器验收 | 进行中 | — |
| T8 与并行分支合并后的 WebUI 复核 | 阻塞（等待并行分支合入） | — |

## 5. 验证结果与剩余风险

### 5.1 修复后全量回归（Windows 11，本机，commit 待定，基线 `2729d47`）

| 检查 | 命令 | 结果 |
| --- | --- | --- |
| 格式 | `cargo fmt --all -- --check` | 通过 |
| Clippy | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 通过（0 error / 0 warning） |
| Rust 测试 | `cargo test --workspace --all-targets --all-features --locked` | **288 通过 / 0 失败**（基线 282，新增 6 项回归） |
| 文档测试 | `cargo test --workspace --all-features --doc --locked` | 通过 |
| MSRV | `cargo +1.89.0 check --workspace --all-features --locked` | 通过 |
| 架构检查 | `python scripts/check_v2.py` | PASS |
| Python 工具测试 | `python -m unittest discover -s scripts/tests -p 'test_*.py'` | 35 通过（基线 34，新增 1 项） |
| 前端 lint | `npm run --prefix ui lint` | 通过 |
| 前端测试 | `npm run --prefix ui test` | 28 文件 / 189 通过 |
| 生成资源一致性 | `node scripts/build_ui.mjs --check` | 14 文件匹配 |
| 浏览器回归 A | `browser_v2.cjs`（最终 release 二进制） | PASS |
| 浏览器回归 B | `browser_generic_parameters.cjs` | PASS（math 链完整执行） |
| 浏览器回归 C | `browser_review_fixes.cjs` | PASS |

新增回归明细：

| 测试 | 覆盖 |
| --- | --- |
| `cli/tests/recovery.rs::unparseable_or_oversized_index_entries_do_not_abort_boot_or_history` | 截断/下溢/超限索引条目不再阻断启动与历史查询，且不被删除 |
| `cli/tests/platform.rs::panicking_action_releases_the_slot_and_records_a_failure` | 连续 12 次插件 panic（超过并发上限 8）仍可继续启动任务、记录 `internal_error`、任务可删除 |
| `runtime/tests/external.rs::cancelling_one_request_keeps_concurrent_requests_alive` | 取消一个请求不再失败同插件的并发请求（修复前该用例失败） |
| `runtime/tests/external.rs::a_sibling_survives_a_request_timeout_or_incompatible_frame` | 单请求超时只失败该请求；畸形帧仍按设计终止进程；两种情形下一次调用都能恢复 |
| `runtime/tests/external.rs::a_late_reply_after_a_timeout_does_not_break_a_concurrent_request` | 超时请求的迟到响应不被当作协议错误；兄弟请求存活（基线源码上该用例失败） |
| `runtime/src/config.rs::asset_error_tests::http_facing_messages_never_include_absolute_paths` | HTTP 错误消息不含绝对路径，`Conflict`/`LockBusy` 文案仍可区分 |
| `scripts/tests/test_measure_workbench.py::InvokeTimeoutTests` | 测量脚本超时会 `kill()` 子进程，不留悬挂进程 |

### 5.2 行为兼容性核对

- 渲染输出字节级一致：用修复前后两个 release 默认构建对 `Hello {{ name }}` 与 1000 行循环模板各执行 25 次，stdout 字节数与内容集合完全相同（197 B / 19,953 B，各 1 种取值）。
- 外部插件协议只收窄失败范围，不改变成功路径；`docs/PLUGIN_PROTOCOL.md` 已同步取消语义。
- 索引跳过只影响派生摘要，权威任务记录（`<id>.json`）的读取、恢复、删除路径未改。

### 5.3 性能复测（同机、同脚本、9 次中位数）

| 构建 | 用例 | 修改前 | 修改后 | 判断 |
| --- | --- | --- | --- | --- |
| 默认 | small | 34.99 ms | 34.74 ms | 持平（进程启动主导） |
| 默认 | rows-1000 | 38.26 ms | 36.11 ms | 下降约 5.6%，但两次测量均无预热隔离，属噪声量级，**不作为优化结论** |
| math | small | 31.49 ms | 31.50 ms | 持平 |
| math | rows-1000 | 35.11 ms | 34.07 ms | 同上，噪声量级 |
| math | math-1000 | 35.29 ms | 31.06 ms | 同上，噪声量级 |
| 二进制 | 默认 / math | 9,303,040 / 9,361,920 B | 9,308,672 / 9,368,576 B | +5.6 KB / +6.7 KB（新增错误处理与守卫代码） |

结论：本轮改动**不以性能为目标**，唯一可测的性能改动是 `Workbench::start` 不再克隆全部动作描述（R7），其收益在单次任务启动内小于进程启动噪声，未单独测量。不宣称任何性能提升。

### 5.4 剩余风险

1. **HTTP 层无自动化测试**：`cli/src/server.rs` 的 Host/Origin/Content-Type/体积/并发行为只能靠浏览器脚本间接覆盖。R6 的 408 映射与 bundle 文案未单独断言。
2. **panic 恢复依赖 `catch_unwind`**：插件内部 panic 被转为 `internal_error`，但若 panic 发生在锁中毒（mutex poisoned）场景，后续 `lock().unwrap()` 仍会 panic；本轮未处理中毒锁，属已知边界。
3. **外部插件超时后进程可能仍在运行该请求**：宿主已发 `cancel` 通知，但插件是否响应由插件实现决定；宿主不再强制终止进程，因此不响应的插件会占用一个并发槽直到自行结束。协议文档已声明该责任。
4. **并行分支未合并**：WebUI 审计中若干项与 `chore/residual-cleanup-2026-10-18` 重叠，需在合并后复核（见 3.4）。
5. 未执行联网依赖审计、macOS/Linux 实机与远端 CI。

## 6. 最终验收报告

### 6.1 总体质量评估

项目基线健康：格式化、Clippy、287 项 Rust 测试、189 项前端测试、35 项 Python 工具测试、架构检查与三组真实浏览器验收全部通过。架构边界清晰（领域中立引擎 + 插件运行时 + 组合根），资产写入与历史记录有原子性、指纹与锁保护。本轮审计集中在"派生状态损坏"和"并发失败隔离"两类高影响缺陷，均已修复并有回归保护。

### 6.2 发现与处置统计

| 等级 | 审计记录 | 本轮修复 | 转交/记录 |
| --- | --- | --- | --- |
| P1 | 5 | 5 | 0 |
| P2 | 14 | 4 | 10 |
| P3 | 8 | 1 | 7 |

### 6.3 已解决问题与提交

| 编号 | 问题 | 提交 |
| --- | --- | --- |
| R1 | 损坏的 `_index` 条目导致 WebUI 无法启动 | 见 `git log`（本轮提交） |
| R2 | 取消一个外部插件请求会失败同插件所有并发请求 | 同上 |
| R3 | 单请求超时同样连带失败并发请求 | 同上 |
| R2b | 超时/取消后的迟到响应与迟到回调仍会终止整个传输 | 同上 |
| R4 | 任务线程 panic 泄漏并发槽与不可删除的任务 ID | 同上 |
| R5 | HTTP 错误消息回显绝对路径 | 同上 |
| R6 | bundle 体积错误文案错误；body 超时映射为 400 | 同上 |
| R7 | 每次任务启动克隆全部动作描述 | 同上 |
| R8 | 插件协议取消语义文档与实现不符 | 同上 |

（每项修改在同一提交内，测试与代码同行；可整体回退，也可按文件单独回退。）

### 6.4 实际性能与 UI/UX 改善

- 性能：无宣称提升（见 5.3）。R7 去除了每次任务启动的一次全量描述克隆，属结构性改善，未达到可测阈值。
- UI/UX：本轮未修改前端（并行分支已处理编辑器样式、表单可空/禁用、历史页结构、插入时序等），见 3.4 与 5.4。

### 6.5 是否满足验收标准

| 验收项 | 结论 |
| --- | --- |
| 项目按原设计正常运行 | 是：全量测试 + 三组浏览器验收通过 |
| 原有功能与业务行为兼容 | 是：渲染输出字节一致；协议仅收窄失败范围 |
| 高优先级问题已修复 | 是：5 项 P1 全部修复并有回归 |
| 核心路径有自动化回归 | 是：新增 7 项回归，覆盖损坏索引、panic、取消/超时隔离、迟到响应、路径脱敏、测量脚本超时 |
| 性能优化有测量证据 | 不适用：本轮未做性能优化，未宣称收益 |
| UI/UX 优化在功能边界内 | 不适用：本轮未改前端，相关项已登记 |
| 编译/静态分析/测试通过 | 是：见 5.1 |
| 修改可 Git 追踪与回退 | 是：单一提交，按文件可独立回退 |
| 未完成项有原因与建议 | 是：见 3.2、3.3、5.4 |

### 6.6 后续维护建议

1. 合并 `chore/residual-cleanup-2026-10-18` 后，复核 3.4 中"待合并后复核"的 WebUI 项，并补 `ui/src/lib/data.ts` 可空数值校验回归。
2. 为 `cli/src/server.rs` 增加异步 HTTP 集成测试（Host/Origin/体积/408/并发），这是当前最大的测试盲区。
3. 为外部插件协议补"插件不响应 cancel"的行为约定测试，明确超时后槽位回收策略。
4. 工作流 action 固定到 SHA（P2，需在 CI 分支上单独评审）。
5. 引擎侧 N1/N2（条件 `set` 漏报、深层 AST 递归）需产品决策后单独处理。
