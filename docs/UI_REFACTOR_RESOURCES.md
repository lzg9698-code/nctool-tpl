# WebUI 重构资源

本文件记录 WebUI「视觉设计 + 架构」重构的选型、实际采用情况与兼容边界。任务入口见 [nctool-ui-refactor skill](../.agents/skills/nctool-ui-refactor/SKILL.md)。

> **后续变更（2026-10-17）**：本文描述的 `ui/src/features/workbench/**` 拆分实现从未被生产入口 `ui/src/main.tsx → App.tsx` 引用，已整目录删除；仅服务该入口的 `zustand`、`sonner` 依赖一并移除。表内这两项现为历史选型记录，不代表当前安装内容。当前活路径为 App 与其 `ui/src/lib/`、`ui/src/components/` 模块。详见[优化记录](OPTIMIZATION_LOG_2026-10-17.md)。

版本与实施状态更新于 **2026-10-14**。已采用版本以 `ui/package.json` 和锁文件为依据；候选版本来自当天重构前的 registry 查询，仅供后续评估。仓库约束见 [AGENTS.md](../AGENTS.md)、[贡献指南](CONTRIBUTING.md) 与 `scripts/check_v2.py`。用户已授权的重构包含实现所需依赖调整，无须因本文件再次请求许可；超出任务范围的批量升级不在授权内。

## 一、重构前体检基线

以下数值是 **2026-10-14 重构前**的体检记录，不代表当前源码行数或当前产物大小。

| 基线项目 | 数值 |
| --- | --- |
| `ui/src` 源码 | 32 个文件、约 10085 行（含测试） |
| `App.tsx` 单文件 | 2361 行 |
| `styles.css` | 2985 行手写 CSS |
| `WorkbenchUi.tsx` | 462 行（Menu/Modal/JsonEditor/Empty/PageTitle） |
| `Form.tsx` / `SchemaEditor.tsx` | 419 / 307 行 |
| 生产产物 | `index-*.js` 122 KB、`react-*.js` 222 KB、`editor-*.js` 424 KB、CSS 41 KB |
| 原有依赖 | React 19.3、Vite 7.3.6、TS 5.9.3、Vitest 5.0.3、CodeMirror 6、lucide-react 1.49.0、lossless-json 4.3.1、diff 8.0.4 |

重构前的主要问题与本次处理如下：

1. `App.tsx` 集中约 30 个 `useState` 及保存、冲突、删除确认、预设、诊断跳转等逻辑。本次拆成组装入口、页面视图、controller、操作 hooks、会话存储和模型。
2. 既有 CSS **已有变量**，包括 `--brand`、`--radius` 等，但主题覆盖与语义令牌不够集中。本次抽出令牌与编辑器样式，并接入 Tailwind v4 的 `@theme`。
3. 原 Menu 使用 `<details>/<summary>`，Modal 的焦点、Esc 和恢复行为由项目维护。本次改用 Radix DropdownMenu/Dialog 基元，继续验证具体交互。
4. 原静态标记测试能检查结构和边界，缺少真实键盘、菜单与对话框交互覆盖。本次保留静态测试，补充 Testing Library + jsdom 测试。
5. 原来没有 lint script。本次增加 TypeScript 的未使用变量/参数检查；这不是 ESLint。

## 二、实际采用的技术栈

本次先评估工具链是否需要升级，再接入所需基建和组件。原来的「先升 Vite 8」只是候选方案；当前实现维持已有 Vite、TypeScript、Vitest。

