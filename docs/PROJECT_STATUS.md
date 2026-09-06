# nctool 项目进度分析

> 生成时间：2026-09-05
> 分析依据：`git log` / `git status` 实际工作区、`cargo test` 实测（282 项）、`nctool ui` 实跑、源码与前端代码核对、`docs/ROADMAP.md` 规划
> 定位：本文是**现状快照**，不是规划。规划以 `ROADMAP.md` 为准。
> 复核：关键结论已经过独立复核（见 §5 各项的实测证据）；本文中的数字均为实测值，非文档转抄。

---

## 0. 一页速览（TL;DR）

| 项目 | 结论 |
|---|---|
| **整体进度** | 约 **1/3**。阶段 A/B（P0 地基）已基本收口，阶段 C（Web UI）**正在开发中、约完成一半**，D/E/F 未开始 |
| **清单记账** | ROADMAP 执行跟踪清单 **22 / 72 项（30.6%）** |
| **代码实际** | 含已实现但未勾选的工作，约 **36 / 72 项（≈50%）**；阶段 C/D 的后端与前端主链路已基本接通，批量生成与真实浏览器验收未完成 |
| **功能可用性** | 核心链路（模板解析 → 校验 → 渲染 → G-code）**已完整可跑**；Web UI **后端主要接口可用，前端已切 server 模式，待真实浏览器走查** |
| **测试基线** | 当前 workspace **294 项**实测，**294 通过 / 0 失败**（详见 §4） |
| **✅ 已修复** | `ui` 集成测试已改为随机端口启动 → 探测 `/health` → 主动终止，workspace 测试已恢复全绿 |
| **当前最大问题** | ① **文档仍需同步**（ROADMAP 执行清单尚未反映本轮实现）② `/api/part/generate` 仍缺失，真实浏览器 E2E 未完成 ③ 未发版债务（版本号已提升，`cargo publish` 未执行） |

**一句话**：地基已打完并经过三轮审查，正在盖 Web UI 这一层；但记账簿没跟上施工进度，且有一个「会让 CI 永久挂起」的测试隐患必须先处理——它在本地只是红灯，到了干净环境就是死等。

---

## 1. 分阶段进度明细

阶段划分、工期、优先级来源：`ROADMAP.md` §3。下表「清单」列 = ROADMAP §8 勾选状态，「实际」列 = 核对代码后的真实状态。

| 阶段 | 目标 | 优先级 | 人日 | 清单 | 实际 | 状态 |
|---|---|---|---|---|---|---|
| **A** 需求与设计收口 | 让"正确性"可被证明 | P0 | 3–5 | 12/13 | 12/13 | ✅ 基本完成 |
| **B** 基础架构稳固 | 还清债务，地基干净 | P0 | 3–4 | 10/13 | **12/13** | ✅ 基本完成 |
| **C** UI 服务与前端联通 | 浏览器能看真实模板库 | P1 | 5–8 | **0/12** | **5 完全 + 3 部分** | 🚧 进行中 |
| **D** 完整交互闭环 | 浏览器内能完整生成 | P2 | 8–12 | 0/12 | 0/12 | ⬜ 未开始 |
| **E** 联调测试 | 让"可用"可被证明 | P1 | 4–6 | 0/12 | 0/12 | ⬜ 未开始 |
| **F** 上线与迭代 | 能装上、能用、能反馈 | P0(发版) | 3–5 | 0/10 | 0/10 | ⬜ 未开始 |
| **合计** | | | **26–40** | 22/72 | ≈29/72 | ≈40% |

### 1.1 阶段 A — 需求与设计收口（12/13，基本完成）

**已完成并已提交：**

