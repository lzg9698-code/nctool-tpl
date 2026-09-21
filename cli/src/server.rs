//! 本地 Web UI 服务（阶段 C）：tiny_http + 只读 API + 内嵌单文件前端。
//!
//! 安全约定（ROADMAP C1.3 / R5）：
//! - **仅回环**：`--host` 传非回环地址时 [`listen_addr`] **直接拒绝**，而不是
//!   打印警告后放行。本服务能读模板目录并驱动渲染，暴露到局域网没有任何使用
//!   场景，只剩攻击面。（此处曾写"由命令层打印警告、本模块不做判断"，那是更早
//!   的实现；照那句话改回去会把这个决定悄悄撤销。）
//! - 不执行任何 shell 命令；不提供任何写操作
//! - 请求体读取设 1 MiB 上限，防异常载荷
//! - **失败响应不回显内部正文**：500 只给泛化文案，详情写 stderr —— 内部错误的
//!   正文含模板文件的绝对路径（见 [`internal_error`]）
//! - **跨站请求防护**：`/api/` 下的请求校验 `Origin` / `Sec-Fetch-Site`
//!   （见 [`cross_site_guard`]）——只绑回环并不够，浏览器里的任意页面都能向
//!   `127.0.0.1:<port>` 发请求（DNS rebinding / CSRF）；本服务无状态、不落盘，
//!   但"被陌生网页驱动"仍应拦住
//! - 每个响应都带 CSP / nosniff / Referrer-Policy（见 [`SECURITY_HEADERS`]）
//!
//! 设计：路由逻辑收敛到纯函数 [`route`]（无网络依赖，可直接单元/集成测试），
//! [`serve`] 只负责 tiny_http 粘合（监听、解析、响应）。

use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::rc::Rc;

use nctool_core::asset::{now_iso8601, Preset, PresetStore, SpecFingerprint, WriteError};
use nctool_core::machine::MachinePreset;
use nctool_core::pipeline::{GCodeGenerator, GenerationOptions, OutputFormat};
use nctool_core::registry::{TemplateCategory, TemplateSource};
use nctool_tpl::Variable;

use crate::args::parameter_set_from_json;
use crate::cli::CategoryArg;
use crate::commands::templates::extract_variables;
use crate::context::Ctx;
use crate::output::CliError;

/// 内嵌单文件前端（`ui/index.html`，演示与服务双模式）。
pub const UI_HTML: &str = include_str!("../ui/index.html");

/// 单个请求体上限：inspect 的模板源码远小于此，超出视为异常载荷。
const MAX_BODY_BYTES: usize = 1024 * 1024;

/// 所有响应统一附加的安全响应头。
///
/// CSP 说明：前端是单文件内嵌页面，脚本与样式都是内联的，故 `script-src` /
/// `style-src` 仍需 `'unsafe-inline'`（**已知取舍**）。它挡不住"页面里被注入
/// `<script>`"，但能挡住外链加载、`object` / `frame` 嵌入与表单外发
/// ——把"本地工具页面"的攻击面收回到页面自身。
///
/// **当前依赖**：这条取舍成立的前提是页面里**不存在**可注入的插值点。
/// 2026-09-18 之前不成立 —— `esc()` 不转引号，模板文件名 / 参数名里的 `"` 就能
/// 加出 `onerror=`，而 `unsafe-inline` 恰好放行属性事件处理器（P1-1）。
/// 现已补全转义（见 `ui/index.html` 的 `esc` / `selEsc`），并逐点核对过全部
/// `innerHTML` 插值位。**改动前端插值时必须一并复核**：CSP 在这里不是兜底。
///
/// 若要去掉 `'unsafe-inline'`，需要把内联 `<script>`/`<style>` 外置成同源文件
/// （或每响应注入 nonce）。属独立改造，未随本轮进行。
const SECURITY_HEADERS: &[(&str, &str)] = &[
    (
        "Content-Security-Policy",
        "default-src 'self'; \
         script-src 'self' 'unsafe-inline'; \
         style-src 'self' 'unsafe-inline'; \
         img-src 'self' data:; \
         connect-src 'self'; \
         object-src 'none'; \
         base-uri 'none'; \
         form-action 'none'; \
         frame-ancestors 'none'",
    ),
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
];

/// 浏览器访问本服务时可能使用的同源写法。
///
/// `addr` 是实际监听地址（`127.0.0.1:8787` / `[::1]:8787`）；浏览器还可能用
/// `localhost` 或另一种回环写法访问，故一并列入白名单。
fn allowed_origins(addr: &SocketAddr) -> Vec<String> {
    let port = addr.port();
    vec![
        format!("http://{addr}"),
        format!("http://localhost:{port}"),
        format!("http://127.0.0.1:{port}"),
        format!("http://[::1]:{port}"),
    ]
}

/// 跨站请求防护：返回 `Some(403)` 表示应拒绝该请求。
///
/// 判定（只针对 `/api/` 下的请求）：
/// 1. `Origin` 存在且不在 [`allowed_origins`] 内 → 拒绝；
/// 2. `Sec-Fetch-Site` 存在且不是 `same-origin` / `none` → 拒绝；
/// 3. 两个头都不存在（curl 等非浏览器客户端）→ **放行**。
///
/// 第 3 条是刻意的：本服务是本地命令行工具，刻意保留"用 curl 直接调 API"的用法；
/// 浏览器侧的跨站风险由前两条覆盖——浏览器的 `fetch` / `XHR` / 表单提交**必然**
/// 带 `Origin`，伪造不了。
pub fn cross_site_guard(headers: &[(&str, &str)], allowed: &[String]) -> Option<Resp> {
    let get = |name: &str| -> Option<&str> {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| *v)
    };
    if let Some(origin) = get("origin") {
        // Origin 形如 `http://host:port`，个别实现会带尾斜杠
        let origin = origin.trim_end_matches('/');
        if !allowed.iter().any(|a| a.eq_ignore_ascii_case(origin)) {
            return Some(Resp::Json(
                403,
                err(
                    "forbidden_origin",
                    format!("跨站请求被拒绝：Origin 为 {origin}，本服务只接受同源请求"),
                ),
            ));
        }
    }
    if let Some(site) = get("sec-fetch-site") {
        if !site.eq_ignore_ascii_case("same-origin") && !site.eq_ignore_ascii_case("none") {
            return Some(Resp::Json(
                403,
                err(
                    "forbidden_origin",
                    format!("跨站请求被拒绝：Sec-Fetch-Site 为 {site}"),
                ),
            ));
        }
    }
    None
}

/// 路由结果：JSON（带状态码）或 HTML 页面。
#[derive(Debug)]
pub enum Resp {
    /// JSON 响应（HTTP 状态码 + 已通过统一包络封装的载荷）
    Json(u16, serde_json::Value),
    /// 内嵌前端页面
    Html,
}

/// 成功包络：`{ ok: true, data }`。
fn ok(data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "ok": true, "data": data })
}

/// 错误包络：`{ ok: false, error: { kind, message } }`（kind 与 CLI 错误对齐）。
fn err(kind: &str, message: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "ok": false, "error": { "kind": kind, "message": message.into() } })
}

/// 纯函数路由：解析请求并产出响应（无网络依赖，可测试）。
///
/// - `method`：大写 HTTP 方法
/// - `path`：不含 query 的路径（已保持百分号编码，按需在处理器内解码）
/// - `query`：不含 `?` 的原始 query 串
/// - `body`：原始请求体字节
pub fn route(ctx: &Ctx, method: &str, path: &str, query: &str, body: &[u8]) -> Resp {
    match (method, path) {
        ("GET", "/health") => Resp::Json(
            200,
            ok(serde_json::json!({
                "status": "ok",
                "version": env!("CARGO_PKG_VERSION"),
            })),
        ),
        ("GET", "/api/templates") => templates_list(ctx, query),
        ("GET", "/api/machines") => machines_list(ctx),
        ("GET", "/api/presets") => presets_list(ctx, query),
        ("POST", "/api/presets") => presets_save(ctx, body),
        ("POST", "/api/presets/delete") => presets_delete(ctx, body),
        ("POST", "/api/inspect") => inspect(ctx, body),
        ("POST", "/api/validate") => validate(ctx, body),
        ("POST", "/api/render") => render(ctx, body),
        ("GET", p) => match p.strip_prefix("/api/templates/") {
            Some(raw) => template_detail(ctx, raw),
            None => Resp::Json(404, err("not_found", format!("未知接口: {method} {path}"))),
        },
        _ => Resp::Json(404, err("not_found", format!("未知接口: {method} {path}"))),
    }
}

// ---------------------------------------------------------------------------
// GET /api/templates
// ---------------------------------------------------------------------------

fn templates_list(ctx: &Ctx, query: &str) -> Resp {
    let category = parse_query(query)
        .into_iter()
        .find(|(k, _)| k == "category")
        .map(|(_, v)| v)
        /* `?category=` 表示「不筛选」，不是非法值：前端清空分类选择时会发出这种请求，
        此前一律 400，等于把「清空筛选」变成错误。 */
        .filter(|v| !v.trim().is_empty());
    if category
        .as_deref()
        .is_some_and(|value| parse_category(value).is_none())
    {
        return Resp::Json(
            400,
            err(
                "bad_request",
                "无效的模板分类，可选值为 general/milling/turning/drilling/grooving/machine",
            ),
        );
    }
    let gen = match ctx.build_registry() {
        Ok(g) => g,
        Err(e) => return internal_error(e),
    };
    let cat_core = category.as_deref().and_then(parse_category);
    let entries = gen.registry().list(cat_core);
    let templates: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "category": CategoryArg::from_core(e.category),
                "description": e.description,
            })
        })
        .collect();
    Resp::Json(200, ok(serde_json::json!({ "templates": templates })))
}

