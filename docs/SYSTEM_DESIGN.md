# 系统设计与配置

当前 2.2 CLI/WebUI 与 2.0 插件的实现规范。历史 1.x 的配置、模型和路由不再作为当前契约。组件边界见[架构](ARCHITECTURE.md)，外部服务与 SDK 见[插件协议](PLUGIN_PROTOCOL.md)。

## 目录与启动

CLI 全局参数为 `--home`、`--workspace`、`--profile template`。其他 profile 值（含旧 `nc`）以 `invalid_profile` 拒绝；工作区默认当前目录；显式 `--home` 优先，否则依次检查 `NCTOOL_HOME`、Windows 的 `%LOCALAPPDATA%/nctool`、`$XDG_CONFIG_HOME/nctool`、`$HOME/.config/nctool`。最后 HOME 不存在时以当前目录下 `.config/nctool` 兜底。

home 包含 `config.json` 和已安装的 `plugins/`，工作区包含资产和 `.nctool/runs/`。浏览器草稿按工作区标识隔离，不属于已保存资产。WebUI 默认 `127.0.0.1:8788`，可用 `ui --host <回环 IP> --port <端口>` 修改。

默认构建包含通用模板与共享参数组合；数学扩展需以 `--features math` 构建或使用完整发行二进制。构建步骤见[贡献指南](CONTRIBUTING.md)。

## 配置组合

`home/config.json`：

```json
{
  "profile": "template",
  "enabled": ["python-report"],
  "disabled": [],
  "providers": {},
  "plugins": {}
}
```

启动选择顺序：CLI `--profile` 覆盖已保存 profile；profile 的基础插件加 `enabled`；最后移除 `disabled`。`template` 组合加载模板与共享参数服务；独立 `math` 扩展或外部插件须在 `enabled` 中显式启用。外部插件只在显式启用后启动。

`plugins` 是按插件 ID 保存的配置，按插件的 `config_schema` 验证；`providers` 是服务 ID 到提供方插件 ID 的显式映射。多候选服务没有选择时失败，不默认选择最后加载者。

配置修改需要旧指纹，安装不执行插件代码。运行中不热替换插件，当前能力目录与下次启动配置分别显示。

## 资产

工作区下 `templates/<id>.json`、`presets/<id>.json` 及插件声明的其他集合（共享参数为 `parameters/<id>.json`）。资产 ID 是单个合法路径段；引用别的模板使用资产 ID，不从全局目录分类推断类型。

模板资产示例：

```json
{
  "source": "Hello {{ user.name }}",
  "tags": ["letters"],
  "schema": {"type":"object","required":["user"]},
  "defaults": {"user":{"name":"Ada"}},
  "metadata": {"custom":{"language":"en"}}
}
```

通用模板服务只按约定保存资产元数据，不解释领域字段。`template.render` 的请求规格/defaults 优先于资产规格/defaults，调用方显式 context 覆盖同名默认值；默认值采用顶层合并。

读资产返回完整 `{hash,len,mtime}` 的不透明 JSON 字符串指纹。新建时 expected 为 null/缺省；修改必须携带读取到的指纹。写入在文件锁内比较、原子替换并取得返回指纹，拒绝陈旧覆盖和符号链接越界。

JSON 输入经过数值下溢扫描与实际解析器确认：非零十进制值被解析为零时失败；字符串内容和合法次正规数不受影响。

## 动作数据流

`template.render`：支持 `source` 或已保存的 `template`、`templates` 内存引用集合、`context` 对象、`schema/defaults`、`lenient`、`trim_blocks/lstrip_blocks`、显式 `extensions`。

`template.inspect`：递归收集静态模板引用及变量的必选/可选、文件名和行列位置。动态引用无法静态穷举，最终由渲染器解析。`include_snapshot:true` 同时返回模板集合和主模板资产元数据的不可变快照；`template.render` 可仅传入该 `snapshot` 和 context，完全不重读资产。受管渲染的校验与渲染共用同一份快照，避免编辑发生在校验与渲染之间时改变程序内容。

