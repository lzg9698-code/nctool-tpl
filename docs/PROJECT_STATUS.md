# nctool 项目进度分析

> 生成时间：**2026-09-18**（上一版 2026-09-10，本次为全量重写）
> 分析依据：本文数字**全部为实测值**，非文档转抄 —— `git log` / `git status`、
> `cargo test --workspace --all-targets`（536 项）、
> `cargo llvm-cov --workspace --all-features --lcov` + `scripts/check_coverage_caliber.py`
> （生产口径行 88.65%）、
> `cargo fmt --all --check`、`cargo clippy --workspace --all-targets`、
> CLI 与模板目录实际执行、CI run #34 的 jobs API 明细。
> 定位：本文是**现状快照**，不是规划。规划以 `ROADMAP.md` 为准。
> 上一版（09-10）记 344 项测试 / 6 个内置模板 / 覆盖率门未生效 —— 这三项均已过时，见 §4 与 §5。

---

## 0. 一页速览（TL;DR）

| 项目 | 结论 |
|---|---|
| **整体进度** | **发版已完成**（2026-09-18）：阶段 A–F 的交付项只剩 2 条外部依赖/Backlog。三个 crate 已发布 crates.io（tpl 0.4.0 / core 0.3.0 / cli 0.3.0），三个 GitHub Release 已带三平台二进制 |
| **清单记账** | ROADMAP §8 执行跟踪 **70 / 72 项（97%）** 勾选（A 12/13 · B 12/13 · C 12/12 · D 12/12 · E 12/12 · F 10/10）。本轮共新勾 4 项（E2.2 / E2.3 / B-Backlog / F1.3），均有 CI 或发布物取证；余 2 项为 A1 机床预设按键值核对（无手册资源）与 B4.2（Backlog） |
| **发布状态** | 三个 crate 在 crates.io 可见（实测指定版本端点 `created_at` 为 2026-09-18T05:01–05:02Z）；三个 Release 各挂 3 个二进制（Windows 4.25 MB / Linux 3.62 MB / macOS-arm64 3.09 MB）。已下载 Windows 产物实测 `nctool 0.3.0` 可运行并正确产出 G-code；亦以 `cargo install nctool-cli --version 0.3.0` 从 crates.io 全新安装验证通过 |
| **功能可用性** | CLI **10 个子命令**全部可用（除 `part` 占位）+ Web UI 完整交互闭环；模板库已整合 NCTool_V3 资产：**7 个内置 + 25 个文件模板**（隐藏 3） |
| **测试基线** | workspace **536 项**实测（`--all-targets`），**536 通过 / 0 失败**；另 2 项 `#[ignore]`（万行实测，需 `--release --ignored`）与 1 项 doc-test（`core/src/model.rs`，标记 `#[ignore]`）；`cargo test --workspace --doc` 另有 1 项通过（`src/lib.rs`） |
| **覆盖率** | **生产口径**行 **88.65%**（4319/4872，门禁 ≥ 88%）；对照 llvm-cov 原始口径 92.99% —— 二者差 4.34pt，因为原始口径把 `src/*.rs` 内的 `#[cfg(test)]` 段计入分母，会随「新增测试」虚涨，**不作门禁** |
| **CI 状态** | 三平台矩阵（fmt / clippy `-D warnings` / test `--workspace` / doc `-D warnings` / audit）+ 覆盖率门 + 前后端对拍门。发版提交 `3031a71`（run #37）**全绿**；此前连续多次绿灯（最近 `db8d928` / `6e5a3a7`） |
| **✅ 本轮（09-15 ~ 09-18）** | ① 架构评估 P0×3 + P1×4 + P2-1 **全部收口** ② 覆盖率从「未度量」到真门禁并两次上调（无 → 89% → 90%；**09-18 改为生产口径 ≥ 88%**，见 §4）③ CI 补 `--workspace`（被测项 155 → 527）④ NCTool_V3 模板资产整合（`manifest.rs` / `variables.rs` / `derive.rs` + 25 模板 + INDEX G420 全套）⑤ server.rs 接口契约补测 10 项 ⑥ **发版**：0.4.0 / 0.3.0 / 0.3.0 上线 crates.io + 三平台 Release 二进制 |
| **当前最大问题** | ① `part generate` 未实现（Backlog #2）② 工艺评审长期外部依赖（R1，Q2=否）③ 覆盖率洼地 `cli/src/commands/ui.rs` 23.08%、`src/extract.rs` 79.89% ④ 第三轮审查批次三/四未做，其中 Web UI 存储型 XSS（P1-1）是剩余项里唯一的真安全问题 ⑤ 发版遗留：`Release` 工作流的 `Publish <crate>` 步骤会红（Q11 见 §6.4） |

