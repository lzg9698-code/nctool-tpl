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

## 3. 提交流程

- **主分支**：`master`（当前 main vs master：本项目沿用 `master`）
- **提交风格**：不强制 squash / rebase / merge commit。每条 commit 应当**自包含**（独立编译、独立测试通过），便于 `git bisect` 与 `git revert`。
- **commit message**：中文或英文均可，但需含**动机**（why），不只是"改了 X"（what）。本次提交对照参考：`git log --oneline`。
- **PR 流程**（v1.0 后启用；0.x 阶段 main 上直推 + 事后 review）：
  - 1 个 approve（维护者本人可省）
  - CI 全绿（含 ubuntu/windows/macos 三平台）
  - 若有 `Changed` 破坏项 → 必须在 PR 描述里写迁移指南
- **不阻塞**：`coverage` job（`B-Backlog` 标记）非阻断；其他全部必须绿

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
