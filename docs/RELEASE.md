# 迭代节奏约定

> 适用阶段：nctool 0.x（API 演进期）→ 1.0（API 冻结期）。
> 本文档说明**版本号、发布节奏、提交流程、兼容性窗口**。不属于发布材料的内容（如何写模板/配置机床）见其他 docs。

## 1. 版本号策略

遵循 [语义化版本](https://semver.org/) `MAJOR.MINOR.PATCH`。

- **0.x 阶段**（当前）：`minor` 升级**可以**包含破坏性变更。前提是：变更已在 `CHANGELOG.md` 的 `Changed` 节标注，并在 README/相关文档说明迁移方式。本项目当前处于此阶段（详见 CHANGELOG 「未发布」段对 A3 1.0 API 冻结清单的描述）。
- **1.0.0**：公共 API 稳定后发布。此后严格遵循：
  - `MAJOR` 升级 = 破坏性变更
  - `MINOR` 升级 = 向后兼容的新功能
  - `PATCH` 升级 = 向后兼容的修复

**多 crate 同步**：本仓库是 workspace（`nctool-tpl` / `nctool-core` / `nctool-cli`）。版本号通常**一起 bump**，但允许 core/tpl 落后于 cli 一次（cli 是薄壳）。

## 2. 发布节奏

- **按需发布**，不锁固定周期。累积到以下任一情况就发：
  - 重要功能闭环（如阶段 C/D 完成后）
  - 累积 ≥ 10 个有用户价值的修复或改进
  - 安全修复（立即发，单独 minor）
  - 长分支（> 2 周）准备合并前
- 不发"空"版本。CHANGELOG 至少有一条实质改动。
- 发版前必须：`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`cargo audit` 全部 0 告警。

## 2.5 发版清单（Runbook）

> 本节把 U-22 / U-24 / U-23 / U-25 四条发版流程缺陷固化成**可重复的操作序列**。
> 顺序是硬约束，不是建议。

### 2.5.1 唯一入口：推 tag 交 CI（U-22）

**`cargo publish` 只允许在 CI 里执行，本地手工发布路径已废止。**

原因（2026-09-22 实测事故）：上一轮按「改版本号 → 提交 → 推 tag → **本地 publish**」
顺序操作，CI 在 tag 推送后又尝试发布同一版本 → `crate version already uploaded`，
`Publish` 步骤变红。**发布物本身不受影响**，但红了就得靠人去分辨"是真失败还是重复发布"，
而这类分辨正是应该被流程消除的东西。

```bash
# ✅ 唯一正确姿势：只推 tag，发布由 release.yml 完成
git tag nctool-tpl-v0.4.1 && git push origin nctool-tpl-v0.4.1
```

若 `CARGO_REGISTRY_TOKEN` 未配置，release.yml 会打印 `::notice::` 并跳过
（**不红**）——那是"凭据缺失"的既有跳过分支，不是失败。

### 2.5.2 顺序与同步点（U-24）：`tpl → core → cli`，逐个等绿

依赖方向决定了顺序：`cli → core → tpl`。**必须按 tpl → core → cli 推 tag**，
且**前一个的 `Publish` 步骤成功后，再推下一个**：

```
nctool-tpl  0.4.x   ← 最底层（模板解析/渲染/NC 过滤器）
  ↑ nctool-core 0.3.x 依赖  nctool-tpl = "0.4.0"（core/Cargo.toml:16）
  ↑ nctool-cli  0.3.x 依赖  nctool-core = "0.3.0"（cli/Cargo.toml:13）
                         + nctool-tpl  = "0.4.0"（cli/Cargo.toml:14）
```

**版本约束同步点**（改版本号时三处必须一起看）：

| 改了谁的版本 | 还要改哪里 |
| --- | --- |
| `Cargo.toml:7`（`nctool-tpl`） | `core/Cargo.toml:16` 与 `cli/Cargo.toml:14` 的 `version = "…"` |
| `core/Cargo.toml:3`（`nctool-core`） | `cli/Cargo.toml:13` 的 `version = "…"` |
| `cli/Cargo.toml:3`（`nctool-cli`） | —（没有下游） |

漏改同步点的症状是**先发的那个在下游解析成 crates.io 上的旧版本**，
`cargo publish` 才会报"依赖未发布"——此时 tpl 已经发出去了，退不回来。

### 2.5.3 发布包不含可运行测试资源（U-23 / U-25）

**显式声明**：发布包（`.crate`）**不含可运行测试所需的 workspace 级资源**，
因此在发布包里跑 `cargo test` 会有用例失败。**这是预期行为，不是发布物缺陷**。

```
cargo install nctool-cli          # ✅ 不受影响（测试资源本就不该进发布包）
cargo add nctool-core             # ✅ 不受影响（下游作为库依赖，跑不到这些测试）
在 .crate 包内 cargo test         # ❌ 会失败 —— 已知，不修复
```

**严重度前置检查**（判断"包内测试失败"要不要紧时，先看这一条）：

> 该测试是否只在 `#[cfg(test)]` 内跨出 crate 目录读资源（如 `../../templates`）？
> **是** → 只影响"在发布包里跑测试"，`cargo install` 与下游库用户**均不受影响**，
> 严重度降级为整洁项；**否** → 按真实缺陷处理。

cargo 的打包模型是"crate 目录内的文件才随包"，任何 `../../` 的运行时读取或
`include_str!` 在发布包里都必然断。**迁移测试资源进各 crate 的方案不做**：
违反单一来源硬约束，且包内测试失败不影响构建与安装。

### 2.5.4 演练记录

见 §2.6。

## 2.6 发版演练记录

不发真实版本的前提下验证「三 crate 按序可解析」。Windows 上打包可能触发 rustc ICE，
故统一带 `CARGO_INCREMENTAL=0`：

```bash
CARGO_INCREMENTAL=0 cargo publish -p nctool-tpl  --dry-run --allow-dirty
CARGO_INCREMENTAL=0 cargo publish -p nctool-core --dry-run --allow-dirty
CARGO_INCREMENTAL=0 cargo publish -p nctool-cli  --dry-run --allow-dirty
```

### 最近一次演练：2026-09-24（Windows，rustc stable）

| crate | 结果 | 说明 |
| --- | --- | --- |
| `nctool-tpl` 0.4.0 | ✅ RC=0 | Packaged 66 files（459.5KiB），验证编译通过 |
| `nctool-core` 0.3.0 | ✅ RC=0 | Packaged 32 files（824.8KiB），验证编译通过（依赖 tpl 0.4.0 解析成功） |
| `nctool-cli` 0.3.0 | ❌ **RC=101，46 个编译错误** | 见下 |

**`nctool-cli` 失败的原因不是包内容，而是依赖版本落后**。`cargo publish` 的验证
步骤把 `path` 依赖剥掉、改用 **crates.io 上的已发布版本**编译，于是本地 HEAD 里
`nctool-core` 新增的模块在已发布的 `nctool-core 0.3.0` 里全都不存在：

```
error[E0432]: unresolved import `nctool_core::asset`   （asset / part 模块）
error[E0433]: cannot find `json_num` in `nctool_core`
error[E0425]: cannot find function `lint` in crate `nctool_tpl`
error[E0560]: struct `GenerationOptions` has no field named `line_number_start`
error[E0599]: no method named `generate_outcome` found for struct `Rc<GCodeGenerator>`
…
```

**这条演练把 U-24 的顺序约束从"经验"变成了"实测"**：只要 `core` / `tpl` 还没发，
`cli` 就**一定**发不出去 —— 而且失败形态是 46 条让人以为是代码坏了的编译错误，
而不是一句"依赖版本未发布"。

**因此发版的硬门槛**：推 `nctool-cli-v*` 之前，必须先确认

1. `core/Cargo.toml:16` 与 `cli/Cargo.toml:14` 的 `nctool-tpl` 版本**已发布**；
2. `cli/Cargo.toml:13` 的 `nctool-core` 版本**已发布**；
3. 三者都验证过 `cargo publish --dry-run` 通过。

演练即这套检查：按 §2.6 的三条命令跑一遍，第三条绿了才允许推 cli 的 tag。

## 3. 提交流程

- **主分支**：`master`（当前 main vs master：本项目沿用 `master`）
- **提交风格**：不强制 squash / rebase / merge commit。每条 commit 应当**自包含**（独立编译、独立测试通过），便于 `git bisect` 与 `git revert`。
- **commit message**：中文或英文均可，但需含**动机**（why），不只是"改了 X"（what）。本次提交对照参考：`git log --oneline`。
- **PR 流程**（v1.0 后启用；0.x 阶段 main 上直推 + 事后 review）：
  - 1 个 approve（维护者本人可省）
  - CI 全绿（含 ubuntu/windows/macos 三平台）
  - 若有 `Changed` 破坏项 → 必须在 PR 描述里写迁移指南
- **全部必须绿**：含 `coverage`（生产代码行覆盖 ≥ 92%，2026-09-26 口径修订后由 91% 上调）与 `msrv`（1.89 上 `cargo check`）两个 job —— 二者都是阻断项，没有例外

## 4. 兼容性窗口

| 阶段 | 行为 |
| --- | --- |
| 0.x | 允许破坏变更，提前在 CHANGELOG 标注；受影响用户有 ≥ 1 个 minor 的缓冲期 |
| 1.0.x | 仅 `PATCH` 兼容修复（`.0.x → .0.x+1`） |
| 1.x → 2.0 | 重大变更需先合并 RFC issue / 讨论 ≥ 2 周；CHANGELOG 标注完整迁移路径 |
| 弃用 | 标记 `#[deprecated]` 至少持续 1 个 minor，CHANGELOG 提示替代方案；`1.minor` 后移除 |

**二进制兼容**与**源码兼容**分开：
- 0.x 阶段：源码与二进制都允许破坏
- 1.0+：源码兼容（C API 同构不破坏 ABI），二进制兼容仅维护最新 minor

## 4.5 破坏性变更归档（下一次 minor 窗口执行）

> 收录「**本轮不做、但已排定窗口**」的破坏性变更。原则是**无限期推迟等于遗忘**，
> 故每条都要写清迁移说明要点，等窗口一到就能直接执行，不必重新论证。

### B-1：`derived_names` / `invalidate_analysis` 可见性收窄（U-16 / P2-15）

| 项 | 内容 |
| --- | --- |
| 位点 | `core/src/derive.rs:247` `pub fn derived_names`、`core/src/registry.rs:238` `pub fn invalidate_analysis` |
| 目标 | `pub` → `pub(crate)`（或 `#[doc(hidden)]` 过渡一个 minor） |
| 为什么是破坏性 | `nctool-core` 已发布 crates.io；收窄 `pub` 会让下游 `use nctool_core::derive::derived_names;` 直接编译失败 |
| 窗口 | **下一个 minor（`nctool-core 0.4.0`）**，与同窗口其他破坏项一起做，避免连开两次破坏性窗口 |

**迁移说明要点**（届时照抄进 CHANGELOG `Changed` 节）：

1. 两者在本仓库内**均无生产调用点** —— `derived_names` 只被 `derive.rs` 的单元测试
   使用（`:530-531`），`invalidate_analysis` 只有声明与文档引用。下游也没有已知的公开用例。
2. 迁移方式 = **删除该 import**，没有替代 API：
   - `derived_names` 的用途（"哪些参数是派生的"）在渲染路径上由 `derive::apply` 内部消费，
     调用方要的是派生结果而非名单；确实需要名单时用 `specs.iter().filter(|s| s.derive.is_some())`。
   - `invalidate_analysis` 的存在理由是 `TemplateEntry::source_text` 为 `pub`、
     改写后需手动失效缓存；**正确修法是让 `source_text` 变私有**（同时列为本窗口的 B-2 候选），
     使缓存失效由类型系统保证，届时 `invalidate_analysis` 直接删掉。
3. 若担心误伤，先走 `#[doc(hidden)]` + CHANGELOG 提示一个 minor，再在下下个 minor 删除。

### B-2（候选，同窗口评估）：`TemplateEntry::source_text` 私有化

B-1 的根因项。当前"改写源码后必须记得调 `invalidate_analysis`"全靠调用方自觉 ——
这正是 P2-15 把它列为破坏性项的原因。私有化后由 `set_source` 之类的方法内部完成失效，
不变量从"约定"变成"类型系统保证"。**与 B-1 同窗口做，否则 B-1 收窄后仍要留一个 pub 方法。**

### 本轮已发生的收紧（已进 CHANGELOG，不属本归档）

- 派生 `fallback` 语义收窄（CHANGELOG「批次二十四」）—— 行为变更，已在 `Changed` 节标注。
- 零件定义未知字段拒绝（CHANGELOG「批次二十五」）—— 行为变更，已在 `Changed` 节标注。
- 两者都是**收紧而非新增**：发布前需在 CHANGELOG 显著位置提示，受影响面限于
  "此前依赖静默兜底/静默忽略"的用法。

## 5. 决策与争议

- **轻量共识**：维护者本人是首要决策者（项目尚未到多人协作规模）。
- **争议升级**：开启 [Discussion](https://github.com/lzg9698-code/nctool-tpl/discussions) → 7 天讨论期 → 维护者拍板 → 在 commit / PR 里说明取舍。
- **工艺问题不通过 issue 处理**：见 [PROCESS_CHECKLIST.md](PROCESS_CHECKLIST.md) — 内置模板与机床预设未经真实工艺评审，**不应作为工具 bug 处理**。

## 6. 安全修复流程

- 标记 `security` + `urgent` 双 label
- 走 `TBD` private disclosure（如未来启用 GitHub Security Advisories）
- 修复 + 0-day 通告 + 立即发版 + backport 到上一个 minor

## 7. Backlog 排序

按下列维度打分（5 分制），加权后排序：

| 维度 | 权重 | 说明 |
| --- | --- | --- |
| 用户影响 | ×3 | 多少人用、影响多大流程 |
| 投入产出 | ×2 | 复杂度 vs 收益 |
| 风险 | ×2 | 是否引发数据丢失 / 工艺事故 |
| 依赖关系 | ×1 | 是否阻塞其他项 |

每季度（或累积 ≥ 5 项时）整理一次 Backlog；详见 [ROADMAP.md](ROADMAP.md) 末尾的 Backlog 段。

## 8. 相关文档

- [CHANGELOG.md](../CHANGELOG.md) — 版本演进记录
- [ROADMAP.md](ROADMAP.md) — 阶段划分与任务清单
- [PROCESS_CHECKLIST.md](PROCESS_CHECKLIST.md) — 工艺核对（不属于工具迭代范畴）
- [.github/ISSUE_TEMPLATE/](../.github/ISSUE_TEMPLATE/) — 提交 issue 的标准字段