**一句话**：地基、功能、联调、质量门、发版全部完成且全绿；剩下的只有「外部工艺评审」与 Backlog 里的增量功能。

---

## 1. 分阶段进度明细

阶段划分、工期、优先级来源：`ROADMAP.md` §3。下表「清单」列 = ROADMAP §8 勾选状态（2026-09-18 实测统计）。

| 阶段 | 目标 | 优先级 | 人日 | 清单 | 未完成项 | 状态 |
|---|---|---|---|---|---|---|
| **A** 需求与设计收口 | 让"正确性"可被证明 | P0 | 3–5 | 12/13 | A1 子项：3 机床预设按手册核对（外部依赖） | ✅ 基本完成 |
| **B** 基础架构稳固 | 还清债务，地基干净 | P0 | 3–4 | 12/13 | B4.2 golden 变更保护步骤化（Backlog） | ✅ 基本完成 |
| **C** UI 服务与前端联通 | 浏览器能看真实模板库 | P1 | 5–8 | 12/12 | — | ✅ 完成 |
| **D** 完整交互闭环 | 浏览器内能完整生成 | P2 | 8–12 | 12/12 | — | ✅ 完成 |
| **E** 联调测试 | 让"可用"可被证明 | P1 | 4–6 | **12/12** | — | ✅ 完成（本轮勾 E2.2/E2.3） |
| **F** 上线与迭代 | 能装上、能用、能反馈 | P0(发版) | 3–5 | **10/10** | — | ✅ 完成（本轮勾 F1.3） |
| **合计** | | | **26–40** | **70/72 (97%)** | 见 §6.1 | ≈97% |

### 1.1 阶段 A — 需求与设计收口（12/13）

**已完成：** A0 前提假设、A1 工艺核对清单（**自动核对版**：`PROCESS_CHECKLIST.md` §1–§7 逐行核对 7 个内置模板 × 3 预设，P0 错误 0）、A2 golden 基线（现 **21 组正向 + 3 组负向**）、A3 1.0 API 冻结清单、A4 MachineConfig 键名 schema（20 键）、A5 确认纪要。

**唯一未完成项（外部依赖）：** 3 个机床预设键值按**手册**核对 —— 无手册资源，已按降级预案声明「仅供开发测试，待外部评审」。A1 是 AI/代码级自动核对，**非**真实工艺评审；README 显著声明。

### 1.2 阶段 B — 基础架构稳固（12/13）

B1–B5 全部完成。**本轮新勾 B-Backlog**：CI coverage job 根因已修复（见 §5.6），覆盖率自 09-17 起是真门禁。仅 B4.2（golden 变更保护步骤化）仍在 Backlog —— `NCTOOL_UPDATE_GOLDEN` 机制可用，人工刷新流程未文档化。

### 1.3 阶段 C — UI 服务与前端联通（12/12）

`nctool ui`（tiny_http，**仅回环**）+ 全部契约端点（templates / templates/{name} / machines / inspect / validate / render / part 占位）+ 前端 server 模式接线。**非回环地址现在是直接拒绝**（含 `localhost` 主机名，只接受 IP 字面量），比原规划「警告后允许」更严格。

### 1.4 阶段 D — 完整交互闭环（12/12）

参数表单（消费参数规格：候选值下拉 / 派生参数移出表单 / 条件必选标注触发条件）、防抖预览、校验面板、生成选项、复制/下载、机床切换、主题与响应式全部可用，人工验收 37/37 通过（09-10）。

### 1.5 阶段 E — 联调测试（12/12，本轮收口）

