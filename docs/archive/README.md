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
| [MACHINE_CONFIG_GUIDE.md](legacy/MACHINE_CONFIG_GUIDE.md) | 旧 core、TOML 与内置机床方案 | [当前机床指南](../MACHINE_CONFIG_GUIDE.md) |
| [PROCESS_CHECKLIST.md](legacy/PROCESS_CHECKLIST.md) | 旧内置模板与机床方案的自动核对 | [当前工艺清单](../PROCESS_CHECKLIST.md) |
| [UI_ACCEPTANCE_CHECKLIST.md](legacy/UI_ACCEPTANCE_CHECKLIST.md) | 旧 UI 与 2026-09-10 走查结果 | [当前验收清单](../UI_ACCEPTANCE_CHECKLIST.md) |
| [REAL_PART_WALKTHROUGH.md](legacy/REAL_PART_WALKTHROUGH.md) | 旧内置模板的简化零件走查 | [多工序示例](../../examples/v2/process.json) · [工艺清单](../PROCESS_CHECKLIST.md) |

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

当前状态页已与历史记录分开。历史日志、浏览器截图和 `.nctool-backups/` 快照为当时本地产物，不保证随仓库提供；记录不等于本轮已重新执行，源码快照不等于提交或发布。缺失历史文件会明确标注，不能以当前文件或新造记录替代。
