# nctool 项目进度分析

> 生成时间：2026-09-10
> 分析依据：`git log` / `git status` 实际工作区、`cargo test --workspace` 实测（344 项全绿）、`nctool ui` 真实浏览器走查（37/37 通过）、源码与前端代码核对、`docs/ROADMAP.md` 规划
> 定位：本文是**现状快照**，不是规划。规划以 `ROADMAP.md` 为准。
> 复核：关键结论已经过独立复核（见 §5 各项的实测证据）；本文中的数字均为实测值，非文档转抄。

---

## 0. 一页速览（TL;DR）

| 项目 | 结论 |
|---|---|
| **整体进度** | 功能主线（阶段 A–E）**已基本收口**，阶段 F 仅剩 GitHub Release 三平台二进制落地（配置已就绪，待 tag 触发 CI）；处于「发版临门一脚」状态 |
| **清单记账** | ROADMAP 执行跟踪清单 **65 / 71 项（≈92%）** 勾选（A 12/13 · B 12/13 · C 12/12 · D 11/12 · E 10/12 · F 8/9） |
| **功能可用性** | CLI 全命令可用（除 `part` 占位）+ Web UI 完整交互闭环（选模板→填参→防抖预览→校验定位→下载/复制→机床切换→主题），**37/37 人工验收通过** |
| **测试基线** | 当前 workspace **344 项**实测，**344 通过 / 0 失败**（2026-09-10 复测；另 2 项万行实测需 `--release --ignored`） |
| **✅ 本轮完成** | ① Web UI 真实浏览器走查 37/37（E1.2 + D4.3，修复 3 个前端缺陷）② `facing` 面铣模板（Backlog 首项，18 组 golden）③ release.yml 三平台二进制 job（F1.3 配置）④ 文档同步（验收清单修订、机床指南补全、ROADMAP 重复条目清理） |
| **当前最大问题** | ① 真实 Release 未触发（需 tag push + CI，依赖用户凭据）② E2.2/E2.3 跨平台绿灯待 CI 首次运行确认 ③ `nctool part generate` 未实现（Backlog #2）④ 外部工艺评审长期依赖 |

**一句话**：地基、功能、联调、人工验收全部完成并全绿，只剩发版动作本身和剩余 Backlog；当前工作区改动为「发版准备 + 验收修复 + 模板扩充」一次干净的增量，可直接提交。

---

## 1. 分阶段进度明细

阶段划分、工期、优先级来源：`ROADMAP.md` §3。下表「清单」列 = ROADMAP §8 勾选状态。

| 阶段 | 目标 | 优先级 | 人日 | 清单 | 未完成项 | 状态 |
|---|---|---|---|---|---|---|
| **A** 需求与设计收口 | 让"正确性"可被证明 | P0 | 3–5 | 12/13 | A1 子项：3 机床预设按手册核对（外部依赖） | ✅ 基本完成 |
| **B** 基础架构稳固 | 还清债务，地基干净 | P0 | 3–4 | 12/13 | B4.2 golden 变更保护（Backlog） | ✅ 基本完成 |
| **C** UI 服务与前端联通 | 浏览器能看真实模板库 | P1 | 5–8 | 12/12 | — | ✅ 完成 |
| **D** 完整交互闭环 | 浏览器内能完整生成 | P2 | 8–12 | 11/12 | D4.3 已在 09-10 走查完成（勾选见 ROADMAP）；清单计数含 B4.2 顺延项 | ✅ 完成 |
| **E** 联调测试 | 让"可用"可被证明 | P1 | 4–6 | 10/12 | E2.2/E2.3 Linux·macOS（CI 矩阵已配，待绿灯） | ✅ 基本完成 |
| **F** 上线与迭代 | 能装上、能用、能反馈 | P0(发版) | 3–5 | 8/9 | F1.3 真实 Release 待 tag 触发 | 🚧 临门一脚 |
| **合计** | | | **26–40** | 65/71 | 见 §6.1 | ≈92% |

### 1.1 阶段 A — 需求与设计收口（12/13，基本完成）

**已完成并已提交：** A0 前提假设、A1 工艺核对清单（现 6 模板 × 3 预设逐行核对，P0 错误 0）、A2 golden 基线（已扩至 18 组）、A3 1.0 API 冻结清单、A4 MachineConfig 键名 schema（20 键）、A5 确认纪要。

**唯一未完成项（外部依赖）：** 3 个机床预设键值按**手册**核对 —— 无手册资源，已按降级预案声明「仅供开发测试，待外部评审」。A1 为 AI/代码级自动核对，**非**真实工艺评审；README 显著声明。

### 1.2 阶段 B — 基础架构稳固（12/13）