CLI E2E 契约、HTTP 契约、21 组正向 golden + 3 组负向报告、错误边界、413、万行性能均已覆盖。**本轮勾选 E2.2 / E2.3**：取证为 CI run #34（`d3d144f`）的 `ubuntu-latest` 与 `macos-latest` quality job 均 conclusion = success（XDG 配置路径与 Unix 行尾由该 job 覆盖）。

### 1.6 阶段 F — 上线与迭代（10/10，本轮收口）

文档（README / CHANGELOG / 指南）、Issues 模板、迭代节奏、Backlog 排序均已完成。**本轮勾选 F1.3**：push 三个 tag 后 Release 工作流跑通三平台原生构建，三个 GitHub Release 各带 3 个二进制资产；三个 crate 同步发布到 crates.io。发布物已验证（下载 Windows 产物实测可运行 + `cargo install` 从 crates.io 全新安装实测可运行）。唯一遗留是 `Publish <crate>` 步骤会红，成因与处置见 §6.4。

---

## 2. 已完成能力清单（端到端可跑）

| 能力 | 入口 | 状态 |
|---|---|---|
| 模板解析 / 变量提取 | `tpl::parse` / `extract_variables` / `extract_undeclared` / `extract_template_refs` | ✅ |
| 参数校验 | `core::validate` + `ValidationReport`（行列定位，15 类 `IssueKind`） | ✅ |
| G-code 生成 | `GCodeGenerator`（严格 / 宽松）+ 后处理（行号 / ASCII / 空行 / 头注释） | ✅ |
| 模板注册表 | `TemplateRegistry`，**7 个内置模板** + 文件模板发现（递归） | ✅ |
| 模板清单元数据 | `core::manifest`（`templates.yaml`：分类/描述/可见性/输出后缀/params 覆盖层） | ✅ |
| 变量库 | `core::variables`（`variables.yaml`：按变量名的全局规格） | ✅ |
| 派生计算上移 | `core::derive`（查表换算从模板搬到规格，模板只做替换） | ✅ |
| 参数规格系统 | 类型 / 候选值 / 区间 / 整数 / 条件必选 `required_if` / 派生 `derive` | ✅ |
| 机床预设 | `generic` / `wfl_m65` / `index_ms40` + 20 键 schema + `nctool.toml` 自定义 | ✅ |
| CLI 全命令 | templates / inspect / validate / render / generate / machine / config / ui / completion | ✅ 除 `part` 外全通 |
| Web UI | 服务模式全链路 + 跨站防护（Origin / Sec-Fetch-Site）+ CSP 等安全头 | ✅ 37/37 验收 |
| golden 回归 | **21 组正向**（7 模板 × 3 预设，输出 + 校验报告）+ **3 组负向**（缺参/类型/越界报告） | ✅ 全绿 |
| **未通** | `part` 命令（占位 exit 7）、`part generate` 批量生成（Backlog #2） | ❌ |

**内置模板 7 个**：`program_header`、`program_footer`、`tool_change`、`safe_move`、`drill_cycle`、`facing`、`slot_milling`。
**文件模板 25 个**：车削 5（含 `undercut` ES/FS 拆分版 + 内部片段）、切槽 1（`circlip_groove`）、机床 19（INDEX G420 方案包）。

---

## 3. 本轮（2026-09-15 ~ 09-18）交付

按性质分四类，全部已提交并推送（`21b4d34` → `3031a71`，共 12 个 commit；发版产物见 §0 与 §6.4）：

**一、架构评估收口（`docs/ARCHITECTURE_REVIEW.md`）**

