# 插件 SDK / 外部协议 v1

## 描述文件

一个外部插件目录含 `plugin.json`、入口程序及其依赖文件。完整示例见 `examples/plugins/python-report`。必需结构：

```json
{
  "descriptor": {
    "id":"my-plugin", "version":"2.0.0", "protocol_version":1,
    "requires":[{"id":"template.render","version":1}],
    "provides":[{"id":"my.service","version":1,"action":"my.action"}],
    "config_schema":{"type":"object"},
    "panels":[{"id":"my-panel","title":"My panel","actions":["my.action"],"view":"form"}]
  },
  "command":["python3","plugin.py"],
  "actions":[{
    "id":"my.action", "title":"My action",
    "input_schema":{"type":"object"},
    "output_schema":{"type":"object"},
    "example":{}
  }],
  "limits":{"timeout_ms":30000,"max_message_bytes":1048576,"max_concurrency":8}
}
```

版本使用 SemVer，协议与服务版本是正整数。ID 只允许 ASCII 字母、数字、点、下划线、连字符，最长 128 字符，以字母或数字开头。动作建议使用插件命名空间；稳定服务 ID 可以绑定不同提供方的动作。

Schema 使用 JSON Schema；只允许本地 `$ref`，不请求网络资源。注册时校验 Schema，调用前校验 input，返回时校验 `result.data`。配置也按 Schema 验证。

Limits：timeout 10–3,600,000 ms；消息 1 KiB–16 MiB；并发 1–64。包最多 2048 个文件、64 MiB；拒绝符号链接和特殊文件，复制到暂存目录校验后发布，不覆盖同名已安装包。

## 双向 JSON-RPC

每条 UTF-8 JSON 消息以换行结束，`stdout` 只承载协议；日志用 `stderr`。`command` 是 argv 数组，不经 shell；当前目录是已安装插件目录。

宿主请求均有字符串 ID，如 `host-1`。启动顺序：

1. `initialize`，params 为 `{protocol_version:1,plugin_id,config}`；返回 `{protocol_version:1,plugin_id}`。
2. `describe`；返回 `{descriptor,actions}`，必须与安装描述一致，包括缺省字段的规范值。
3. `invoke`，params 为 `{action,input}`；返回下面的 ActionResult。

```json
{
  "jsonrpc":"2.0", "id":"host-3",
  "result":{
    "data":{"text":"Hello"},
    "artifacts":[{"name":"hello.txt","media_type":"text/plain","text":"Hello"}],
    "diagnostics":[]
  }
}
```

插件可以在 invoke 期间请求已声明的宿主服务：

```json
{
  "jsonrpc":"2.0", "id":"plugin-1", "method":"host.call",
  "params":{"parent_id":"host-3","service":"template.render","version":1,"input":{"source":"Hello"}}
}
```

宿主异步返回相同插件请求 ID 的响应。`parent_id` 必须对应活动宿主请求；服务必须出现在 `descriptor.requires`，版本必须匹配。嵌套调用深度最多 32；重复动作链失败，避免循环等待。

宿主发 `cancel` 通知 `{id:<活动宿主请求ID>}` 和 `shutdown` 通知 `{}`。首版采取终止失效进程的有界取消方式；下一次调用重新握手，不自动重试已失败的动作。进程启动不使用全局 shell 或表达式过滤器回调。

标准错误：JSON-RPC error `{code:-32000,message,data:{code,message,diagnostics}}`。未知响应 ID、错误 JSON、协议版本不一致、能力描述不一致及超大消息均明确失败。

## 内置 Rust 插件

实现 `nctool_plugin_sdk::Plugin`：descriptor/actions/extensions/listeners/activate/invoke/shutdown。消费者通过 `Context::call(service,version,input)` 使用接口，不能导入其他插件的领域模型。

`RenderExtension` 对一个新的 Renderer 注册过滤器；`EventListener` 订阅 ActionStarted/ActionFinished。注册归属插件，停止时释放；启动失败必须在 shutdown 清理自己的外部资源。外部插件首版仅提供动作与服务，复杂视图用 Schema 表单；不执行第三方前端脚本。

Python 适配器位于 `sdk/python/nctool_plugin.py`，使用独立输入循环、Future 响应分发和动作线程池，能在等待宿主回调时继续接收消息。

## 模板快照契约

`template.inspect` 接受 `include_snapshot:true`，响应增加 `snapshot:{template,templates,asset}`；其中 templates 是模板名到源码的完整集合，asset 是主模板的元数据或 null。`template.render` 接受 `{snapshot,context,...渲染选项}`，此时不再读取工作区；snapshot 不能与 source/template/templates 混用。领域校验和渲染应共用这份不可变数据。SDK 提供 `TemplateSnapshot` 类型。

## 可选前端和集合贡献（SDK v1 扩展）

旧描述不需要新增字段。缺省的 asset_collections 和 ui_schema 不序列化，保留已有进程握手形状。

动作可提供 `ui_schema`，以同名 properties 补充标题、分组、单位等显示提示；它不能替代输入校验。Panel.view 可选内置视图 `nc.generate`、`nc.machines`、`nc.process`；其他值走通用表单。