/// 分类字符串 → core 枚举（接受英文 id 与中文显示名，宽松匹配）。
///
/// 必须与 `cli/src/cli.rs` 的 `CategoryArg` 保持同一套取值：少一个分支，
/// 该分类的模板在 HTTP 侧被 **400 拒绝**，而 CLI 侧正常 —— 两侧对同一分类
/// 给出不同答案，且前端分类栏也会缺一项、计数永远对不上。
/// 分类名解析：规则在 `TemplateCategory` 的 `FromStr` 实现（与枚举定义同处一个文件），
/// 此处只做转发——避免"新增分类要改多处字符串表、漏改就 400"。
fn parse_category(s: &str) -> Option<TemplateCategory> {
    s.parse().ok()
}

// ---------------------------------------------------------------------------
// GET /api/templates/{name}
// ---------------------------------------------------------------------------

fn template_detail(ctx: &Ctx, raw_name: &str) -> Resp {
    let name = percent_decode(raw_name, false);
    let gen = match ctx.build_registry() {
        Ok(g) => g,
        Err(e) => return internal_error(e),
    };
    let system_vars = gen.registry().system_vars().to_vec();

    // 注册表模板（内置 / 目录）：携带分类、描述与参数规格
    if let Some(e) = gen.registry().get(&name) {
        let vars = match extract_variables(&e.source_text, &e.name, &system_vars) {
            Ok(v) => v,
            Err(err) => return cli_error(err),
        };
        let params: Vec<serde_json::Value> = e.params.iter().map(spec_json).collect();
        return Resp::Json(
            200,
            ok(serde_json::json!({
                "template": {
                    "name": e.name,
                    "category": CategoryArg::from_core(e.category),
                    "description": e.description,
                    "builtin": matches!(e.source, TemplateSource::Builtin),
                    "source": e.source_text,
                    "params": params,
                    "variables": vars_json(&vars),
                },
            })),
        );
    }

    // HTTP API 仅允许访问已注册模板；CLI 的本地文件路径能力不在此复用。
    Resp::Json(
        404,
        err("template_not_found", format!("模板不存在: {name}")),
    )
}

/// `ParamSpec` → 前端规格 JSON。
///
/// 与前端演示数据字段对齐：`kind` 用首字母大写类型名，`desc` 为
/// `description` 的别名（两份都给，前后端字段名不敏感）。
pub(crate) fn spec_json(spec: &nctool_core::ParamSpec) -> serde_json::Value {
    let kind = match spec.kind {
        nctool_core::ParamKind::Number => "Number",
        nctool_core::ParamKind::Integer => "Integer",
        nctool_core::ParamKind::String => "String",
        nctool_core::ParamKind::Bool => "Bool",
        // 前端据此外渲染「列表」控件（JSON 数组输入框）
        nctool_core::ParamKind::List => "List",
        // 前端据此渲染下拉选择；候选值见 `options` 字段
        nctool_core::ParamKind::Choice => "Choice",
        // 未标注类型：前端回退为文本输入
        nctool_core::ParamKind::Any => "Any",
    };
    serde_json::json!({
        "name": spec.name,
        "kind": kind,
        "required": spec.required,
        "default": spec.default,
        "min": spec.min,
        "max": spec.max,
        "integer": spec.integer,
        "unit": spec.unit,
        // 候选项白名单：前端可据此渲染 <select>，空/未声明时为 null
        "options": spec.options,
        // 条件必选：前端可据此把参数标为「条件必选」并展示触发条件
        "requiredIf": spec.required_if,
        // 派生规则：前端可据此把参数标为「由系统派生」并展示来源（不要求用户填写）
        "derive": spec.derive,
        "desc": spec.description,
        "description": spec.description,
    })
}

