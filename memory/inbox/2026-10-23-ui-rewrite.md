---
by: Codex
date: 2026-10-23
title: WebUI 直接重写及验证状态
---

完成：生产前端接入 Ant Design 6.6.7、TanStack Query 5.104.2、Zustand 5.0.16、React Router 7.18.4，保留 CodeMirror/lossless-json/diff 开源库；全部页面重新实现，旧生产源码和 Radix/lucide 依赖退出。新存储仅使用 nctool:rewrite:v1，不读取、转换或清理旧浏览器数据；没有操作用户运行资产，没有提交或发布。

决定：用户选择保留后端/API、完整组件库、桌面模板工作台；要求直接重写，不做旧 UI 适配或资源迁移。自写部分限制在 Schema 映射、无损数值、指纹冲突、签名/依赖交付门禁等业务适配。

验证：Windows 上类型检查、30 项单元/组件测试、构建、生成资源一致性、架构检查、default/math 独立宿主构建通过；新增 Chromium 专项 26 组通过。npm 审计 0 漏洞。证据在 target/ui-rewrite-20261023/，当前说明 docs/UI_REWRITE.md。

遗留：RustSec 数据库下载超时，Rust 审计未完成。自动审批拒绝整体替换两份原浏览器回归脚本，理由是可能损失测试覆盖或用户改动；已恢复并核实原三组脚本与 HEAD 完全一致，新增专项为独立文件。原 CI 仍指向旧 UI 定位脚本，入口适配/切换未完成，不能声称原 CI 或三平台通过。静态资源总量约 0.95 MB 增至 1.77 MB（未压缩），编辑器按需加载。

保留：用户既有 runtime/src/external.rs、runtime/tests/external.rs 改动；Vite 7.3.7 修改保留，前端依赖锁按本轮需求更新，原 cli/ui/source.sha256 值已记录在基线报告后按正常构建生成。
