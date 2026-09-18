//! 本地 Web UI 服务（阶段 C）：tiny_http + 只读 API + 内嵌单文件前端。
//!
//! 安全约定（ROADMAP C1.3 / R5）：
//! - 默认仅绑定回环地址 `127.0.0.1`；显式 `--host` 指定非回环时由命令层
//!   打印安全警告（本模块不做判断，保持单一职责）
//! - 不执行任何 shell 命令；不提供任何写操作
//! - 请求体读取设 1 MiB 上限，防异常载荷
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
/// 若要去掉 `'unsafe-inline'`，需要给 `<script>`/`<style>` 注入每响应 nonce，
/// 属于后续加固项。
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
        .map(|(_, v)| v);
    if category
        .as_deref()
        .is_some_and(|value| parse_category(value).is_none())
    {
        return Resp::Json(
            400,
            err(
                "bad_request",
                "无效的模板分类，可选值为 general/milling/turning/drilling/machine",
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
fn parse_category(s: &str) -> Option<TemplateCategory> {
    match s {
        "general" | "通用" => Some(TemplateCategory::General),
        "milling" | "铣削" => Some(TemplateCategory::Milling),
        "turning" | "车削" => Some(TemplateCategory::Turning),
        "drilling" | "钻孔" => Some(TemplateCategory::Drilling),
        "machine" | "机床" => Some(TemplateCategory::Machine),
        _ => None,
    }
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

fn validation_json(
    template: &str,
    report: &nctool_core::validate::ValidationReport,
) -> serde_json::Value {
    let issues: Vec<serde_json::Value> = report
        .issues
        .iter()
        .map(|i| {
            let level = match i.level {
                nctool_core::validate::ValidationLevel::Error => "error",
                nctool_core::validate::ValidationLevel::Warning => "warning",
                nctool_core::validate::ValidationLevel::Info => "info",
            };
            serde_json::json!({ "level": level, "param": i.param, "message": i.message })
        })
        .collect();
    serde_json::json!({
        "template": template,
        "ok": report.is_ok(),
        "errors": report.errors().count(),
        "warnings": report.warnings().count(),
        "issues": issues,
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
                "report": validation_json(&name, &report),
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
    let format = match opts.get("format").and_then(|v| v.as_str()) {
        None | Some("gcode") => OutputFormat::Gcode,
        Some("text") => OutputFormat::Text,
        Some(other) => {
            return Err(Resp::Json(
                400,
                err("bad_request", format!("不支持的输出格式: {other}")),
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
    let report_json = validation_json(&name, &report);
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
    let mut machines: Vec<serde_json::Value> = Vec::new();
    for p in MachinePreset::all() {
        let cfg = p.config();
        machines.push(serde_json::json!({
            "id": p.id(),
            "vendor": cfg.vendor,
            "model": cfg.model,
            "config": cfg.config,
            "builtin": true,
        }));
    }
    for (id, m) in &ctx.loaded.merged.machine {
        if MachinePreset::from_id(id).is_none() {
            machines.push(serde_json::json!({
                "id": id,
                "vendor": m.vendor,
                "model": m.model,
                "config": m.config,
                "builtin": false,
            }));
        }
    }
    Resp::Json(200, ok(serde_json::json!({ "machines": machines })))
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

/// 启动服务并阻塞处理请求（Ctrl-C 终止进程退出）。
pub fn serve(ctx: Ctx, host: &str, port: u16) -> Result<(), CliError> {
    let addr = listen_addr(host, port)?;
    let server = tiny_http::Server::http(addr)
        .map_err(|e| CliError::new("io", format!("绑定 {addr} 失败: {e}")))?;
    eprintln!("nctool ui 已启动 → {}（Ctrl-C 退出）", browser_url(addr));
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
fn internal_error(e: CliError) -> Resp {
    Resp::Json(500, err("internal", e.message))
}

/// 业务校验错误（模板解析失败等）：400 + CLI 错误信息。
fn cli_error(e: CliError) -> Resp {
    Resp::Json(400, err(e.kind, e.message))
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

    #[test]
    fn route_health() {
        let Resp::Json(status, payload) = route(&test_ctx(), "GET", "/health", "", &[]) else {
            panic!("/health 应返回 JSON")
        };
        assert_eq!(status, 200);
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["data"]["status"], "ok");
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

    /// `validation_json` 的 level 映射：前端按 level 区分展示，
    /// 映射错了会把 error 显示成提示（用户就看不到"不能生成"的原因）。
    #[test]
    fn validation_json_carries_issue_levels() {
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
        // 因为对调用方而言"服务端内部炸了"才是可操作的信息
        let Resp::Json(status, payload) = internal_error(CliError::new("registry", "boom")) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 500);
        assert_eq!(payload["error"]["kind"], "internal");
        assert_eq!(payload["error"]["message"], "boom");

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