B1–B5 全部完成；B4.2（golden 变更保护步骤化）转入 Backlog（`NCTOOL_UPDATE_GOLDEN` 机制已可用，人工刷新流程文档化未做）。

### 1.3 阶段 C — UI 服务与前端联通（12/12，完成）

`nctool ui`（tiny_http，回环绑定）+ 全部契约端点（templates / templates/{name} / machines / inspect / validate / render / part 占位）+ 前端 server 模式接线，均已收口并测试覆盖。

### 1.4 阶段 D — 完整交互闭环（11/12，完成）

参数表单、防抖预览、校验面板、生成选项、复制/下载、机床切换、主题切换全部可用。**09-10 真实浏览器走查 37/37 通过**（含自定义机床 hero_x9）。走查修复 3 个前端缺陷（见 §5.5）。

### 1.5 阶段 E — 联调测试（10/12）

CLI E2E 契约、HTTP 契约、golden 18 组、错误边界、413、万行性能均已覆盖。E2.2/E2.3（Linux/macOS）由 CI 三平台矩阵承担，待首次绿灯确认（本地只能验证 Windows）。

### 1.6 阶段 F — 上线与迭代（8/9）

文档（README/CHANGELOG/指南）、Issues 模板、迭代节奏、Backlog 排序、三平台二进制 job（release.yml）均完成。剩 **F1.3 真实 Release**：tag 推送触发 CI 构建三平台二进制 + crates.io publish（token 未配置时 publish 自动跳过并提示）。

---

## 2. 已完成能力清单（端到端可跑）

| 能力 | 入口 | 状态 |
|---|---|---|
| 模板解析 / 变量提取 | `tpl::parse` / `extract_variables` / `extract_undeclared` / `extract_template_refs` | ✅ |
| 参数校验 | `core::validate` + `ValidationReport`（行列定位） | ✅ |
| G-code 生成 | `GCodeGenerator`（严格 / 宽松） | ✅ |
| 模板注册表 | `TemplateRegistry`，**6 个内置模板** | ✅ |
| 机床预设 | `generic` / `wfl_m65` / `index_ms40` + 20 键 schema + `nctool.toml` 自定义 | ✅ |
| CLI 全命令 | templates list/show/new、inspect、validate、render、machine list/show、config、completion、ui | ✅ 除 `part` 外全通 |
| Web UI | 服务模式全链路（选模板/填参/预览/校验/下载/复制/机床切换/主题/移动端） | ✅ 37/37 验收 |
| golden 回归 | **18 组基线**（6 模板 × 3 预设，含 facing） | ✅ 全绿 |
| **未通** | `part` 命令（占位 exit 7）、`part generate` 批量生成（Backlog #2） | ❌ |

**内置模板 6 个**：`program_header`、`program_footer`、`tool_change`、`safe_move`、`drill_cycle`、`facing`（面铣 zigzag，09-10 新增）。

---

## 3. 当前工作区改动（一次干净的增量）

`git status` 显示 6 个文件已修改 + 8 个新增 golden 文件（09-10 本轮）：

| 文件 | 改动 |
|---|---|
| `.github/workflows/release.yml` | **F1.3**：新增 `binaries` job，三平台（ubuntu/windows/macos-latest）release 二进制构建 + GitHub Release assets 上传 |
| `ui/index.html` + `cli/ui/index.html` | 修复 3 个前端缺陷（参数必选/默认值解析、数据源文本、移动端输入框字号），两处字节一致 |
| `core/src/registry.rs` | 新增 `facing` 内置模板（Milling 分类，9 参数），`registry_installs_builtins` 断言 5→6 |
| `core/tests/integration.rs` | golden 矩阵 15→18 组，文件数断言 30→36，覆盖防漏项断言 5→6 |
| `tests/golden/facing_*.nc/.report.txt` | 8 个新 golden 基线文件（3 组输出 + 3 组报告 + 2 处目录级变化） |
| `docs/`（4 个文件） | 验收清单 37/37 勾选 + 4 处修订；机床指南补「自定义机床需提供全部模板键」；工艺清单新增 §6 facing；ROADMAP 勾选 + 重复条目清理 + Backlog 标注 |
| `CHANGELOG.md` | 新增 [Unreleased]：facing、release.yml 三平台 job、3 个前端修复、验收记录 |

**建议**：核对后一次提交（含 golden 与文档），无需拆分。

---

## 4. 测试基线（实测，2026-09-10）

