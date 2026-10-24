---
by: Codex GPT-6
date: 2026-10-24
---

# FTU 插件迁移只读核对

- 用户明确想把同级 FTU 接入 NCtool 作为插件。
- 已实际读取 D:/Dev/Projects/nctool/FTU 的 package.json、README、src、测试清单和构建脚本。未修改原 FTU。
- FTU 是 TypeScript/Vite 原生 DOM 工具；core.ts、machining.ts、sinumerik.ts 可以复用为 Node 外部插件核心。main.ts 的全局 DOM 与 window 监听需要拆分生命周期。
- 当前外部插件不支持第三方前端资源；保留完整轮廓编辑、图形和播放需要通用插件界面机制扩展。已向用户询问完整界面路线或先核心插件路线，尚待选择。
- 新增 docs/FTU_PLUGIN_MIGRATION.md 和导航入口，提出待实现的 ftu.analyze / ftu.generate，未实施插件或宿主扩展。
- 重点边界：草稿 null/NaN 与执行输入分离、诊断 severity 到 level 映射、从 project 重新计算后导出、SVG 从 DOM 导出逻辑中提取、Node 长计算不能堵住 cancel。
- FTU 文件可读，但 git -C ../FTU status 受到 Permission denied 限制，未确认其 Git 脏状态。未运行 FTU 测试，历史测试记录未当作本次通过。
