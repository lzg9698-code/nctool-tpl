# 演示模式与真实服务模式差异清单

日期：2026-09-05  
适用页面：`ui/index.html`  
服务入口：`nctool ui`

## 1. 模式定义

| 项目 | 演示模式（demo） | 真实服务模式（server） |
|---|---|---|
| 默认入口 | 直接打开静态 `ui/index.html`，或显式切换 `API.mode` | `nctool ui` 提供页面时默认启用 |
| 数据来源 | HTML 内置的 `TEMPLATES`、`MACHINE_PRESETS` | Rust 注册表、配置层叠和 `nctool-core` |
| 请求地址 | 本地 mock 函数，不发 HTTP | `window.location.origin` 同源 HTTP API |
| 适用目的 | 离线演示、界面开发和交互原型 | 本机模板浏览、校验和 G-code 生成 |

## 2. 数据与模板行为

### 模板列表

- demo：直接读取前端静态模板数组；列表可以即时使用。
- server：启动时请求 `GET /api/templates`；服务端返回内置模板和安全加载的目录模板。
- server 模式下，未知分类返回 400；不存在的模板不会被前端静默替换为 demo 模板。

### 模板详情

- demo：从内存中的静态对象获取源码和参数规格。
- server：选择模板后请求 `GET /api/templates/{name}`，详情包含源码、参数规格和变量提取结果。
- server 仅接受注册表中的逻辑模板名；绝对路径、`..`、编码路径和目录外符号链接不能作为 HTTP 模板引用。

### 机床

- demo：使用前端内置三套预设；可使用浏览器 `localStorage` 模拟自定义机床。
- server：请求 `GET /api/machines`，使用服务端内置预设和配置文件中的自定义机床。
- server 模式禁用浏览器本地自定义机床编辑，避免“页面显示的机床”与真实生成配置不一致。

## 3. 渲染与校验行为

| 项目 | demo | server |
|---|---|---|
| 模板引擎 | 前端自研 mini Jinja 解释器 | Rust `nctool-core` / `nctool-tpl` 管线 |
| 默认值 | 前端规格和本地逻辑 | Core 注册表规格与生成管线 |
| 校验入口 | `API.mock` 内的 `buildReport` | `POST /api/validate` |
| 渲染入口 | 浏览器内 `doRender` | `POST /api/render` |
| 输出一致性 | 仅适合演示，不保证与 CLI 一致 | 目标是与 CLI/Core 同一管线，需持续逐字节回归 |
| 严格/宽松 | 前端状态模拟 | 服务端严格校验与 `lenient` 选项 |
| 生成选项 | 前端 `normalizeOpts` | 服务端严格解析；非法类型/格式返回 400 |

## 4. 错误与响应契约

真实服务模式统一使用：

```json
{
  "ok": true,
  "data": {}
}
```

或：

```json
{
  "ok": false,
  "error": {
    "kind": "bad_request",
    "message": "..."
  }
}
```

校验问题字段为：

```json
{
  "level": "error | warning | info",
  "param": "可选参数名",
  "message": "面向用户的说明"
}
```

- 不再使用 `warn` 作为等级名称。
- 不再以 `msg` 作为正式错误文案字段。
- `GET /api/templates` 的非法分类返回 400。
- 未知模板返回 404 `template_not_found`。
- 未知机床返回 404 `machine_not_found`。
- 超过 1 MiB 请求体返回 413 `payload_too_large`。
- 校验未通过的 render 请求返回 200，并在 `data.blocked` 中表达是否阻断生成。

## 5. 可写能力差异

- demo：源码修改、浏览器自定义机床和批量生成可以在浏览器内存/localStorage 中模拟。
- server：当前仅提供只读模板/机床 API 和内存中的校验/生成结果；页面禁用模板保存、浏览器自定义机床和批量 mock。
- 服务端不会将浏览器内修改写回磁盘，也不会执行 shell 命令。

## 6. 安全边界差异

- demo：主要风险是演示结果与真实 Core 语义不一致。
- server：默认仅允许回环 IP 监听；非回环地址直接拒绝。服务端请求体限制为 1 MiB，并对模板逻辑名、参数类型、生成选项和机床 ID 做校验。
- 在工艺评审和真实机床空运行完成前，两种模式都不能被视为生产级 G-code 验证工具。

## 7. 验证建议

1. UI 服务模式优先验证：模板列表 → 模板详情 → 填参 → 校验 → 预览 → 复制/下载。
2. 对同一模板和参数，比较 server API 输出与 CLI `nctool render` 输出。
3. 若需要演示前端界面，明确标注“演示模式结果可能与真实生成不一致”。
4. 不要把 demo 模式的 localStorage 数据当作服务端配置或发布能力。
