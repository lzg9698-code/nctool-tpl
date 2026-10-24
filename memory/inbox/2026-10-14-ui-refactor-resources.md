---
by: Claude
date: 2026-10-14
---

准备 WebUI「视觉设计 + 架构」重构资源，未改动产品源码与依赖。

新增 `.agents/skills/nctool-ui-refactor/`（SKILL.md + agents/openai.yaml），格式对齐既有三个技能。新增 [docs/UI_REFACTOR_RESOURCES.md](../../docs/UI_REFACTOR_RESOURCES.md)：体检基线、分批选型表、Node 引擎矩阵、不推荐项、六类硬约束、落地顺序与验收指向。同步 docs/CODEX_DEVELOPMENT.md（三个技能改为四个并加资源入口）、docs/README.md 导航、AGENTS.md 技能小节。

决定：TypeScript 维持 5.9.x，不跟进 7.0.2（Go 原生版，生态未稳）。jsdom 钉 28.0.0——latest 30.1.2 要求 Node `^22.22.2 || ^24.15.0`，超出本项目 22.12 基线与 CI 的 `node-version: 22`。排除 RJSF 等自带数值解析的 Schema 表单库，会破坏 lossless-json 的大整数精度。不引入前端路由与 react-query：单 CLI 托管静态页面，无多 URL 需求。

版本均于 2026-10-14 用 `npm view <pkg> version` 对 registry 实际查询，非记忆。

遗留：依赖升级（Vite 8 / Tailwind v4 等）需单独授权并重跑 `python scripts/audit_dependencies.py`，本轮未安装、未改 package.json 与包锁。实际重构分步实施未开始。核对过 `npm run --prefix ui test`（7 文件 37 项全过）、`node scripts/build_ui.mjs --check` 通过、`scripts/tests/test_check_v2.py` 5 项通过；未运行浏览器验收。