| 编号 | 问题 | 状态 |
|---|---|---|
| P0-1 | 注册表无缓存，每请求全量重建 | ✅ 已修复 |
| P0-2 | 覆盖率门禁未生效（`continue-on-error` 掩盖） | ✅ 已修复（见 §5.6） |
| P0-3 | 同模板单次渲染重复解析 2–3 次 | ✅ 已修复 |
| P1-1 | `--param` 归一逻辑双份实现，等价性无测试 | ✅ 已修复（共享 fixture 对拍 40 例 + CI 门禁） |
| P1-2 | 最微妙逻辑（`extract.rs`）零内联测试 + 无属性测试 | ✅ 已修复（18 项样例 + 300 例属性测试，未引入 proptest） |
| P1-3 | 存在绕过校验的公开渲染入口（可输出 `NaN`） | ✅ 已修复（`render_template*` 加有限性闸门） |
| P1-4 | 本地 UI 无 Origin/CSRF 校验与 CSP | 🟡 部分（判定 + 安全头已加；一次性 token 未做，取舍见 CHANGELOG） |
| P2-1 | `check_vars` 248 行单函数 | ✅ 已修复（55 行编排 + 7 个专职函数） |
| P2-2 ~ P2-6 | 数据表内联 / `model.rs` 拆分 / 扩展点 / UI 单源 / 上下文复用 | ⬜ 未动（P2-5 经评估**决定不做**，理由已记录） |

**二、质量门建设**

- CI 三条命令补 `--workspace`：此前 CI **只检查根 crate**（155 项），core / cli 从未真正被检查却一直显示全绿 —— 修后被测项 155 → 527。
- 覆盖率从「未度量」到**真门禁**：安装方式改 `taiki-e/install-action`（根因见 §5.6），移除非阻断，设 `--fail-under-lines`，09-17 由 89 上调至 90。（**09-18 更正**：该阈值口径失真 —— llvm-cov 把 `#[cfg(test)]` 段计入分母，加测试会推高数字。已改为 `scripts/check_coverage_caliber.py` 判定的**生产代码口径 ≥ 88%**，见 §4。）
- 补测两处覆盖率洼地：`cli/src/output.rs`（65.22% → 98.41%，承载退出码矩阵与 `--format json` 包络契约）、`cli/src/server.rs`（74.01% → 86.83%，承载错误分类 / `blocked` 语义 / `options` 后处理契约）。

**三、模板资产整合（NCTool_V3 → 本项目）**

新增 `core/src/manifest.rs`（1496 行）、`core/src/variables.rs`（367 行）、`core/src/derive.rs`（317 行）；递归模板发现；`templates.yaml` 清单与 `variables.yaml` 变量库；移植 25 个 `.j2`（含 INDEX G420 全套 19 个机床模板）与越程槽 ES/FS 拆分版；引入 `Choice` / `List` / `Any` 参数类型、`required_if` 条件必选、角度制三角函数过滤器（`sin_d` 等）。

**四、文档与安全**

`docs/SYSTEM_DESIGN.md`（v2.0 权威设计）、`docs/architecture-overview.html`（自包含架构图）、`docs/ARCHITECTURE_REVIEW.md`（六维度评分，总体 7.5）、`docs/TEMPLATE_INTEGRATION_PLAN.md`；本地 UI 加跨站防护与安全响应头；本轮修正本文与 ROADMAP 的文档漂移（见 §6.3）。

---

## 4. 测试与质量基线（2026-09-18 实测）

| crate / 目标 | 单元 | 集成 | 小计 | 结果 |
|---|---|---|---|---|
| nctool-tpl | 137 | 18 + 1(属性) | 156 | ✅ 全过 |
| nctool-core | 203 | 11 + 3(万行) | 217 | ✅ 全过 |
| nctool-cli | 76 | 43 + 44(E2E) | 163 | ✅ 全过 |
| **合计** | **416** | **120** | **536** | **536 通过 / 0 失败** |

另 2 项 `#[ignore]`（`large_program` 万行实测，需 `--release --ignored`）、1 项 doc-test
`#[ignore]`（`core/src/model.rs`）。上表是 `--all-targets` 的口径，**不含 doctest** ——
CI 为此单列 `Doc tests` 步骤（`cargo test --workspace --doc`，当前 1 项通过）。

**覆盖率**

| 口径 | 行覆盖 | 说明 |
|---|---|---|
| **生产代码**（门禁口径） | **88.65%**（4319/4872） | `scripts/check_coverage_caliber.py` 剔除 `src/*.rs` 内的 `#[cfg(test)]` 段后统计 |
| llvm-cov 原始 | 92.99%（8869/9538） | 把测试段本身计入分母（9538 行中 6908 行是测试代码），**会随新增测试虚涨，不作门禁** |
| 门禁 | — | **生产口径 ≥ 88%**（CI 真门禁，余量约 32 行） |