| 用途 | 包 | 当前锁定版本 | 本次处理 |
| --- | --- | --- | --- |
| 构建 | `vite` / `@vitejs/plugin-react` | 7.3.6 / 5.2.0 | 维持 |
| 类型 | `typescript` | 5.9.3 | 维持 |
| 测试 | `vitest` | 5.0.3 | 维持，新增组件交互测试 |
| DOM 环境 | `jsdom` | 28.0.0 | 新增，兼容 Node 22.12 基线 |
| 测试 | `@testing-library/react` / `@testing-library/dom` | 16.3.3 / 10.4.2 | 新增 |
| 测试 | `@testing-library/user-event` | 14.6.8 | 新增，模拟键鼠操作 |
| 样式 | `tailwindcss` / `@tailwindcss/vite` | 4.3.3 / 4.3.3 | 新增，Vite 插件与 CSS-first `@theme` |
| 基元 | `@radix-ui/react-dialog` | 1.3.1 | 新增，替换 Modal 实现 |
| 基元 | `@radix-ui/react-dropdown-menu` | 2.2.1 | 新增，用于操作菜单 |
| 提示 | `sonner` | 2.0.8 | 新增，通知展示 |
| 状态 | `zustand` | 5.0.15 | 新增，保存工作区会话与文档草稿 |

### Node 引擎要求

下表来自当前锁文件，不能把 Vite 与 Vitest 的要求合并成一条。

| 包 | `engines.node` |
| --- | --- |
| Vite 7.3.6 | `^20.19.0 \|\| >=22.12.0` |
| Vitest 5.0.3 | `^22.12.0 \|\| ^24.0.0 \|\| >=26.0.0` |
| jsdom 28.0.0 | `^20.19.0 \|\| ^22.12.0 \|\| >=24.0.0` |

项目开发基线仍为 Node 22.12+。重构前查询的 jsdom 30.1.2 要求 `^22.22.2 || ^24.15.0`，仅 Node 22.12 不满足；CI 的 `node-version: 22` 会解析该主版本可用的最新小版本，不能据此断言 CI 必然安装失败。本次选择 jsdom 28.0.0，以保留现有开发基线；更换版本时重新核对 engines、peer dependencies 和实际安装/测试结果。

### 未采用的候选

以下是重构前的建议，不属于本次已安装栈，也不构成未来产品的全局禁用清单。

| 候选 | 本次取舍与后续评估条件 |
| --- | --- |
| Vite 8.3.4 / plugin-react 6.1.2 | 当前 Vite 7 满足本次接入需要，未升级；以后按插件、构建与 Node 兼容性评估 |
| Vitest 5.0.4 / lucide-react 1.57.0 | 本次维持 5.0.3 / 1.49.0；补丁或同系列版本也需检查变更与回归，不能预先断言安全 |
| TypeScript 7.0.2 | 本次维持 5.9.3；编译器实现、生态兼容性及迁移收益需核对官方资料，不用未核实的实现语言判断代替评估 |
| `tw-animate-css` 1.4.0、`clsx` 2.1.1、`tailwind-merge` 3.7.0 | 按实际动效和 class 组合需求选择，本次未引入 |
| Radix Tabs 1.2.1 / Select 2.4.1 / Tooltip 1.4.1 | 按具体控件的可访问性与交互收益评估；原 `title=` 检索包含 React props，不等于每一处都是 tooltip 候选 |
| Radix Popover 1.3.1 | 用于弹出内容，菜单应评估 DropdownMenu 的键盘与 menu 语义；本次菜单采用 DropdownMenu 2.2.1 |
| `cmdk` 1.1.1 | 需要命令面板或快速切换时再评估 |
| React Router 8.4.0 / TanStack Router 1.170.44 | 当前页面状态满足已有单页入口；出现多 URL、深链接或历史导航需求时再评估 |
| TanStack React Query 5.104.2 | 当前 API 封装与任务轮询保留；出现请求缓存、共享失效或并发需求时再评估 |
| RJSF 6.11.0 | 默认 numeric 处理需验证大整数精度；可以通过 custom adapter/widgets 接入无损数据，不能宣称绝对不可用。本次保留现有 Form |
| Monaco 0.57.0 | 本次保留 CodeMirror；后续按编辑能力、加载成本与实际产物评估 |
| React Flow / XYFlow 12.12.0、Svelte / Vue 重写 | 当前任务没有流程图或框架迁移需求 |

## 三、本次实现结构

