# 同步粗车与越程槽专用插件

两个内置插件分别提供 V3 风格的业务表格、Rust 工艺计算、批量生成和轨迹预设。正式运行不需要 Python，也不读取 V3 安装目录。CLI/WebUI 为 2.2，插件为 2.0，下述服务契约均为 v1。

## 启动与输入

运行仓库的 `启动UI.bat`，或构建 `nctool-cli --features nc-bundle --locked` 后以 `--profile nc ui --port 0` 启动。已有服务需要重启才能加载新插件。默认 template profile 不提供这些页面。

侧栏选择“同步粗车”或“越程槽”。每侧有自己的输入表和结果，支持添加、复制、上移、删除行。所有窗口通过左右标签切换，输入和结果按侧别保留。1600px 及以上并列输入与程序，其余窗口上下排列；表格横向滚动限定在表格内。同步车 1280px 及以上并列 CH1/CH2，窄屏切换通道。侧别、结果视图与通道标签支持方向键和 Home/End。

同步粗车填写起止 X/Z、固定 ap1、每行 ap2、同步进给和起始同步号。X 与 ap1/ap2 使用直径量，Z 使用 mm。ap2 可留空；“计算 ap2”会自动回填各行有效计算值，计算预览或生成时也会补齐留空值并保留手填值。后端按两位采用值重算刀次。舍入导致零值、越界或刀次变化的候选不会自动填写；无有效值时保留草稿并停止生成。推荐返回前输入已变化时，不覆盖新草稿。无需切削行保留 V3 分支和编号行为。

越程槽填写毛坯直径、安全距离、ES/FS 类型、标准、D1/Z；ES 另填 D2、C 和角度。切换 FS 隐藏 ES 字段并保留草稿，切回可恢复；标准重新选择。后端查询来自 V3 的 34 项标准并计算几何。

“生成右侧/左侧”只处理所选侧，“生成全部”处理双方。任一行失败，整次任务不交付文件。同步粗车每侧输出 CH1、CH2 两个 SPF，双方共四个；越程槽每侧一个 SPF。结果支持代码/2D、复制、导出、诊断和生成时间。

## 轨迹预设

每侧在“轨迹预设与本次调整”中选择一个预设，点击“编辑所选预设”直接修改预设名称和轨迹参数，无需填写加工行或先计算。“保存预设”更新自定义预设；修改 ID 可以另存一份，内置 `v3-compatible` 必须填写新 ID 另存。编辑草稿按工作区、插件和侧别保留，刷新或切换页面后可继续；保存失败或指纹冲突保留草稿，“重新读取所选预设”替换编辑草稿，“放弃预设编辑”清除编辑草稿。保存通过 JsonStore/WriteKernel、基线指纹和原子写入。

编辑器保存的是预设，不直接改变本次加工的人工调整。新预设保存后在列表中明确选择，需要使用其原始参数时点击“恢复预设值”。保存已引用的预设会立即使双方相关预览、确认和生成结果失效，需重新计算；未引用它的一侧继续保留结果。下方临时调整仍可在计算后“将本次调整另存预设”或“保存本次调整到预设”。

| 插件 | 策略 / 版本 | 当前可调整参数 |
| --- | --- | --- |
| 同步粗车 | `v3-paired` / 1 | X 接近余量、X 退刀余量、左侧耦合进刀角、左右进刀/退刀进给 |
| 越程槽 | `v3-standard` / 1 | 接近附加余量、切削进给、FS 退出延长 |

左侧耦合角同时改变实际起始 Z、分层运动和 ANG。同步事件、配对通道、模式及刀补启停由后端控制，标准几何不允许通过覆盖值改写。

临时调整先计算、确认、再生成。相关尺寸、机床、标准或有效预设变化后需重新确认；切换预设保留人工调整，可恢复预设值。同步号调度、后处理选项不改变几何确认。界面提供与上次生成的轨迹片段差异。

资产含 `format_version/plugin/strategy/strategy_version/parameters`。后端产生带阶段、坐标模式、补偿和同步事件的类型化计划，程序与预览消费同一计划。未知策略拒绝执行，资产不执行脚本。未来可扩展策略和计划版本；当前没有任意图形节点编辑。