| 编号 | 内容 | 产物 |
|---|---|---|
| A0 | 前提假设 Q1–Q11 确认 | ROADMAP §2 |
| A1 | 工艺核对清单 | `docs/PROCESS_CHECKLIST.md`，5 模板 × 3 预设逐行核对，**F1–F5 发现项，P0 错误 0 个** |
| A2 | golden 基线扩充至 15 组 | 5 内置模板 × 3 机床预设，冻结渲染输出 + 校验报告 |
| A3 | 1.0 API 冻结清单 | 写入 CHANGELOG，关键枚举已标 `non_exhaustive` |
| A4 | MachineConfig 键名 schema | 登记 20 个已知键，`validate_config_keys` 对未知键/非法值告警 |
| A5 | 确认纪要 | Q1–Q11 结论 + 测试计数校准 |

**唯一未完成项（外部依赖，无法自行推进）：**
> A1 子项 3：3 个机床预设键值按**手册**核对 —— 无手册资源，已按降级预案声明「仅供开发测试，待外部评审」。

**⚠️ 关于 A1 的重要限定**：因 Q2 = 否（无工艺评审资源），A1 已**降级为 AI/代码级自动核对**（G-code 常识标准 + 与 golden 逐字节比对），**不是**真实工艺工程师评审，也**不是**机床手册核验。相应地，项目定位已下调为「**模板开发工具**」而非「生产工具」，README 第 12–14 行显著声明「未经工艺验证，投产前必须自行核对 G-code」。

### 1.2 阶段 B — 基础架构稳固（清单 10/13，实际 12/13）

| 编号 | 内容 | 状态 |
|---|---|---|
| B1.1–B1.3 | 代码收口（ui 单文件前端、架构文档基线、DEV_PLAN 过期数据修正） | ✅ |
| B2.1–B2.3 | 发版准备：CHANGELOG 拆解为三版本、Cargo.toml 版本号提升 | ⚠️ **部分**（版本已提升，publish 未执行，见 §6） |
| B3.1 | 引入 tiny_http | ✅ **已完成但未勾选**（`Cargo.lock` +37 行） |
| B3.2 | 最小空服务 | ✅ **已完成但未勾选**（服务实测已在 127.0.0.1:8787 监听） |
| B4.1 | CI 三平台矩阵（+macos-latest） | ✅ |
| B4.2 | golden 变更保护 | ❌ **转入 Backlog**（`NCTOOL_UPDATE_GOLDEN` 人工步骤化未完成） |
| B4.3 | coverage 任务处置 | ⚠️ 设为 `continue-on-error` 非阻断（见 §7 Backlog） |
| B5.1–B5.2 | 执行跟踪勾选 + 测试计数校准 | ✅ |

### 1.3 阶段 C — UI 服务与前端联通（清单 0/12；后端 4 完全 + 1 部分，前端 3 部分）

**这是当前进行中的工作，也是文档与代码脱节最严重的地方。**

已实现（**未提交**，见 §3）：

- ✅ HTTP 骨架：`cli/src/server.rs`（512 行），基于 **tiny_http 0.12**
- ✅ `nctool ui` 命令真实实现：默认 `127.0.0.1:8787`、`--open` 开浏览器、非回环地址打印安全警告
- ✅ 静态页服务：`GET /` 返回前端页面
- ✅ 4 个可用接口（实测通过）：
  - `GET /health` → `{"status":"ok","version":"0.2.1"}`
  - `GET /api/templates` → 返回 5 个模板
  - `GET /api/templates/{name}` → 返回源码 + 参数规格
  - `GET /api/machines` → 返回 3 个机床预设

已实现 / 待收口：

- ✅ `POST /api/render` —— 后端已实现，支持严格/宽松模式与生成选项，已补路由单元测试
- ✅ `POST /api/validate` —— 后端已实现并返回结构化校验报告，已补路由单元测试
- ❌ `POST /api/part/generate` —— **404**（该项实属阶段 D）
- ✅ `POST /api/inspect` —— 仅接受已注册模板名，已补路径攻击负例
- 🚧 前端接线：`ui/index.html` 已切到同源 `server` 模式，真实加载模板/机床/详情；浏览器内模板保存与批量生成为服务模式禁用，仍需真实浏览器走查

