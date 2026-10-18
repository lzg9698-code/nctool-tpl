---
name: nctool-ui-refactor
description: 重构 NCtool WebUI 的视觉设计与前端架构，核对设计系统、状态管理、可访问性与测试资源；在用户要求改版界面、拆分 App.tsx、评估前端工具链或调整 WebUI 依赖时使用。
---

# NCtool WebUI 重构

先读仓库 [AGENTS.md](../../../AGENTS.md) 与 [WebUI 重构资源](../../../docs/UI_REFACTOR_RESOURCES.md)，再以当前源码、package.json 和锁文件确认已采用栈。资源中的候选版本不是强制升级清单。用户已授权的重构包含实现所需依赖调整，不因 Skill 再次请求许可；超范围批量升级、提交、推送或发布需有对应任务授权。

## 开始前

- 记录 `git status --short` 与涉及文件 diff，区分用户既有修改与本轮改动，保留未提交内容。
- 大范围迁移使用 `scripts/source_baseline.py` 在 Git 忽略目录建立恢复快照；记录 `cli/ui/source.sha256` 原值，不为消除脏状态重建它。纯文档同步不触发前端重建。
- 需要安装依赖时用 `npm ci --prefix ui` 核对锁文件；调整依赖时只处理任务所需项，说明预期锁文件变化并执行审计。
- 先评估是否需要工具升级。本次已采用 Vite 7.3.6、TypeScript 5.9.3、Vitest 5.0.3，均维持；不要把 Vite 8 或 Vitest 补丁升级作为开始重构的前置条件。

## 兼容边界

