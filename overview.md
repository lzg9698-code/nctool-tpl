# 本轮交付概览（两条开发线合并后的当前快照）

> 更新：2026-09-24。本文是**当前快照摘要**，权威现状见 `docs/PROJECT_STATUS.md`，规划见 `docs/ROADMAP.md`。
> 2026-09-24 之前本项目存在两条并行开发线（本地线含三大编辑模块 / `part generate` / 下溢与精度修复，
> 远程线含 `nctool lint` / golden 刷新守卫 / 发布包收窄），本文是**两线合并后**的统一视图。

## 当前位置

- 阶段 A–F **全部收口**（ROADMAP 执行跟踪 71/72，99%；余 A1 机床预设按手册核对 = 外部依赖）。
- 三个 crate 已发版 crates.io：`nctool-tpl` 0.4.0 / `nctool-core` 0.3.0 / `nctool-cli` 0.3.0，
  并挂三平台 GitHub Release 二进制。
- CLI **11 个子命令**全部可用：`templates` / `inspect` / `lint` / `validate` / `render` /
  `machine` / `preset` / `config` / `ui` / `part` / `completion`。Web UI 完整交互闭环。
- 模板库：**7 个内置模板 + 25 个文件模板**（NCTool_V3 资产已整合，21 组正向 golden + 3 组负向报告）。
- 三大编辑模块（模板 / 机床配置 / 预设参数）**T01–T05 全部落地**，均经 QA 独立验证。

## 质量基线（2026-09-24 合并后复测）

| 门 | 结果 |
|---|---|
| workspace 测试（`--all-targets`） | **1061 通过 / 0 失败**（另 2 项 `#[ignore]`：万行级实测） |
| Doc-test | 4 通过 + 1 `#[ignore]` |
| 覆盖率（生产口径） | **92.62%**（8144/8793），门禁 **≥ 91%**；llvm-cov 原始口径 95.13%（仅参考） |
| `cargo fmt --all --check` | 干净 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 零告警 |
| UI 生成物 | 两份 md5 一致（`125e8e16af569fb509dddfa0a20c13af`），各 2942 行 |
| 对拍门 | 参数 40 例 / 接口 11 条 / 选项 5 组，全绿 |
| golden | 46 文件字节不变（`integration.rs` 两组基线测试断言） |
| MSRV | 1.85（三个 Cargo.toml 与 CI `msrv` job 一致） |
| CI | 三平台矩阵 + 覆盖率门 + 前后端对拍门 + UI md5 一致门，均为阻断项 |

> 全部实测于 2026-09-24（两条开发线合并后）。覆盖率阈值 **91%** 沿用远程线 2026-09-22 的上调结果，
> 本地线历史文档写的是 89% —— **以 91% 为准**。

## 一、三大编辑模块计划（本地线，已交付）

| 文档 | 内容 |
| --- | --- |
| `docs/PROJECT_PLAN_EDIT_MODULES.md` | **主交付物**：项目计划 + T01 执行记录（§6.7）+ T02 执行记录（§6.8）+ 已知问题登记（§6.9） |
| `docs/PRD_EDIT_MODULES.md` | 需求：现状缺口、用户故事、F1–F17 流程、AC 验收标准、P0/P1/P2 需求池 |
| `docs/ARCH_DESIGN_EDIT_MODULES.md` | 架构设计 **v1.1**：四大难题决策、文件清单、类图/时序图、共享约定（§7 共 16 条）、已知边界 |
| `docs/SYSTEM_DESIGN.md` | 同步至 **v2.1**：crate 职责矩阵修订 + 新增设计决策 **D19** + 扩展点 + 已知边界 |

**规划期间推翻的前提**：三个模块都不是"从零建"——预设编辑前端**已完整实现**（localStorage），模板/机床编辑
在演示模式已有界面但服务模式被显式禁用；后端 7 条路由**全部只读**。真缺口 = 持久化到 CLI 可见位置 +
schema 化校验 + 写操作安全 + UI 硬上限落位。

**已拍板决策**：编辑主入口 = CLI 为主、UI 为辅；预设持久化 = 文件后端默认启用、落配置目录；机床缺键 = 阻断保存。

## 二、T01/T02 的 QA 发现（缺陷分类学，可复用）

QA 在 T01/T02 击穿并推动修复的缺陷，按性质分类后比结论更有价值：