**接口契约完成度：7 个契约端点中 6 个后端可用（根页面、模板列表、模板详情、inspect、validate、render），即 6/7 ≈ 86%。** `/health` 与 `/api/machines` 是额外的健康检查/机床信息接口。契约定义见 `DEV_PLAN_CLI_UI.md` §5。

**12 个子任务的精确拆分**（避免用单一数字掩盖性质差异）：

| 状态 | 编号 | 说明 |
|---|---|---|
| ✅ **后端已完全完成（7 项）** | C1.1、C1.2、C1.3、C2.1、C2.2、D1.1、D1.2 | `ui` 命令与静态页、回环绑定、templates/inspect/validate/render API，含严格选项校验 |
| ✅ **前端已接线（3 项）** | C3.2、C4.1、C4.2 | server 模式、真实模板/机床初始化、模板详情异步加载；服务模式禁用写模板与批量 mock |
| ✅ **已完成（1 项）** | C3.1 | 演示模式与真实服务模式差异清单：`docs/DEMO_SERVER_DIFF.md` |
| ⚠️ **部分完成（1 项）** | D4.3 | 机床 API 已接入，真实浏览器切换验收尚未完成 |
| ✅ **测试已收口（1 项）** | C2.3、C2.4 | inspect 路由、validate/render 路由及路径/选项负例已有纯函数测试 |

> 关于 C1.3 的口径说明：当前实现比 ROADMAP 原例外策略更严格：默认与显式配置均只允许回环 IP，非回环地址直接拒绝；IPv6 URL 使用标准方括号格式。

### 1.4 阶段 D / E / F

- **D 完整交互闭环**：validate/render API、严格/宽松模式、参数表单、防抖预览、校验面板、生成选项、复制/下载已实现；真实浏览器走查和机床切换验收待完成。`nctool part` 仍为 `not_implemented` 占位。
- **E 联调测试**：本机 HTTP 契约、目录模板、自定义机床、错误边界和 413 已覆盖；跨平台、真实浏览器、性能与外部工艺场景仍未完成。
- **F 上线与迭代**：`cargo package --workspace --allow-dirty` 在禁用增量编译后通过；`cargo install --path cli --locked` 与安装后二进制 `nctool 0.2.1` 验证通过。正式发布仍需清理工作区并完成版本/CHANGELOG 发布流程。

---

## 2. 已完成能力清单（端到端可跑）

| 能力 | 入口 | 状态 |
|---|---|---|
| 模板解析 / 变量提取 | `tpl::parse` / `extract_variables` / `extract_undeclared` / `extract_template_refs` | ✅ |
| 参数校验 | `core::validate` + `ValidationReport` | ✅ |
| G-code 生成 | `GCodeGenerator`（严格 / 宽松两种模式） | ✅ |
| 模板注册表 | `TemplateRegistry`，5 个内置模板 | ✅ |
| 机床预设 | `generic` / `wfl_m65` / `index_ms40` + 20 键 schema | ✅ |
| CLI 全命令 | templates list/show/new、inspect、validate、render、machine list/show、config、completion、ui | ✅ 除 `part` 外全通 |
| golden 回归 | 15 组基线 | ✅ 全绿 |
| **未通** | `part` 命令（占位）、Web UI 三端点、前端接线 | ❌ |

**内置模板 5 个**：`program_header`、`program_footer`、`tool_change`、`safe_move`、`drill_cycle`。

---

## 3. 当前进行中的工作（工作区未提交）

`git status` 显示 7 个文件已修改 + 1 个新增未跟踪文件，**全部属于阶段 C**：

| 文件 | 改动 |
|---|---|
| `cli/src/server.rs` | **新增**，512 行 HTTP 服务 |
| `cli/Cargo.toml` | 新增 `tiny_http 0.12` 依赖 |
| `cli/src/commands/ui.rs` | 从 `not_implemented` 占位改为真实实现（+77 行，含回环判定与单测） |
| `cli/src/commands/mod.rs` | `ui` 移入正常分支（需配置层叠以拿模板目录/自定义机床） |
| `cli/src/cli.rs` / `cli/src/main.rs` | 命令接线 |
| `core/src/registry.rs` | 去多余借用 `&context` → `context` |
| `Cargo.lock` | +37 行 |