描述可提供 `asset_collections:[{id,export_action,import_action}]`。集合 ID 在工作区导入导出时必须唯一，两个动作必须由本插件声明。导出动作输入 `{}` 并返回集合数据；导入动作接收 `{collection,validate_only:true}` 做纯预检查，返回 count/mapping；正式导入返回 receipt。后续集合失败时宿主调用 `{rollback:receipt}`，提供方必须只撤销本次导入，不能无条件覆盖或删除已有资产。

生成服务可返回 `data.replay_input`，内容必须适合本动作的 input_schema。宿主重执行通过该输入新建任务；缺省则使用历史原始输入。快照固定输入资产，但不会固定当前插件程序版本。

### 历史重执行环境（宿主 API 扩展）

新任务记录增加可选 `environment:{format_version:1,host_version,plugins,services}`。plugins 为已启用插件 ID 到 `{version,protocol_version}` 的映射；services 保存执行时版本化服务的提供方和动作绑定。记录不保存插件配置、可执行文件内容或操作系统环境。

`GET /api/v2/runs/{id}/replay-check` 只读比较历史与当前宿主环境，返回 `{status,differences}`。status 为 matching（已记录版本和绑定一致）、changed（宿主版本、插件启停/版本/协议或服务绑定变化）、unknown（旧记录缺少环境或记录格式版本不可比较）。比较包含全部已启用插件，不承诺每个差异都影响所选动作。matching 不保证程序、配置和产物完全一致，同版本的程序修改仍需独立追踪。

`POST /api/v2/runs/{id}/rerun` 保持原有请求和 `{id}` 响应，创建任务时重新比较实际环境并写入 `replay:{source_run_id,check}`，不依赖浏览器预检查结果。原历史记录和产物不改写。环境变化和 unknown 会在历史页明确提示；执行继续遵守当前动作的 Schema、能力和领域校验，不因提示放宽约束。旧记录仍可读取和重执行；新任务记录当前环境。

### 文档工作台的生成方式贡献

动作可在 `ui_schema.document_input` 声明 `{title,description,default?,match_metadata?,bindings}`。`bindings` 将动作输入字段映射到通用文档数据：source、id、context、effective_context（默认值与当前参数合并）、schema、defaults 或 metadata.<路径>。undefined 不发送，未完成数值会阻止调用，后端仍按动作契约验证。

通用模板动作声明 default；领域动作通过 match_metadata 匹配资产元数据键。用户选择的生成方式优先，其次是可选 metadata.execution.action，再次是唯一的元数据匹配/默认提供方。多个匹配不会按加载顺序覆盖，不可用的显式选择不会降级为文本渲染。

模板库的按钮、异步任务和输出按这个动作执行；源码草稿及领域规格随当次输入提交。内置 NC 动作负责 NC 校验与扩展注册，通用渲染器保持隔离。


### 执行、数值与领域校验补充（2026-10-03）

宿主能力接口的 `document_actions` 包含已安装但未激活的文档动作声明，不代表该动作可执行。声明的元数据匹配存在、对应动作未激活时，WebUI 阻止执行。composition 入口保留旧 `metadata.nc → nc.generate` 的关联声明，纯模板构建仍不依赖或加载 NC 实现。保存文档时把所选动作写入 `metadata.execution.action`；新领域资产应使用这个字段表达可迁移的执行意图。用户可以明确切换普通文本，宿主不自动降级。

JSON 入口接受 `[-2^63, 2^64-1]` 范围内的整数字面量，超出范围返回 `numeric_range`；字符串中的数字不转换。小数/指数走有限 f64 表示，非零值归零返回 `numeric_underflow`。NC 的整数参数模型仍是 i64，超过该范围明确拒绝。资产、HTTP/CLI、运行记录和外部 RPC 共用同一入口检查；它们不承诺任意精度小数。

NC 动作现在接受可选 `schema`（对象或布尔）与 `defaults`（对象），工序输入可传同样字段。没有显式值时，使用冻结根资产的定义；传源码草稿不会丢掉所选根资产的元数据。优先级为用户参数 > 显式/根资产通用 defaults > NC specs defaults > 模板内联 default；派生规格计算出的值始终覆盖同名输入。Schema 与 NC specs 同时校验同一份有效参数，在注入 machine 系统变量之前执行 Schema 校验。因此 `additionalProperties:false` 不会误拒系统机床；显式 NC specs 也不会解除 Schema 的范围或枚举限制。

NC 参数继续支持数值、i64 整数、字符串、布尔和递归列表；对象参数属于通用模板能力，NC 模型对不支持的类型报错。动态 include/import/extends 仍可用，所有提供的参数都经过已声明 specs 的值级校验，实际渲染采用严格字段策略。NC 请求的 lenient 不再放宽未定义字段；显式 default 与 is defined 的可选分支仍可用。缺失机床成员、动态下标或子模板参数不会静默清空。普通 template.render 保持独立的严格/宽松选择。

模板 inspect 冻结所有可用源码，静态引用列举是诊断信息，不替代 Jinja 执行控制流。缺失的可选引用、候选 include、未执行分支由实际渲染决定，CLI、直接动作与后台任务采用相同语义。NC 在后处理完成后检查正文中的非有限数值字址，并忽略注释与引号字符串。

通用插件页按工作区/面板保存值与原始 JSON 草稿，解析失败或顶层类型不符时不能用上一份有效值替代执行。结果绑定动作及提交输入签名；编辑、切换动作后标为过期，失败/取消无当前可交付产物。历史页保留明确的原始历史导出入口。编辑器的受控值同步不触发用户修改回调。