按[资源文件第四节](../../../docs/UI_REFACTOR_RESOURCES.md#四重构期间的兼容边界)核对，重点是：

- 数值精度：沿用 `lossless-json`，大整数保留原始文本；非有限数、下溢、整数越界报错，不静默纠正。评估 Schema 表单库时验证默认 numeric 路径和 custom adapter/widgets，不能先损失精度再交给后端。
- 资产写入：保留 `expected` 指纹、冲突保留草稿及加载磁盘/另存副本语义，不自动覆盖。
- 交付门禁：stale 禁止当前复制/导出；失败、取消、中断不交付当前产物，多文件整体交付，资格由统一模型与执行签名派生。
- 退役兼容：旧 `metadata.nc`（含空/null）、退役执行动作及混合加工包维持拒绝行为；旧加工历史只可查看原 artifact，不恢复为可执行文档或重跑。
- 生成资源：源码在 `ui/src`，通过 `node scripts/build_ui.mjs` 生成 `ui/dist` 与 `cli/ui`，不手改生成文件。2026-10-14 当时保留 App.tsx 入口、api.ts 无损编码和 `check_v2.py` 的断言；其后 `check_v2.py` 已随 NC 移除改为检查退役源码路径、`math` 唯一可选 feature 与依赖白名单（见[资源文件第四节](../../../docs/UI_REFACTOR_RESOURCES.md#四重构期间的兼容边界)）。未来经授权改变文件组织时可以同步改为等价检查，不能把当时或当前的字符串断言误写成永久架构禁令。
- 请求限制：后端 `body_limit` 为 1 MiB，新增输入或上传交互沿用相应入口限制。

## 分步实施

先确认当前阶段，不重新执行已经完成的迁移。2026-10-14 本轮源码已落实以下步骤，Windows 两组通用浏览器回归与新增主题/布局专项验收已通过；后续任务按实际差异选择必要步骤。

1. 评估工具链兼容性，接入测试基建。本次新增 jsdom 28.0.0、Testing Library React 16.3.3/dom 10.4.2/user-event 14.6.8，保留原静态测试。Node engines 分别核对，不能套用 Vite 的要求描述 Vitest。jsdom 28 保留 Node 22.12 基线；CI `node-version: 22` 解析最新小版本不等于 jsdom 30 必然在 CI 失败。
2. 接入 Tailwind 4.3.3 与 Vite 插件，把已有 CSS 变量整理为 `styles/tokens.css`，抽出 `styles/editor.css`；布局和组件样式仍主要保留在 styles.css，未全量转换为 Tailwind，也未引入 preflight。
3. 添加页头 light/dark 切换，浏览器全局 `nctool:theme` 保存偏好；CodeMirror 通过 CSS 变量原位换色，主题切换不重建 EditorView。导航使用蓝色令牌，两种主题保留深蓝侧栏。专项浏览器已验证编辑器 DOM 保持和草稿/主题刷新恢复，未直接断言选区与撤销历史。桌面导航不设独立高度限制，滚动条沿用主题/导航变量。
4. 使用 Radix Dialog 1.3.1 和 DropdownMenu 2.2.1 替换 Modal/Menu；菜单使用对应 menu 语义，Popover 用于弹出内容。接入 Sonner 2.0.8，同时补键盘、Esc 和焦点恢复测试。
5. 拆 App 外壳为 views/controller、`useDocumentOperations`、`useParameterPresets`、session/scoped store/model；使用 Zustand 5.0.15 管理工作区会话草稿，保持模型、精度和执行签名边界。
6. 清理实际冗余样式并执行静态检查。本次 `npm run --prefix ui lint` 为 `tsc --noEmit --noUnusedLocals --noUnusedParameters`，不是 ESLint。

保留现有 `renderToStaticMarkup` 的有效结构和边界断言，在容易回归的交互处新增 Testing Library 测试，不要求全量转换。`toEqual` 可以比较 LosslessNumber 对象；应另外检查原始字面量、序列化与错误边界，避免把无损对象误当普通 number。后续路由、请求缓存、表单适配器与编辑器选型按产品需求评估，不从本次取舍推导全局禁用。

## 验证

使用 `$nctool-verify` 的方法，按实际变更执行 `npm run --prefix ui lint`、`npm run --prefix ui test`、`node scripts/build_ui.mjs`、`node scripts/build_ui.mjs --check`、`python scripts/check_v2.py`。需要比较包体积时采用当前构建结果，不把重构前基线当新结果。构建会更新生成资源，检查一致性前先完成正式生成并核对差异。

依赖变更执行 `python scripts/audit_dependencies.py`，保留原始报告，不执行 `npm audit fix`。网络失败记为未完成；2026-10-14 本轮 npm 审计 0 漏洞，Rust 公告数据库下载两次中断，整体审计未完成。重试报告在 `target/ui-refactor-20261014/dependency-audit-retry/`。

浏览器在隔离 home/workspace 与空闲 loopback 端口运行，按 [UI 验收清单](../../../docs/UI_ACCEPTANCE_CHECKLIST.md)检查布局、对比度、菜单键盘、对话框焦点，以及精度、冲突、草稿与退役边界。三组 CI 通用回归为 `scripts/browser_v2.cjs`、`scripts/browser_review_fixes.cjs` 与 `scripts/browser_generic_parameters.cjs`，使用同一固定二进制和 Playwright/Chromium 环境。单元测试或构建成功不能替代真实浏览器验收。

`scripts/browser_ui_refactor.cjs` 已于 2026-10-18 删除：它是 2026-10 重构期的一次性专项脚本，依赖当时已被移除的主题切换入口，且不在 CI 与 `check_v2.py` 中。原始字节保存在 `docs/archive/legacy/rust-nc-removal-20261016.tar.gz`；2026-10-14 Windows 三份历史报告仍留在本地忽略目录 `target/ui-refactor-20261014/`，只作当时的证据，不代表当前构建可复现。

## 交付

说明实际改动的组件与依赖、测试/构建结果、当前包体积比较和未完成项。未执行或仍在运行的检查不写通过；本轮 Windows 浏览器通过按上述报告限定覆盖范围，Rust 审计继续标未完成。文档同步按 `$nctool-docs-release` 核对版本、命令和链接，只修改当前任务指定文档，不自动生成 records、调整导航或扩大产品代码范围。