`template.render.managed`：普通模板输入另加共享参数 `bindings`、可选 `preset_id`；先解析共享定义/预置与人工输入得到冻结上下文，再走通用渲染，返回渲染结果、参数解析与冻结 `replay_input`。冻结形式不能与 `context/schema/defaults/bindings/preset_id` 混用。字段与服务见[参数管理指南](PARAMETER_MANAGEMENT.md)。

动作实例与 JSON Schema 以 `nctool actions` / `/api/v2/capabilities` 为运行时真值。

CLI `actions` 返回动作数组，每项含 plugin 及展开的动作描述；`run` 成功时直接输出 ActionResult，包含 data/artifacts/diagnostics，生成正文在 `data.text`。HTTP 则使用下述 ok/data 外层包装，两者不能混用响应解析方式。

## HTTP 与运行约束

- `GET /api/v2/plugins`：当前插件状态、下次启动配置与是否需要重启。
- `GET /api/v2/capabilities`：动作 Schema、服务绑定、插件贡献的页面。
- `GET /api/v2/config`；`POST /api/v2/config`，体 `{config,expected}`。
- `POST /api/v2/plugins/<id>/enable|disable`。
- `POST /api/v2/actions/<action-id>`，体 `{input,request_id?}`。
- `POST /api/v2/cancel/<request-id>`。

成功响应 `{ok:true,data:...}`，错误 `{ok:false,error:{code,message,diagnostics}}`。动作结果为 data/artifacts/diagnostics。冲突 409，未知入口 404，插件并发上限 503，插件超时 504。

模板集合最多 1024 份、合计 16 MiB；单次渲染默认 1,000,000 条指令预算和 16 MiB 流式输出上限，避免失控循环和输出分配。

HTTP 使用 Axum/Tokio，仅回环地址；普通请求体上限 1 MiB，资产包校验/导入请求上限 64 MiB，读取超时 10 秒，工作线程并发上限 8。取消入口不竞争动作执行配额。POST 必须 application/json，校验 Host、Origin 和 Sec-Fetch-Site。

外部进程默认超时 30 秒、单条消息 1 MiB、并发 8；可配置范围见协议。超时或取消会终止进程，所以同一进程中的其他活动请求也会失败；其他插件与宿主仍可用。首版不提供副作用回滚或操作系统级沙箱。插件应通过宿主服务交付产物，避免在执行过程中提前写出成品。

## 2.1 工作台接口

新增通用动作 `template.catalog`、`template.copy`、`template.remove`、`preset.copy`、`preset.remove`。目录返回显示名称、说明、标签和版本指纹。复制不会覆盖已有资产；删除需要指纹且拒绝已知静态模板引用。重命名在元数据中修改显示名称，内部标识和 include 引用保持稳定。

- `GET /api/v2/workspace`：工作区标识与当前插件组合。
- `POST /api/v2/runs`，体 `{action,input,title?}`：创建异步任务，返回 ID。
- `GET /api/v2/runs`：兼容入口，返回最近最多 100 条轻量索引；完整记录仍保留在磁盘。
- `POST /api/v2/runs/query`，体 `{query?,status?,offset?,limit?}`：按标题/标识/动作关键词和状态检索全部摘要，返回 `{runs,total,offset,limit}`。关键词不区分大小写；状态仅接受 queued/running/succeeded/failed/cancelled/interrupted；offset 为非负整数，limit 默认 25、范围 1–100。结果按开始时间降序、同时间戳标识升序排列。请求不读取完整记录，但仍扫描全部索引；offset 分页在并发新增/删除时不提供快照一致性。
- `GET /api/v2/runs/<id>`：实际输入、状态、结果、错误与插件版本。
- `GET /api/v2/runs/<id>/replay-check`：只读比较历史与当前环境，返回 `{status,differences}`。
- `POST /api/v2/runs/<id>/cancel` / `remove`：取消任务或删除已结束记录。