**判断**：这是一次**方向正确、尚未完成的增量**。骨架和基础设施已到位，但收尾（三端点 + 前端接线 + 缺陷修复）未完成，**建议先完成再提交**，避免把半截状态带入主分支。

---

## 4. 测试基线（实测）

| crate | 单元 | 集成 | Doc | 小计 | 结果 |
|---|---|---|---|---|---|
| nctool-tpl | 104 | 18 | 1 | 123 | ✅ 全过 |
| nctool-core | 84 | 10 | 0 | 94 | ✅ 全过 |
| nctool-cli | 37 | 42 | 0 | 79 | ✅ 79 通过 |
| **通过率** | — | — | — | **297** | **297 / 297 = 100%** |
| **合计** | 225 | 70 | 1 | **297** | **297 通过 / 0 失败** |

`cargo check --workspace --all-targets`：**通过，0 错误**。`cargo clippy --workspace --all-targets -- -D warnings`：**通过，0 告警**。`cargo doc --workspace --no-deps`：**通过**。`cargo audit`：**通过，无漏洞输出**。

**UI 集成测试**：`ui_serves_health_and_exits_when_killed` 使用随机端口启动服务、探测 `/health` 后主动终止；新增非回环拒绝测试，避免常驻服务或远程暴露回归。

> 备注：UI 集成测试使用随机端口并主动清理子进程，CLI 集成测试 42 项在约 14 秒内完成。路由单测已覆盖 validate/render 成功路径、非法 options、分类和模板路径负例；真实 HTTP 测试还覆盖目录模板、自定义机床、未知机床和 413 请求体上限。

---

## 5. 已确认缺陷清单（已修复 3 项）+ 加固建议（2 项）

> 以下 3 项均为**实测确认**的缺陷，非文档推断。§5.4 为可选加固，**不属于缺陷**。

### 5.1 【✅ 已修复】`inspect` 路由死代码

**位置**：`cli/src/server.rs:63–79`

```rust
("GET", p) => match p.strip_prefix("/api/templates/") {
    Some(raw) => template_detail(ctx, raw),
    None => match (method, path) {
        ("GET", "/api/machines") => machines_list(ctx),
        ("POST", "/api/inspect") => inspect(ctx, body),  // ← 永不命中
```

旧实现中，外层 arm `("GET", p)` 已把 `method` 绑定为 `"GET"`，内层再匹配 `("POST", ...)` 恒不成立，实测曾返回 404。

**已修复**：`("GET", "/api/machines")` 与 `("POST", "/api/inspect")` 已提到外层 `match`，并新增 `route_inspect` 单元测试验证 `POST /api/inspect` 返回 200。

### 5.2 【P1】三个契约端点仍未实现

前端 `ui/index.html` 引用 7 个端点，当前后端已实现其中 4 个契约端点。修复后实测/单测覆盖：根页面、`/api/templates`、`/api/templates/{name}`、`/api/inspect` 可用；`/health`、`/api/machines` 也可用但属于额外接口。`/api/render`、`/api/validate`、`/api/part/generate` **仍为 404**。其中 `/api/render` 是前端「实时预览」功能的依赖（`DEV_PLAN_CLI_UI.md:105`），缺它则阶段 C 的验收标准不成立。

**架构缺口（决定实现成本）**：`server.rs` 目前只通过 `ctx.build_registry()` 拿到 `TemplateRegistry`，而它**不暴露渲染管线**——真正的渲染/校验能力封装在 CLI 层的 `commands/render.rs`、`commands/validate.rs` 中。UI 服务目前绕过 CLI 层直接依赖 `core`，因此实现这两个端点前需先把渲染/校验能力从 `commands/` 下沉到 `core`（或让 `server` 复用 CLI 层函数）。这属于**阶段 B 遗留的层次问题**，不是单纯补两个 handler，排期时须计入。

