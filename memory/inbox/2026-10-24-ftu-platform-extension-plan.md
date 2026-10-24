---
by: Codex GPT-6
date: 2026-10-24
---

# FTU 驱动的通用插件底座扩展计划

- 用户允许根据 FTU 项目需要拓展底座基础能力，并明确要求做计划。本次只改计划文档，未实施或迁移。
- 新增 docs/FTU_PLUGIN_PLATFORM_PLAN.md，同步原迁移方案、导航和路线图。
- 建议完整界面路线：插件静态前端、隔离 iframe/消息桥、通用项目及草稿服务、任务/历史/产物复用、TypeScript browser/Node SDK。FTU 模型和算法留在外部插件。
- 建议宿主扩展 sidecar nctool.package.json 独立 format_version 1，保留现有 plugin.json 与进程协议 2 的严格形状；桥和项目分别版本化。全部为待实现提案。
- P0 基线 → P1 无领域知识的最小闭环与契约 → P2 界面及 SDK → P3 文档/草稿/任务闭环 → P4 FTU → P5 集成验收。无日期承诺、无发布授权。
- 当前 server 统一 DENY/frame-ancestors none，插件页面需专门响应策略；不全局放宽。P1 需实测沙箱 origin、CSP、脚本加载、消息来源、无损数字及下载方案。
- 原 FTU 保留，Git 状态读取此前受权限限制；实施前须确认。当前未运行产品或 FTU 测试，仅检查文档。