/// 变量集 → `{ required: [...], optional: [...] }`（含行列定位）。
fn vars_json(vars: &[Variable]) -> serde_json::Value {
    let var = |v: &Variable| serde_json::json!({ "name": v.name, "line": v.line, "col": v.col });
    serde_json::json!({
        "required": vars.iter().filter(|v| !v.optional).map(var).collect::<Vec<_>>(),
        "optional": vars.iter().filter(|v| v.optional).map(var).collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------------------
// POST /api/validate and POST /api/render
// ---------------------------------------------------------------------------

fn api_body(body: &[u8]) -> Result<serde_json::Value, Resp> {
    serde_json::from_slice(body)
        .map_err(|e| Resp::Json(400, err("bad_request", format!("请求体不是合法 JSON: {e}"))))
}

fn api_template_params(
    value: &serde_json::Value,
) -> Result<(&str, nctool_core::ParameterSet), Resp> {
    let template = value
        .get("template")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            Resp::Json(
                400,
                err("bad_request", "请求体需要非空字符串字段 \"template\""),
            )
        })?;
    let empty = serde_json::json!({});
    let params_value = value.get("params").unwrap_or(&empty);
    let params = parameter_set_from_json(params_value)
        .map_err(|e| Resp::Json(400, err(e.kind, e.message)))?;
    Ok((template, params))
}

/// HTTP 模板解析只允许注册表中的逻辑名称，不复用 CLI 的文件路径解析能力。
///
/// 返回共享的缓存注册表（`Rc`）：`Ctx::build_registry` 按模板目录指纹缓存，
/// 避免每个请求都重新遍历目录、读取并解析全部模板。
fn registered_template(ctx: &Ctx, name: &str) -> Result<(Rc<GCodeGenerator>, String), Resp> {
    let gen = ctx.build_registry().map_err(internal_error)?;
    if gen.registry().get(name).is_none() {
        return Err(Resp::Json(
            404,
            err("template_not_found", format!("模板不存在: {name}")),
        ));
    }
    Ok((gen, name.to_string()))
}

fn api_machine(ctx: &Ctx, value: &serde_json::Value) -> Result<nctool_core::MachineConfig, Resp> {
    let id = value
        .get("machine")
        .and_then(|v| v.as_str())
        .unwrap_or("generic");
    ctx.resolve_machine(Some(id)).map_err(|e| {
        let status = if e.kind == "machine_not_found" {
            404
        } else {
            400
        };
        Resp::Json(status, err(e.kind, e.message))
    })
}

fn validate(ctx: &Ctx, body: &[u8]) -> Resp {
    let value = match api_body(body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let (template, params) = match api_template_params(&value) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let (gen, name) = match registered_template(ctx, template) {
        Ok(v) => v,
        Err(r) => return r,
    };
    match gen.registry().validate(&name, &params) {
        Ok(report) => Resp::Json(
            200,
            ok(serde_json::json!({
                "report": crate::output::report_json(&name, &report),
            })),
        ),
        Err(e) => Resp::Json(500, err("registry", e.to_string())),
    }
}

fn generation_options(value: &serde_json::Value) -> Result<(GenerationOptions, bool), Resp> {
    let Some(options) = value.get("options") else {
        return Ok((GenerationOptions::default(), false));
    };
    let opts = options
        .as_object()
        .ok_or_else(|| Resp::Json(400, err("bad_request", "options 必须是 JSON 对象")))?;
    let get_bool = |name: &str| -> Result<bool, Resp> {
        opts.get(name)
            .map(|v| {
                v.as_bool().ok_or_else(|| {
                    Resp::Json(
                        400,
                        err("bad_request", format!("options.{name} 必须是布尔值")),
                    )
                })
            })
            .unwrap_or(Ok(false))
    };
    let get_u32 = |name: &str, default: u32| -> Result<u32, Resp> {
        opts.get(name)
            .map(|v| {
                let n = v.as_u64().ok_or_else(|| {
                    Resp::Json(
                        400,
                        err("bad_request", format!("options.{name} 必须是非负整数")),
                    )
                })?;
                u32::try_from(n).map_err(|_| {
                    Resp::Json(400, err("bad_request", format!("options.{name} 超出范围")))
                })
            })
            .unwrap_or(Ok(default))
    };
    // 必须把「键缺失」与「类型错误」分开：`and_then(as_str)` 对
    // `{"format": 1}` / `true` / `[]` 同样返回 `None`，合并处理就会**静默按
    // gcode 生成**，调用方以为选项生效。同函数的 get_bool / get_u32 已对类型错误
    // 返回 400，此处是唯一漏网（09-05 OPTIONS-SILENT-FALLBACK-001 的残留）。
    let format = match opts.get("format") {
        None => OutputFormat::Gcode,
        Some(serde_json::Value::String(s)) => match s.as_str() {
            "gcode" => OutputFormat::Gcode,
            "text" => OutputFormat::Text,
            other => {
                return Err(Resp::Json(
                    400,
                    err("bad_request", format!("不支持的输出格式: {other}")),
                ))
            }
        },
        Some(_) => {
            return Err(Resp::Json(
                400,
                err("bad_request", "options.format 必须是字符串"),
            ))
        }
    };
    let lenient = get_bool("lenient")?;
    Ok((
        GenerationOptions {
            format,
            line_numbers: get_bool("lineNumbers")?,
            line_number_step: get_u32("lineStep", 10)?,
            max_line_number: get_u32("maxLine", 9999)?,
            add_header_comment: get_bool("addHeader")?,
            strip_blank_lines: get_bool("stripBlank")?,
            ascii_only: get_bool("ascii")?,
        },
        lenient,
    ))
}

fn render(ctx: &Ctx, body: &[u8]) -> Resp {
    let value = match api_body(body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let (template, params) = match api_template_params(&value) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let (gen, name) = match registered_template(ctx, template) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let machine = match api_machine(ctx, &value) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let (opts, lenient) = match generation_options(&value) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let report = match gen.registry().validate(&name, &params) {
        Ok(v) => v,
        Err(e) => return Resp::Json(500, err("registry", e.to_string())),
    };
    let report_json = crate::output::report_json(&name, &report);
    if report.has_errors() && !lenient {
        return Resp::Json(
            200,
            ok(serde_json::json!({
                "blocked": true,
                "report": report_json,
                "output": "",
                "machine": machine.id,
            })),
        );
    }
    let output = if lenient {
        gen.generate_lenient(&name, &params, &machine, &opts)
    } else {
        gen.generate(&name, &params, &machine, &opts)
    };
    match output {
        Ok(output) => Resp::Json(
            200,
            ok(serde_json::json!({
                "blocked": false,
                "report": report_json,
                "output": output,
                "template": name,
                "machine": machine.id,
            })),
        ),
        Err(e) => Resp::Json(400, err("render", e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// GET /api/machines
// ---------------------------------------------------------------------------

/// 机床列表：内置预设 + 配置文件自定义机床（与 `machine list` 口径一致）。
///
/// `nctool ui` 的前端机床切换需要它；属于 ROADMAP C2 三端点之外的必要补充。
fn machines_list(ctx: &Ctx) -> Resp {
    // 枚举规则（预设 + 自定义、按预设 id 去重）来自 core 的单一来源；
    // 此前与 `commands/machine.rs::list` 各写一遍，两份会漂移。
    let machines: Vec<serde_json::Value> = MachinePreset::entries(&ctx.loaded.merged.machine)
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "vendor": m.vendor,
                "model": m.model,
                "config": m.config,
                "builtin": m.builtin,
            })
        })
        .collect();
    Resp::Json(200, ok(serde_json::json!({ "machines": machines })))
}

// ---------------------------------------------------------------------------
// GET/POST /api/presets
// ---------------------------------------------------------------------------

/// 预设文件路径（含红线 9 约束）。
///
/// **复用 `commands::preset::preset_path` 而不是自己拼**：R-9（预设文件不得落在
/// 模板根内）在 CLI 侧是无条件强制的，HTTP 侧若另写一份判据，两边必然漂移，
/// 而漂移的方向恰好是"HTTP 忘了查"→ 把 `presets.yaml` 写进模板根被当模板扫描。
fn presets_path(ctx: &Ctx) -> Result<std::path::PathBuf, CliError> {
    crate::commands::preset::preset_path(ctx, &crate::cli::PresetFileArgs { file: None })
}

/// `GET /api/presets`：预设列表（含结构化陈旧字段）。`?template=<名>` 过滤。
///
/// 文件损坏 / 版本未知时**降级为警告**并继续（只读命令不被坏文件拦住，D13），
/// 警告随响应返回给前端，而不是静默丢掉。
fn presets_list(ctx: &Ctx, query: &str) -> Resp {
    let template = parse_query(query)
        .into_iter()
        .find(|(k, _)| k == "template")
        .map(|(_, v)| v)
        .filter(|v| !v.trim().is_empty());
    let path = match presets_path(ctx) {
        Ok(p) => p,
        Err(e) => return cli_error(e),
    };
    let got = match PresetStore::load(&path) {
        Ok(g) => g,
        Err(e) => return write_error_resp(e),
    };
    let items: Vec<serde_json::Value> = got
        .file
        .presets
        .iter()
        .filter(|p| template.as_deref().is_none_or(|t| p.template == t))
        .map(|p| {
            // 模板不可解析 → `stale = null`、`resolvable = false`。
            // **不**退回"新鲜"：那是静默误报（与 CLI 侧 `stale_of` 同一口径）。
            let stale = crate::commands::preset::stale_of(ctx, p);
            serde_json::json!({
                "name": p.name,
                "template": p.template,
                "paramCount": p.params.len(),
                "createdAt": p.created_at,
                "specFingerprint": p.spec_fingerprint,
                "resolvable": stale.is_some(),
                "stale": stale.as_ref().map(|r| r.is_stale()),
                "staleParams": stale.as_ref().map(|r| r.stale_params.clone()),
                "missingRequired": stale.as_ref().map(|r| r.missing_required.clone()),
            })
        })
        .collect();
    Resp::Json(
        200,
        ok(serde_json::json!({
            "path": path.display().to_string(),
            "presets": items,
            "warnings": got.warnings,
        })),
    )
}

/// `POST /api/presets`：保存 / 覆盖一个预设。
///
/// 请求体：`{"name","template","params":{...},"force":bool}`。
/// 校验链路与 `preset save` **逐条一致**（参数归属 → L3 值校验 → 同名拒绝），
/// 否则同一份数据 CLI 能存、HTTP 不能存（或反之），是典型的两套口径。
fn presets_save(ctx: &Ctx, body: &[u8]) -> Resp {
    let parsed: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", format!("请求体不是合法 JSON: {e}"))),
    };
    let name = match parsed.get("name").and_then(|t| t.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Resp::Json(400, err("bad_request", "请求体需要非空字符串字段 \"name\"")),
    };
    let template = match parsed.get("template").and_then(|t| t.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return Resp::Json(
                400,
                err("bad_request", "请求体需要非空字符串字段 \"template\""),
            )
        }
    };
    let force = parsed
        .get("force")
        .and_then(|f| f.as_bool())
        .unwrap_or(false);

    // 参数：复用 CLI 的扁平解析器（`{"x":21.0}`），与 `--params-file` 同形。
    let empty = serde_json::json!({});
    let raw_params = parsed.get("params").unwrap_or(&empty);
    let params = match parameter_set_from_json(raw_params) {
        Ok(p) => p,
        Err(e) => return cli_error(e),
    };
    if params.is_empty() {
        return Resp::Json(400, err("bad_request", "未提供任何参数；空预设无意义"));
    }

    let path = match presets_path(ctx) {
        Ok(p) => p,
        Err(e) => return cli_error(e),
    };
    // ① 参数归属：必须是模板真正引用的变量（不能只看规格表——规格是可选层）
    let specs = match crate::commands::preset::specs_of(ctx, &template) {
        Ok(s) => s,
        Err(e) => return cli_error_mapped(e),
    };
    let (known, _required) = match crate::commands::preset::template_vars(ctx, &template) {
        Ok(v) => v,
        Err(e) => return cli_error_mapped(e),
    };
    let unknown: Vec<&str> = params
        .values
        .keys()
        .filter(|k| !known.contains(k.as_str()))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        let mut hint: Vec<&str> = known.iter().map(String::as_str).collect();
        hint.sort_unstable();
        return Resp::Json(
            400,
            err(
                "bad_request",
                format!(
                    "参数不属于模板 {}：{}\n该模板使用的变量：{}",
                    template,
                    unknown.join(", "),
                    if hint.is_empty() {
                        "（无）".to_string()
                    } else {
                        hint.join(", ")
                    }
                ),
            ),
        );
    }
    // ② L3 值级校验（与手填值同规，AC-3.9）
    let report = nctool_core::validate::check_param_values(&specs, &params);
    if report.has_errors() {
        return Resp::Json(
            400,
            err(
                "validation",
                format!("参数值校验失败：\n{}", report.summary()),
            ),
        );
    }
    // ③ 同名：默认拒绝，不静默覆盖用户既有预设
    let existing = match crate::commands::preset::load_lenient(&path) {
        Ok(f) => f,
        Err(e) => return cli_error(e),
    };
    if existing.get(&name).is_some() && !force {
        return Resp::Json(
            409,
            err(
                "name_conflict",
                format!("同名预设已存在：{name}。需要覆盖请带 force: true"),
            ),
        );
    }

    let spec_fingerprint = SpecFingerprint::of(&specs);
    let preset = Preset {
        name: name.clone(),
        template: template.clone(),
        params: params.clone(),
        created_at: now_iso8601(),
        spec_fingerprint: spec_fingerprint.clone(),
    };
    let outcome = match PresetStore::upsert(&path, preset) {
        Ok(o) => o,
        Err(e) => return write_error_resp(e),
    };
    Resp::Json(
        200,
        ok(serde_json::json!({
            "name": name,
            "template": template,
            "paramCount": params.len(),
            "path": path.display().to_string(),
            "action": outcome.action,
            "specFingerprint": spec_fingerprint,
            "fileFingerprint": outcome.fingerprint,
        })),
    )
}

/// `POST /api/presets/delete`：删除一个预设。请求体 `{"name"}`。
///
/// 走 **POST 而非 DELETE + 路径段**：预设名允许 `.`/`-` 等字符，放进路径段要
/// 额外处理编码与 `..` 语义；更要紧的是前端 `API` 封装目前只做 GET/POST，
/// 引入新方法会让三处同步面扩大。删除属于**有副作用**的操作，与保存一样
/// 必须经 `PresetStore`（乐观锁 + 原子写），不能直接删文件。
fn presets_delete(ctx: &Ctx, body: &[u8]) -> Resp {
    let parsed: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", format!("请求体不是合法 JSON: {e}"))),
    };
    let name = match parsed.get("name").and_then(|t| t.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Resp::Json(400, err("bad_request", "请求体需要非空字符串字段 \"name\"")),
    };
    let path = match presets_path(ctx) {
        Ok(p) => p,
        Err(e) => return cli_error(e),
    };
    let outcome = match PresetStore::remove(&path, &name) {
        Ok(o) => o,
        Err(e) => return write_error_resp(e),
    };
    Resp::Json(
        200,
        ok(serde_json::json!({
            "name": name,
            "path": path.display().to_string(),
            "action": outcome.action,
            "fileFingerprint": outcome.fingerprint,
        })),
    )
}

