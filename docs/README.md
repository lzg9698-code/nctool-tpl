# 文档总览

本目录的当前文档面向 **NCtool 2.2 工作台**。CLI/WebUI 为 2.2.1，核心组件为 2.0.0，外部协议和版本化服务仍使用各自的协议版本。具体版本以各组件清单为准。

2026-10-16 范围决定：**移除全部内置 NC 能力**，保留通用模板、模板内参数、插件机制和独立数学扩展，见[修订方案](archive/records/DEVELOPMENT_PLAN_2026-10-16.md)。源码层已完成该移除：默认组合为 `template`，`math` 为唯一可选 feature，旧 `nc` profile 与内置 NC 插件、动作/服务、页面、示例与资源均已退出当前实现。兼容保护已本机验证。P1（入门示例、参数编辑）、P2（职责拆分、样式模块化、历史管理、性能测量）与 P3（提交、三平台 CI、审计、`v2.2.0` 发布与下载后签收）均已完成，见[发行候选](archive/records/P3_RELEASE_CANDIDATE_2026-10-17.md)与[项目状态](PROJECT_STATUS.md)。旧加工指南已从顶层导航移除，原始版本见[历史归档](archive/README.md)。

第一次使用先阅读[项目 README](../README.md)；旧版命令、GUI 和迁移资料在[历史归档](archive/README.md)，不作为当前操作说明。

## 使用指南

| 文档 | 适合的读者与内容 |
| --- | --- |
| [WebUI 使用指南](WEBUI_USER_GUIDE.md) | 使用者：启动、模板表单、生成导出、历史、插件与资产迁移 |
| [模板编写指南](TEMPLATE_WRITING_GUIDE.md) | 模板作者：JSON 资产、参数定义、模板引用与显式渲染扩展 |
| [模板参数](PARAMETER_MANAGEMENT.md) | 每个模板的参数定义、默认值与本次填写 |

已移除的内置能力（机床、工序、同步粗车、越程槽、轨迹、旧固定工作台迁移）不再设独立入口页。原始指南按原路径保存在 `docs/archive/legacy/` 的恢复包中，恢复方法见[历史归档索引](archive/README.md)；当前能力见[项目状态](PROJECT_STATUS.md)，领域能力通过[插件协议](PLUGIN_PROTOCOL.md)自行实现。移除前的完整源码上下文与恢复说明见[NC 移除恢复说明](archive/records/NC_REMOVAL_RECOVERY_2026-10-16.md)。

## 开发与接口

| 文档 | 内容 |
| --- | --- |
| [架构与依赖边界](ARCHITECTURE.md) | 引擎、插件、运行时、宿主和 WebUI 的职责与数据流 |
| [系统设计与配置](SYSTEM_DESIGN.md) | 配置目录、JSON 资产、动作、HTTP 与任务接口 |
| [插件协议](PLUGIN_PROTOCOL.md) | SDK、双向 JSON-RPC、服务、页面和资产集合贡献 |
| [贡献指南](CONTRIBUTING.md) | 工具链、构建、测试、源码快照与修改边界 |
| [AI 开发工具与 Skills](CODEX_DEVELOPMENT.md) | Codex/Claude Code 环境、专用技能、环境自检、构建目录、GitHub、浏览器、Context7 与依赖审计 |
| [WebUI 验收清单](UI_ACCEPTANCE_CHECKLIST.md) | 当前工作台的正常、失败、冲突及恢复场景 |

## 项目状态与发布

| 文档 | 内容 |
| --- | --- |
| [项目状态](PROJECT_STATUS.md) | 当前能力、限制、待验证事项与历史证据入口 |
| [路线图](ROADMAP.md) | 按 P0–P3 组织的待办与交付门槛，无交付日期承诺 |
| [当前开发边界（AI 复刻用）](DEVELOPMENT_BOUNDARY_2026-10-17.md) | 按源码契约列出组件 API、数据格式、协议、常量与硬约束，供独立复刻 |
| [发布说明](RELEASE.md) | `v2.*` 标签、预览版语义、CI 前置验证、二进制与校验文件 |
| [2.3 预览说明](PREVIEW_2026-10-20.md) | 与 2.2 的破坏性差异、验证证据、已知限制与回退 |
| [变更记录](../CHANGELOG.md) | 未发布改动及原有版本历史 |

阶段记录（发行候选、NC 移除实施、优化日志、开发方案、审查与修复记录）已移入[历史归档](archive/README.md)的 `records/`，仍可回溯，但不再作为当前操作说明。

## 历史资料

[历史归档索引](archive/README.md)区分旧版设计与迁移资料、阶段审查与验证记录。归档中的日期、测试数量和通过状态只描述记录时点。文档提及的本地 `target/` 日志和 `.nctool-backups/` 快照不随仓库或发行包保证提供。

文档维护时以代码和工作流核对行为；命令集中使用现有示例，验收结果记录日期与平台。不要用“CI 已配置”代替“CI 已实际通过”，也不要用测试数量代替当前验证结果。
