# 机床配置指南（历史入口）

**当前版本已移除机床配置能力。** NCtool 不再包含机床资产、`machine.*` 动作、`nc.generate` 或 `--profile nc`；默认组合为 `template` + `parameters`，数学为唯一可选扩展。本文保留为历史入口，不再描述可执行操作。

## 替代入口

- 当前构建、启动与文档导航见[文档总览](README.md)、[系统设计](SYSTEM_DESIGN.md)与[贡献指南](CONTRIBUTING.md)。
- 领域能力通过开放的[插件协议](PLUGIN_PROTOCOL.md)自行实现，不再内置机床模型。
- 移除范围与兼容边界见[项目状态](PROJECT_STATUS.md)与[开发方案](DEVELOPMENT_PLAN_2026-10-16.md)。
- 旧版与验证记录见[历史归档](archive/README.md)；旧机床指南原文见[legacy/MACHINE_CONFIG_GUIDE.md](archive/legacy/MACHINE_CONFIG_GUIDE.md)。

历史文中提到的机床 JSON 资产、动作（`machine.list/read/save/remove`）、`nc.generate` 的机床选择与后处理选项均已退出当前实现。旧加工模板与冻结引用会被拒绝，不会降级为普通渲染。
