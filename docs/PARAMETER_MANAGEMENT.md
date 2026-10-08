# 独立参数与受管 NC 生成

同步粗车和越程槽的批量输入已接入[专用插件页面](SPECIALIZED_TURNING_PLUGINS.md)，单条模板 API 保留兼容路径；轨迹预设由业务插件管理。

通用 `parameters` 插件管理共享定义、预置、解析和来源。可选 `nc-parameters` 插件管理刀具、材料、加工预置、标准表和原生工艺计算；`process-parameters` 仅通过版本化 JSON 服务组合工序。默认构建包含通用参数功能，NC 构建需要 `nc-bundle`，运行时选择 `--profile nc`。原有 `template.render`、`nc.generate`、`process.generate` 保留。

## 资产与输入

定义与共享预置分别保存到 `parameters/definitions`、`parameters/presets`；NC 资产位于 `parameters/nc/{tool_types,tools,materials,profiles,standards}`。这些集合使用 JsonStore/WriteKernel 的基线指纹、原子写入、集合锁及导入回滚。资产 `format_version` 为 1，`id` 与文件名一致。

定义示例：

```json
{"format_version":1,"id":"part-depth","title":"加工深度","schema":{"type":"number","exclusiveMinimum":0,"maximum":30},"unit":"mm","group":"零件","description":"本次加工的目标深度"}
```

可选 `default` 只用于初始化；草稿、运行输入、资产和生成结果分别保存。共享预置为 `{"format_version":1,"id":"rough","values":{"part-depth":5}}`。模板通过 `metadata.parameters.bindings` 绑定 JSON Pointer，例如 `{"/part/depth":{"id":"part-depth","unit":"mm"}}`。

优先级为定义默认 → NC specs 默认 → 模板默认 → 显式选择的共享预置 → 已选加工推荐 → 人工输入。刀具事实与工艺派生字段由后端确定；请求不能替换模板已有绑定、工艺声明或 specs 来放宽约束。共享定义、模板 Schema、请求 Schema 和领域约束分别校验；绑定单位、Schema 的 `x-unit` 和 NC specs 单位必须兼容。

通用参数允许嵌套 JSON、列表和 null。原始工艺表单可以有结构化对象，处理器产生的 NC 模板上下文只允许现有 ParameterSet 支持的数值、精确整数、字符串、布尔和列表；拒绝非有限数及超出安全范围的整数。

## 服务与动作

以下新服务均采用各自服务名的 v1 契约，动作可经 CLI `run`、HTTP `POST /api/v2/actions/{动作}` 或工作台任务执行。HTTP 请求外层为 `{"input":请求}`，后台任务使用 `POST /api/v2/runs`，字段为 `action` 和 `input`。

| 服务 | 请求要点 | 结果 |
| --- | --- | --- |
| `parameter.resolve` | bindings、values、defaults、schema、preset_id；或 snapshot | values、origins、definitions、snapshot |
| `nc.parameters.resolve` | selection、可选 snapshot 和 max_spindle_rpm | slots、needs_confirmation、snapshot |
| `nc.operation.prepare` | operation `{id,version:1}`、inputs、可选 standard | inputs、contexts、warnings、派生字段、algorithm_version、输出计划 |
| `template.render.managed` | 普通模板输入，加 bindings、preset_id | 渲染结果、参数解析、冻结 replay_input |
| `nc.generate.managed` | template/source、operation_inputs、parameter_selection、机床和选项；或唯一 managed_snapshot | 完整产物、prepared、resolved、冻结 replay_input |
| `process.generate.managed` | 有序 ops、可选 outputs；或唯一 process_snapshot | 全部产物、编号终点、冻结 replay_input |

资产动作前缀分别为 `parameter`、`nc.parameter`：`catalog/read/save/remove`、`collection.export/import`。保存请求包含 `group/name/asset/expected`；读取返回 fingerprint，修改时将该值作为 expected。新建使用不存在基线；保存冲突保留草稿。`parameter.snapshot`、`parameter.dependencies.check`、`nc.managed.freeze/check` 和 `process.managed.check` 提供冻结与依赖检查。动作的完整输入 Schema 以 `nctool actions` 为准。

删除资产必须传入读取所得的字符串 `expected` 指纹；null 或其他类型会被拒绝，过期指纹返回冲突并保留资产。保存的新建基线仍允许 null。受管服务同时校验冻结快照、已解析工序和 NC 上下文；直接调用服务也不能绕过类型、整数精度或领域校验。历史重执行验证冻结内容，不重新读取当前资产来替换原快照。

通用示例：

```json
{"source":"深度 {{ part.depth }} mm","context":{"part":{"depth":6}},"bindings":{"/part/depth":{"id":"part-depth","unit":"mm"}},"schema":{"type":"object","required":["part"]}}
```

## 刀具和加工预置

每个作用域选择一个共享预置；每个刀具槽位选择一个加工预置或明确的手动模式。`nc.parameters.match` 按 tool_id、material_id、operation 筛选，具体刀具预置优先于刀具类型预置，后端不会自动选用候选。