// ---------------------------------------------------------------------------
// POST /api/inspect
// ---------------------------------------------------------------------------

/// 变量提取。请求体为 `{"template": "已注册模板名"}`。
///
/// HTTP 层只接受注册表中的逻辑模板名，不接受文件路径或任意源码，避免
/// 将 CLI 的本地文件能力暴露给未认证的 HTTP 客户端。
fn inspect(ctx: &Ctx, body: &[u8]) -> Resp {
    let parsed: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", format!("请求体不是合法 JSON: {e}"))),
    };
    let tpl = match parsed.get("template").and_then(|t| t.as_str()) {
        Some(tpl) if !tpl.is_empty() => tpl,
        _ => {
            return Resp::Json(
                400,
                err("bad_request", "请求体需要非空字符串字段 \"template\""),
            )
        }
    };
    let gen = match ctx.build_registry() {
        Ok(g) => g,
        Err(e) => return internal_error(e),
    };
    let entry = match gen.registry().get(tpl) {
        Some(entry) => entry,
        None => return Resp::Json(404, err("template_not_found", format!("模板不存在: {tpl}"))),
    };
    let system_vars = gen.registry().system_vars().to_vec();
    let vars = match extract_variables(&entry.source_text, &entry.name, &system_vars) {
        Ok(v) => v,
        Err(err) => return cli_error(err),
    };
    let v = vars_json(&vars);
    Resp::Json(
        200,
        ok(serde_json::json!({
            "template": entry.name,
            "issues": [],
            "required": v["required"],
            "optional": v["optional"],
        })),
    )
}

// ---------------------------------------------------------------------------
// HTTP 粘合
// ---------------------------------------------------------------------------

/// 解析并验证监听地址：本地 UI 只允许绑定回环地址。
pub fn listen_addr(host: &str, port: u16) -> Result<SocketAddr, CliError> {
    let ip: IpAddr = host
        .parse()
        .map_err(|_| CliError::new("args", format!("监听地址必须是 IP 地址: {host}")))?;
    if !ip.is_loopback() {
        return Err(CliError::new(
            "args",
            format!("为安全起见，UI 仅允许绑定回环地址，拒绝: {host}"),
        ));
    }
    Ok(SocketAddr::new(ip, port))
}

/// 将监听地址格式化为浏览器可用 URL（IPv6 使用方括号）。
pub fn browser_url(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

/// 绑定监听地址，返回服务实例与**实际生效的地址**。
///
/// 与 [`serve`] 分开是刻意的：调用方必须先确认绑定成功，再去开浏览器 ——
/// 端口被占用时先弹浏览器只会打开一个死页（P2-24）。另外 `--port 0` 时端口由
/// 内核分配，必须回读 `server_addr()`，否则横幅与浏览器都会指向 `:0`。
pub fn bind(addr: SocketAddr) -> Result<(tiny_http::Server, SocketAddr), CliError> {
    let server = tiny_http::Server::http(addr)
        .map_err(|e| CliError::new("io", format!("绑定 {addr} 失败: {e}")))?;
    let actual = server.server_addr().to_ip().unwrap_or(addr);
    Ok((server, actual))
}

/// 处理请求直到进程退出（Ctrl-C 终止）。
///
/// 接收**已绑定**的实例与它的实际地址：本函数不打印启动横幅（那要等回读端口
/// 之后才准确），也不负责开浏览器，两者都由 [`crate::commands::ui`] 在绑定
/// 成功之后完成。
pub fn serve(server: tiny_http::Server, addr: SocketAddr, ctx: Ctx) -> Result<(), CliError> {
    let allowed = allowed_origins(&addr);

    for mut request in server.incoming_requests() {
        let method = request.method().as_str().to_ascii_uppercase();
        let url = request.url().to_string();
        let (path, query) = match url.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (url, String::new()),
        };
        let headers: Vec<(&str, &str)> = request
            .headers()
            .iter()
            .map(|h| (h.field.as_str().as_str(), h.value.as_str()))
            .collect();

        let resp = match (method.as_str(), path.as_str()) {
            ("GET", "/") | ("GET", "/index.html") => Resp::Html,
            _ => {
                // 跨站防护先于路由：被拒绝的请求不必再读请求体、不必建注册表
                let blocked = path
                    .starts_with("/api/")
                    .then(|| cross_site_guard(&headers, &allowed))
                    .flatten();
                if let Some(resp) = blocked {
                    resp
                } else {
                    let mut body = Vec::new();
                    let too_large = request
                        .as_reader()
                        .take(MAX_BODY_BYTES as u64 + 1)
                        .read_to_end(&mut body)
                        .is_ok()
                        && body.len() > MAX_BODY_BYTES;
                    if too_large {
                        Resp::Json(413, err("payload_too_large", "请求体超过 1 MiB 上限"))
                    } else {
                        route(&ctx, &method, &path, &query, &body)
                    }
                }
            }
        };

        let response = match resp {
            Resp::Html => with_security_headers(
                tiny_http::Response::from_string(UI_HTML).with_header(
                    tiny_http::Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"text/html; charset=utf-8"[..],
                    )
                    .expect("静态 Content-Type 头合法"),
                ),
            ),
            Resp::Json(status, payload) => {
                let data = serde_json::to_string(&payload).unwrap_or_else(|_| {
                    String::from(
                        r#"{"ok":false,"error":{"kind":"internal","message":"序列化失败"}}"#,
                    )
                });
                with_security_headers(
                    tiny_http::Response::from_string(data)
                        .with_status_code(tiny_http::StatusCode(status))
                        .with_header(
                            tiny_http::Header::from_bytes(
                                &b"Content-Type"[..],
                                &b"application/json; charset=utf-8"[..],
                            )
                            .expect("静态 Content-Type 头合法"),
                        ),
                )
            }
        };
        let _ = request.respond(response);
    }
    Ok(())
}

/// 附加 [`SECURITY_HEADERS`]：所有响应一视同仁，包括错误响应
/// （403/404/500 同样不该成为绕过 CSP 的口子）。
fn with_security_headers<R: std::io::Read>(
    mut resp: tiny_http::Response<R>,
) -> tiny_http::Response<R> {
    for (name, value) in SECURITY_HEADERS {
        let header = tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes())
            .unwrap_or_else(|_| panic!("安全头 {name} 的键值应为合法 ASCII"));
        resp = resp.with_header(header);
    }
    resp
}

/// 内部错误（注册表构建失败等）：映射为 500 + CLI 错误信息。
/// 500：**不回显内部错误正文**。
///
/// `build_registry` 的失败消息里含模板文件的绝对路径（形如
/// `读取模板失败 \\?\C:\Users\<用户名>\Desktop\...\a.j2: …`），原样回给浏览器
/// 等于把用户名与目录结构一并泄露，而这也不是调用方能处理的信息（P1-17）。
/// 详情写 stderr —— 本服务是本地工具，用户就坐在启动它的那个终端前面，看得到。
fn internal_error(e: CliError) -> Resp {
    eprintln!("error: {e}");
    Resp::Json(
        500,
        err("internal", "服务内部错误，详情见运行 nctool ui 的终端输出"),
    )
}

/// 业务校验错误（模板解析失败等）：400 + CLI 错误信息。
///
/// 与 500 不同，这里回显正文：消息描述的是**调用方传进来的模板**哪里有问题
/// （语法错误、行列号），是调用方能据以修正的信息，且不含磁盘路径。
fn cli_error(e: CliError) -> Resp {
    Resp::Json(400, err(e.kind, e.message))
}

/// CLI 错误 → HTTP 响应，且**按 kind 校正状态码**。
///
/// `cli_error` 一律给 400；但 `template_not_found` 在服务端其它三处
/// （`template_detail` / `registered_template` / `inspect`）都是 **404**。
/// 若 `/api/presets` 走 `cli_error`，同一个 kind 会因端点不同而给出两种状态码
/// —— 前端按状态码分支时必然错判。这里只为需要的 kind 做纠正，其余仍是 400。
fn cli_error_mapped(e: CliError) -> Resp {
    match e.kind {
        "template_not_found" | "machine_not_found" | "preset_not_found" => {
            Resp::Json(404, err(e.kind, e.message))
        }
        _ => cli_error(e),
    }
}

/// 写内核错误 → HTTP 响应。
///
/// 状态码按**语义**而非一律 400：乐观锁冲突是 409（可重试），越界 / 名称非法是
/// 400（调用方能改），IO / 内容损坏是 500（本地环境或磁盘问题，调用方无从修正，
/// 且正文可能含绝对路径，按 [`internal_error`] 的口径不回显细节）。
///
/// `WriteError` 是 `#[non_exhaustive]`：兜底归 500，不静默降级成 400。
fn write_error_resp(e: WriteError) -> Resp {
    match e {
        WriteError::Conflict { path, .. } => Resp::Json(
            409,
            err(
                "write_conflict",
                format!("写入冲突：{} 已被外部修改，未覆盖。请重试", path.display()),
            ),
        ),
        WriteError::PathEscape { rel, reason } => {
            if reason.contains("已存在") {
                Resp::Json(409, err("name_conflict", format!("{reason}：{rel}")))
            } else {
                Resp::Json(
                    400,
                    err("bad_request", format!("预设名非法：{rel}（{reason}）")),
                )
            }
        }
        WriteError::ReadOnly { path } => {
            eprintln!("error: 预设文件只读或无写入权限: {}", path.display());
            Resp::Json(500, err("internal", "目标文件只读或无写入权限"))
        }
        // "预设不存在"：调用方问题，404（**不是** 500 —— 早期因为它被塞进
        // `Corrupt` 分支而报成服务端内部错误）
        WriteError::NotFound(m) => Resp::Json(404, err("preset_not_found", m)),
        WriteError::Corrupt(m) => {
            eprintln!("error: 预设文件损坏: {m}");
            Resp::Json(500, err("internal", "预设文件内容损坏，详情见服务终端输出"))
        }
        WriteError::Io(e) => {
            eprintln!("error: 预设文件读写失败: {e}");
            Resp::Json(500, err("internal", "预设文件读写失败"))
        }
        // `#[non_exhaustive]`：新增变体一律归 500（服务端问题），不猜 400。
        _ => Resp::Json(500, err("internal", "预设文件读写失败")),
    }
}