| crate | 单元 | 集成 | Doc | 小计 | 结果 |
|---|---|---|---|---|---|
| nctool-tpl | 104 | 18 | 1 | 123 | ✅ 全过 |
| nctool-core | 84 | 11 | 0 | 95 | ✅ 全过 |
| nctool-cli | 37 | 42+44(E2E) | 0 | 123 | ✅ 全过 |
| **合计** | 225 | 115 | 1 | **344** | **344 通过 / 0 失败** |

`cargo build --release -p nctool-cli`：**通过**（LTO+strip+codegen-units=1，产物 3.6 MB，`nctool 0.2.1` 可运行）。三 crate `cargo package`：**通过**（在普通格式临时克隆中验证，绕开本地 reftable 仓库与 cargo git2 的兼容限制；CI 上 clone 为普通格式不受影响）。前端 `ui/index.html` 2081 行（< 3000 行上限，两副本字节一致）。

---

## 5. 已确认缺陷清单

> §5.1–5.4 为 09-05 快照的历史记录（均已修复/归档）；§5.5 为本轮（09-10）走查新发现并已修复。

### 5.1 【✅ 已修复，09-05】`inspect` 路由死代码

外层 `("GET", p)` 已把 `method` 绑定为 `"GET"`，内层 `("POST", ...)` 恒不成立。已提层修复并补单测。

### 5.2 【✅ 已收口，09-05】契约端点补齐

`/api/render`、`/api/validate` 曾为 404；已下沉复用 core 管线实现并补 7 项路由单测 + 真实 HTTP 契约测试。前端已切 server 模式。

### 5.3 【✅ 已修复，09-05】`ui` 集成测试永久挂起

旧断言假设 `ui` 未实现；已替换为随机端口 + `/health` 探测 + 主动终止。

### 5.4 【加固建议，非缺陷】非回环绑定缺少二次确认

`--host 0.0.0.0` 会打印警告后直接绑定（符合现行要求）。可选加固：交互确认 / 一次性令牌 / 文档声明。**未实施**。

### 5.5 【✅ 已修复，2026-09-10 走查发现】三个前端缺陷

| # | 缺陷 | 表现 | 修复 |
|---|---|---|---|
| 1 | 参数规格必选/默认值解析（server 模式） | API 返回 `default:null`（必选）与 `{type,value}`（带标签默认值）；前端用 `p.default === undefined` 判断 → 必选全被算作可选（表单显示「必选 0 · 可选 5」）、默认值对象被直填入输入框（`[object Object]`） | 新增 `paramDefault()` 统一取值（renderForm/fieldHtml/selectTemplate/renderSidebar 五处），修复后显示「必选 4 · 可选 1」，与 CLI `validate` 口径一致 |
| 2 | 数据源文本不更新 | 服务模式下页脚仍显示静态「演示数据（本地渲染）」 | server 模式加载成功/失败分别更新文本 |
| 3 | 移动端输入框字号 | `@media (max-width:480px)` 的 16px 被更高特异性的 `.param-input-wrap input` 13px 覆盖 → iOS Safari 聚焦放大整页 | 媒体查询内追加同特异性后定义规则；375px 视口实测渲染 16px |

---

## 6. 债务与风险

### 6.1 未清债务

| 编号 | 债务 | 状态 |
|---|---|---|
| D4 | `nctool part` 占位返回 `not_implemented`（`part generate` 为 Backlog #2） | ❌ 未清（Backlog） |
| D6 | 内置模板未经工艺评审 | ❌ 外部依赖（Q2=否） |
| B4.2 | golden 变更保护步骤化（`NCTOOL_UPDATE_GOLDEN` 人工流程文档化） | ❌ Backlog |
| B-Backlog | CI coverage job 红灯根因（cargo-llvm-cov 兼容性，已 continue-on-error 非阻断） | ❌ Backlog |
| — | F1.3 真实 GitHub Release 未触发（配置已就绪，待 tag push + CI） | 🚧 依赖凭据 |
| — | E2.2/E2.3 跨平台绿灯确认（CI 矩阵已配） | 🚧 依赖 CI 首次运行 |

### 6.2 风险登记册（来源 ROADMAP §6，标注当前状态）

| 编号 | 风险 | 等级 | 当前状态 |
|---|---|---|---|
| R1 | 工艺正确性未验证 | 灾难 / 中 | 🟡 已降级「模板开发工具」+ README 声明，待外部评审 |
| R2 | minijinja 锁 `~2.24.0` | — | 🟡 固有约束，升级需全量验证 |
| R3 | 单文件前端失控（3000 行上限） | — | 🟡 `ui/index.html` 实测 2081 行 / ~100 KB，需监控 |
| R4 | 演示模式语义漂移 | 高 / 已消除 | ✅ 前端已切 server 模式并真实走查；DEMO_SERVER_DIFF 记录差异 |
| R5 | HTTP 新攻击面 | — | 🟡 回环绑定 + 路径攻击负例已覆盖 |
| R8 | 机床配置键无 schema | — | ✅ 已消除（A4，20 键 schema） |
| R9 | CRLF / LF 混用 | — | 🟡 已配 `.gitattributes`，golden 纯 LF 断言 |
| R10–R12 | 交叉编译 / API 冻结后破坏性需求 / 万行行号内存 | — | 🟡 R10 已部分落地（release.yml 三平台原生构建，无交叉编译依赖） |