```json
{"slots":{"rough":{"tool_id":"mill-12","material_id":"steel","operation":"milling","profile_id":"rough-steel","overrides":{"rpm":2500}}}}
```

刀具资产的 `attributes` 是事实；模板的 `metadata.parameters.tool_bindings` 指定槽位、`kind: attribute|value` 和 field，将事实/加工值注入对应原始路径。选择刀具后，WebUI 禁用相应事实输入，直接 API 传入冲突值会报错。缺少齿数、补偿号等必需属性时要求补全资产，不推测。

固定加工预置使用 `mode: fixed`、values.rpm/feed。换算预置使用 values.cutting_speed 与 milling 的 feed_per_tooth，或 drilling/turning 的 feed_per_revolution；刀具直径用 mm、切削速度用 m/min。`feed_mode` 明确为 `per_minute` 或 `per_revolution`。车削使用 inputs.work_diameter，而非刀具直径。转速 `1000×Vc/(πD)` 取最近整数、半值取偶；每分钟进给保留三位小数。人工改转速后，未覆盖的每分钟进给随最终转速重算；超出机床转速限值拒绝生成。

手动模式为 `manual:true`，在 inputs 中提供固定 rpm/feed 或换算量，并明确 feed_mode。键槽处理器只接受铣削、每分钟进给预置，不能把每转进给注入原模板的 F 值。人工 overrides 只支持 rpm/feed。预览返回 confirmed_for；存在调整时将其回填选择才能生成。条件或调整改变后重新确认，原调整保留；“恢复推荐”清除 overrides。确认指纹包含刀具、材料、预置、换算输入和调整值。

## 原生工艺与完整生成

迁移业务模板声明 `metadata.nc.operation` 的 id、version、input_bindings（原始路径）。v1 处理器使用固定原始业务 Schema，迁移绑定保持这些路径。工作台直接显示该 Schema，不要求填写中间值。

| 处理器 | 原始输入 | 后端准备 |
| --- | --- | --- |
| keyway.rough / finish / chamfer | geometry、rough_tool、finish_tool、process、chamfer、enable_chamfer | 转速、进给、圆角降速、层深/累计深度、路径和备刀 |
| sync.left / right | 起止直径/位置、两侧切深、进给、同步起号 | 刀次、实际起点、双通道、下一同步号及无需切削分支 |
| groove | ES/FS、标准号、侧别、尺寸、安全距离 | 标准查询、角度、切割/补偿坐标、圆角和几何校验 |
| circlip | 直径、宽度、切深、进给和 z_positions | 受限算术、三位精度、位置数组和刀宽校验 |
| direct | 普通 NC 参数 | 沿用 NC 校验；组合主程序的程序字段只能绑定本次结果 |

流程为原始输入 → 参数/刀具解析 → 工艺校验/计算 → 完整上下文校验 → 冻结模板与机床渲染 → 后处理 → 一次性交付。`prepared` 记录算法版本、输入和派生结果。键槽层深精度 0.001 mm、最多 10000 层；同步车接近整数刀次容差 1e-8。卡簧表达式只接受数字、括号、正负号及 +、-、*、/、**，单式最多 256 字符、64 个 AST 节点，幂指数绝对值不超过 8，中间结果有限且绝对值不超过 1e9。

## 组合、文件和历史

```json
{"ops":[{"id":"turn","template":"v3-sync-turning-right-template","operation_inputs":{"sta_x":60,"sta_z":0,"end_x":50,"end_z":-10,"ap_1":2,"ap_2":3,"sync_feed":0.2,"current_sync_num":10}}],"outputs":[{"name":"channel1.SPF","segments":[{"op":"turn","channel":1}]},{"name":"channel2.SPF","segments":[{"op":"turn","channel":2}]}]}
```

后续工序 `sync_from:"turn"` 使用本次下一同步号。主程序 `result_bindings` 的 JSON Pointer 指向 `{op,channel}`；仅可引用已完成的前置工序，不从变量库或旧生成文件读取文本。一个文件中的工序必须使用相同机床和后处理选项，行号在拼接后统一生成。任一校验、计算或渲染失败均无产物。

CLI 单文件使用 `--out`；多文件使用 `--out-dir 新目录`，所有文件先写到同级临时目录，全部写完才重命名交付。目标存在时拒绝覆盖。WebUI 可逐文件导出完整成功结果。

运行记录冻结定义、预置、刀具、标准、原始输入、确认状态、派生上下文、算法版本、模板与机床。历史重执行先核对运行环境，使用原 replay_input 创建新记录，不修改旧记录。工作台输入、确认状态或资源变化会使旧结果失效；资产依赖定期检查失败也禁止导出。历史产物仍可查看。

通用受管渲染的 replay_input 包含 snapshot、parameter_snapshot 和 parameter_request，保存原始解析请求以保留预置/默认来源；这一冻结形式不能混用 context、schema、defaults、bindings 或 preset_id。生成源码草稿时，依赖检查核对原磁盘基线与引用，不把本次未保存源码误判为资产变化。

V3 工作区资产包、逐模板映射和实际验收入口见 [迁移包说明](../examples/v3-managed/README.md)。技术对照只证明软件行为，不等于机床工艺审核。