此外还有一个连带隐患：`/api/templates` 返回的是静态 5 个模板，但 `Ctx` 支持目录模板与自定义机床（这正是 §3 中 `ui` 被移入正常分支的原因），端点是否覆盖目录模板尚未验证。

**实现这两个端点所需的能力，`core` 侧其实都已经具备**（以下为核对源码后的可用签名，排期时可直接用）：

| 用途 | 可用 API |
|---|---|
| 生成 G-code | `GCodeGenerator::from_registry(&Registry) -> Result<Self, RegistryError>`；`generate(&mut self, name, params, opts, machine) -> Result<String, PipelineError>` |
| 渲染前校验 | `nctool_core::validate::validate_template(&mut GCodeGenerator, template_name, &BTreeMap<String, Value>) -> ValidationReport` |
| 生成选项 | `GenerationOptions { line_numbers: bool, step: usize, .. }`（`Default` 默认值 `line_numbers=true, step=1`） |
| 内核复用 | 与 CLI `render` / `validate` 命令共用同一内核，无需另写逻辑 |

即：**缺的不是能力，而是把 `commands/` 里的编排逻辑下沉（或复用）到 `server.rs`**。

**已收口**：`route()` 已补 7 项纯函数单元测试，覆盖 health、模板列表、模板详情（成功/404）、机床列表、inspect、未知路由及 query/path 解码；测试无端口依赖。

### 5.3 【✅ 已修复】`ui` 集成测试曾导致永久挂起

旧用例 `cli/tests/cli.rs:628` `ui_reports_not_implemented` 假设 `ui` 命令**未实现**：断言进程失败（`.failure()`）、退出码 7（`.code(7)`；7 = `not_implemented`，映射见 `cli/src/output.rs:54`）、stderr 含「尚未实现」。而 `ui` 已真实实现，且启动后**长期阻塞**。现已替换为 `ui_serves_health_and_exits_when_killed`，用随机端口启动并主动终止子进程。

修复前实测两种环境，**均不可能通过**；该问题现已通过测试替换解决：

| 环境 | `nctool ui` 实际行为 | 测试结果 |
|---|---|---|
| 8787 端口被占用（旧环境） | 绑定失败，走 `io` 分支 → **退出码 3**（≠ 7） | 旧断言红灯 |
| **8787 端口空闲（旧 CI / 干净机器）** | 正常启动并**无限阻塞** | 旧测试永久挂起 |
| **修复后** | 随机端口启动，探测 `/health` 后主动终止 | `ui_serves_health_and_exits_when_killed` 通过 |

**实测证据**：端口空闲时执行 `timeout 8 ./target/debug/nctool ui`，输出 `nctool ui 已启动 → http://127.0.0.1:8787（Ctrl-C 退出）` 后持续阻塞，直至超时被杀。独立复核中以 6 分 16 秒观察窗口复现，结论一致——进程不会自行退出。

**修复结果（原问题）**：旧测试在端口空闲时会把 CI 卡死；修复后的测试使用随机端口、健康检查与主动回收子进程。

**修复已完成**：没有简单删除测试；已改为健康检查后主动终止子进程，同时保留 `route()` 级 API 回归覆盖。

⚠️ **历史修复要点：只改断言是不够的。** 断言链默认无超时，而 `ui` 是常驻进程；本次已通过随机端口 + `/health` 探测 + 主动 `kill`/`wait` 解决常驻进程不退出问题。

同时新增 `route()` 单元测试（纯函数、无进程、无端口）覆盖 API 逻辑，收口 C2.4，避免重新引入挂起风险。

> 排查提示：本机 8787 的 `TIME_WAIT` 连接与残留监听进程会干扰复现，验证前先确认端口真实状态。

