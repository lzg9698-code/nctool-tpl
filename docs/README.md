# 文档总览

本目录面向当前通用模板工作台。CLI/WebUI 为 **2.3.0（2026-10-24 发布）**，核心组件清单为 **2.0.0**，外部进程协议为 **2**，服务版本独立。以源码清单和协议常量为准。当前前端入口、技术栈及草稿边界见 [WebUI 直接重写](UI_REWRITE.md)；当前能力和待验证项见 [项目状态](PROJECT_STATUS.md)。

默认组合为 `template` + `parameters`，`math` 为唯一可选 feature。内置 NC 能力和旧 Tauri GUI 已退出活动实现；历史发行和阶段验证仅在对应记录的时间点成立。

第一次使用先阅读[项目 README](../README.md)；旧版命令、GUI 和迁移资料在[历史归档](archive/README.md)，不作为当前操作说明。

## 使用指南

| 文档 | 适合的读者与内容 |
| --- | --- |
| [WebUI 使用指南](WEBUI_USER_GUIDE.md) | 使用者：启动、模板表单、生成导出、历史、插件与资产迁移 |
| [WebUI 直接重写](UI_REWRITE.md) | 开发者：当前组件库、状态职责、无迁移边界与新增验证入口 |
| [模板编写指南](TEMPLATE_WRITING_GUIDE.md) | 模板作者：JSON 资产、参数定义、模板引用与显式渲染扩展 |
| [独立参数与受管生成](PARAMETER_MANAGEMENT.md) | 共享定义、预置、绑定、来源、冻结快照与通用受管渲染 |

已移除的内置能力（机床、工序、同步粗车、越程槽、轨迹、旧固定工作台迁移）不再设独立入口页。原始指南按原路径保存在 `docs/archive/legacy/` 的恢复包中，恢复方法见[历史归档索引](archive/README.md)；当前能力见[项目状态](PROJECT_STATUS.md)，领域能力通过[插件协议](PLUGIN_PROTOCOL.md)自行实现。移除前的完整源码上下文与恢复说明见[NC 移除恢复说明](archive/records/NC_REMOVAL_RECOVERY_2026-10-16.md)。

## 开发与接口

| 文档 | 内容 |
| --- | --- |
| [架构与依赖边界](ARCHITECTURE.md) | 引擎、插件、运行时、宿主和 WebUI 的职责与数据流 |
| [系统设计与配置](SYSTEM_DESIGN.md) | 配置目录、JSON 资产、动作、HTTP 与任务接口 |
| [宿主与插件契约](CONTRACTS.md) | 跨语言类型、结构化诊断、协议版本与共享验收样例 |
| [历史、追溯与调用诊断](OPERATIONS.md) | 历史保留、备份恢复、环境指纹和调用元数据 |
| [插件作者验收](PLUGIN_ACCEPTANCE.md) | 独立开发验收工具、测试计划、故障注入与进程恢复 |
| [插件协议](PLUGIN_PROTOCOL.md) | SDK、双向 JSON-RPC、服务、页面和资产集合贡献 |
| [贡献指南](CONTRIBUTING.md) | 工具链、构建、测试、源码快照与修改边界 |
| [AI 开发工具与 Skills](CODEX_DEVELOPMENT.md) | Codex/Claude Code 环境、专用技能、环境自检、构建目录、GitHub、浏览器、Context7 与依赖审计 |
| [WebUI 验收清单](UI_ACCEPTANCE_CHECKLIST.md) | 当前工作台的正常、失败、冲突及恢复场景 |

## 项目状态与发布

| 文档 | 内容 |
| --- | --- |
| [项目状态](PROJECT_STATUS.md) | 当前能力、限制、待验证事项与历史证据入口 |
| [路线图](ROADMAP.md) | 按 P0–P3 组织的待办与交付门槛，无交付日期承诺 |
| [2026-10-17 开发边界快照（历史参考）](DEVELOPMENT_BOUNDARY_2026-10-17.md) | 历史源码契约快照；当前实现以架构、宿主契约和源码为准 |
| [发布说明](RELEASE.md) | `v2.*` 标签、CI 前置验证、二进制与校验文件 |
| [变更记录](../CHANGELOG.md) | 未发布改动及原有版本历史 |

阶段记录（发行候选、NC 移除实施、优化日志、开发方案、审查与修复记录）已移入[历史归档](archive/README.md)的 `records/`，仍可回溯，但不再作为当前操作说明。

## 历史资料

[历史归档索引](archive/README.md)区分旧版设计与迁移资料、阶段审查与验证记录。归档中的日期、测试数量和通过状态只描述记录时点。文档提及的本地 `target/` 日志和 `.nctool-backups/` 快照不随仓库或发行包保证提供。

文档维护时以代码和工作流核对行为；命令集中使用现有示例，验收结果记录日期与平台。不要用“CI 已配置”代替“CI 已实际通过”，也不要用测试数量代替当前验证结果。