// ---------------------------------------------------------------------------
// URL 工具
// ---------------------------------------------------------------------------

/// 解析 query 串为键值对（`+` 与 `%XX` 均解码）。
fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|pair| {
            let mut it = pair.splitn(2, '=');
            let k = percent_decode(it.next().unwrap_or(""), true);
            let v = percent_decode(it.next().unwrap_or(""), true);
            (k, v)
        })
        .collect()
}

/// 百分号解码。`plus_as_space` 仅用于 query 值；路径中 `+` 是合法字面字符。
fn percent_decode(input: &str, plus_as_space: bool) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    out.push(h * 16 + l);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' if plus_as_space => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    fn test_ctx() -> Ctx {
        Ctx::for_test()
    }

    fn origins() -> Vec<String> {
        allowed_origins(&"127.0.0.1:8787".parse().unwrap())
    }

    #[test]
    fn same_origin_is_allowed() {
        for origin in [
            "http://127.0.0.1:8787",
            "http://localhost:8787",
            "http://127.0.0.1:8787/",
        ] {
            assert!(
                cross_site_guard(&[("origin", origin)], &origins()).is_none(),
                "同源应放行: {origin}"
            );
        }
    }

    #[test]
    fn foreign_origin_is_rejected() {
        // 浏览器里任意页面都能向 127.0.0.1:<port> 发请求，Origin 是唯一可信凭据
        let Some(Resp::Json(status, payload)) =
            cross_site_guard(&[("origin", "http://evil.example")], &origins())
        else {
            panic!("跨站 Origin 应被拒绝")
        };
        assert_eq!(status, 403);
        assert_eq!(payload["error"]["kind"], "forbidden_origin");
        assert!(
            payload["error"]["message"]
                .as_str()
                .unwrap()
                .contains("evil.example"),
            "报错要回显被拒的 Origin: {payload}"
        );
    }

    #[test]
    fn sec_fetch_site_is_checked() {
        assert!(cross_site_guard(&[("sec-fetch-site", "same-origin")], &origins()).is_none());
        // 直接地址栏访问（无来源页面）也是允许的
        assert!(cross_site_guard(&[("sec-fetch-site", "none")], &origins()).is_none());
        assert!(
            cross_site_guard(&[("sec-fetch-site", "cross-site")], &origins()).is_some(),
            "cross-site 应被拒绝"
        );
    }

    #[test]
    fn non_browser_client_without_origin_is_allowed() {
        // curl 等不带浏览器专属头——本服务刻意保留"命令行直接调 API"的用法
        assert!(cross_site_guard(&[], &origins()).is_none());
        assert!(cross_site_guard(&[("user-agent", "curl/8.0")], &origins()).is_none());
    }

    #[test]
    fn allowed_origins_covers_loopback_aliases() {
        let allowed = origins();
        for want in [
            "http://127.0.0.1:8787",
            "http://localhost:8787",
            "http://[::1]:8787",
        ] {
            assert!(
                allowed.contains(&want.to_string()),
                "缺少 {want}: {allowed:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // /api/presets
    // -----------------------------------------------------------------------

    /// 预设端点的隔离环境：模板目录与配置目录都指向临时区。
    ///
    /// **必须**把配置目录也隔离掉：`default_preset_path()` 读的是 `APPDATA`
    /// （见 core::asset::preset），不隔离的话测试会写进用户真实的
    /// `%APPDATA%\nctool\presets.yaml` —— 跑一次测试毁一次用户的预设。
    ///
    /// # 为什么要串行
    ///
    /// `APPDATA` 是**进程级**变量，而 cargo 默认让同一 target 的测试**并行**跑在
    /// 多个线程里。于是 A 测试设的 `APPDATA` 会被 B 测试覆盖，而 A 后续的写盘就
    /// 落进了 B 的临时目录 —— 表现为断言拿到"另一个环境的数据"。
    ///
    /// 实测踩到：坏文件测试写完坏内容后 `warnings` 为空，因为它的写落到了
    /// 另一测试的 `..._list_empty_2` 目录里。修法是**持锁跑完整个测试体**
    /// （不是只在建环境时持锁），见 [`preset_endpoint_test`]。
    struct PresetEnv {
        work: PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    /// 全局串行锁：所有改动 `APPDATA` 的测试都必须经此获取环境。
    static PRESET_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 取锁 + 建隔离环境，返回的 `PresetEnv` 在 drop 时释放锁。
    ///
    /// **所有**触碰预设端点的测试都必须走这个入口，不要直接 `PresetEnv::new`
    /// —— `new` 只负责建目录，本身不持锁，单独用会在并行测试下相互踩 env。
    fn preset_endpoint_test(tag: &str) -> PresetEnv {
        // 锁中毒（前一个测试 panic）时照样继续：宁可后续断言失败，
        // 也不要让所有用例一起挂掉而看不见真正的原因。
        let guard = PRESET_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        PresetEnv::new_locked(tag, guard)
    }

    impl PresetEnv {
        /// 建目录 + 改 env。`guard` 由调用方持有，保证整个测试体串行。
        fn new_locked(tag: &str, guard: std::sync::MutexGuard<'static, ()>) -> Self {
            static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let work = std::env::temp_dir().join(format!(
                "nctool_apitest_{}_{}_{}",
                std::process::id(),
                tag,
                n
            ));
            let _ = std::fs::remove_dir_all(&work);
            let root = work.join("templates");
            std::fs::create_dir_all(&root).expect("建模板目录");
            std::fs::write(
                root.join("t.j2"),
                "G0 X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }} F{{ feed | default(100) }}\nM30\n",
            )
            .expect("写模板");
            // 配置目录隔离：APPDATA（Windows）/ XDG_CONFIG_HOME（其它）
            let cfg = work.join("config");
            std::fs::create_dir_all(&cfg).expect("建配置目录");
            std::env::set_var("APPDATA", &cfg);
            std::env::set_var("XDG_CONFIG_HOME", &cfg);
            std::env::set_var("HOME", &cfg);
            std::env::set_var("USERPROFILE", &cfg);
            PresetEnv {
                work,
                _guard: guard,
            }
        }

        /// 建一个 `template_dir` 指向临时模板根的 ctx。
        fn ctx(&self) -> Ctx {
            let mut ctx = Ctx::for_test();
            ctx.template_dir = Some(self.work.join("templates"));
            ctx
        }

        fn body(&self, name: &str, params: &str, force: bool) -> String {
            format!(r#"{{"name":"{name}","template":"t.j2","params":{params},"force":{force}}}"#)
        }
    }

    fn json_body(v: &serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(v).unwrap()
    }

    /// GET 空文件：合法返回空列表，不是错误。
    #[test]
    fn presets_list_empty_is_ok() {
        let env = preset_endpoint_test("list_empty");
        let Resp::Json(status, payload) = route(&env.ctx(), "GET", "/api/presets", "", &[]) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200);
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["data"]["presets"].as_array().unwrap().len(), 0);
        assert!(payload["data"]["warnings"].as_array().unwrap().is_empty());
    }

    /// 保存 → 列表：**结构化陈旧字段**必须齐备（D7：消费方不解析文本）。
    #[test]
    fn presets_save_then_list_has_structured_stale_fields() {
        let env = preset_endpoint_test("save_list");
        let ctx = env.ctx();
        let body = env.body("p1", r#"{"x":21.0,"y":15.0}"#, false);
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets", "", body.as_bytes())
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");
        assert_eq!(payload["data"]["name"], "p1");
        assert_eq!(payload["data"]["paramCount"], 2);
        // `WriteAction` 派生 `#[serde(rename_all = "lowercase")]`：契约是小写，
        // 与 camelCase 的 `specFingerprint` 并存（前者是 core 的既有契约，不动它）
        assert_eq!(payload["data"]["action"], "created");
        // 两个指纹必须都给出且语义不同（规格基线 vs 文件乐观锁）
        assert!(payload["data"]["specFingerprint"]
            .as_str()
            .unwrap()
            .starts_with("fnv1a64:"));
        assert!(payload["data"]["fileFingerprint"].as_str().is_some());

        let Resp::Json(status, payload) = route(&ctx, "GET", "/api/presets", "", &[]) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200);
        let one = &payload["data"]["presets"][0];
        assert_eq!(one["name"], "p1");
        assert_eq!(one["paramCount"], 2);
        assert_eq!(one["resolvable"], true);
        assert_eq!(one["stale"], false);
        assert!(one["staleParams"].is_array());
        assert!(one["missingRequired"].is_array());
    }

    /// 未带 force 的同名保存 → 409 + `name_conflict`（不静默覆盖）。
    #[test]
    fn presets_save_duplicate_is_409() {
        let env = preset_endpoint_test("dup");
        let ctx = env.ctx();
        let body = env.body("p1", r#"{"x":21.0,"y":15.0}"#, false);
        let _ = route(&ctx, "POST", "/api/presets", "", body.as_bytes());
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets", "", body.as_bytes())
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 409);
        assert_eq!(payload["error"]["kind"], "name_conflict");
        // force=true 才覆盖
        let forced = env.body("p1", r#"{"x":30.0,"y":15.0}"#, true);
        let Resp::Json(status, payload) =
            route(&ctx, "POST", "/api/presets", "", forced.as_bytes())
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");
        assert_eq!(payload["data"]["action"], "updated");
    }

    /// 参数不属于模板 → 400（与 CLI 同一判据：模板引用变量，不是规格表）。
    #[test]
    fn presets_save_rejects_unknown_param() {
        let env = preset_endpoint_test("unknown");
        let body = env.body("bad", r#"{"x":21.0,"nope":1.0}"#, false);
        let Resp::Json(status, payload) =
            route(&env.ctx(), "POST", "/api/presets", "", body.as_bytes())
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 400);
        assert_eq!(payload["error"]["kind"], "bad_request");
        assert!(
            payload["error"]["message"]
                .as_str()
                .unwrap()
                .contains("nope"),
            "{payload}"
        );
    }

    /// 空参数 → 400；模板不存在 → 404。
    #[test]
    fn presets_save_rejects_empty_params_and_missing_template() {
        let env = preset_endpoint_test("empty");
        let ctx = env.ctx();
        let body = env.body("e", "{}", false);
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets", "", body.as_bytes())
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 400, "{payload}");

        let missing = r#"{"name":"m","template":"no_such.j2","params":{"x":1.0},"force":false}"#;
        let Resp::Json(status, payload) =
            route(&ctx, "POST", "/api/presets", "", missing.as_bytes())
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 404, "{payload}");
        assert_eq!(payload["error"]["kind"], "template_not_found");
    }

    /// 删除：存在则 200，不存在则报错（不静默成功）。
    ///
    /// **每个场景各用一个隔离环境**：`PresetEnv` 改的是进程级 env（APPDATA），
    /// 同一测试里复用一个环境会让"上一步写坏的文件"影响下一步。此前正是在一个
    /// 环境里连做多步，第二步的 500 来自第一步留下的坏文件，把真正的断言带偏。
    #[test]
    fn presets_delete_roundtrip() {
        let env = preset_endpoint_test("del");
        let ctx = env.ctx();
        let body = env.body("doomed", r#"{"x":21.0,"y":15.0}"#, false);
        let _ = route(&ctx, "POST", "/api/presets", "", body.as_bytes());

        let del = json_body(&serde_json::json!({ "name": "doomed" }));
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets/delete", "", &del)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");
        assert_eq!(payload["data"]["action"], "deleted");

        // 再删一次：404 preset_not_found（**不是** 500 —— 这是调用方问题）
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets/delete", "", &del)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 404, "删不存在的预设应为 404: {payload}");
        assert_eq!(payload["error"]["kind"], "preset_not_found");

        // 列表确认为空
        let Resp::Json(_, payload) = route(&ctx, "GET", "/api/presets", "", &[]) else {
            panic!("应返回 JSON")
        };
        assert_eq!(payload["data"]["presets"].as_array().unwrap().len(), 0);
    }

    /// `?template=` 过滤生效。
    #[test]
    fn presets_list_filters_by_template() {
        let env = preset_endpoint_test("filter");
        let ctx = env.ctx();
        let body = env.body("p1", r#"{"x":21.0,"y":15.0}"#, false);
        let _ = route(&ctx, "POST", "/api/presets", "", body.as_bytes());

        let Resp::Json(_, payload) = route(&ctx, "GET", "/api/presets", "template=t.j2", &[])
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(payload["data"]["presets"].as_array().unwrap().len(), 1);

        let Resp::Json(_, other) = route(&ctx, "GET", "/api/presets", "template=u.j2", &[]) else {
            panic!("应返回 JSON")
        };
        assert_eq!(other["data"]["presets"].as_array().unwrap().len(), 0);
    }

    /// 红线 9：预设文件**不得**落进模板根 —— HTTP 侧与 CLI 侧同一判据。
    ///
    /// 这条是回归守卫：当年 `preset_path` 的条件式校验让红线静默失效过一次。
    #[test]
    fn presets_path_refuses_inside_template_root() {
        let env = preset_endpoint_test("r9");
        let mut ctx = env.ctx();
        // 让配置目录本身就落在模板根内 → 默认预设路径必然命中红线
        ctx.template_dir = Some(env.work.join("config"));
        let Resp::Json(status, payload) = route(&ctx, "GET", "/api/presets", "", &[]) else {
            panic!("应返回 JSON")
        };
        assert_ne!(status, 200, "预设文件落在模板根内应被拒");
        assert!(
            payload["error"]["message"]
                .as_str()
                .unwrap()
                .contains("不得落在模板目录内"),
            "{payload}"
        );
    }

    /// 坏文件降级：只读命令给警告 + 空列表，**不是** 500（D13）。
    #[test]
    fn presets_corrupt_file_degrades_read_but_refuses_write() {
        let env = preset_endpoint_test("corrupt");
        let ctx = env.ctx();
        let path = nctool_core::asset::default_preset_path();
        // 父目录此时可能还不存在（预设文件尚未被创建过）——先建再写坏内容
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, "version: 1\npresets: [ {unclosed").unwrap();

        let Resp::Json(status, payload) = route(&ctx, "GET", "/api/presets", "", &[]) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "只读命令不应被坏文件拦住: {payload}");
        assert!(payload["data"]["presets"].as_array().unwrap().is_empty());
        assert!(
            !payload["data"]["warnings"].as_array().unwrap().is_empty(),
            "降级必须带警告，不能静默丢: {payload}"
        );

        let body = env.body("w", r#"{"x":21.0,"y":15.0}"#, false);
        let Resp::Json(status, _payload) = route(&ctx, "POST", "/api/presets", "", body.as_bytes())
        else {
            panic!("应返回 JSON")
        };
        assert_ne!(status, 200, "坏文件上不得写入（会毁掉用户数据）");
    }

    #[test]
    fn route_health() {
        let Resp::Json(status, payload) = route(&test_ctx(), "GET", "/health", "", &[]) else {
            panic!("/health 应返回 JSON")
        };
        assert_eq!(status, 200);
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["data"]["status"], "ok");
    }

    /// 守卫（第四轮 P0-2）：后端必须真的路由得了 `scripts/api_routes.json`
    /// 登记的每一条接口。
    ///
    /// 与 `scripts/check_api_parity.mjs`（前端侧消费同一份 fixture）合成闭环：
    /// 新增端点漏改后端路由臂 → 本测试红；漏改前端封装或 fixture → 那边红。
    /// 断言只看「是不是未知接口」而不看状态码：fixture 里用的是最小请求体，
    /// 400（参数不全）是正常的，404 + "未知接口" 才是契约漂移。
    #[test]
    fn api_routes_are_routable() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../scripts/api_routes.json"))
                .expect("api_routes.json 必须是合法 JSON");
        let routes = fixture["routes"]
            .as_array()
            .expect("fixture 顶层必须有 routes 数组");
        assert!(!routes.is_empty(), "fixture 的 routes 不应为空");

        for (i, r) in routes.iter().enumerate() {
            let method = r["method"]
                .as_str()
                .unwrap_or_else(|| panic!("route #{i}: method 缺失"));
            let path = r["path"]
                .as_str()
                .unwrap_or_else(|| panic!("route #{i}: path 缺失"));
            let query = r["query"].as_str().unwrap_or("");
            let body = r["body"].as_str().unwrap_or("");

            let Resp::Json(status, payload) =
                route(&test_ctx(), method, path, query, body.as_bytes())
            else {
                panic!("{method} {path} 应返回 JSON");
            };
            let msg = payload["error"]["message"].as_str().unwrap_or_default();
            assert!(
                !msg.starts_with("未知接口"),
                "route #{i}: {method} {path} 未登记到 route()（status={status}, msg={msg}）\n\
                 新增端点请同时改：后端路由臂 + scripts/api_routes.json + 前端封装"
            );
        }
    }

    /// 回归（第四轮 P2-24）：`--port 0` 时端口由内核分配，`bind` 必须回读**实际**
    /// 地址 —— 否则启动横幅与 `--open` 打开的浏览器都会指向 `:0`。
    #[test]
    fn bind_with_port_zero_reports_actual_port() {
        let (srv, actual) = bind("127.0.0.1:0".parse().unwrap()).expect("绑定回环应成功");
        assert_ne!(actual.port(), 0, "应回读内核分配的实际端口");
        assert_eq!(actual.ip().to_string(), "127.0.0.1");
        drop(srv);
    }

    /// 回归（第四轮 P2-24）：绑定失败必须返回 `Err` 而不是 panic，且错误可读 ——
    /// `ui::run` 依赖它在**开浏览器之前**暴露端口占用。
    #[test]
    fn bind_reports_error_instead_of_panicking() {
        // 192.0.2.0/24 是 TEST-NET-1，本机不可绑定，用于稳定触发失败路径
        // 不用 `expect_err`：`tiny_http::Server` 未实现 Debug，`Ok` 分支无法打印
        let err = match bind("192.0.2.1:8787".parse().unwrap()) {
            Err(e) => e,
            Ok(_) => panic!("不可绑定的地址应返回 Err"),
        };
        assert_eq!(err.kind, "io");
        assert!(
            err.message.contains("绑定"),
            "错误应说明是绑定失败: {}",
            err.message
        );
    }

    #[test]
    fn route_templates_list() {
        let Resp::Json(status, payload) = route(&test_ctx(), "GET", "/api/templates", "", &[])
        else {
            panic!("模板列表应返回 JSON")
        };
        assert_eq!(status, 200);
        assert_eq!(payload["ok"], true);
        assert!(payload["data"]["templates"]
            .as_array()
            .is_some_and(|items| { items.iter().any(|item| item["name"] == "drill_cycle") }));
    }

    #[test]
    fn route_template_detail_and_not_found() {
        let Resp::Json(status, payload) =
            route(&test_ctx(), "GET", "/api/templates/drill_cycle", "", &[])
        else {
            panic!("模板详情应返回 JSON")
        };
        assert_eq!(status, 200);
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["data"]["template"]["name"], "drill_cycle");

        let Resp::Json(status, payload) = route(
            &test_ctx(),
            "GET",
            "/api/templates/no_such_template",
            "",
            &[],
        ) else {
            panic!("不存在模板应返回 JSON")
        };
        assert_eq!(status, 404);
        assert_eq!(payload["error"]["kind"], "template_not_found");
    }

    #[test]
    fn route_machines() {
        let Resp::Json(status, payload) = route(&test_ctx(), "GET", "/api/machines", "", &[])
        else {
            panic!("机床列表应返回 JSON")
        };
        assert_eq!(status, 200);
        assert_eq!(payload["ok"], true);
        assert!(payload["data"]["machines"]
            .as_array()
            .is_some_and(|items| { items.iter().any(|item| item["id"] == "generic") }));
    }

    #[test]
    fn route_inspect() {
        let body = br#"{"template":"drill_cycle"}"#;
        let Resp::Json(status, payload) = route(&test_ctx(), "POST", "/api/inspect", "", body)
        else {
            panic!("inspect 应返回 JSON")
        };
        assert_eq!(status, 200);
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["data"]["template"], "drill_cycle");
        assert_eq!(payload["data"]["issues"], serde_json::json!([]));

        let body = br#"{"template":"../Cargo.toml"}"#;
        let Resp::Json(status, payload) = route(&test_ctx(), "POST", "/api/inspect", "", body)
        else {
            panic!("路径形式应返回 JSON")
        };
        assert_eq!(status, 404);
        assert_eq!(payload["error"]["kind"], "template_not_found");
    }

    #[test]
    fn route_validate_and_render() {
        let body = br#"{"template":"drill_cycle","params":{"x":21,"y":15,"depth":-10,"feed":100}}"#;
        let Resp::Json(status, payload) = route(&test_ctx(), "POST", "/api/validate", "", body)
        else {
            panic!("validate 应返回 JSON")
        };
        assert_eq!(status, 200);
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["data"]["report"]["ok"], true);

        let Resp::Json(status, payload) = route(&test_ctx(), "POST", "/api/render", "", body)
        else {
            panic!("render 应返回 JSON")
        };
        assert_eq!(status, 200);
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["data"]["blocked"], false);
        assert!(payload["data"]["output"]
            .as_str()
            .is_some_and(|s| s.contains("X21.000")));
    }

    #[test]
    fn route_rejects_invalid_category() {
        let Resp::Json(status, payload) = route(
            &test_ctx(),
            "GET",
            "/api/templates",
            "category=unknown",
            &[],
        ) else {
            panic!("非法分类应返回 JSON")
        };
        assert_eq!(status, 400);
        assert_eq!(payload["error"]["kind"], "bad_request");
    }

    #[test]
    fn parse_category_covers_every_core_variant() {
        // 回归（P1-8）：`parse_category` 曾缺 `grooving` / `切槽`，而
        // `cli/src/cli.rs::CategoryArg` 有 —— HTTP 侧把「切槽」当非法值 400，
        // CLI 侧正常，两侧对同一分类给出不同答案；前端分类栏也缺一项，
        // 该分类的模板只在「全部」里出现、计数永远对不上。
        // 遍历 core 的全部分类逐个断言：将来新增分类时这里先红，
        // 而不是等用户报「某个分类的模板就是不显示」。
        // 遍历 core 的 ALL（而非硬编码清单）：新增分类时这里立刻红
        for c in TemplateCategory::ALL {
            assert_eq!(
                parse_category(c.label()),
                Some(c),
                "中文显示名必须可解析：{}",
                c.label()
            );
        }
        // 英文 id 同样要通
        assert_eq!(parse_category("grooving"), Some(TemplateCategory::Grooving));
    }

    #[test]
    fn empty_category_query_means_no_filter() {
        // 回归（P1-8）：`?category=`（空值）此前被判为非法分类返回 400，
        // 前端清空分类筛选就会撞上 —— 「不筛选」被当成了「筛一个不存在的分类」。
        let Resp::Json(status, payload) =
            route(&test_ctx(), "GET", "/api/templates", "category=", &[])
        else {
            panic!("空分类应返回 JSON")
        };
        assert_eq!(status, 200, "空分类应视为不筛选：{payload}");
    }

    #[test]
    fn render_options_format_must_be_string() {
        // 回归（P1-9）：`and_then(as_str)` 把「键缺失」与「类型错误」混为一谈，
        // `{"format": 1}` / `true` / `[]` 都落到 None 分支被**静默按 gcode 生成**，
        // 调用方以为选项生效。同函数的 get_bool / get_u32 已对类型错误返回 400，
        // 此处是唯一漏网（09-05 OPTIONS-SILENT-FALLBACK-001 的残留）。
        for bad in ["1", "true", "[]", "{}"] {
            let body = format!(
                r#"{{"template":"drill_cycle","params":{{}},"options":{{"format":{bad}}}}}"#
            );
            let Resp::Json(status, payload) =
                route(&test_ctx(), "POST", "/api/render", "", body.as_bytes())
            else {
                panic!("类型错误的 format 应返回 JSON")
            };
            assert_eq!(
                status, 400,
                "format={bad} 必须被拒绝而非静默回退：{payload}"
            );
        }
        // 合法的 format 不受影响
        let ok_body = br#"{"template":"drill_cycle","params":{},"options":{"format":"text"}}"#;
        let Resp::Json(status, _) = route(&test_ctx(), "POST", "/api/render", "", ok_body) else {
            panic!("应返回 JSON")
        };
        assert_ne!(status, 400, "合法的 format 不应被拒绝");
    }

    #[test]
    fn route_rejects_invalid_render_options() {
        let body = br#"{"template":"drill_cycle","params":{},"options":{"lineNumbers":"true"}}"#;
        let Resp::Json(status, payload) = route(&test_ctx(), "POST", "/api/render", "", body)
        else {
            panic!("非法 options 应返回 JSON")
        };
        assert_eq!(status, 400);
        assert_eq!(payload["error"]["kind"], "bad_request");

        let body = br#"{"template":"drill_cycle","params":{},"options":{"format":"unknown"}}"#;
        let Resp::Json(status, _payload) = route(&test_ctx(), "POST", "/api/render", "", body)
        else {
            panic!("非法 format 应返回 JSON")
        };
        assert_eq!(status, 400);
    }

    #[test]
    fn route_unknown_path() {
        let Resp::Json(status, payload) = route(&test_ctx(), "GET", "/api/no_such", "", &[]) else {
            panic!("未知路由应返回 JSON")
        };
        assert_eq!(status, 404);
        assert_eq!(payload["error"]["kind"], "not_found");
    }

    /// 三个端点共用同一套入参校验：缺 `template` / 空串 / 类型不对都要 400。
    /// 若某个端点漏了这道校验，它会把 `""` 当模板名去查注册表 → 404 而非 400，
    /// 前端拿到的错误分类就错了。
    #[test]
    fn api_requires_non_empty_template_field() {
        for path in ["/api/validate", "/api/render", "/api/inspect"] {
            for body in [
                &br#"{}"#[..],
                &br#"{"template":""}"#[..],
                &br#"{"template":123}"#[..],
            ] {
                let Resp::Json(status, payload) = route(&test_ctx(), "POST", path, "", body) else {
                    panic!("{path} 应返回 JSON")
                };
                let shown = String::from_utf8_lossy(body);
                assert_eq!(status, 400, "{path} body={shown}");
                assert_eq!(
                    payload["error"]["kind"], "bad_request",
                    "{path} body={shown}"
                );
            }
        }
    }

    #[test]
    fn unknown_template_is_404() {
        let body = br#"{"template":"no_such_template"}"#;
        for path in ["/api/validate", "/api/render", "/api/inspect"] {
            let Resp::Json(status, payload) = route(&test_ctx(), "POST", path, "", body) else {
                panic!("{path} 应返回 JSON")
            };
            assert_eq!(status, 404, "{path}");
            assert_eq!(payload["error"]["kind"], "template_not_found", "{path}");
        }
    }

    #[test]
    fn render_unknown_machine_is_404() {
        let body = br#"{"template":"drill_cycle","params":{"x":1,"y":1,"depth":-1,"feed":100},"machine":"no_such_machine"}"#;
        let Resp::Json(status, payload) = route(&test_ctx(), "POST", "/api/render", "", body)
        else {
            panic!("render 应返回 JSON")
        };
        assert_eq!(status, 404, "机床不存在应是 404 而不是 400: {payload}");
        assert_eq!(payload["error"]["kind"], "machine_not_found");
    }

    /// 严格模式下校验未通过 → `blocked: true` 且**不产出任何 G-code**；
    /// 宽松模式下同一份入参放行。前端靠 `blocked` 决定是展示报告还是展示程序。
    #[test]
    fn render_blocks_on_errors_unless_lenient() {
        let Resp::Json(status, payload) = route(
            &test_ctx(),
            "POST",
            "/api/render",
            "",
            br#"{"template":"drill_cycle","params":{}}"#,
        ) else {
            panic!("render 应返回 JSON")
        };
        assert_eq!(status, 200);
        assert_eq!(payload["data"]["blocked"], true);
        assert_eq!(payload["data"]["output"], "", "被拦截时不得产出 G-code");
        assert_eq!(payload["data"]["machine"], "generic");

        let valid =
            br#"{"template":"drill_cycle","params":{"x":21,"y":15,"depth":-10,"feed":100}}"#;
        let Resp::Json(status, payload) = route(&test_ctx(), "POST", "/api/render", "", valid)
        else {
            panic!("render 应返回 JSON")
        };
        assert_eq!(status, 200);
        assert_eq!(payload["data"]["blocked"], false);
        assert_eq!(payload["data"]["template"], "drill_cycle");
    }

    #[test]
    fn generation_options_full_set_is_applied() {
        // 所有开关都给上（format 走默认 gcode）：覆盖 get_u32 取值路径与完整
        // options 构造。注意 `format: "text"` 是"不带行号的纯文本"，
        // 与 `gcode` 是两种后处理，见下一个测试。
        let body = br#"{"template":"drill_cycle","params":{"x":21,"y":15,"depth":-10,"feed":100},"options":{"lineNumbers":true,"lineStep":5,"maxLine":999,"addHeader":true,"stripBlank":true,"ascii":true}}"#;
        let Resp::Json(status, payload) = route(&test_ctx(), "POST", "/api/render", "", body)
        else {
            panic!("render 应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");
        let out = payload["data"]["output"].as_str().expect("应有 output");

        // 行号步长必须真的生效：按 N<数字> 抽出编号，相邻差值应恒为 5。
        // （不断言具体位宽——位宽来自机床配置，属于另一条契约）
        let nums: Vec<u32> = out
            .split_whitespace()
            .filter_map(|t| t.strip_prefix('N'))
            .filter_map(|t| t.parse().ok())
            .collect();
        assert!(nums.len() >= 2, "应有多行带行号: {out}");
        assert!(
            nums.windows(2).all(|w| w[1] - w[0] == 5),
            "行号步长应为 5: {nums:?}"
        );
    }

    /// `format` 决定后处理：`gcode`（默认）带行号，`text` 是纯文本。
    /// 二者混了会让"导出给机床的程序"少掉行号。
    #[test]
    fn format_text_omits_line_numbers() {
        let base = r#"{"template":"drill_cycle","params":{"x":21,"y":15,"depth":-10,"feed":100},"options":{"lineNumbers":true,"format":"%s"}}"#;
        let render = |fmt: &str| -> String {
            let body = base.replace("%s", fmt);
            let Resp::Json(status, payload) =
                route(&test_ctx(), "POST", "/api/render", "", body.as_bytes())
            else {
                panic!("render 应返回 JSON")
            };
            assert_eq!(status, 200, "{payload}");
            payload["data"]["output"]
                .as_str()
                .expect("应有 output")
                .to_string()
        };

        // 按 token 判定（不按子串）：行号位宽由机床配置决定，
        // 断言 "N000" 这类子串会随位宽/步长变化而误判
        let numbered = |s: &str| {
            s.split_whitespace()
                .filter(|t| {
                    t.strip_prefix('N').is_some_and(|rest| {
                        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
                    })
                })
                .count()
        };
        let gcode = render("gcode");
        let text = render("text");
        assert!(numbered(&gcode) >= 2, "gcode 应带行号: {gcode}");
        assert_eq!(numbered(&text), 0, "text 不应带行号: {text}");
    }

    #[test]
    fn generation_options_rejects_bad_values() {
        for (options, needle) in [
            (r#"{"lineStep":"10"}"#, "必须是非负整数"),
            (r#"{"lineStep":4294967296}"#, "超出范围"),
            (r#"{"maxLine":-1}"#, "必须是非负整数"),
        ] {
            let body = format!(r#"{{"template":"drill_cycle","params":{{}},"options":{options}}}"#);
            let Resp::Json(status, payload) =
                route(&test_ctx(), "POST", "/api/render", "", body.as_bytes())
            else {
                panic!("render 应返回 JSON")
            };
            assert_eq!(status, 400, "options={options}");
            let msg = payload["error"]["message"].as_str().unwrap_or_default();
            assert!(msg.contains(needle), "options={options} 实际: {msg}");
        }

        // options 存在但不是对象
        let Resp::Json(status, payload) = route(
            &test_ctx(),
            "POST",
            "/api/render",
            "",
            br#"{"template":"drill_cycle","params":{},"options":[]}"#,
        ) else {
            panic!("render 应返回 JSON")
        };
        assert_eq!(status, 400);
        assert!(payload["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("必须是 JSON 对象"));
    }

    /// 报告 JSON 的 level 映射：前端按 level 区分展示，
    /// 映射错了会把 error 显示成提示（用户就看不到"不能生成"的原因）。
    #[test]
    fn report_json_carries_issue_levels() {
        // 缺必选 → error
        let Resp::Json(status, payload) = route(
            &test_ctx(),
            "POST",
            "/api/validate",
            "",
            br#"{"template":"drill_cycle","params":{}}"#,
        ) else {
            panic!("validate 应返回 JSON")
        };
        assert_eq!(status, 200);
        let report = &payload["data"]["report"];
        assert!(report["errors"].as_u64().unwrap() >= 1, "{report}");
        let issues = report["issues"].as_array().expect("应有 issues");
        assert!(
            issues.iter().any(|i| i["level"] == "error"),
            "应含 error: {issues:?}"
        );

        // 多给一个模板不引用的参数 → warning（与 error 分属不同 level）
        let body = br#"{"template":"drill_cycle","params":{"x":21,"y":15,"depth":-10,"feed":100,"typo_param":1}}"#;
        let Resp::Json(status, payload) = route(&test_ctx(), "POST", "/api/validate", "", body)
        else {
            panic!("validate 应返回 JSON")
        };
        assert_eq!(status, 200);
        let report = &payload["data"]["report"];
        assert!(report["warnings"].as_u64().unwrap() >= 1, "{report}");
        let issues = report["issues"].as_array().unwrap();
        assert!(issues.iter().any(|i| i["level"] == "warning"), "{issues:?}");
        // level 只能是这三个字符串之一
        assert!(issues.iter().all(|i| matches!(
            i["level"].as_str(),
            Some("error") | Some("warning") | Some("info")
        )));
    }

    #[test]
    fn security_headers_are_attached() {
        let resp = with_security_headers(tiny_http::Response::from_string("x"));
        let find = |name: &str| {
            resp.headers()
                .iter()
                .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
                .map(|h| h.value.as_str().to_string())
        };
        assert_eq!(find("x-content-type-options").as_deref(), Some("nosniff"));
        assert_eq!(find("referrer-policy").as_deref(), Some("no-referrer"));

        let csp = find("content-security-policy").expect("必须带 CSP");
        for needle in [
            "default-src 'self'",
            "object-src 'none'",
            "base-uri 'none'",
            "frame-ancestors 'none'",
            "form-action 'none'",
        ] {
            assert!(csp.contains(needle), "CSP 缺少 {needle}: {csp}");
        }
    }

    #[test]
    fn internal_and_cli_errors_map_status() {
        // 500 一律归 "internal"：原始分类（registry / io / …）会被替换掉，
        // 因为对调用方而言"服务端内部炸了"才是可操作的信息。
        //
        // 正文同样不回显（P1-17）：内部错误的正文含模板文件的绝对路径，
        // 原样回给浏览器等于泄露用户名与目录结构，而这不是调用方能处理的信息。
        let leaked = CliError::new(
            "io",
            r"读取模板失败 \\?\C:\Users\someone\Desktop\proj\turning\a.j2: 拒绝访问",
        );
        let Resp::Json(status, payload) = internal_error(leaked) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 500);
        assert_eq!(payload["error"]["kind"], "internal");
        let msg = payload["error"]["message"]
            .as_str()
            .expect("message 应为字符串");
        assert!(
            !msg.contains("Users") && !msg.contains("a.j2") && !msg.contains("Desktop"),
            "500 正文不得包含磁盘路径: {msg}"
        );

        // 400 仍回显正文：描述的是调用方传进来的模板哪里有问题（语法/行列号），
        // 是调用方能据以修正的信息，且不含磁盘路径。
        let Resp::Json(status, payload) = cli_error(CliError::new("render", "坏模板")) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 400);
        assert_eq!(payload["error"]["kind"], "render");
        assert_eq!(payload["error"]["message"], "坏模板");
    }

    #[test]
    fn percent_decode_paths_and_queries() {
        // 路径：+ 保持字面
        assert_eq!(percent_decode("a+b", false), "a+b");
        assert_eq!(percent_decode("%E9%92%BB%E5%AD%94", false), "钻孔");
        // query：+ 转空格
        assert_eq!(percent_decode("a+b", true), "a b");
        // 非法序列保持字面
        assert_eq!(percent_decode("%ZZ", false), "%ZZ");
        assert_eq!(percent_decode("%A", false), "%A");
    }

    #[test]
    fn query_parsing() {
        let q = parse_query("category=%E9%93%A3%E5%89%8A&x=1");
        assert_eq!(q[0], ("category".to_string(), "铣削".to_string()));
        assert_eq!(q[1], ("x".to_string(), "1".to_string()));
    }
}
