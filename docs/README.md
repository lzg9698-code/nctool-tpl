# 文档总览

本目录的当前文档面向 **NCtool 2.2 工作台**。CLI/WebUI 为 2.2.0，核心组件为 2.0.0，外部协议和版本化服务仍使用各自的协议版本。具体版本以各组件清单为准。

第一次使用先阅读[项目 README](../README.md)；旧版命令、GUI 和迁移资料在[历史归档](archive/README.md)，不作为当前操作说明。

## 使用指南

| 文档 | 适合的读者与内容 |
| --- | --- |
| [WebUI 使用指南](WEBUI_USER_GUIDE.md) | 使用者：启动、模板表单、生成导出、历史、插件与资产迁移 |
| [模板编写指南](TEMPLATE_WRITING_GUIDE.md) | 模板作者：JSON 资产、参数定义、模板引用、扩展与 NC 元数据 |
| [机床配置指南](MACHINE_CONFIG_GUIDE.md) | NC 使用者：机床资产、配置值、生成选项与冲突处理 |
| [独立参数与受管生成](PARAMETER_MANAGEMENT.md) | 共享定义、预置、刀具槽位、原生计算、组合文件与快照 |
| [同步粗车与越程槽专用插件](SPECIALIZED_TURNING_PLUGINS.md) | 业务表格、左右批量、轨迹预设、原始输入接口与组合 |
| [工艺核对清单](PROCESS_CHECKLIST.md) | NC 资产维护者：模板、机床和输出的逐项核对 |

## 开发与接口

| 文档 | 内容 |
| --- | --- |
| [架构与依赖边界](ARCHITECTURE.md) | 引擎、插件、运行时、宿主和 WebUI 的职责与数据流 |
| [系统设计与配置](SYSTEM_DESIGN.md) | 配置目录、JSON 资产、动作、HTTP 与任务接口 |
| [插件协议](PLUGIN_PROTOCOL.md) | SDK、双向 JSON-RPC、服务、页面和资产集合贡献 |
| [贡献指南](CONTRIBUTING.md) | 工具链、构建、测试、源码快照与修改边界 |
| [Codex 开发工具与 Skills](CODEX_DEVELOPMENT.md) | 项目规则、专用技能、GitHub、浏览器、Context7 与依赖审计 |
| [WebUI 验收清单](UI_ACCEPTANCE_CHECKLIST.md) | 当前工作台的正常、失败、冲突及恢复场景 |

## 项目状态与发布

| 文档 | 内容 |
| --- | --- |
| [项目状态](PROJECT_STATUS.md) | 当前能力、限制、待验证事项与历史证据入口 |
| [路线图](ROADMAP.md) | 已记录且仍待完成的工作，无交付日期承诺 |
| [发布说明](RELEASE.md) | `v2.*` 标签、CI 前置验证、二进制与校验文件 |
| [变更记录](../CHANGELOG.md) | 未发布改动及原有版本历史 |

## 历史资料

[历史归档索引](archive/README.md)区分旧版设计与迁移资料、阶段审查与验证记录。归档中的日期、测试数量和通过状态只描述记录时点。文档提及的本地 `target/` 日志和 `.nctool-backups/` 快照不随仓库或发行包保证提供。

文档维护时以代码和工作流核对行为；命令集中使用现有示例，验收结果记录日期与平台。不要用“CI 已配置”代替“CI 已实际通过”，也不要用测试数量代替当前验证结果。
