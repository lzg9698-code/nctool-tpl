# WebUI 直接重写

2026-10-23；CLI/WebUI 清单版本为 2.3.0（未发布），核心组件清单版本为 2.0.0。入口为 `ui/src/main.tsx → App.tsx`，全部页面连接到该生产入口。

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

统一门禁为 `node scripts/browser_acceptance.cjs`，本地与 CI 均运行上述三组当前页面验收。原 `browser_v2.cjs`、`browser_review_fixes.cjs`、`browser_generic_parameters.cjs` 仅保留为对应新版脚本的命令别名，不再执行旧界面定位。每组启动后比较所有嵌入资源并记录构建摘要；报告由 CI 上传。覆盖范围与执行结果见[项目状态](PROJECT_STATUS.md)，跨语言类型和诊断见[宿主与插件契约](CONTRACTS.md)。

## 按钮名称回归

本轮快速加载验收捕获到透明的默认 loading 图标在离场动画期间残留于可访问性树，使按钮名称变成 loading 加动作文字。全局按钮加载图标改为装饰用途（aria-hidden），保留加载期间的执行保护；插件执行按钮还显式标记 busy 和提交中禁用。单元测试覆盖快速加载切换，原插件脚本连续 5 次通过后再运行统一门禁，没有放宽原选择器或业务断言。

API 查询中 Context7 仅提供 Ant Design 6.5.0，已注明版本差异并用本机锁定 6.6.7 的 button/ConfigProvider 源码与类型声明核对 loadingIcon 全局配置；依赖未升级。