任务保存在工作区 `.nctool/runs/`，单条记录上限 64 MiB，最多并发 8 个任务。生成成功后才记录可交付产物；取消、失败和服务中断不提供当前产物。通用模板记录完整源集合快照；第三方/领域任务保留动作输入和插件版本，其额外领域快照由提供方接口决定。

WebUI 使用模式只显示参数表单与结果，编辑模式显示源码和参数定义。JSON 属于高级入口，无效 JSON 会保留并阻止执行，不能回退到旧的有效输入。生成结果带有输入关联，修改输入后旧结果的导出会禁用。

## 2.2 插件管理与工作区迁移

- `POST /api/v2/config/validate {config}`：仅校验描述、配置、服务提供方与依赖，返回启动顺序。保存和启停接口也执行同一预检查，错误不会保存。
- `POST /api/v2/plugins/inspect {source}`：检查本地插件目录，返回声明，不启动入口。
- `POST /api/v2/plugins/install {source}`：校验后复制到 home/plugins；不自动启用。
- `POST /api/v2/plugins/uninstall {id}`：只卸载当前未运行的外部插件。
- `POST /api/v2/runs/<id>/rerun`：生成新任务，优先使用提供方的 replay_input。成功的受管渲染结果固定模板与冻结参数；执行器版本仍是当前版本，不承诺跨版本逐字一致。
- `GET /api/v2/bundle`：导出当前启用插件声明的所有资产集合。
- `POST /api/v2/bundle/validate {bundle}`：检查完整性、提供方可用性、资产结构与同名冲突，返回集合及稳定标识映射，不写入资产。
- `POST /api/v2/bundle/import {bundle}`：全部预检查后应用，失败时通过各集合返回的 receipt 反向回滚已导入资产。指纹不匹配时不会删除其他编辑写入的数据，回滚冲突会明确报告。

资产包结构为 `{format:"nctool-workspace",version:1,collections:{id:data},manifest:[{collection,provider,version,fingerprint}]}`。内容指纹用于发现改动，不是签名或来源认证。导出保留所有稳定标识，模板引用无需重写；首版导入拒绝同名冲突，不自动重命名，也不导入运行历史或宿主启动配置。

模板集合包含 templates/presets，共享参数插件贡献 definitions/presets 集合。单个资产最多 1 MiB、每集合最多 2048 项、整体包最多 64 MiB。集合插件负责内容与路径校验；外部提供方必须遵守 validate_only 和回滚契约。内置导入具备受保护的原子单文件写入和回滚，跨插件导入不提供隔离外部副作用的数据库事务。

通用模板视图通过已保存模板的 schema 生成参数控件；无 schema 时按基础类型渲染。共享参数绑定、预置选择与草稿按工作区恢复，未完成数值不转成合法旧值。

模板可通过 `metadata.parameters.bindings` 声明共享参数绑定；工作台据此生成控件并写入请求。显式输入优先，有效选择进入 replay_input。

## 历史重执行环境

新任务记录 `environment:{format_version:1,host_version,plugins,services}`；plugins 保存已启用插件的声明版本和协议版本，services 保存版本化服务绑定。记录不包含插件配置、程序内容或操作系统环境。

replay-check 的 status 为 `matching`（已记录版本与绑定一致）、`changed`（宿主/插件/服务差异）或 `unknown`（缺少信息或格式不可比较），differences 为说明字符串数组。matching 不保证产物相同。

rerun 无需请求体字段，响应 data 为 `{id}`；创建任务时后端重新比较环境，在新记录写入 `replay:{source_run_id,check}`，不改写原记录与产物。旧记录仍可读取和重执行，继续执行当前 Schema、能力与领域校验。完整记录契约见[插件协议](PLUGIN_PROTOCOL.md#历史重执行环境宿主-api-扩展)。
