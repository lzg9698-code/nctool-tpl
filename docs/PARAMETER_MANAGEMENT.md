# 独立参数与共享参数渲染

当前 2.2 工作台的通用参数能力。参数定义、共享预置、绑定、来源与冻结解析由内置 `parameters` 插件提供；通用受管渲染 `template.render.managed` 在此之上渲染文本。加工专用的 `nc-parameters`、刀具/材料/标准、原生工艺计算与相关服务已随 NC 一并移除。

## 资产与输入

定义与共享预置分别保存到 `parameters/definitions`、`parameters/presets`。这些集合使用 JsonStore/WriteKernel 的基线指纹、原子写入、集合锁及导入回滚；资产 `format_version` 为 1，`id` 与文件名一致。

定义示例：

```json
{"format_version":1,"id":"part-depth","title":"加工深度","schema":{"type":"number","exclusiveMinimum":0,"maximum":30},"unit":"mm","group":"零件","description":"本次加工的目标深度"}
```

可选 `default` 只用于初始化；草稿、运行输入、资产和生成结果分别保存。共享预置为 `{"format_version":1,"id":"rough","values":{"part-depth":5}}`。模板通过 `metadata.parameters.bindings` 绑定 JSON Pointer，例如 `{"/part/depth":{"id":"part-depth","unit":"mm"}}`。

优先级为定义默认 → 模板默认 → 显式选择的共享预置 → 人工输入。共享定义、模板 Schema、请求 Schema 分别校验；绑定单位与 Schema 的 `x-unit` 必须兼容。显式 `preset_id: null` 表示取消预置，仅字段缺省时才回退到预置。

通用参数允许嵌套 JSON、列表和 null；受管渲染产生的模板上下文只允许数值、精确整数、字符串、布尔和列表，拒绝非有限数及超出安全范围的整数。

## 服务与动作

以下服务均采用各自服务名的 v1 契约，可经 CLI `run`、HTTP `POST /api/v2/actions/{动作}` 或工作台任务执行。HTTP 请求外层为 `{"input":请求}`，后台任务使用 `POST /api/v2/runs`，字段为 `action` 和 `input`。

| 服务 | 请求要点 | 结果 |
| --- | --- | --- |
| `parameter.resolve` | bindings、values、defaults、schema、preset_id；或 snapshot | values、origins、definitions、snapshot |
| `parameter.snapshot` | `{}` | 冻结的参数定义与预置集合 |
| `parameter.dependencies.check` | snapshot、可选 template_snapshot | 冻结集合与当前资产的差异 |
| `template.render.managed` | 普通模板输入，加 bindings、preset_id | 渲染结果、参数解析、冻结 replay_input |

资产动作前缀为 `parameter`：`parameter.catalog`、`parameter.read`、`parameter.save`、`parameter.remove` 与 `parameter.collection.export/import`，通过 `group` 区分 `definitions`/`presets`。保存请求包含 `group/name/asset/expected`；读取返回 fingerprint，修改时将该值作为 expected；新建使用不存在基线；保存冲突保留草稿。动作的完整输入 Schema 以 `nctool actions` 为准。

删除资产必须传入读取所得的字符串 `expected` 指纹；`null` 或其他类型会被拒绝，过期指纹返回冲突并保留资产。保存的新建基线仍允许 `null`。受管服务同时校验冻结快照与已解析参数，直接调用也不能绕过类型或整数精度校验。历史重执行验证冻结内容，不重新读取当前资产来替换原快照。

通用示例：

```json
{"source":"深度 {{ part.depth }} mm","context":{"part":{"depth":6}},"bindings":{"/part/depth":{"id":"part-depth","unit":"mm"}},"schema":{"type":"object","required":["part"]}}
```

## 文件和历史

CLI 单文件使用 `--out`；多文件使用 `--out-dir 新目录`，所有文件先写到同级临时目录，全部写完才重命名交付。目标存在时拒绝覆盖。WebUI 可逐文件导出完整成功结果。

运行记录冻结定义、预置、原始输入、派生上下文与模板快照。历史重执行先核对运行环境，使用原 replay_input 创建新记录，不修改旧记录。工作台输入或资源变化会使旧结果失效；资产依赖定期检查失败也禁止导出；历史产物仍可查看。

通用受管渲染的 replay_input 包含 snapshot、parameter_snapshot 和 parameter_request，保存原始解析请求以保留预置/默认来源；这一冻结形式不能混用 context、schema、defaults、bindings 或 preset_id。生成源码草稿时，依赖检查核对原磁盘基线与引用，不把本次未保存源码误判为资产变化。

数学扩展的启用与单次 `extensions` 选择记录在输入签名与重放中；通用渲染不涉及机床、G 代码或工艺校验。
