---
by: Claude Code / mythos-magpie-8067699270[1m]
date: 2026-10-16
---

任务：NC 移除的 ui/ 清理（分支 feat/shared-parameter-inputs，基线 08ab296，快照 target/nc-removal-20261016/before）。

做了什么：删除加工专用前端实现 10 个文件（plugins/NcViews.tsx、SyncTurningView.tsx、TurningWorkbench.tsx、UndercutView.tsx；lib/turningModel.ts(+.test)、syncAp2.ts(+.test)；components/SpecializedTemplateEditor.tsx、TrajectoryPresetEditor.tsx）。清理引用：registry 只保留 parameter.library；ParameterViews 去掉 NC 参数/ManagedProcess，保留通用共享参数库与预置选择；managedParameters 删除 validatedSelection/managedRawInput/managedFormSchema（保留通用 shared helper）；managedDependencies 去掉 nc/process snapshot 路由；documentExecution 恢复加工意图拒绝（domainTemplate/domainAction → 「加工能力已移除」，不再放行 NC 动作）；App/PluginSettings/SyntaxGuide(syntaxGuide.ts)/HistoryPage 去掉 NC 入口与选项，旧加工历史经 historyExecutionReason 变为只读。

决定了什么：专用模板识别 specializedTemplate 保留在 templateCatalog（最小判定，用于拒绝/隐藏）；generalDocuments 维持隐藏；不在 ui/ 增删依赖。旧加工历史只读边界=HistoryPage+restoreDocument+App.restoreRun 三处拒绝。

验证：npm run --prefix ui lint 通过；npm run --prefix ui test 通过（24 文件 / 147 测试）。日志 target/nc-removal-20261016/ui-clean.log。未生成 UI 资源、未提交、未改 ui/dist 与 cli/ui。

遗留：components/ChoiceTabs.tsx 因 TurningWorkbench 删除已无消费者（通用组件，未删，未列入清单）；agent-specific `action?: string` 字段在 syntaxGuide 中现无示例使用但保留；主线需自行重跑资源生成与浏览器验收。
