# 历史归档索引

归档于 2026-10-07。旧版资料集中在 `legacy/`，阶段实现、审查与验证证据在 `records/`。本文列出原路径、适用背景与当前替代入口；归档中的日期、命令、测试数量和通过状态仅描述原记录时点。

当前入口为[文档总览](../README.md)。2.x 不自动迁移旧 CLI、API、工序格式和业务模板；不要直接执行归档步骤来配置当前工作区。

## 旧版设计与指南

下表原路径均以 `docs/` 为根。旧文档原文保留，仅添加归档标识、调整相对链接，并说明已不存在的历史引用。

| 原路径 / 归档文件 | 适用版本与背景 | 当前替代入口 |
| --- | --- | --- |
| [ARCHITECTURE_2026-09-03.md](legacy/ARCHITECTURE_2026-09-03.md) | 2026-09-03 三 crate 架构快照 | [当前架构](../ARCHITECTURE.md) |
| [DEV_PLAN_CLI_UI.md](legacy/DEV_PLAN_CLI_UI.md) | 2026-09-01 起的旧 CLI/单文件 UI 计划 | [系统设计](../SYSTEM_DESIGN.md) · [路线图](../ROADMAP.md) |
| [UI_DESIGN_PROPOSAL.html](legacy/UI_DESIGN_PROPOSAL.html) | 2026-09-11 单文件 WebUI 设计 | [WebUI 指南](../WEBUI_USER_GUIDE.md) |
| [GUI_USER_GUIDE.md](legacy/GUI_USER_GUIDE.md) | 旧 Tauri GUI 操作；源码及启动入口已移除，历史文件 `gui/README.md` 当前仓库未保留 | [WebUI 指南](../WEBUI_USER_GUIDE.md) |
| [UPGRADE_READINESS.md](legacy/UPGRADE_READINESS.md) | 2026-09-30 的 1.0 候选就绪度 | [项目状态](../PROJECT_STATUS.md) |
| [ROADMAP.md](legacy/ROADMAP.md) | 旧 0.x → 1.0 A–F 阶段与进展 | [当前路线图](../ROADMAP.md) |
| [RELEASE.md](legacy/RELEASE.md) | 旧版本策略、三 crate 发布与 1.0 候选演练 | [当前发布说明](../RELEASE.md) |
| [TEMPLATE_WRITING_GUIDE.md](legacy/TEMPLATE_WRITING_GUIDE.md) | 旧文件模板与 NC 写法 | [当前模板指南](../TEMPLATE_WRITING_GUIDE.md) |
| [MACHINE_CONFIG_GUIDE.md](legacy/MACHINE_CONFIG_GUIDE.md) | 旧 core、TOML 与内置机床方案 | 顶层入口页已于 2026-10-18 移除，见[项目状态](../PROJECT_STATUS.md) |
| [PROCESS_CHECKLIST.md](legacy/PROCESS_CHECKLIST.md) | 旧内置模板与机床方案的自动核对 | 顶层入口页已于 2026-10-18 移除，见[项目状态](../PROJECT_STATUS.md) |
| [UI_ACCEPTANCE_CHECKLIST.md](legacy/UI_ACCEPTANCE_CHECKLIST.md) | 旧 UI 与 2026-09-10 走查结果 | [当前验收清单](../UI_ACCEPTANCE_CHECKLIST.md) |
| [REAL_PART_WALKTHROUGH.md](legacy/REAL_PART_WALKTHROUGH.md) | 旧内置模板的简化零件走查 | 历史示例文件，当前仓库未保留 |

## 旧版迁移与融合

