# NCtool 2.0 系统设计

当前实现规范。历史 1.x 的配置、模型和路由不再作为 2.0 契约。

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

启动选择顺序：CLI `--profile` 覆盖已保存 profile；profile 的基础插件加 `enabled`；最后移除 `disabled`。`template` 加载模板服务；`nc` 加载模板、math、NC、多工序。外部插件只在显式启用后启动。

`plugins` 是按插件 ID 保存的配置，按插件的 `config_schema` 验证；`providers` 是服务 ID 到提供方插件 ID 的显式映射。多候选服务没有选择时失败，不默认选择最后加载者。

配置修改需要旧指纹，安装不执行插件代码。运行中不热替换插件，当前能力目录与下次启动配置分别显示。

## 资产

工作区下 `templates/<id>.json`、`presets/<id>.json`、可选 `machines/<id>.json`。资产 ID 是单个合法路径段；引用别的模板使用资产 ID，不借助全局目录分类推断加工类型。

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

NC 规格可放在 `metadata.nc.specs`，通用模板服务只保存扩展元数据。`template.render` 的请求规格/defaults 优先于资产规格/defaults，调用方显式 context 覆盖同名默认值；默认值采用顶层合并。

读资产返回完整 `{hash,len,mtime}` 的不透明 JSON 字符串指纹。新建时 expected 为 null/缺省；修改必须携带读取到的指纹。写入在文件锁内比较、原子替换并取得返回指纹，拒绝陈旧覆盖和符号链接越界。

JSON 输入经过数值下溢扫描与实际解析器确认：非零十进制值被解析为零时失败；字符串内容和合法次正规数不受影响。

## 动作数据流

`template.render`：支持 `source` 或已保存的 `template`、`templates` 内存引用集合、`context` 对象、`schema/defaults`、`lenient`、`trim_blocks/lstrip_blocks`、显式 `extensions`。

`template.inspect`：递归收集静态模板引用及变量的必选/可选、文件名和行列位置。动态引用无法静态穷举，最终由渲染器解析。`include_snapshot:true` 同时返回模板集合和主模板资产元数据的不可变快照；`template.render` 可仅传入该 `snapshot` 和 context，完全不重读资产。NC 校验、规格解析和渲染使用同一份快照，避免编辑发生在校验与渲染之间时改变程序内容。

`nc.generate`：支持 `source/template/templates`、`params`、领域 `specs`、`machine` 或 `machine_id`、`options`。不使用普通模板 context 作为领域参数。NC 规格保留原有范围、整数、白名单、条件必选、查表派生和机床动态上界算法；任何领域错误都会失败。模板 lenient 不绕过领域校验。

`process.generate`：支持 name、公共 params/machine/options、非空 ops。工序覆盖公共参数与选项；每段从上一段真实末行号续编，不能自行重置。任何工序失败时返回聚合诊断，成功工序正文不交付。先在内存构造最终结果，再由 CLI `--out` 原子写出。

动作实例与 JSON Schema 以 `nctool actions` / `/api/v2/capabilities` 为运行时真值。

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
- `GET /api/v2/runs`：最近 100 条轻量索引；完整记录仍保留在磁盘。
- `GET /api/v2/runs/<id>`：实际输入、状态、结果、错误与插件版本。
- `POST /api/v2/runs/<id>/cancel` / `remove`：取消任务或删除已结束记录。

任务保存在工作区 `.nctool/runs/`，单条记录上限 64 MiB，最多并发 8 个任务。生成成功后才记录可交付产物；取消、失败和服务中断不提供当前产物。通用模板记录完整源集合快照；第三方/领域任务保留动作输入和插件版本，其额外领域快照由提供方接口决定。

WebUI 使用模式只显示参数表单与结果，编辑模式显示源码和参数定义。JSON 属于高级入口，无效 JSON 会保留并阻止执行，不能回退到旧的有效输入。生成结果带有输入关联，修改输入后旧结果的导出会禁用。

## 2.2 插件管理与工作区迁移

- `POST /api/v2/config/validate {config}`：仅校验描述、配置、服务提供方与依赖，返回启动顺序。保存和启停接口也执行同一预检查，错误不会保存。
- `POST /api/v2/plugins/inspect {source}`：检查本地插件目录，返回声明，不启动入口。
- `POST /api/v2/plugins/install {source}`：校验后复制到 home/plugins；不自动启用。
- `POST /api/v2/plugins/uninstall {id}`：只卸载当前未运行的外部插件。
- `POST /api/v2/runs/<id>/rerun`：生成新任务，优先使用提供方的 replay_input。成功的内置 NC/工序结果固定模板、机床和选项；执行器版本仍是当前版本，不承诺跨版本逐字一致。
- `GET /api/v2/bundle`：导出当前启用插件声明的所有资产集合。
- `POST /api/v2/bundle/validate {bundle}`：检查完整性、提供方可用性、资产结构与同名冲突，返回集合及稳定标识映射，不写入资产。
- `POST /api/v2/bundle/import {bundle}`：全部预检查后应用，失败时通过各集合返回的 receipt 反向回滚已导入资产。指纹不匹配时不会删除其他编辑写入的数据，回滚冲突会明确报告。

资产包结构为 `{format:"nctool-workspace",version:1,collections:{id:data},manifest:[{collection,provider,version,fingerprint}]}`。内容指纹用于发现改动，不是签名或来源认证。导出保留所有稳定标识，模板引用无需重写；首版导入拒绝同名冲突，不自动重命名，也不导入运行历史或宿主启动配置。

模板集合包含 templates/presets，NC 集合包含 machines。单个资产最多 1 MiB、每集合最多 2048 项、整体包最多 64 MiB。集合插件负责内容与路径校验；外部提供方必须遵守 validate_only 和回滚契约。内置导入具备受保护的原子单文件写入和回滚，跨插件导入不提供隔离外部副作用的数据库事务。

NC 视图通过已保存模板的 schema 生成参数控件；metadata.nc.specs 优先作为领域规格，无领域规格时可从通用 schema 提取数值类型和范围。临时源码使用独立参数，不沿用先前所选模板的规格。机床配置和工序草稿按工作区恢复，未完成数值不转成合法旧值。

NC 资产可声明 `metadata.nc.render_options:{trim_blocks,lstrip_blocks}` 作为该领域的空白选项默认值；请求中显式布尔值优先。有效选项进入 replay_input。普通 template.render 不读取领域选项，仍由调用方显式指定。此能力支持用户选择的 V3 模板转换，不恢复发行包的旧业务模板。
