# 原生工程工作台与外部插件

本页描述本地新增实现，尚未发布。当前支持声明式工程工作台、通用项目和二维场景，FTU 为独立 Rust 示例；现有模板/参数/math 组合不自动安装 FTU。使用步骤见 [FTU 插件](../external/ftu/README.md)。

本轮实际验证、已知告警及本机证据范围见[Windows 本地交付记录](archive/records/ENGINEERING_FTU_IMPLEMENTATION_2026-10-24.md)。

## 插件清单

进程协议仍为 2，plugin.json 和 describe 形状不变。可选 `nctool.package.json` 为宿主侧格式 1：

```json
{
  "format_version": 1,
  "id": "my-plugin",
  "workbenches": [{
    "id": "editor", "title": "我的工作台", "kind": "project",
    "analyze_action": "my-plugin.analyze",
    "generate_action": "my-plugin.generate",
    "schema": {"type":"object", "properties":{}},
    "example": {}, "tables": [], "formats": ["json"]
  }]
}
```

清单 ID 必须与插件 ID 一致，所有动作须由本插件声明。可选 prepare_action 返回 `data.input` 冻结生成请求；可选 template_action 返回 `data.asset` 为模板副本编辑提供初始内容。每包最多 16 个工作台，工作台 ID/kind 不重复。Schema 和示例必须为对象项目，示例通过 Schema；记录表 field 绑定顶层数组，row 是新建记录初值。完整例子见 [无加工语义的折线插件](../examples/plugins/engineering-demo)和 FTU 的清单。

宿主在安装暂存校验和启动扫描时验证清单。坏清单不会悄悄回退表单；禁用后新启动不暴露工作台。外部进程仍为同用户权限代码，不是操作系统沙箱。首版不加载第三方前端 JavaScript 或 iframe，视图由宿主 React 组件组合。

## 通用项目与执行

`GET /api/v2/capabilities` 新增 workbenches，包含清单字段与 plugin 归属。前端路由为 `#/engineering/<plugin>/<view>`。项目 API 为 POST `/api/v2/engineering/<plugin>/<view>/<operation>`：

| operation | 输入 / 返回 |
| --- | --- |
| list | `{}` → `{items:[id...]}` |
| read | `{id}` → `{document,fingerprint}` |
| save | `{id,document,expected:null或旧指纹}` → `{fingerprint}` |
| remove | `{id,expected:旧指纹}` → `{removed}` |
| preview | `{request_id,input}` → ActionResult；只调用该工作台的 analyze_action |

文档格式为 `{format_version:1,raw:<项目JSON原文>,invalid:<字段到未完成数字文本的映射>,formats:[...],template:<工作区模板ID或空串>}`。存储 Schema 与执行 Schema 分开，项目未完成也能保存；raw 内容不作为可执行代码解析。

文档目录为 `<workspace>/.nctool/plugin-data/<plugin>/<kind>/`。目录逐段安全解析、文件经 JsonStore/WriteKernel，拒绝路径逃逸；未启用或未注册的工作台无数据入口。新建为 create-only，覆盖/删除要求旧指纹，冲突不覆盖草稿；卸载保留工作区项目。

集合以 `documents.<plugin>.<kind>` 参与已有资产包导入导出，支持纯预检查和带指纹回滚。缺插件时拒绝相关集合；不能无条件删除导入后被修改的数据。

预览去抖 300ms，取消旧请求并丢弃旧修订响应；宿主最多同时处理两个工程预览，避免独占正式任务容量。正式生成复用现有 runs；若声明 prepare_action，宿主先完成准备，再入队。失败、取消和输入/模板变化后当前产物不可交付；历史原产物保持独立出口。

## Scene2D 格式 1

场景通过分析结果 `data.scene` 提供：`{format_version:1,axes:{x,y,y_scale},items,path}`。每项包含 id/entity/layer/kind：line/arc 使用 start/end/length，圆弧增加 center/sweep；polyline 使用 points 数组，宿主计算播放长度；point/text 使用 position，text 另含安全文本 text。坐标为 `[x,y]`，数学逆时针 sweep 为正，单位由轴标签说明；宿主将数学 Y 映射为屏幕向上，保持原始坐标比例，y_scale 仅影响刻度显示。

path 为播放顺序的线/圆弧/折线元素 ID 数组。entity 对应记录 id，点击图形可定位表格；缺少记录 ID 的表格仍可编辑。场景检查版本、规模、有限数值类型、重复 ID、圆弧半径和路径引用；每场景最多 10000 元素、100000 个坐标点，文本最长 1024 字符，不接受原始 SVG/HTML 注入。

记录表每页 20 行，图形选中跨页记录时切换到对应页。撤销/重做为有界会话历史，刷新恢复当前草稿而不恢复 undo 栈。离开页面停止动画与预览，不无条件取消已经提交的正式任务。

## 构建与验证

前端只编辑 ui/src，使用 `node scripts/build_ui.mjs` 生成资源。独立插件不加入默认 workspace；`python scripts/build_ftu.py` 在本平台生成可安装包，不修改用户配置。

新增验证：Rust 清单拒绝/项目冲突与回滚测试、前端场景边界测试、FTU 解析几何与后处理篡改测试、`node scripts/browser_engineering.cjs`。现有统一浏览器门禁和协议故障验收继续执行；CI 为三平台配置独立插件构建/测试及 MSRV，Linux 追加工作台浏览器验收。CI 配置不代表本轮远端已通过。