- `ui/src/App.tsx` 保留为组装入口；`ui/src/features/workbench/` 中的 views、controller、`useDocumentOperations`、`useParameterPresets`、session、scoped store 与 model 分担页面、操作和状态。
- 工作区页头提供明亮/深色切换，浏览器全局 `nctool:theme` 保存偏好；首次无有效偏好时参考系统配色。根节点 CSS 变量原位更新 CodeMirror 颜色，主题切换不重建 `EditorView`，浏览器已验证编辑器 DOM 保持和草稿/主题刷新恢复；该专项脚本没有直接断言选区与撤销历史。
- `ui/src/styles/tokens.css` 集中语义令牌，`styles/editor.css` 提供编辑器样式；布局与组件 CSS 仍主要在 `styles.css`。入口引入 Tailwind theme/utilities，未引入 preflight，未全量转换为 Tailwind。蓝色 nav 令牌使两种主题均保留深蓝侧栏。
- `WorkbenchUi` 使用 Radix Dialog/DropdownMenu；Sonner 提供通知。Zustand store 由每个 WorkspaceProvider 独立创建，持有文档和会话状态；草稿持久化仍由 session/drafts 执行，交付资格仍由模型与执行签名派生。
- `npm run --prefix ui lint` 实际执行 `tsc --noEmit --noUnusedLocals --noUnusedParameters`。现有静态测试保留，新增组件测试覆盖菜单、对话框与工作区交互；Windows 两组通用浏览器回归与主题/布局专项已通过。
- 桌面导航移除独立 max-height 限制，不产生无须存在的滚动区域；全局滚动条使用主题变量，侧栏覆盖为导航配色。专项验收覆盖四种窗口尺寸及桌面导航不滚动。

## 四、重构期间的兼容边界

### 数值与 JSON 精度

- 保留 `lossless-json` 4.3.1 的 parse/stringify 与 `LosslessNumber`；大整数保留原始文本，避免经 `Number()` 往返。
- 保留 Form 的 `DraftNumber`/`numberText` 和 encode 校验；非有限数、下溢、整数越界须报错，不静默纠正。
- 测试按真实类型检查：`toEqual` 可以比较 LosslessNumber 对象，但不要把它误当普通 number；精度边界还需断言原始字面量与序列化结果。
- 引入 Schema 表单库时先验证默认 numeric 路径及自定义适配器，不能让 UI 层提前损失精度。

### 资产、执行与兼容

- 保存/删除经既有动作与 `expected` 指纹；冲突保留草稿，提供加载磁盘版本/另存副本，不能自动覆盖。
- 输入、受管依赖与执行签名变化使结果 stale，继续禁止当前复制/导出；失败、取消、中断不交付当前产物，多文件整体交付。
- 旧 `metadata.nc`（含空对象/null）与退役执行动作阻止 render 降级；旧加工历史只可查看原 artifact，不重跑或恢复为可执行文档。
- 混合加工集合包整体阻断；退役保留名不能被同名外部插件接管。

### 生成资源与检查脚本

源码在 `ui/src`；`node scripts/build_ui.mjs` 生成 `ui/dist`、`cli/ui` 和嵌入表，不手改生成文件。构建前记录可能已有修改的 `cli/ui/source.sha256`，再核对生成差异，不为清理脏状态重建它。

截至 2026-10-16，`scripts/check_v2.py` 检查以下实现特征（NC 移除后已扩充）：

- `ui/src/App.tsx` 存在；`ui/src/lib/api.ts` 含 `/api/v2/` 与 `encode(body)`。
- `cli/ui/index.html` 含 `/assets/` 与 `type="module"`；`cli/ui/assets.rs` 存在。
- 退役源码路径不再存在：`plugins/nc`、`plugins/process`、`plugins/sync-turning`、`plugins/undercut`、`ui/src/plugins/NcViews.tsx`、`SyncTurningView.tsx`、`TurningWorkbench.tsx`、`UndercutView.tsx`、`ui/src/lib/turningModel.ts`、`syncAp2.ts`、`ui/src/components/SpecializedTemplateEditor.tsx`、`TrajectoryPresetEditor.tsx`。
- 默认与 `math` 构建树均无 NC 包，`math` 是唯一可选 feature，生产依赖符合白名单；`src/lint.rs` 不存在，渲染器不注册 `nc_` 或数学过滤器。

