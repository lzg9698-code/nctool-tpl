# 本轮交付概览：三大编辑模块计划 + T01/T02 落地

## 一、项目计划（已交付）

为 **模板编辑 / 机床配置编辑 / 预设参数编辑** 三个模块编制了项目计划，覆盖模块范围、关键操作流程、模块关联关系、预期交付成果与阶段划分。

| 文档 | 内容 |
| --- | --- |
| `docs/PROJECT_PLAN_EDIT_MODULES.md` | **主交付物**：项目计划 + T01 执行记录（§6.7）+ T02 执行记录（§6.8）+ 已知问题登记（§6.9） |
| `docs/PRD_EDIT_MODULES.md` | 需求：现状缺口、用户故事、F1–F17 流程、AC 验收标准、P0/P1/P2 需求池 |
| `docs/ARCH_DESIGN_EDIT_MODULES.md` | 架构设计 **v1.1**：四大难题决策、文件清单、类图/时序图、共享约定（§7 共 16 条）、已知边界 |
| `docs/SYSTEM_DESIGN.md` | 同步至 **v2.1**：crate 职责矩阵修订 + 新增设计决策 **D19** + 扩展点 + 已知边界 |

**规划期间推翻的前提**：三个模块都不是"从零建"——预设编辑前端**已完整实现**（localStorage），模板/机床编辑在演示模式已有界面但服务模式被显式禁用；后端 7 条路由**全部只读**。真缺口 = 持久化到 CLI 可见位置 + schema 化校验 + 写操作安全 + UI 硬上限落位。

**已拍板决策**：编辑主入口 = CLI 为主、UI 为辅；预设持久化 = 文件后端默认启用、落配置目录；机床缺键 = 阻断保存。

---

## 二、T01 共用写盘底座（已通过验证）

| 项 | 实测值 |
| --- | --- |
| 六道质量门 | 全部 RC=0 |
| 测试 | 641 passed / 0 failed / 2 ignored |
| 覆盖率（生产口径） | 89.35% |
| `Cargo.lock` | 112 → 112，**零新增 crate** |

**交付物**：`core/src/asset/{mod,atomic,guard,path,spec_fingerprint}.rs`、`nctool_tpl::extract_member_accesses`、`output.rs` 新增 kind `write_conflict`/`name_conflict`（→6）。

**QA 击穿并修复的 2 个缺陷**：
1. **P1 路径逃逸**：`SafePath::resolve("Z:")` 返回根外路径。根因是「注释里的假设没被代码强制」——`validate_asset_name` 注释写"恰为一个**普通**组件"，实现却只数 `components().count() == 1`，而 Windows 上 `"Z:"` 恰好是 1 个 `Prefix` 组件。**修法两层**：名称校验要求唯一 `Component::Normal` + `resolve` 对 `candidate`（而非 `anchor`）断言根包含。
2. **P2 指纹歧义**：`specFingerprint` 的 `None` 记 `-` 与真实值 `"-"` 撞车。**修法**：全字段长度前缀编码。这是改编码的**最后窗口**。

---

## 三、T02 模块一 · 模板编辑（已通过两轮验证，路由 NoOne）

| 项 | 实测值 |
| --- | --- |
| 六道质量门 | fmt / clippy `-D warnings` / test / rustdoc `-D warnings` 全部 RC=0（主理人复跑）；覆盖率 QA 实测 89.52% |
| 测试 | **734 passed / 0 failed / 2 ignored** |
| `Cargo.lock` | 112 包，零新增 crate |
| 契约 | 退出码 0–7 未变、golden 45 文件无变更、`cli_e2e` 44/44 绿 |
| UI | 两份 md5 一致（`a45f60cba1db266b26056bdeb696842d`）、各 2766 行（+44/−13，在 +45 预算内） |

**交付物**：`core/src/asset/template.rs`（`TemplateWriter` + `templates.yaml` 定点文本编辑）；`core/src/validate.rs` 新增两个 `pub` 入口 `check_spec_consistency` / `check_param_values`；`cli/src/commands/templates.rs` 新增 `edit`/`derive`/`rename` 且 `new` 迁移写内核 + 落清单；`cli/tests/cli_edit_e2e.rs`；两份 `ui/index.html`。

**新规范：保存前校验分级 L1/L2/L3**（`ARCH_DESIGN_EDIT_MODULES.md` §7 第 15 条）

| 级别 | 触发 | 阻断 |
| --- | --- | --- |
| L1 语法 | 总是 | `parse` 失败 → 退出码 1（报行列） |
| L2 规格自洽 | 总是 | **不依赖参数值**的检查 → 退出码 1 |
| L3 参数值校验 | 提供 `--param`/`--params-file` 时 | 只校验**已提供**参数的值，**不查缺失** → 退出码 1 |

**QA 击穿并修复的缺陷（按性质分类——这类问题今天反复出现）**

| 类型 | 实例 |
| --- | --- |
| **真缺陷（静默失效）** | **P1**：`manifest_entry_body` 只在空行/顶格行停、**不在同级键行停** → `derive` 吞并相邻兄弟条目 → 重复键 → `serde_yaml` 拒收**整份清单** → **全部模板元数据静默丢失**。仓库自带清单用空行分隔故 CI 永不触发，**只有用户手写清单会中招** |
| **文档承诺的行为不存在** | **P2-1**：模块文档承诺"清单解析失败 → 降级警告"，但定点编辑是纯文本操作、全链路从不 parse → 非法 YAML 被**无警告改写** |
| **P4 单一来源违规** | `default` 自洽判定被手写两遍（`validate.rs::check_spec_defaults` 已有，`asset/template.rs` 又写一遍）→ 同一模板在 `nctool validate` 与 `templates edit` 下可能结论不同 |
| **测试把缺陷固化成预期** | 工程师曾写 `entry_body_falls_back_when_field_less_indented_than_key`，**锁住的正是 P1 的错误行为**。**一条钉住缺陷的测试比没有测试更糟**——它让缺陷看起来被验证过。已删除并换成断言正确语义的两条测试 |
| **绝对句被代码证伪** | D19 原文"`nctool-cli` 不 `fs::write`/`File::create`"，而 `config.rs:183`（`config init`）就是 → 作用域收准为**资产写**，例外登记在案 |
| **假并发保证** | 注释称重名检查已"变为内核原子判定（并发下也不会互相覆盖）"。**不成立**：`write_guarded(expect=None)` 仍是 check-then-write |

---

## 四、未做 / 待办

- **T03–T05 未启动**（预设编辑 / 机床编辑 / 收口）。
- **5 项已知问题已登记**（计划 §6.9）：K-1 目录占位退出码分类、K-2 `IssueKind` 缺"来源"维度、K-3 重名检查非原子、K-4 `$EDITOR` 分支未自动化覆盖、K-5 悬空符号链接向量未实测。
- **两处文档漂移待修**：`docs/PROJECT_STATUS.md` 记「UI 2685 行」（实测 2735/2766）与「XSS P1-1 未修」（已修）。
- **⚠️ T01 + T02 全部改动至今未提交**（`git status` 有大量 `M`/`??`，含 4 个新增源文件、5 个新测试文件、3 份新文档）。项目 ROADMAP 把「未纳入版本控制」列为**已发生的最高优先风险**，建议尽快提交一次作为检查点。