覆盖洼地（下一步补测优先级）：

| 文件 | 行覆盖（生产口径） | 说明 |
|---|---|---|
| `cli/src/commands/ui.rs` | **23.08%** | 全项目最低（13 行生产代码仅 3 行覆盖）—— 子进程启动路径难以单测 |
| `src/extract.rs` | **79.89%** | 承载「必选/可选判定」这一最微妙逻辑（判错会静默产出错误 G-code 或误拦用户）；已补 18 样例 + 300 例属性测试，未覆盖的疑为分支组合 |
| `cli/src/commands/inspect.rs` | 80.79% | 输出层 |
| `cli/src/server.rs` | 83.46% | 本轮刚补过（74% → 83%） |
| `core/src/variables.rs` | 83.70% | 变量提取 |

覆盖高地：`cli/src/output.rs` 98.41%、`core/src/validate.rs` 96.66%。

**其他质量信号**

- `cargo fmt --all --check`：干净。
- `cargo clippy --workspace --all-targets`：零告警。
- 生产代码规模：`src` 3430 + `core/src` 8182 + `cli/src` 4527 = **16139 行**；测试文件 2673 行（另有各模块内联测试）。
- `ui/index.html` **2685 行** ×2 副本（md5 一致），上限 3000 行 → **余量收窄至 315 行**（见 §6.2 R3）。

---

## 5. 已确认缺陷清单

### 5.1 【✅ 已修复，09-05】`inspect` 路由死代码

外层 `("GET", p)` 已把 `method` 绑定为 `"GET"`，内层 `("POST", ...)` 恒不成立。已提层修复并补单测。

### 5.2 【✅ 已收口，09-05】契约端点补齐

`/api/render`、`/api/validate` 曾为 404；已下沉复用 core 管线实现并补 7 项路由单测 + 真实 HTTP 契约测试。

### 5.3 【✅ 已修复，09-05】`ui` 集成测试永久挂起

旧断言假设 `ui` 未实现；已替换为随机端口 + `/health` 探测 + 主动终止。

### 5.4 【✅ 已解决，09-18 更正】非回环绑定

上一版本文记「`--host 0.0.0.0` 会打印警告后直接绑定」——**与现行代码相反**。现行 `server::listen_addr` 对非回环地址**直接报错拒绝**（`is_loopback()` 判定），`localhost` 等主机名也一并拒绝（只接受 IP 字面量），由 `cli/src/commands/ui.rs` 单测守护。比原加固建议更严格，威胁模型已闭合。

### 5.5 【✅ 已修复，2026-09-10 走查发现】三个前端缺陷

| # | 缺陷 | 修复 |
|---|---|---|
| 1 | 参数规格必选/默认值解析（server 模式）：`default:null` 被判为「可选」、`{type,value}` 对象被直填成 `[object Object]` | 新增 `paramDefault()` 统一取值（五处），口径与 CLI `validate` 一致 |
| 2 | 数据源文本不更新（服务模式仍显示「演示数据」） | 加载成功/失败分别更新文本 |
| 3 | 移动端输入框字号：`@media` 内 16px 被更高特异性规则覆盖 → iOS Safari 聚焦放大整页 | 媒体查询内追加同特异性后定义规则 |

### 5.6 【✅ 已修复，2026-09-17】CI coverage job 长期红灯（根因）

**两层掩盖**：① `continue-on-error: true` 把**持续失败**粉饰成非阻断项 —— 「CI 全绿」并不代表覆盖达标，覆盖率实际**从未被度量**；② 安装方式不当 —— `cargo install cargo-llvm-cov` 要在 runner 上从源码编译整套依赖，`--locked` 撞上 crates.io 索引漂移，连续两次 exit 101 卡死于该步骤。

**修复**：改用官方 `taiki-e/install-action@cargo-llvm-cov` 下载**预编译二进制**（绕开整条 cargo 构建链，不受索引状态影响），并单独加一步 `cargo llvm-cov --version` 把「没装上」与「装上了但跑不起来」区分开；移除非阻断后成为真门禁。

