# 2026-10-14 WebUI 视觉与架构重构

本次在 Windows 11、Node 24.15.0、未提交工作树上实施。基准 HEAD 为 `e8fafd5e47a032d52f9f7a6adfbb384d454e0e15`；基准包含用户此前的 Rust 模板精简改动，不能用 HEAD diff 将其全部算作本轮修改。恢复快照位于本地忽略目录 `target/ui-refactor-20261014/baseline`，原 `cli/ui/source.sha256` 为 `902f34974e630cbfb6fda0e0fb9aad3637763ca335017db20ba442bd82594c23`。

## 实施结果

- 应用入口 `ui/src/App.tsx` 从 2361 行拆为 259 行外壳。`features/workbench` 分离侧栏、页头、模板页面、源码/参数/结果面板、模型、文档及预设操作、生命周期和 scoped Zustand store。业务操作保留 expected 指纹、草稿数字类型与退役边界。
- Tailwind 4.3.3 接入 Vite，保留现有组件 CSS，不引入 preflight。明暗语义颜色、编辑器和提示样式集中到 `styles/tokens.css`、`styles/editor.css`；深蓝导航、浅灰工作区、清晰面板和主题切换。本次并未将全部 2935 行布局/组件规则迁移为 utility classes。
- Radix Dialog 1.3.1、DropdownMenu 2.2.1 替换原对话框/菜单，Sonner 2.0.8 提供通知且显式保留 alert/status。Zustand 5.0.15 管理工作区/文档/任务；草稿仍经原有 tagged 序列化持久化，未使用普通 JSON persist 覆盖精度。
- 新增 Testing Library/jsdom 28 交互测试，保留原有静态与领域边界测试。`npm run --prefix ui lint` 使用 TypeScript 未使用声明检查，不是 ESLint。
- 修正 Ctrl+Enter callback 对最新输入错误状态的引用。修正固定高度导航造成的无意义滚动条；桌面导航自然占高，模板列表仍可滚动，滚动条配色跟随区域主题。
- Vite 7.3.6、TypeScript 5.9.3、Vitest 5.0.3、React 19.3.0 和 CodeMirror 等既有版本保持，未进行无关工具链升级。

## 实际验证

| 检查 | 结果 |
| --- | --- |
| `npm ci --prefix ui` | 通过 |
| `npm run --prefix ui lint` | 通过 |
| `npm run --prefix ui test` | 11 个文件、56 项通过 |
| `node scripts/build_ui.mjs` / `--check` | 正式构建及嵌入资源一致性通过，6 文件 |
| `python scripts/check_v2.py` | 当前架构、退役源码/产物和打包边界通过 |
| `cargo build -p nctool-cli --features math --locked --target-dir target/ui-refactor-20261014/host` | 独立验收二进制通过；预览占用时首次增量构建拒绝覆盖，停止本轮预览后重跑通过 |
| `scripts/browser_v2.cjs` | 模板、精度、冲突、插件、共享参数、显式 math、历史和集合包回归通过 |
| `scripts/browser_review_fixes.cjs` | 无效/错误类型 JSON、草稿恢复、退役标记、失败/过期交付及 HTTP 精度边界通过 |
| `scripts/browser_ui_refactor.cjs` | 主题偏好/草稿刷新恢复、Editor DOM 保持、深色编辑器、菜单到对话框焦点圈定/恢复、侧栏折叠和四种 viewport 布局通过，无 pageerror |

浏览器均使用本轮固定二进制、临时 home/workspace、随机 loopback 端口与已有独立 Playwright/Chromium；不使用用户运行资产。专项检查宽度/高度为 1280×720、1440×900、1920×1080、375×812，各页 document scrollWidth 不超过窗口宽度；桌面导航无额外滚动。没有直接断言选区位置或撤销历史。

报告目录（本地忽略，不保证随仓库分发）：

- `target/ui-refactor-20261014/browser-main-final/report.json`
- `target/ui-refactor-20261014/browser-boundaries-retry/report.json`
- `target/ui-refactor-20261014/browser-design/report.json`；同目录有 `workbench-light.png`、`workbench-dark.png`、`workbench-mobile.png`。

初次浏览器执行发现旧欢迎标题及旧菜单 button 定位不再匹配，已同步为新标题和 menuitem；Sonner 错误/成功通知缺少原 alert/status 语义，已修复并重跑两组通过。失败报告保留在对应前次目录。

## 体积与未完成项

主应用 JS 约 122 KB → 257 KB；React chunk 约 222 KB，CodeMirror 约 424 KB 基本不变；CSS 约 41 KB → 53 KB（未压缩）。增长来自无障碍组件、通知和新样式；没有将体积下降作为本轮结论。

执行 `python scripts/audit_dependencies.py --output target/ui-refactor-20261014/dependency-audit-retry`：npm 原始报告为 0 漏洞；RustSec 数据库两次下载中断，整体审计退出码 2，**未完成**。原始错误及 summary 保留在报告目录；未自动修复或隐藏告警。

本次未运行完整 Rust workspace tests/Clippy/MSRV、Linux/macOS CI 或发布门槛；现有源码未提交、推送或发布。新专项浏览器脚本尚未加入 CI。资源选择说明见 [UI 重构资源](../../UI_REFACTOR_RESOURCES.md)，用户操作见 [WebUI 指南](../../WEBUI_USER_GUIDE.md)。
