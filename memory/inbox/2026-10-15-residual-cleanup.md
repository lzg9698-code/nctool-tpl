---
by: Claude Code
date: 2026-10-15
---

按用户要求处理残余清理方案 [RESIDUAL_CLEANUP_PLAN](../../docs/RESIDUAL_CLEANUP_PLAN_2026-10-14.md) 全部 P0–P4。

**P0 已完成**：先给 `.gitignore` 加 `/prototypes/`、`/docs/nextgen-programming/`；随后按用户决定将研究材料迁出仓库，两条临时忽略项已撤回。

**P2 迁移已完成**（用户选定并入 `shaft_programmer/`）：
- `prototypes/`（33 文件）→ `shaft_programmer/private/prototypes/`
- `nc/`（23MB 真实样例）→ `shaft_programmer/private/nc/`
- `docs/nextgen-programming/drawing-evidence.json` → `shaft_programmer/private/nextgen-programming/`
- 4 个测试 → `shaft_programmer/tests/`，路径改 `private/...`、`parents[1]`、`check(..., ROOT/'private')`

关键发现：必须放进产品约定的 `private/` 目录。`common.engine_hash()` 会哈希产品根下**所有** `.py`，而 `app/package.allowed()` 只打包 `app/core/machines/web`；研究 `.py` 直接放根会让两者不一致，`test_package`/`test_installation` 报「打包期间实现变化」。实测定位后放入 `private/`（该路径已被两处排除）即恢复。`shaft_programmer` 327 项全通过。

**P1/P3/P4 已完成**：删 `styles.css` 失效 `.eyebrow`/`.modal-backdrop` 与失效 `.welcome > p:not(.eyebrow)`；`CHANGELOG` 补 UI 重构条目、`docs/README.md:3` 与 `PROJECT_STATUS` 待办更新；13 处 `../shaft_programmer/` 链接实为错一层，改 `../../`（根 README 改 `../`），现行文档 0 失效链接；旧 `scripts/browser_ui.cjs` 实测失效（旧设置页选择器）且与归档副本字节相同，已删除，其对比度检查移植进 `browser_ui_refactor.cjs` 并扩到明暗双主题 + 非空断言。

**遗留（既有，非本轮引入）**：`scripts/tests/test_workbench_migration.py` 报 `No module named 'shaft_programmer'`，已用重构前快照证明早于本轮；该未跟踪迁移工具的产品归属待定。Rust 依赖审计仍因公告库下载中断未完成。

验证：rustjinja lint/56 测试/build_ui --check/check_v2 通过；三组浏览器回归通过；shaft_programmer 327 项通过。未提交、未推送。