**附带踩坑（重要，避免重现）**：CI 步骤名里出现**「冒号 + 空格」**（`Generate coverage report (gate: lines >= 89%)`）会让 YAML 解析失败 → **整个工作流加载不起来**。失败形态极具迷惑性：run 的 `name` 退化成文件路径 `.github/workflows/ci.yml`、jobs API 返回 `total_count: null`、页面上看不到任何步骤，只看「failure」会误判成某个 job 挂了。改 ci.yml 后先跑一次 `yaml.safe_load` 再推。

---

## 6. 债务与风险

### 6.1 未清债务

| 编号 | 债务 | 状态 |
|---|---|---|
| D4 | `nctool part` 占位返回 `not_implemented`（`part generate` 为 Backlog #2） | ❌ 未清（Backlog） |
| D6 | 内置模板与机床预设未经**真实工艺评审** | ❌ 外部依赖（Q2=否） |
| B4.2 | golden 变更保护步骤化（`NCTOOL_UPDATE_GOLDEN` 人工流程文档化） | ❌ Backlog |
| P2-2~P2-4, P2-6 | 架构评审 P2 剩余项（数据表内联 / `model.rs` 拆分 / 扩展点 / 上下文复用） | ❌ 未动 |
| — | ~~F1.3 真实 GitHub Release 未触发~~ | ✅ 已清（2026-09-18，三个 Release 各带三平台二进制） |
| — | ~~CHANGELOG `[未发布]` 积压未发版~~ | ✅ 已清（2026-09-18 转为 0.4.0 / 0.3.0 / 0.3.0 版本段并发布） |
| — | `scripts/check_docs_links.py` 存在但**未接入 CI**（无调用点） | 🚧 工具闲置（非现存问题：09-18 对全部 tracked `.md` 实跑一遍，链接与锚点均通过；缺的是防未来漂移的门禁） |
| — | `Release` 工作流的 `Publish <crate>` 步骤在本地先发布时会红 | 🚧 见 §6.4（发布物不受影响；下次先推 tag 即可避免） |
| — | 发布包包含仓库级杂物（`output/` 原型 PNG、`ui/`、`scripts/`、启动脚本），tpl 包 925 KiB | 🚧 需加 `include`/`exclude`；注意 `include_str!("../ui/index.html")` 使 `ui/` 不可简单排除 |

### 6.2 风险登记册（来源 ROADMAP §6，标注当前状态）

| 编号 | 风险 | 等级 | 当前状态 |
|---|---|---|---|
| R1 | 工艺正确性未验证 | 灾难 / 中 | 🟡 已降级「模板开发工具」+ README 声明，待外部评审 |
| R2 | minijinja 锁 `~2.24.0` | — | 🟡 固有约束，升级需全量验证 |
| R3 | 单文件前端失控（3000 行上限） | — | 🟡 **余量收窄**：`ui/index.html` 2081（09-10）→ **2685 行**，上限 3000，余量 315 行。新增 UI 功能前应评估拆分 |
| R4 | 演示模式语义漂移 | 高 / 已消除 | ✅ 前端已切 server 模式并真实走查；DEMO_SERVER_DIFF 记录差异 |
| R5 | HTTP 新攻击面 | — | 🟡 回环绑定（现为**拒绝非回环**）+ 跨站判定 + CSP + 路径攻击负例已覆盖 |
| R8 | 机床配置键无 schema | — | ✅ 已消除（A4，20 键 schema） |
| R9 | CRLF / LF 混用 | — | 🟡 已配 `.gitattributes`，golden 纯 LF 断言 |
| R10–R12 | 交叉编译 / API 冻结后破坏性需求 / 万行行号内存 | — | 🟡 R10 已部分落地（release.yml 三平台原生构建，无交叉编译依赖） |

### 6.3 本轮修正的文档漂移