2D 使用程序精度下的名义坐标点。ANG、RND、刀补运动只是示意，没有刀补圆弧或碰撞仿真。软件对照通过不等于机床工艺审核。

## 会话、模板与组合

“会话、导入与导出”支持专用 JSON 会话和 V3 配置，先预览转换结果再应用；忽略旧输出和旧刀数。尺寸属于会话，不自动成为共享默认。草稿按工作区/插件保存并在刷新后恢复。资产位于 `operations/<plugin>/sessions` 与 `operations/<plugin>/trajectories`，集合为 `sync-turning-assets`、`undercut-assets`。

`sync.left/sync.right/groove` 模板不会显示在通用模板列表、计数、标签、搜索、引用及通用 NC/工序选择器中；请从侧栏进入对应插件，在当前侧“高级设置 · 模板格式”中选择并“编辑所选模板”。内置 V3 格式无需模板导入，可“新建自定义模板”，以 `{{ trajectory_program }}` 为起点编辑完整 Jinja/NC 源码。源码可增加条件、循环、注释或自定义 NC 指令，也可用派生工艺变量自行组织输出；保存后重新计算或生成实际消费此源码。

模板 ID 不变时保存原资产，修改 ID 另存副本；名称和源码可直接编辑。校验模板检查 Jinja 语法并列出引用变量，实际变量绑定与 NC 校验在生成时执行。保存携带读取时的指纹，冲突/错误保留源码草稿；草稿按工作区、插件、侧别保留，刷新可恢复。保存已引用的模板立即使相关预览、确认与生成结果失效，无关侧结果保留。内置格式保持原样，新模板保存后明确选择使用；两个插件的模板仍从通用模板库隐藏。

用于专用工作台的模板必须声明 `metadata.nc.trajectory_plan_version: 1` 并匹配工艺与侧别。旧模板可在编辑器中显式勾选“用于专用工作台（轨迹计划 v1）”，完成源码适配后保存；其 Schema、元数据、默认值和历史源记录原样保留。取消此标记后保留兼容 API 路径，专用批量生成拒绝它。自定义代码不改变后端几何计算或预设轨迹，2D 图只显示预设计划，不包含模板额外编写的运动代码；实际交付以生成代码为准。

[工作区资产包](../examples/v3-managed/workspace.json)包含升级的三份包装模板，原转换源保存在 `metadata.nc.legacy_source`，源哈希和改写记录见 [mapping.json](../examples/v3-managed/mapping.json)。没有自动覆盖用户旧资产。旧自由模板仍可走兼容单条 `nc.generate.managed` API，但不能在专用页面中套用轨迹调整；升级时使用工作区导入冲突流程。

专用模板源码可在插件高级设置、文件或现有 API 中维护；工作区资产包继续支持导入导出。通用单文件导入不接受专用模板。旧通用编辑草稿仅隐藏，不删除，也不自动覆盖插件草稿；历史记录与既有工序保持原内容，已冻结任务重执行继续使用原模板快照。

结果区区分计算预览与生成成功；预览不能复制或交付。最近任务状态与取消直接可见，详细记录在“生成记录与详细诊断”中展开查看。

“加入组合”复制本次有效原始输入及选择，创建明确通道输出；后续同侧同步批次通过 `sync_from` 推进编号。多工序使用共同冻结资源，失败不交付部分文件。主程序只引用本次输出。

输入或资源变化后，旧结果不能复制、导出或加入组合。单侧编辑影响本侧，全局条件影响双方。历史重执行先检查环境，再使用原快照，不改写旧记录。

## v1 接口和示例

| 服务/动作 | 契约 |
| --- | --- |
| `sync-turning.describe` / `undercut.describe` | 原始输入 Schema、轨迹参数 Schema、内置预设 |
| `sync-turning.recommend` | `sta_x/end_x/ap_1/ap2_min/ap2_max` → 原候选、采用值、刀次、可生成状态 |
| `undercut.standards` | 可选 `standard_id` → 标准表和指纹 |
| `<plugin>.prepare` | 原始批次或 `job_snapshot` → 上下文、计划、预览和确认值 |
| `<plugin>.freeze` | 原始输入 → 资产、模板、标准、机床等冻结资源 |
| `<plugin>.generate` | 原始批次或快照 → 全部产物、派生结果和重执行输入 |
| `<plugin>.check` | `job_snapshot` → 当前依赖是否变化 |
| `<plugin>.asset.catalog/read/save/remove` | `group: sessions/trajectories`，保存带 `expected` 基线 |
| `<plugin>.asset.collection.export/import` | 插件资产集合导入导出 |