2026-10-14 当时用的“产物 chunk 名排除”断言已随 NC 移除替换为退役源码路径检查；入口路径与源码字符串断言是当前检查方式，未来经授权调整架构时可以同步更新等价检查，并非永久禁止重命名或重组。退役能力与无损请求编码仍需被验证。`ui/tsconfig.json` 当前排除旧 `src/workbench`，避免恢复的旧工作台混入主构建；不要与新 `src/features/workbench` 混淆。

后端 `body_limit` 与 `input_limit` 为 1 MiB，bundle 上限 64 MiB。这是请求体/输入限制，不限制静态资源加载；新增上传交互需沿用相应入口限制。

## 五、实施阶段与验证状态

实施时按影响分步验证，下面区分本次已经落实的源码与仍待完成的验收，不用阶段表代替测试结果。

| 阶段 | 本次落实 | 验证要求 |
| --- | --- | --- |
| 基线 | 核对已有 diff 与源码指纹；大范围迁移使用忽略目录恢复快照 | 保留用户修改与生成差异 |
| 工具与测试 | 维持 Vite/TS/Vitest，新增 jsdom 28 和 Testing Library | 静态测试保留，新增交互测试 |
| 设计与组件 | Tailwind v4、主题令牌、Radix Dialog/DropdownMenu、Sonner | 键盘、Esc、焦点恢复、布局与对比度 |
| 状态与页面 | 拆 App 为 controller/views/hooks/model，使用 scoped Zustand store | 草稿恢复、保存冲突、精度与结果过期 |
| 静态与生成检查 | TypeScript lint；正式构建与嵌入资源一致性 | lint、test、build、`build_ui.mjs --check`、`check_v2.py` |
| 浏览器与审计 | Windows 两组通用回归及主题/布局专项通过；npm 审计 0 漏洞，Rust 审计未完成 | 本机通过不替代三平台 CI；审计网络失败保持未完成 |

运行方法见[贡献指南](CONTRIBUTING.md)和 [nctool-verify skill](../.agents/skills/nctool-verify/SKILL.md)。浏览器使用隔离 home/workspace 与空闲 loopback 端口；按 [UI 验收清单](UI_ACCEPTANCE_CHECKLIST.md)核对精度、冲突、退役边界及主题、菜单和对话框，不因更换组件库跳过。

依赖变更运行 `python scripts/audit_dependencies.py`，保留原始报告，不执行 `npm audit fix`。2026-10-14 本轮 npm 审计为 0 漏洞；Rust 公告数据库下载两次中断，因此整体审计尚未完成。重试报告位于 `target/ui-refactor-20261014/dependency-audit-retry/`（本地忽略目录，不保证随仓库分发）。后续测试、构建、包体积及 App 行数应在对应验证记录中按实际结果列明。

已核对的 Windows 浏览器报告位于 `target/ui-refactor-20261014/` 下的 `browser-main-final/report.json`、`browser-boundaries-retry/report.json` 和 `browser-design/report.json`，三份均为 passed。新增 `scripts/browser_ui_refactor.cjs` 覆盖主题与编辑器 DOM 保持、刷新后草稿/主题恢复、菜单到对话框的 Tab/Esc 焦点恢复、1280×720/1440×900/1920×1080/375×812 无页面横向溢出、桌面导航不滚动及侧栏折叠/展开；专项报告记录无 pageerror，保存明亮/深色/移动窗口截图。报告与截图均为本地忽略目录证据。

## 六、相关文档

- [AGENTS.md](../AGENTS.md) — 工作范围、修改与验证约束
- [贡献指南](CONTRIBUTING.md) — 工具链、构建、验证命令
- [WebUI 使用指南](WEBUI_USER_GUIDE.md) — 当前界面操作
- [WebUI 验收清单](UI_ACCEPTANCE_CHECKLIST.md) — 重构后走查项
- [项目状态](PROJECT_STATUS.md) — 当前能力与待补充验证
- [nctool-verify skill](../.agents/skills/nctool-verify/SKILL.md) — 验证与浏览器验收方法