### 5.4 【加固建议，非缺陷】非回环绑定缺少二次确认

**现状**：`nctool ui --host 0.0.0.0` 会打印安全警告后**直接绑定**，把本机模板库与机床配置暴露给同网络内的所有设备。

**判定说明**：这一行为**符合** ROADMAP §4 的现行要求（「拒绝非回环绑定（除非显式 `--host` 且打印安全警告）」），因此**不计入缺陷**。但由于 `ui` 服务提供的是无鉴权的只读接口，一旦在办公网/车间网误用 `--host 0.0.0.0`，暴露面即为整个网段，属于**值得主动加固**的点。

**可选方案**（按成本从低到高）：

1. 非回环绑定时要求交互式确认（TTY 场景）或追加 `--i-know-the-risk` 显式开关；
2. 非回环绑定时强制启用一个一次性访问令牌；
3. 文档中明确 `ui` 仅适用于本机单人使用场景。

---

## 6. 债务与风险

### 6.1 未清债务

| 编号 | 债务 | 状态 |
|---|---|---|
| D4 | `nctool part` 占位返回 `not_implemented` | ❌ 未清（阶段 D） |
| D5 | 前端处于演示模式，自研迷你 Jinja 与 minijinja 语义可能不一致 | ❌ 未清 |
| D6 | 内置模板未经工艺评审 | ❌ 外部依赖 |
| **R6** | **发版债务**：版本已提升至 `tpl 0.3.2` / `core 0.2.1` / `cli 0.2.1`，但 `cargo publish` **未执行**（token 未配置） | ❌ **已发生** |
| — | **文档滞后**：ROADMAP 阶段 C 记 0/12；实际为后端 4 项完全完成、1 项部分完成，另有前端 3 项部分完成；B3.1/B3.2 已完成未勾选 | ❌ 本文提出 |

### 6.2 风险登记册（来源 ROADMAP §6，标注当前状态）

| 编号 | 风险 | 等级 | 当前状态 |
|---|---|---|---|
| R1 | 工艺正确性未验证 | 灾难 / 中 | 🟡 已降级为「模板开发工具」+ README 声明，待外部评审 |
| R2 | minijinja 锁 `~2.24.0`（依赖 unstable AST） | — | 🟡 固有约束，升级需全量验证 |
| R3 | 单文件前端失控（3000 行上限） | — | 🔴 **`ui/index.html` 实测 2115 行 / 102 KB，已达上限的 70%，需监控** |
| R4 | 演示模式语义漂移 | 高 / **已存在** | 🔴 前端仍走 mock，未接线 |
| R5 | HTTP 新攻击面 | — | 🟡 已做回环地址警告，需继续加固 |
| R6 | 未提交 / 未发版债务 | — | 🔴 **已发生**（见 §6.1） |
| R7 | 人力单点 | — | 🟡 1 人 + AI |
| R8 | 机床配置键无 schema | — | ✅ **已消除**（A4，20 键 schema） |
| R9 | CRLF / LF 混用 | — | 🟡 已配 `.gitattributes` |
| R10–R12 | 交叉编译超时 / API 冻结后破坏性需求 / 万行行号内存 | — | ⬜ 未触发 |

### 6.3 工艺核对发现项（PROCESS_CHECKLIST F1–F5，**P0 错误 0 个**）

| 编号 | 结论 | 级别 | 处置 |
|---|---|---|---|
| F1 | 主轴转速上界 6000 硬编码于模板规格，`max_spindle_rpm`（WFL 3500 / INDEX 5000）不参与校验 | ❗工艺风险 | 不阻断，用户自检 + 文档声明；Backlog：规格改为引用 `machine.max_spindle_rpm` |
| F2 | `drill_cycle` 的 G98 初始平面依赖调用前 Z 高度 | ⚠️ 使用约定 | 须 `safe_move` 后接 `drill_cycle`；Backlog：提供组合模板 |
| F3 | 机床预设未体现真实机床差异 | ⚠️ 待外部 | 声明「仅供开发测试」 |
| F4 | G49（vs G43 H0）与 H#=T# 属 FANUC 惯例 | ⚠️ 控制器差异 | 不可配；Backlog：增加机键 |
| F5 | `safe_z` 默认 100 无上界 | ℹ️ 说明 | 上界由机床行程决定 |