| 文档 | 原问题 | 处置 |
|---|---|---|
| `docs/PROJECT_STATUS.md`（本文） | 停在 09-10：记 344 项测试（实为 527）、6 个内置模板（实为 7 + 25 文件模板）、覆盖率门未生效（已是真门禁）、§5.4 描述的绑定策略与代码相反 | ✅ 09-18 全量重写 |
| `docs/ROADMAP.md` §8 | 3 项已完成但未勾选：`B-Backlog`（根因已修）、`E2.2`/`E2.3`（CI 矩阵已承担并绿灯） | ✅ 09-18 勾选并附 CI 取证 |
| `docs/ROADMAP.md` §0/§8 | TL;DR「当前位置」停留 297 项测试；阶段 B 注记仍记 coverage job 非阻断 | ✅ 09-18 更新 |
| `docs/ROADMAP.md` Q11 | 记 `CARGO_REGISTRY_TOKEN` **未配置**，与事实相反 | ✅ 09-18 更正并给出实证（见 §6.4） |

### 6.4 发版遗留：`Publish <crate>` 步骤会红（Q11 实证）

**现象**：三个 tag 推送后，`Release` 工作流的三个运行整体 conclusion = **failure**。查 job 明细：三个 `Build binary assets`（windows / linux / macos-arm64）**全部 success**，发布物完整；失败的是 `Publish <crate>` job 里的 publish 步骤。

**成因**：`release.yml` 的 publish 步骤以 `env.CARGO_REGISTRY_TOKEN != ''` 为条件，且**确实被执行**（而非走「token 缺失则跳过」分支）—— 这反证了 GitHub Secret 里**已配置** `CARGO_REGISTRY_TOKEN`，而 ROADMAP Q11 一直记的是「未配置」。步骤失败的直接原因是该版本**已被本地先发布**：本轮按「改版本号 → 提交 → 推 tag → 本地 publish」的顺序执行，而 CI 在 tag 推送后也尝试发布同一版本，cargo 报「crate version already uploaded」。

**影响**：无实质影响 —— 三个 crate 各只发布一次且内容正确，三个 Release 的二进制资产齐备（已下载实测可运行）。

**下次怎么做**（二选一）：
1. **先推 tag 交 CI 发布**（推荐）：CI 有 token，能自动 publish，且不会出现红运行 —— 本次的手动 publish 属于把 `CARGO_REGISTRY_TOKEN` 未配置当成了既定事实，是 Q11 记录错误导致的绕路。
2. 若坚持本地先发布：接受该步骤红，或给 publish 步骤加幂等守卫（先查 crates.io 该版本是否已存在再决定是否 publish）。

**附**：本机 `cargo publish` 直接不可用 —— 工作区仓库为 **reftable** 引用格式（`extensions.refstorage = reftable`，`.git/HEAD` 指向 `refs/heads/.invalid` 占位），cargo 内置 libgit2 无法解析，报 `failed to begin git status for repo`。本次改在 `git clone --ref-format=files` 的临时克隆中发布（已验证克隆内 `HEAD → refs/heads/master`）。**这是本机环境限制，与仓库内容无关，CI 不受影响。**

---

## 7. Backlog（F4 排序）

| 排序 | 项 | 加权分 | 状态 |
|---|---|---|---|
| 1 | 内置模板库扩充（面铣/键槽铣/外圆车削/攻丝） | 37 | 🟡 `facing` ✅ + `slot_milling` ✅；余**外圆车削、攻丝** |
| 2 | 零件级批量生成（`nctool part generate`） | 34 | ⬜ E5 走查暴露的 3 局限（行号跨工序不续编/错误不聚合/参数无继承）已作为设计输入 |
| 2 | 参数集命名预设 | 34 | ⬜ 前端 localStorage 即可落地 |
| 4 | 浏览器内模板编辑 | 21 | ⬜ 需写 API + 路径穿越/并发覆盖防护 |
| 5 | i18n | 20 | ⬜ 取决于商业交付决策 |
| — | B4.2 golden 保护步骤化 / P2-2~P2-6 架构评审剩余项 / F1 转速联动 / F4 机键 / F2 组合模板 | — | ⬜ 见 §6.1 与 `PROCESS_CHECKLIST.md` |
| — | `nctool lint`（模板内出现三角函数时提示度制风险） | — | ⬜ 整合方案提出，未实现 |

---

## 8. 下一步建议（按优先级）

**本轮已完成（09-18）**

