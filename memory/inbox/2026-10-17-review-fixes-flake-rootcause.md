---
by: Claude Code
date: 2026-10-17
---

定位并修复 browser_review_fixes.cjs:205 偶发失败。根因不是草稿丢失，而是测试对原生 <details> 的盲目切换：切页返回后折叠区有时已展开，脚本再点 summary 把它收起，CodeMirror 内容不可见，innerText() 得到空串；同时 localStorage 草稿完好。

取证方法：把官方脚本复制成仪器化版本（放 target/，未留在 scripts/），在断言前记录 details.open、.cm-content 文本和全部 nctool:plugin:* 存储。6 次运行 2 次失败，两次均 open:true → 点击后 open:false、text:""，而存储含原始草稿；4 次成功均 open:false → true。另用独立探针 20 次复现同一状态迁移，且 20 次草稿均在。

修复：新增幂等 openDetails（仅在关闭时点击），三处调用替换；断言前用 waitForFunction 等待 CodeMirror 渲染出预期文本。原断言未放宽。修复后连续 6 次通过，主流程/共享参数/边界三组全过。

同轮另修参数界面水平内边距（面板内容、字段标题与清空动作对齐），以及结构化表单失效等缺陷。UI 28 文件 188 项测试、lint、资源一致性均通过。未提交推送发布，未跑远端 CI。
