# 宿主与插件契约

当前源码：CLI/WebUI 2.3.0（未发布），引擎、SDK、运行时及内置插件清单版本 2.0.0；外部进程协议 **2**。协议版本与服务版本、组件版本独立。本文描述当前工作树，不能作为远端 CI 或发行成功证据。

## 稳定边界

- CLI 动作直接返回 `ActionResult`；HTTP 使用 `{ok:true,data}` / `{ok:false,error}` 外层，不能混用。
- 宿主响应类型与读取器在 `ui/src/lib/contracts.ts`。任务、历史分页、工作区、能力描述、模板读取/保存/目录及错误响应在消费前校验。已知类型的 `api` 调用必须提供读取器；动态插件动作仍保留开放业务数据，但动作结果外层统一校验。
- 模板扩展字段、元数据、Schema 关键词和插件业务 `data` 保持开放，不为统一类型删除未知业务字段。`Data` 仅用于这些开放边界与动态表单；它不能代替宿主契约。
- 业务数值沿用无损 JSON。宿主的行列、分页计数、时间戳与服务版本在检查为安全整数后才转换为 JavaScript number；用户参数和产物数据不走这条转换路径。
- 资产保存继续传递原始 `expected` 指纹；类型校验不重新生成指纹，也不改变服务端并发冲突语义。

共享样例是 `tests/fixtures/contracts/v2.json`，由 `cli/tests/contracts.rs`、`ui/src/lib/wireContracts.test.ts`、`scripts/tests/test_wire_contracts.py` 共同读取。覆盖动作、错误、资产扩展字段、大整数与任务记录；这份样例是回归向量，不是所有动作 Schema 的替代品。

## 诊断与协议 2

`PluginError` 保持 `{code,message,diagnostics}`。每项诊断包含 `level/code/message`，以及可选字段：

| 字段 | 含义 |
| --- | --- |
| `path` | JSON 输入实例路径，例如 `/context/n`；根路径可为空字符串 |
| `template` | 引擎报告的模板标识，与字段路径分别记录 |
| `line` | 已知源码行，1 起始；未知时省略 |
| `column` | 已知源码列，1 起始，尽力定位；未知时省略 |

模板错误码为 `template_parse`、`template_not_found`、`template_undefined_variable`、`template_unknown_filter`、`template_unknown_test`、`template_render`；未来未知引擎错误兜底为 `template_error`。输入 Schema 错误继续保留原业务错误码和实例路径。

解析错误保留引擎已有的行列；其他错误保留引擎暴露的模板名，不从消息字符串猜行列。WebUI 展示模板标识与可用位置；字段诊断定位表单，当前模板的源码诊断定位编辑器，其他模板的诊断提示打开对应模板。

由于旧 SDK 对未知诊断字段采用严格解析，外部协议从 1 升为 2。旧协议描述和握手均明确拒绝；服务版本仍为 1，HTTP 路由仍为 `/api/v2`。插件作者须同步更新安装描述中的 `protocol_version` 和 SDK/握手实现后重新安装；不能只修改清单数字。Python SDK 的 `PluginError.detail` 保留完整结构，宿主服务调用失败再向宿主返回时不丢失诊断。普通 Python 异常仍使用 `plugin_error`。

## 演进与验收

变更宿主字段时同步 Rust 类型、TypeScript 类型/读取器、共享样例与调用方。变更外部 wire 格式时检查严格解析器并决定协议版本；变更服务输入/输出语义时单独检查服务版本。不得用提高版本数字代替适配和测试。

当前界面的统一浏览器门禁为 `node scripts/browser_acceptance.cjs`。它运行主流程、共享参数/math、插件/失败恢复三组现行验收，每组独立 home/workspace、随机 loopback 端口和宿主进程。开始交互前逐文件比较宿主实际返回资源与当前生成资源，报告记录源码 HEAD、源码摘要和二进制 SHA-256。该摘要关联本地构建，不表示未提交修改已经进入 HEAD。

Rust 全 workspace、前端、Python 与真实浏览器是不同层次的证据。CI 使用相同浏览器入口；远端三平台结果仍须绑定实际提交核验。

## 执行环境与调用元数据扩展

当前历史环境格式为 2，增加 provenance 启动观察值；它与外部协议 2 和服务版本 1 分别维护。旧格式仍可读取，比较结果为 unknown，不改写原记录。新增诊断入口、历史归档格式与脱敏配置规则见[运行维护](OPERATIONS.md)。插件作者的实机进程测试入口见[插件验收](PLUGIN_ACCEPTANCE.md)。