1. ✅ **发版**：tpl 0.4.0 / core 0.3.0 / cli 0.3.0 发布到 crates.io，三个 tag 与三个 GitHub Release（含三平台二进制）落地；发布物经「下载二进制实测」与「`cargo install` 从 crates.io 全新安装实测」双重验证。
2. ✅ **F1.3 收口**：Release 三平台二进制 job 真实跑通（此前只有配置）。
3. ✅ **文档漂移修正**：本文全量重写、ROADMAP 勾选 4 项 + Q11 更正、README 示例输出按 0.3.0 实测重写、CHANGELOG 补 `[nctool-cli 0.2.2]` 段标题 + 去重。

**第一优先：第三轮审查批次三 —— Web UI 加固**

`docs/CODE_REVIEW_2026-09-18.md` §3 的批次一（静默出错）与批次二（度量闭环）已落地
（提交 `300c49e` / `4afada0`）。剩余项里**唯一的真安全问题**是 P1-1：UI 存在存储型
XSS，且 CSP 明确放行 inline 事件处理器 —— 建议连 P1-4（`TemplateMeta::default()` 与
serde 默认值不一致，未入清单的模板被静默隐藏）一起做，后者与批次一属同一类「静默出错」。

**第二优先：补覆盖率洼地**

`cli/src/commands/ui.rs` 23.08%（全项目最低，子进程启动路径难以单测）、
`src/extract.rs` 79.89%（承载「必选/可选判定」这一最微妙逻辑，判错会导致静默产出错误
G-code 或误拦用户）、`cli/src/commands/inspect.rs` 80.79%。补上去后可上调阈值门
（当前生产口径 ≥ 88%，基线 88.65%，余量约 32 行）。

**第三优先：修发布流程与包内容**

① 按 §6.4 的结论，下次发版**先推 tag 交 CI 发布**；② 给三个 crate 加 `include`/`exclude` 收窄发布包（当前 tpl 包 925 KiB，含 `output/` 原型 PNG、`scripts/`、启动脚本等仓库级杂物）—— 注意 `cli` 的 `include_str!("../ui/index.html")` 意味着 `ui/` 必须保留。

**第四优先：Backlog 继续**

内置模板扩充剩外圆车削与攻丝（`facing` / `slot_milling` 已完成）；`part generate` 设计已攒够输入（E5 走查的三处局限）；参数集预设成本最低（前端 localStorage 即可落地）。

**长期外部依赖（不是代码能解决的）**

外部工艺评审（R1）：工程师逐行核对 ⚠️ 项 + 机床空运行验证 21 组正向 golden。`nctool part` 占位需按 F4 加权结果决定是否投入。

**绝不能砍的三项**（ROADMAP §7.3）：① 工艺正确性评审（A1）② 三机床 golden 基线（A2）③ 发版与安装验证（F1）—— 第 ③ 项本轮已完成。

---

## 9. 相关文档索引

| 文档 | 用途 |
|---|---|
| `docs/ROADMAP.md` | 权威规划（阶段 A–F、执行跟踪、风险册、Backlog 排序） |
| `docs/PROJECT_STATUS.md` | **本文**，现状快照（2026-09-18） |
| `docs/SYSTEM_DESIGN.md` | **当前架构/数据流/设计决策（v2.0，权威）** |
| `docs/architecture-overview.html` | 分层 + 数据流架构图（自包含） |
| `docs/ARCHITECTURE_REVIEW.md` | 六维度架构评估（含 P0/P1/P2 改进项状态） |
| `docs/TEMPLATE_INTEGRATION_PLAN.md` | NCTool_V3 模板资产整合方案 |
| `docs/PROCESS_CHECKLIST.md` | 工艺核对清单（模板 × 预设、F1–F5、外部评审待办） |
| `docs/UI_ACCEPTANCE_CHECKLIST.md` | Web UI 人工验收清单（37/37 已勾选） |
| `docs/MACHINE_CONFIG_GUIDE.md` | 机床配置指南（含自定义机床键完整性说明） |
| `docs/TEMPLATE_WRITING_GUIDE.md` | 模板编写指南 |
| `docs/RELEASE.md` | 版本策略与发布流程 |
| `CHANGELOG.md` | 版本记录与 1.0 API 冻结清单 |
| `README.md` | 含「未经工艺验证」降级声明 |