实时输入和快照互斥，不能注入刀次/补偿坐标。将 `prepare.sides.<side>.confirmed_for` 回填该侧 `trajectory.confirmed_for` 确认覆盖。成功结果返回 `data.replay_input.job_snapshot`，传同一动作重执行。

`nc.operation.prepare` 按工艺分派专用插件，原 `nc.generate.managed/process.generate.managed` 模板请求保留。业务请求用 `{"business":{"plugin":"sync-turning","input":...}}`。业务批次不接受程序文本 `result_bindings`，通过 `outputs.segments` 引用本次通道。

- [同步右侧](../examples/specialized-turning/sync-right.json)
- [左右 ES/FS](../examples/specialized-turning/undercut-es-fs.json)
- [连续同步与双通道组合](../examples/specialized-turning/process.json)
- [自定义同步轨迹资产](../examples/specialized-turning/sync-trajectory.json)
- [可导入工作区包：两份会话和一份自定义轨迹](../examples/specialized-turning/workspace.json)，在“工作区资产”导入，随后明确读取会话或选择预设。

```powershell
target/turning-nc/debug/nctool.exe --home target/turning-example/home --workspace target/turning-example/workspace --profile nc run sync-turning.generate --input examples/specialized-turning/sync-right.json
target/turning-nc/debug/nctool.exe --home target/turning-example/home --workspace target/turning-example/workspace --profile nc run undercut.generate --input examples/specialized-turning/undercut-es-fs.json
target/turning-nc/debug/nctool.exe --home target/turning-example/home --workspace target/turning-example/workspace --profile nc run process.generate.managed --input examples/specialized-turning/process.json
```

HTTP：`POST /api/v2/actions/sync-turning.generate`，请求 `{"input":<示例>}`。后台任务：`POST /api/v2/runs`，请求 `{"action":"sync-turning.generate","input":...}`。所有入口共用 Rust 处理器。

## 原始输入—计算—变量映射

| V3 来源/原始量 | Rust 落点与结果 |
| --- | --- |
| `core/sync_turning.py`：直径、ap1/ap2 | `plugins/sync-turning/src/calculation.rs`：接近整数容差 `1e-8`，否则向上取整；实际起始 X。单行 ≤10000 刀、批次累计 ≤20000 刀 |
| 左侧起点、25° 耦合 | 同步插件 `plans`：实际起始 Z、Z 递推、ANG；预设角度统一参与 |
| 同步起号、行顺序、无需切削 | 配对计划、行游标、`next_sync_num` 和跳过分支；组合引用本次下一编号 |
| ap2 推荐 | `recommend`：N=1..4999、范围容差 `1e-9`；保留候选并重校验两位采用值 |
| `core/undercut_logic.py`、标准：类型/方向/尺寸 | `plugins/undercut/src/calculation.rs`：`X_START/Z_START/D1_CUT/ANG_1/R` 和左右变换 |
| ES：D2/C/角度 | `D2_COMP_CUT = D2 - 2*C*tan(angle) - 0.5`；关系校验与补偿路径 |
| FS：第二角/t2/r | `Z_MID/FS_Z_PLUS1/FS_Z_MINUS1/ANG_2`；策略处理退出延长 |
| 原模板运动、进给、精度 | `plugins/*/src/lib.rs` 构建计划 → `plugins/nc/src/trajectory.rs`；V3 字精度、模式和配对事件校验 |
| V3 表格/配置/预览 | `TurningWorkbench.tsx/turningModel.ts`：原始表单、推荐采用、会话转换及同源示意 |

前端不算刀次或补偿坐标，通用参数库不算 NC 几何。NC 基础服务不依赖专用插件，关闭其中一个不影响另一个。实际验证和限制见[本轮记录](archive/records/SPECIALIZED_TURNING_2026-10-08.md)。