| 类型 | 实例 |
| --- | --- |
| **真缺陷（静默失效）** | **P1**：`manifest_entry_body` 只在空行/顶格行停、**不在同级键行停** → `derive` 吞并相邻兄弟条目 → 重复键 → `serde_yaml` 拒收**整份清单** → **全部模板元数据静默丢失**。仓库自带清单用空行分隔故 CI 永不触发，**只有用户手写清单会中招** |
| | **P1 路径逃逸**：`SafePath::resolve("Z:")` 返回根外路径。根因是「注释里的假设没被代码强制」——注释写"恰为一个**普通**组件"，实现却只数 `components().count() == 1`，而 Windows 上 `"Z:"` 恰好是 1 个 `Prefix` 组件 |
| **文档承诺的行为不存在** | **P2-1**：模块文档承诺"清单解析失败 → 降级警告"，但定点编辑是纯文本操作、全链路从不 parse → 非法 YAML 被**无警告改写** |
| **P4 单一来源违规** | `default` 自洽判定被手写两遍（`validate.rs::check_spec_defaults` 已有，`asset/template.rs` 又写一遍）→ 同一模板在 `nctool validate` 与 `templates edit` 下可能结论不同 |
| **测试把缺陷固化成预期** | 工程师曾写 `entry_body_falls_back_when_field_less_indented_than_key`，**锁住的正是 P1 的错误行为**。**一条钉住缺陷的测试比没有测试更糟**——它让缺陷看起来被验证过。已删除并换成断言正确语义的两条测试 |
| **绝对句被代码证伪** | D19 原文"`nctool-cli` 不 `fs::write`/`File::create`"，而 `config.rs:183`（`config init`）就是 → 作用域收准为**资产写**，例外登记在案 |
| **假并发保证** | 注释称重名检查已"变为内核原子判定（并发下也不会互相覆盖）"。**不成立**：`write_guarded(expect=None)` 仍是 check-then-write |
| **指纹歧义** | `specFingerprint` 的 `None` 记 `-` 与真实值 `"-"` 撞车 → 改为全字段长度前缀编码（这是改编码的**最后窗口**） |

**保存前校验分级 L1/L2/L3**（`ARCH_DESIGN_EDIT_MODULES.md` §7 第 15 条）

| 级别 | 触发 | 阻断 |
| --- | --- | --- |
| L1 语法 | 总是 | `parse` 失败 → 退出码 1（报行列） |
| L2 规格自洽 | 总是 | **不依赖参数值**的检查 → 退出码 1 |
| L3 参数值校验 | 提供 `--param`/`--params-file` 时 | 只校验**已提供**参数的值，**不查缺失** → 退出码 1 |

## 三、远程线并入的独有工作

| 项 | 内容 |
| --- | --- |
| `nctool lint` | 模板内出现弧度制三角函数时提示度制风险（`src/lint.rs` + `cli/src/commands/lint.rs`），呼应「度制三角函数必须用 `_d` 后缀」红线 |
| golden 刷新守卫 | `scripts/refresh_golden.sh`（CI 拒刷 + 数量守卫 + diff 概览）+ CI 自测步骤 |
| 选项对拍门 | `--line-step` / `--max-line` 补齐到 CLI，`scripts/check_option_parity.mjs` + `option_parity_cases.json` 保证 CLI 与 Web API 映射到同一份 `GenerationOptions` |
| 包内容守卫 | `scripts/check_package_contents.py`，`nctool-tpl` 发布包 498 KiB → 126 KiB |
| 弱断言补强 | fuzz 从「不 panic」提升到「产出自洽」 |

## 四、未做 / 待办

- **前端两处缺陷**：P2-25 Bool 参数恒提交 `false`、P2-26 无 `AbortController`（**均需浏览器验证**，
  本机 Windows 不支持 agent-browser；条件不具备时整体延后，禁止盲改合并）。
- **第五轮审查的静默缺陷**：P1-2 派生 fallback 绕过白名单、P2-3 零件定义未知字段未拒绝、
  P2-1 行号上限静默停编号 —— 见 `docs/CODE_REVIEW_2026-09-23.md`，本轮计划的阶段二处理。
- **校验覆盖面结构性缺口**：仅参与派生、不出现在模板正文的参数，其约束全部静默失效 —— 待裁定。
- **发布流程**：三个 crate 的 `exclude` 已收窄；下次发版**先推 tag 交 CI 发布**（本地手工发布路径废止）。
- **Backlog**：内置模板补外圆车削与攻丝、浏览器内模板编辑、i18n。
- **长期外部依赖**：真实工艺评审与机床空运行（R1，Q2=否）—— 代码无法解决。

## 合并遗留（阶段二起处理）

- [ ] **第五轮审查的静默缺陷**：P1-2 派生 fallback 绕过白名单、P2-3 零件定义未知字段未拒绝、
      P2-1 行号上限静默停编号（见 `docs/CODE_REVIEW_2026-09-23.md`）
- [ ] **校验覆盖面结构性缺口裁定**：仅参与派生、未出现在模板正文的参数，其约束全部静默失效
- [ ] **审查报告标注**：`docs/CODE_REVIEW_2026-09-23.md` 补「已修 / 已裁定」标注
