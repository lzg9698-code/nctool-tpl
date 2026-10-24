---
by: Claude Code
date: 2026-10-17
---

按要求盘点收敛后的当前开发边界，新增 `docs/DEVELOPMENT_BOUNDARY_2026-10-17.md`（约 1,290 行），并在 `docs/README.md` 导航加入口。报告以源码为事实依据而非仅抄既有文档，覆盖：8 个 crate 的版本/依赖白名单、根引擎公开 API 与变量提取的兜底/必选判定、资产层指纹（FNV-1a + len + mtime）/锁/原子写/路径防护/JSON 数值边界、SDK 类型与 Python 协议、运行时组合排序与外部 JSON-RPC 握手、模板快照格式 1/2 与领域意图拒绝、共享参数解析优先级与 `template.render.managed` 冻结链路、CLI 命令面、完整 HTTP 路由与错误码映射、任务/历史/重放、WebUI 模块契约与构建指纹、`check_v2.py` 硬门禁、复刻顺序。

核对源码时发现并修正了三处易错事实：`Renderer::add_filter` 拒绝名单是 49 个内置过滤器（不是 48）；math 插件注册 19 个过滤器（不是 20）；前端生产源码当前为 46 文件 / 11,760 行（旧报告记为 33 / 10,250）。另确认 `docs/SYSTEM_DESIGN.md` 的 HTTP 段落已更新为 Axum/Tokio，过时的 tiny_http 说法只存在于 `assets/src/lib.rs`、`assets/src/lock.rs` 的注释里，报告的偏差章节已按此改写。

决定：报告明确区分「仓库内可交付内容」与「gitignore 的本地运行数据」——根目录 `templates/`、`presets/`、`parameters/`（含 `parameters/nc`）不是当前功能或示例，可交付示例只有 `examples/` 下 4 个文件。报告对每条契约标注了来源文件，并把既有文档与源码的已知偏差单列，复刻时以源码为准。

验证：仅做文档改动，按项目规则只核对链接与锚点；报告内 28 个相对链接全部可解析，15 个目录锚点与标题 slug 全部匹配。未运行 Rust/前端/浏览器测试，未执行远端 CI、联网审计，未提交、推送或发布。