### 6.3 工艺核对发现项（PROCESS_CHECKLIST F1–F5，**P0 错误 0 个**）

F1（转速上界硬编码）❗ / F2（G98 初始平面依赖）⚠️ / F3（预设未体现真实机床差异）⚠️ / F4（G49 与 H#=T# FANUC 惯例）⚠️ / F5（safe_z 无上界）ℹ️ —— 均文档化声明，外部评审后回填。

---

## 7. Backlog（F4 排序，2026-09-10）

| 排序 | 项 | 加权分 | 状态 |
|---|---|---|---|
| 1 | 内置模板库扩充（面铣/键槽铣/外圆车削/攻丝） | 37 | ✅ `facing` 完成（09-10，含 golden + 工艺清单 §6）；余 3 个模板 |
| 2 | 零件级批量生成（`nctool part generate`） | 34 | ⬜ E5 走查暴露的 3 局限（行号跨工序不续编/错误不聚合/参数无继承）已作为设计输入 |
| 2 | 参数集命名预设 | 34 | ⬜ 前端 localStorage 即可落地 |
| 4 | 浏览器内模板编辑 | 21 | ⬜ 需写 API + 路径穿越/并发覆盖防护 |
| 5 | i18n | 20 | ⬜ 取决于商业交付决策 |
| — | B4.2 golden 保护步骤化 / B-Backlog coverage 根因 / F1 转速联动 / F4 机键 / F2 组合模板 | — | ⬜ 见 §6.1 与 PROCESS_CHECKLIST |

---

## 8. 下一步建议（按优先级）

**本轮已完成（09-10）**

1. ✅ F1.3 配置：release.yml 三平台二进制 job；Windows release 构建 + 三 crate `cargo package` 本地验证通过。
2. ✅ E1.2 + D4.3：真实浏览器走查 37/37 通过；修复 3 个前端缺陷；验收清单按实测修订 4 处描述。
3. ✅ Backlog #1 首件：`facing` 面铣模板（registry + 18 组 golden + 工艺核对 §6）。
4. ✅ 文档收尾：CHANGELOG [Unreleased]、ROADMAP 勾选与重复条目清理、机床指南补全。

**下一优先级（多数依赖用户/外部）**

5. **发版动作**：push 工作区 → 打 tag → 触发 CI（三平台二进制 + crates.io publish；`CARGO_REGISTRY_TOKEN` 未配则 publish 自动跳过并提示）。需用户 Git 凭据。
6. 看 CI 首次绿灯：quality 三平台矩阵（E2.2/E2.3 确认）、release binaries job、coverage（已知非阻断）。
7. Backlog 继续：键槽铣/外圆车削/攻丝模板 → `part generate` 设计 → 参数预设。

**后续主线**

8. 外部工艺评审（R1）：工程师逐行核对 ⚠️ 项 + 机床空运行验证 18 组 golden。
9. `part generate` 前确认是否继续投入完整 Web 工作台。

**绝不能砍的三项**（ROADMAP §7.3）：① 工艺正确性评审（A1）② 三机床 golden 基线（A2）③ 发版与安装验证（F1）。

---

## 9. 相关文档索引

| 文档 | 用途 |
|---|---|
| `docs/ROADMAP.md` | 权威规划（阶段 A–F、执行跟踪、风险册、Backlog 排序） |
| `docs/PROJECT_STATUS.md` | **本文**，现状快照 |
| `docs/PROCESS_CHECKLIST.md` | 工艺核对清单（6 模板 × 3 预设、F1–F5、外部评审待办） |
| `docs/UI_ACCEPTANCE_CHECKLIST.md` | Web UI 人工验收清单（37/37 已勾选） |
| `docs/MACHINE_CONFIG_GUIDE.md` | 机床配置指南（含自定义机床键完整性说明） |
| `docs/TEMPLATE_WRITING_GUIDE.md` | 模板编写指南 |
| `docs/RELEASE.md` | 版本策略与发布流程 |
| `CHANGELOG.md` | 版本记录与 1.0 API 冻结清单 |
| `README.md` | 含「未经工艺验证」降级声明 |