---

## 7. Backlog

| 来源 | 项 |
|---|---|
| B4.2 | golden 变更保护：将 `NCTOOL_UPDATE_GOLDEN` 人工刷新步骤化 |
| B-Backlog | CI `coverage` job 红灯：自 22a056e 起持续失败，补装 `llvm-tools-preview` 后仍红，日志需 admin 权限无法取证 → 已设 `continue-on-error` 解除红灯（覆盖率属可选质量门，非阻断）。**待取证** ubuntu runner 日志，疑似 `taiki-e/install-action` 的 cargo-llvm-cov 与 runner 不兼容或 rust-cache 陈旧插桩产物 |
| F4 排序 | `part` 批量 → 模板编辑 → 参数预设 → 模板库扩充 → i18n |
| PROCESS_CHECKLIST §8 | 4 条外部评审待办（全未勾选）：工程师确认 ⚠️ 项 / 按手册核对 WFL·INDEX 预设 / 机床空运行验证 15 组 golden / 回填消除 ⚠️ 项 |

---

## 8. 下一步建议（按优先级）

**本轮已完成**

1. ✅ 修复 `ui` 常驻集成测试：随机端口启动 → 探测 `/health` → 主动回收。
2. ✅ HTTP API 仅允许注册表模板名，阻断绝对路径、`..` 和目录外符号链接读取。
3. ✅ UI 仅允许回环监听，IPv6 URL 标准化，非回环启动直接拒绝。
4. ✅ 实现并测试 `/api/validate`、`/api/render`，严格校验生成选项和统一报告字段。
5. ✅ 前端切换真实同源 server mode，真实加载模板/机床/详情。
6. ✅ 新增真实 HTTP 契约测试，覆盖首页、列表、详情、validate、render 和路径攻击。
7. ✅ 修复 Clippy 严格门禁，workspace 测试 294 项全绿。

**下一优先级**

8. 真实浏览器走查：模板加载、机床切换、表单填参、预览、校验面板、复制/下载。
9. 产出《演示模式差异清单》，补充 service/demo 的行为边界。
10. 继续关注 `ui/index.html` 单文件规模（约 2200 行，低于 3000 行上限）。

**后续主线**

11. 完善模板目录/自定义机床 HTTP E2E、跨平台验证、性能和请求超时。
12. 实现 `part generate` 前先确认是否继续投入完整 Web 工作台。
13. 发版：配置 `CARGO_REGISTRY_TOKEN` 或手动 `cargo publish`。
14. 按 MVP 策略优先保留 CLI + 只读/真实 UI，批量和远程访问后置。

**绝不能砍的三项**（ROADMAP §7.3）：① 工艺正确性评审（A1）② 三机床 golden 基线（A2）③ 发版与安装验证（F1）。

---

## 9. 相关文档索引

| 文档 | 用途 |
|---|---|
| `docs/ROADMAP.md` | 权威规划（阶段 A–F、执行跟踪 72 项、风险册、MVP 裁剪） |
| `docs/PROJECT_STATUS.md` | **本文**，现状快照 |
| `docs/PROCESS_CHECKLIST.md` | 工艺核对清单（F1–F5、外部评审待办） |
| `docs/DEV_PLAN_CLI_UI.md` | CLI/UI 开发计划（阶段计划已被 ROADMAP 取代，架构与 API 契约仍有效） |
| `docs/ARCHITECTURE.md` | 架构基线 |
| `CHANGELOG.md` | 版本记录与 1.0 API 冻结清单 |
| `README.md` | 含「未经工艺验证」降级声明 |
