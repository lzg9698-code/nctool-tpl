# WebUI 直接重写

2026-10-23；CLI/WebUI 仍为 2.2，Rust 核心及公共 API 不变。入口为 `ui/src/main.tsx → App.tsx`，全部页面连接到该生产入口。

## 技术与职责

| 范围 | 实现 |
| --- | --- |
| 组件、主题、通知、对话框、菜单、表格、分栏 | Ant Design 6.6.7、官方图标 6.3.4 |
| 页面导航 | React Router 7.18.4；Hash 路由，无需修改 Rust 静态资源路由 |
| 服务端状态 | TanStack Query 5.104.2；按工作区划分键，传递 AbortSignal，查询可重试、写操作不自动重试 |
| 文档及编辑草稿 | Zustand 5.0.16、persist；编辑状态不放入 Query 缓存 |
| 编辑器与比较 | CodeMirror 6 按需加载；diff 提供文本比较 |
| 动态字段 | Ant Design Form/Form.List；业务适配负责 Schema 字段、JSON Pointer、来源及无损数值 |
| 数值编码 | lossless-json 4.3.1；数值原文和未完成字段独立保留，禁止静默转 Number |

React、Vite、TypeScript、Vitest 保持现有基线。Radix 与 lucide-react 已从依赖退出；没有第二套组件体系。这里没有复制到仓库维护的通用按钮、焦点锁、通知队列、请求缓存或拖拽分栏实现。

源码按 `app`、`features`、`state`、`components`、`lib` 分工。`App.tsx` 仅装配主题、工作区、Providers 和路由；各功能通过共享业务契约访问现有 API。文档保存推进指纹及已保存基线，但不覆盖请求期间发生的新编辑；生成绑定提交时签名，后续输入变化立即让产物不可交付。

## 数据与操作边界

- 新浏览器草稿键仅为 `nctool:rewrite:v1:<workspace>`。不读取、改写、删除旧 UI 的存储，不转换旧草稿或布局偏好。
- 启动只读取能力和资产；不进行资源搬运、资产批量改写、配置迁移或自动导入。
- 模板导入先进入新草稿；工作区导入、示例保存和插件安装分别需要使用者明确点击确认。
- 用户资产继续通过现有 API 读写，保存/删除携带指纹；冲突可比较、继续编辑、明确加载磁盘版本或另存副本。
- 参数预览与后台执行分开。共享字段只记录本次覆盖；恢复跟随删除覆盖；空预置明确发送 null。历史恢复保持冻结来源与参数，改变绑定或预置后解除对应冻结选择。
- 成功状态、输入签名及当前依赖共同决定产物交付；多文件使用同一门禁。查询取消不等于取消后台任务，后者调用既有取消接口。
- 自写通用能力前先核对已选库；仅业务语义和已验证缺口新增适配代码。替换一项实现时一并删除其生产引用与无引用依赖。

## 验证入口

```powershell
npm ci --prefix ui
npm run --prefix ui lint
npm run --prefix ui test
node scripts/build_ui.mjs
node scripts/build_ui.mjs --check
python scripts/check_v2.py
python scripts/audit_dependencies.py
```

真实浏览器新增专项：`scripts/browser_rewrite.cjs`、`scripts/browser_rewrite_parameters.cjs`、`scripts/browser_rewrite_plugins.cjs`。共用 `scripts/browser_rewrite_support.cjs`，每组创建独立临时 home/workspace 和本轮宿主；使用 `NCTOOL_BINARY`、`NCTOOL_PLAYWRIGHT`、`NCTOOL_CHROMIUM`、`PLAYWRIGHT_BROWSERS_PATH`；外部插件示例可用 `NCTOOL_PYTHON` 指定解释器。默认报告在 `target/ui-rewrite-20261023/`。

原 `browser_v2.cjs`、`browser_review_fixes.cjs`、`browser_generic_parameters.cjs` 完整保留，尚按旧界面定位，不能作为新版通过证据。CI 仍引用这三组脚本；本次没有绕过或删除 CI 门槛，测试入口切换尚待完成。新增专项通过不表示原 CI 或三平台已通过。

本轮 Windows 报告包含主流程、数值与 JSON、草稿、冲突、四种视口、参数管理、示例保存、集合拒绝、历史恢复、外部插件安装/配置/动作/取消/失败，以及共享参数/math。npm 审计通过，RustSec 数据库下载超时，整体依赖审计未完成。实际最终数量和体积以本轮报告为准，不沿用历史验证。