| 原路径 / 归档文件 | 历史用途 | 当前替代入口 |
| --- | --- | --- |
| [OLD_PROJECT_TEMPLATE_IMPORT_CHECKLIST.md](legacy/OLD_PROJECT_TEMPLATE_IMPORT_CHECKLIST.md) | 旧项目文件模板提取任务 | [模板指南](../TEMPLATE_WRITING_GUIDE.md) |
| [TEMPLATE_ZIP_IMPORT_REPORT.md](legacy/TEMPLATE_ZIP_IMPORT_REPORT.md) | V3 导出包 28 份模板的历史导入与 SHA-256 记录 | [工作区迁移](../WEBUI_USER_GUIDE.md#工作区迁移) |
| [fusion/README.md](legacy/fusion/README.md) | 旧 V3 融合基线；原目录 `docs/fusion/` 整体迁入 | [模板指南](../TEMPLATE_WRITING_GUIDE.md) |

融合目录内的 [source-baseline.json](legacy/fusion/source-baseline.json)、[template-mapping.json](legacy/fusion/template-mapping.json)、[rust-source.patch](legacy/fusion/rust-source.patch) 和 [v3-source.patch](legacy/fusion/v3-source.patch) 保留原始字节及原路径记录，不改写补丁或指纹数据。原 README 提及的 `source.patch` 未以该文件名保留，现存补丁以上述实际文件为准。源资产许可与历史迁移的 unreviewed 状态仍保留原说明。

## 阶段审查与验证

| 原路径 | 归档文件 | 记录背景 |
| --- | --- | --- |
| `docs/CODE_REVIEW_2026-10-02.md` | [代码审查及修复关闭](records/CODE_REVIEW_2026-10-02.md) | 2.2 阶段审查与 2026-10-03 的 R01–R10 关闭证据 |
| `docs/PROJECT_STATUS.md` | [阶段实现与验证历史](records/IMPLEMENTATION_HISTORY_2026-10-04.md) | 2026-10-01 至 10-04 的升级、迭代、本机验证与快照记录 |
| 本轮独立参数与工艺迁移 | [实际验证记录](records/MANAGED_PARAMETERS_2026-10-07.md) | 2026-10-07 Windows；28 份模板、Rust 工艺计算、CLI/HTTP/WebUI 验收 |
| 同步粗车与越程槽专用插件 | [实施与验证记录](records/SPECIALIZED_TURNING_2026-10-08.md) | 2026-10-08 Windows；业务页面、Rust 计算、轨迹预设与兼容回归 |
| HEAD f0fda2d 之后的阶段改动 | [阶段收尾与本地提交](records/PHASE_CLOSEOUT_2026-10-08.md) | 2026-10-08 Windows；四处修复、完整本地门槛、运行资产隔离与提交范围 |
| 项目精简第一轮 | [实施与验证记录](records/2026-10-13-simplification-iteration.md) | 2026-10-13 Windows；产品主线、共享依赖核对、插件政策、构建隔离和轴类契约 |
| NC 最终处理链收敛 | [实施与验证记录](records/2026-10-13-nc-finalize-iteration.md) | 2026-10-13 Windows；共享最终处理、模板读取隔离与新任务重放格式修复 |
| `docs/PROJECT_REVIEW_PLAN_2026-10-15.md` | [项目审查计划](records/PROJECT_REVIEW_PLAN_2026-10-15.md) | 2026-10-15；审查范围与方法 |
| `docs/PROJECT_REVIEW_REPORT_2026-10-15.md` | [项目审查报告](records/PROJECT_REVIEW_REPORT_2026-10-15.md) | 2026-10-15 Windows；确认 8 项缺陷 |
| `docs/P1_FIXES_2026-10-15.md` | [P1 修复记录](records/P1_FIXES_2026-10-15.md) | 2026-10-15；R1–R4 修复与回归 |
| `docs/P2_FIXES_2026-10-15.md` | [P2 修复记录](records/P2_FIXES_2026-10-15.md) | 2026-10-15；R5–R8 修复与回归 |
| `docs/RESIDUAL_CLEANUP_PLAN_2026-10-14.md` | [残余清理范围与状态](records/RESIDUAL_CLEANUP_PLAN_2026-10-14.md) | 2026-10-14/15；上一轮清理边界与证据 |
| `docs/SPECIALIZED_TURNING_PLUGINS_PLAN.md` | [专用插件方案](records/SPECIALIZED_TURNING_PLUGINS_PLAN.md) | 2026-10-08；规划原文 |
| `docs/DEVELOPMENT_PLAN_2026-10-16.md` | [轻量完整易用开发方案](records/DEVELOPMENT_PLAN_2026-10-16.md) | 2026-10-16；移除范围与验收条件 |
| `docs/NC_REMOVAL_IMPLEMENTATION_2026-10-16.md` | [NC 移除最终实施记录](records/NC_REMOVAL_IMPLEMENTATION_2026-10-16.md) | 2026-10-16；实施、本机验证与签名范围 |
| `docs/NC_REMOVAL_RECOVERY_2026-10-16.md` | [NC 移除恢复说明](records/NC_REMOVAL_RECOVERY_2026-10-16.md) | 2026-10-16；恢复包核对方法 |
| `docs/CODE_AND_FUNCTION_STATUS_2026-10-17.md` | [代码与功能现状报告](records/CODE_AND_FUNCTION_STATUS_2026-10-17.md) | 2026-10-17；精简层次与本机测量 |
| `docs/OPTIMIZATION_LOG_2026-10-17.md` | [持续优化记录](records/OPTIMIZATION_LOG_2026-10-17.md) | 2026-10-17；缺陷修复与架构收敛 |
| `docs/P3_RELEASE_CANDIDATE_2026-10-17.md` | [P3 发行候选](records/P3_RELEASE_CANDIDATE_2026-10-17.md) | 2026-10-17；v2.2.0 候选身份与证据 |
| `docs/COMMIT_MESSAGE_CORRECTION_PENDING.md` | [提交说明修正待执行](records/COMMIT_MESSAGE_CORRECTION_PENDING.md) | 一次性事故记录，目标已失效 |
| `docs/CLEANUP_PLAN_2026-10-18.md` | [彻底清理方案与执行结果](records/CLEANUP_PLAN_2026-10-18.md) | 2026-10-18；B1–B6 批次、判定依据与本机验证 |

当前状态页已与历史记录分开。历史日志、浏览器截图和 `.nctool-backups/` 快照为当时本地产物，不保证随仓库提供；记录不等于本轮已重新执行，源码快照不等于提交或发布。缺失历史文件会明确标注，不能以当前文件或新造记录替代。

2026-10-18 起，原 `docs/` 顶层的阶段报告按原文件名整体移入 `records/`，并在文件头标注原路径。这些记录中的源码路径、`examples/v3-managed/*`、`target/*` 报告链接指向已删除或仅本机存在的内容，属历史引用；需要原文时以恢复包为准。
