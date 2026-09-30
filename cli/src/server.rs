//! 本地 Web UI 服务：tiny_http + 模板/机床资产 API + 内嵌单文件前端。
//!
//! 安全约定（ROADMAP C1.3 / R5）：
//! - **仅回环**：`--host` 传非回环地址时 [`listen_addr`] **直接拒绝**，而不是
//!   打印警告后放行。本服务能读模板目录并驱动渲染，暴露到局域网没有任何使用
//!   场景，只剩攻击面。（此处曾写"由命令层打印警告、本模块不做判断"，那是更早
//!   的实现；照那句话改回去会把这个决定悄悄撤销。）
//! - 不执行任何 shell 命令；资产写入仅通过 core 的路径约束、原子写入与指纹保护执行
//! - 请求体读取设 1 MiB 上限，防异常载荷；`Content-Length` 超限在**读取前**即
//!   拒绝（413），且读完与声明长度**对账**——半包/截断的请求体回 400，绝不当
//!   完整请求交给 [`route`]（P1-7）
//! - 请求体读取带**超时**（[`BODY_READ_TIMEOUT`]）：tiny_http 0.12 无读超时，
//!   而本循环是单线程顺序的，一个"发一半就挂"的连接能钉死整个服务（P1-8）
//! - **失败响应不回显内部正文**：500 只给泛化文案，详情写 stderr —— 内部错误的
//!   正文含模板文件的绝对路径（见 [`internal_error`]）
//! - **跨站请求防护**：`/api/` 下的请求校验 `Origin` / `Sec-Fetch-Site`
//!   （见 [`cross_site_guard`]）——只绑回环并不够，浏览器里的任意页面都能向
//!   `127.0.0.1:<port>` 发请求（DNS rebinding / CSRF）；本服务无状态、不落盘，
//!   但"被陌生网页驱动"仍应拦住
//! - **Host 校验**：所有请求（含静态页）校验 `Host` 是否为回环主机名
//!   （见 [`host_guard`]）——`Origin` 防护挡不住 DNS rebinding 后的**同源 GET**
//!   （它不带 `Origin`、`Sec-Fetch-Site: same-origin`，两条分支都放行），
//!   而 `Host` 由浏览器按地址栏主机名填写、JS 改不了，是重绑攻击的必经破绽
//! - 每个响应都带 CSP / nosniff / Referrer-Policy（见 [`SECURITY_HEADERS`]）
//!
//! 设计：路由逻辑收敛到纯函数 [`route`]（无网络依赖，可直接单元/集成测试），
//! [`serve`] 只负责 tiny_http 粘合（监听、解析、响应）。

// T01 库化后本模块成为 `pub mod server`（GUI 复用 [`route`]），其文档里多处交叉引用
// **私有**辅助项（`internal_error` / `SECURITY_HEADERS` / `allowed_origins` /
// `crate::commands::ui`）。私有项在公开文档中不生成超链接，rustdoc 以
// `private_intra_doc_links` 报警 → CI `-D warnings` 变红。此处显式放行该 lint：
// 链接对读源码的维护者仍有跳转价值（`--document-private-items` 下正常解析）。
#![allow(rustdoc::private_intra_doc_links)]

use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use nctool_core::asset::{now_iso8601, Preset, PresetStore, SpecFingerprint, WriteError};
use nctool_core::machine::{MachineKeyKind, MachineKeySchema, MachinePreset};
use nctool_core::pipeline::{GCodeGenerator, GenerationOptions, OutputFormat};
use nctool_core::registry::{TemplateCategory, TemplateSource};
use nctool_core::{ParamValue, ParameterSet};
use nctool_tpl::Variable;

use crate::args::parameter_set_from_json;
use crate::cli::CategoryArg;
use crate::context::Ctx;
use crate::output::{report_json, CliError};

/// 内嵌单文件前端（`cli/ui/index.html`，演示与服务双模式）。
///
/// 该文件是**生成物**：源在仓库根的 `ui/src/*.part.html`，由 `scripts/build_ui.mjs`
/// 拼装（同一份 Buffer 同时写 `ui/index.html` 与 `cli/ui/index.html`）。改前端请改
/// 片段并重新生成 —— 手改生成物会被 `build_ui.mjs --check` 与 `ui_html_copies_stay_in_sync`
/// 的第 2 条断言拦下。
pub const UI_HTML: &str = include_str!("../ui/index.html");

/// 单个请求体上限：inspect 的模板源码远小于此，超出视为异常载荷。
const MAX_BODY_BYTES: usize = 1024 * 1024;

/// 请求体读取超时（P1-8）。
///
/// tiny_http 0.12 **没有任何读超时配置**（`ServerConfig` 只有 `addr` / `ssl`），
/// 而 `serve_requests` 是**单条顺序循环**：一个声明 `Content-Length: 5000` 却只
/// 发几个字节、然后挂住的连接，会让 `read_to_end` 无限阻塞 —— 整个 `nctool ui`
/// 就此永久无响应。10s 足够任何正常客户端发完 1 MiB 以内的载荷。
///
/// 注意 tiny_http 对 `Content-Length <= 1024` 的请求是**在构造 `Request` 之前**
/// 就同步读完的（`request.rs:194-210`，读不满直接 `RequestCreationError`），
/// 因此本超时实际只作用于「声明长度 > 1024」与 chunked 两类请求。
const BODY_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// 同时在读请求体的线程数上限（P1-8 的第二道闸）。
///
/// 超时是**放弃**那个连接而不是中止读（`read` 阻塞在系统调用里，std 无法取消），
/// 于是读线程会一直挂着直到对端断开。若不加限制，恶意客户端反复"发一半就挂"
/// 就能把"服务永久挂死"换成"线程无限增长"。`serve_requests` 是顺序循环，正常
/// 情况下这个计数恒为 0 或 1；达到上限即说明堆了 8 个被放弃的连接，此时对新
/// 请求直接回 503（而不是再赔一个线程进去）。
///
/// 计数是**每个 serve 实例一份**（`Arc<AtomicUsize>`，不是全局 static）：全局
/// 计数会让并行跑的单测互相看见对方的读数，把 503 分支抖到别的用例上。
const MAX_IN_FLIGHT_BODY_READERS: usize = 8;

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
/// 现已补全转义（见 `ui/src/32_script_ui.part.html` 的 `esc` / `selEsc`，经
/// `build_ui.mjs` 进两份生成物），并逐点核对过全部
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

/// Host 头防护（DNS rebinding）：返回 `Some(403)` 表示应拒绝该请求。
///
/// `cross_site_guard` 只能挡住"带 `Origin` 的跨站写"和"带 `Sec-Fetch-Site` 的
/// 跨站读"。DNS rebinding 不走这两条路：攻击者先在 `attacker.example` 上架页，
/// 再把 DNS TTL 置 0 重绑到 `127.0.0.1`——此后浏览器认为它与本服务**同源**，
/// 发出的 GET **不带 `Origin`**（同源 GET 不发）、`Sec-Fetch-Site: same-origin`
/// （两条分支都放行），响应体即可被读取（`/api/presets` 含工艺参数与预设路径、
/// `/api/templates` 含全部模板源码）。
///
/// 拦法是校验 `Host`：它由浏览器按**地址栏里的主机名**填写、JS 改不了，
/// 重绑攻击下必然是 `attacker.example:<port>` —— 不在回环白名单即拒。
/// 伪造 `Host` 的客户端本来就能直连回环地址，没有额外攻击面。
///
/// 判定：
/// 1. `Host` **缺失** → 放行（HTTP/1.0 与 curl `-H` 显式去头的用法；
///    浏览器在 HTTP/1.1 下必然带 `Host`，重绑场景不会走到这条）；
/// 2. 主机名部分 ∈ {`localhost`, `127.0.0.1`, `::1`} 或任意回环 IP → 放行；
/// 3. 其它（含空串、带用户信息、非回环 IP、任意域名）→ 403。
///
/// 端口**不参与**判定：重绑只能借"连到我们端口的那一次"伪造主机名，
/// 而主机名已被第 2 条卡死；放开端口则 `localhost:任意` 仍是我们自己。
pub fn host_guard(headers: &[(&str, &str)]) -> Option<Resp> {
    let host = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("host"))
        .map(|(_, v)| *v)?;
    if host_allowed(host) {
        None
    } else {
        Some(Resp::Json(
            403,
            err(
                "forbidden_host",
                format!("请求被拒绝：Host 为 {host}，本服务只接受回环主机名"),
            ),
        ))
    }
}

/// [`host_guard`] 的主机名判定（纯函数，便于单测穷举）。
fn host_allowed(host: &str) -> bool {
    fn loopback_name(name: &str) -> bool {
        if name.eq_ignore_ascii_case("localhost") {
            return true;
        }
        if let Ok(v4) = name.parse::<std::net::Ipv4Addr>() {
            return v4.is_loopback();
        }
        if let Ok(v6) = name.parse::<std::net::Ipv6Addr>() {
            return v6.is_loopback();
        }
        false
    }

    let h = host.trim();
    if h.is_empty() {
        return false;
    }
    // RFC 3986：IPv6 字面量必须带方括号 —— `[::1]` / `[::1]:8787`
    if let Some(rest) = h.strip_prefix('[') {
        let v6 = match rest.split_once(']') {
            Some((v6, _)) => v6,
            None => return false,
        };
        return loopback_name(v6);
    }
    // 无端口的整体先按字面量判（`127.0.0.1` 的点号会干扰下面的 rsplit）
    if loopback_name(h) {
        return true;
    }
    // 再剥 `:port`：`127.0.0.1:8787` / `localhost:8787`
    match h.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            loopback_name(name)
        }
        _ => false,
    }
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

/// `GET /health` 载荷：状态 + 版本 + **内置模板缺失告警**（`builtinWarnings`）。
///
/// 降级方案（P1-4）把「内置模板注册失败」从 panic 改成跳过——崩溃立刻可见，
/// **悄悄缺失不会**，故必须有 HTTP 出口（纯新增字段，不破坏既有契约；
/// 正常恒为 `[]`）。注册表按指纹缓存（`Ctx::build_registry`），健康检查不会
/// 每次重编译；构建失败时字段给 `[]`——构建失败本身由各业务路由以更响的
/// 通用错误暴露。
fn health_json(ctx: &Ctx) -> serde_json::Value {
    let warnings: Vec<serde_json::Value> = ctx
        .build_registry()
        .map(|gen| {
            gen.registry()
                .builtin_warnings()
                .iter()
                .map(|w| serde_json::json!({ "name": w.name, "message": w.message }))
                .collect()
        })
        .unwrap_or_default();
    ok(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "builtinWarnings": warnings,
    }))
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
        ("GET", "/health") => Resp::Json(200, health_json(ctx)),
        ("GET", "/api/templates") => templates_list(ctx, query),
        ("POST", "/api/templates/create") => template_create(ctx, body),
        ("POST", "/api/templates/save") => template_save(ctx, body),
        ("POST", "/api/templates/derive") => template_derive(ctx, body),
        ("POST", "/api/templates/rename") => template_rename(ctx, body),
        ("GET", "/api/machines") => machines_list(ctx),
        ("POST", "/api/machines") => machine_save(ctx, body),
        ("POST", "/api/machines/delete") => machine_delete(ctx, body),
        ("GET", "/api/config") => config_view(ctx),
        ("GET", "/api/presets") => presets_list(ctx, query),
        ("POST", "/api/presets") => presets_save(ctx, body),
        ("POST", "/api/presets/delete") => presets_delete(ctx, body),
        ("POST", "/api/presets/rename") => presets_rename(ctx, body),
        ("POST", "/api/presets/export") => presets_export(ctx, body),
        ("POST", "/api/presets/import") => presets_import(ctx, body),
        ("POST", "/api/inspect") => inspect(ctx, body),
        ("POST", "/api/lint") => lint_template(ctx, body),
        ("POST", "/api/validate") => validate(ctx, body),
        ("POST", "/api/render") => render(ctx, body),
        ("POST", "/api/part/generate") => part_generate(ctx, body),
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
                "builtin": matches!(e.source, TemplateSource::Builtin),
                "status": e.status,
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

    // 注册表模板（内置 / 目录）：携带分类、描述与参数规格
    if let Some(e) = gen.registry().get(&name) {
        // 参数**闭包**提取（穿透 include/extends），与 CLI `inspect`、
        // `registry.validate` 同一口径：组合模板若只列主模板自身的变量，
        // 片段独有的必选参数（如 `_undercut_common.j2` 的 5 个）在表单里
        // 根本不存在 → 提交必然 validate 失败。系统变量由
        // `extract_params` 内部剔除，这里不再手工传 `system_vars`。
        let vars = match gen.registry().extract_params(&name) {
            Ok(v) => v,
            Err(err) => return cli_error(CliError::from(err)),
        };
        let params: Vec<serde_json::Value> = e.params.iter().map(spec_json).collect();
        let fingerprint = match &e.source {
            TemplateSource::File(path) => nctool_core::asset::WriteKernel::read_fingerprint(path)
                .ok()
                .flatten()
                .map(|fp| fp.as_string()),
            TemplateSource::Builtin => None,
            TemplateSource::Memory => None,
        };
        return Resp::Json(
            200,
            ok(serde_json::json!({
                "template": {
                    "name": e.name,
                    "category": CategoryArg::from_core(e.category),
                    "description": e.description,
                    "builtin": matches!(e.source, TemplateSource::Builtin),
                    "status": e.status,
                    "fingerprint": fingerprint,
                    "source": e.source_text(),
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

// Template and machine asset editing is exposed only by the loopback UI service.
// All writes go through nctool-core's guarded writers (atomic replacement + fingerprint).
fn template_create(ctx: &Ctx, body: &[u8]) -> Resp {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", e.to_string())),
    };
    let (Some(name), Some(category)) = (
        v.get("name").and_then(|x| x.as_str()),
        v.get("category").and_then(|x| x.as_str()),
    ) else {
        return Resp::Json(400, err("bad_request", "需要 name 与 category"));
    };
    let Some(root) = ctx.template_dir.as_deref() else {
        return Resp::Json(400, err("args", "未配置模板目录"));
    };
    let cat = category.to_ascii_lowercase();
    let label = match cat.as_str() {
        "general" => "通用",
        "milling" => "铣削",
        "turning" => "车削",
        "drilling" => "钻孔",
        "grooving" => "切槽",
        "machine" => "机床",
        _ => return Resp::Json(400, err("args", "未知模板分类")),
    };
    let raw = name.trim();
    let key = if raw.to_ascii_lowercase().ends_with(".j2") {
        raw.to_string()
    } else {
        format!("{raw}.j2")
    };
    let source = format!("( {raw} 模板 )\n( 分类: {label} )\n\n");
    if let Err(e) = validate_template_source(root, &key, &source) {
        return e;
    }
    let out = match nctool_core::asset::TemplateWriter::create(root, &key, &source) {
        Ok(x) => x,
        Err(e) => return asset_write_error(e, "template_duplicate"),
    };
    let manifest_warning =
        match nctool_core::asset::TemplateWriter::append_manifest_entry(root, &key, raw, &cat) {
            nctool_core::asset::ManifestOutcome::Degraded(s) => Some(s),
            _ => None,
        };
    Resp::Json(
        200,
        ok(
            serde_json::json!({"name":key,"fingerprint":out.fingerprint,"action":out.action,"manifestWarning":manifest_warning}),
        ),
    )
}

fn template_save(ctx: &Ctx, body: &[u8]) -> Resp {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", e.to_string())),
    };
    let (Some(name), Some(source), Some(hash)) = (
        v.get("name").and_then(|x| x.as_str()),
        v.get("source").and_then(|x| x.as_str()),
        v.get("expectHash").and_then(|x| x.as_str()),
    ) else {
        return Resp::Json(400, err("bad_request", "需要 name、source 与 expectHash"));
    };
    let Some(root) = ctx.template_dir.as_deref() else {
        return Resp::Json(400, err("args", "未配置模板目录"));
    };
    let entry = match ctx
        .build_registry()
        .ok()
        .and_then(|g| g.registry().get(name).cloned())
    {
        Some(e) => e,
        None => {
            return Resp::Json(
                404,
                err("template_not_found", format!("模板不存在：{name}")),
            )
        }
    };
    let key = match entry.source {
        TemplateSource::File(_) => entry.name,
        _ => {
            return Resp::Json(
                400,
                err("bad_request", "内置模板不可直接编辑，请先派生模板"),
            )
        }
    };
    let path = root.join(&key);
    let current = match nctool_core::asset::WriteKernel::read_fingerprint(&path) {
        Ok(Some(fp)) if fp.as_string() == hash => fp,
        Ok(_) => {
            return Resp::Json(
                409,
                err("write_conflict", "模板已被外部修改，请重新载入后再保存"),
            )
        }
        Err(e) => return asset_write_error(e, "template_not_found"),
    };
    if let Err(e) = validate_template_source(root, &key, source) {
        return e;
    }
    match nctool_core::asset::TemplateWriter::save(root, &key, source, Some(current)) {
        Ok(out) => Resp::Json(
            200,
            ok(serde_json::json!({"name":key,"fingerprint":out.fingerprint,"action":out.action})),
        ),
        Err(e) => asset_write_error(e, "template_not_found"),
    }
}

fn template_derive(ctx: &Ctx, body: &[u8]) -> Resp {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", e.to_string())),
    };
    let (Some(source_name), Some(new_name)) = (
        v.get("sourceName").and_then(|x| x.as_str()),
        v.get("newName").and_then(|x| x.as_str()),
    ) else {
        return Resp::Json(400, err("bad_request", "需要 sourceName 与 newName"));
    };
    let Some(root) = ctx.template_dir.as_deref() else {
        return Resp::Json(400, err("args", "未配置模板目录"));
    };
    let entry = match ctx
        .build_registry()
        .ok()
        .and_then(|g| g.registry().get(source_name).cloned())
    {
        Some(e) => e,
        None => {
            return Resp::Json(
                404,
                err("template_not_found", format!("模板不存在：{source_name}")),
            )
        }
    };
    if !matches!(entry.source, TemplateSource::File(_)) {
        return Resp::Json(
            400,
            err("bad_request", "内置模板不可直接派生，请先选择磁盘模板"),
        );
    }
    let name = new_name.trim();
    if let Err(e) = nctool_core::asset::validate_asset_name(name) {
        return Resp::Json(400, err("bad_request", e));
    }
    let filename = if name.to_ascii_lowercase().ends_with(".j2") {
        name.to_string()
    } else {
        format!("{name}.j2")
    };
    let target = nctool_core::asset::sibling_rel_key(&entry.name, &filename);
    let source =
        nctool_core::asset::build_derived_source(entry.source_text(), name, &entry.name, true);
    if let Err(e) = validate_template_source(root, &entry.name, &source) {
        return e;
    }
    match nctool_core::asset::TemplateWriter::derive(root, &entry.name, &target, &source) {
        Ok(out) => Resp::Json(
            200,
            ok(
                serde_json::json!({"name":target,"fingerprint":out.file.fingerprint,"manifestWarning":match out.manifest { nctool_core::asset::ManifestOutcome::Degraded(s) => Some(s), _ => None }}),
            ),
        ),
        Err(e) => asset_write_error(e, "template_duplicate"),
    }
}

fn template_rename(ctx: &Ctx, body: &[u8]) -> Resp {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", e.to_string())),
    };
    let (Some(old), Some(new)) = (
        v.get("oldName").and_then(|x| x.as_str()),
        v.get("newName").and_then(|x| x.as_str()),
    ) else {
        return Resp::Json(400, err("bad_request", "需要 oldName 与 newName"));
    };
    let Some(root) = ctx.template_dir.as_deref() else {
        return Resp::Json(400, err("args", "未配置模板目录"));
    };
    let entry = match ctx
        .build_registry()
        .ok()
        .and_then(|g| g.registry().get(old).cloned())
    {
        Some(e) => e,
        None => return Resp::Json(404, err("template_not_found", format!("模板不存在：{old}"))),
    };
    if !matches!(entry.source, TemplateSource::File(_)) {
        return Resp::Json(400, err("bad_request", "内置模板不可重命名"));
    }
    let name = new.trim();
    if let Err(e) = nctool_core::asset::validate_asset_name(name) {
        return Resp::Json(400, err("bad_request", e));
    }
    let filename = if name.to_ascii_lowercase().ends_with(".j2") {
        name.to_string()
    } else {
        format!("{name}.j2")
    };
    let target = nctool_core::asset::sibling_rel_key(&entry.name, &filename);
    match nctool_core::asset::TemplateWriter::rename(root, &entry.name, &target) {
        Ok(out) => Resp::Json(
            200,
            ok(
                serde_json::json!({"oldName":entry.name,"name":target,"manifestWarning":match out.manifest { nctool_core::asset::ManifestOutcome::Degraded(s) => Some(s), _ => None }}),
            ),
        ),
        Err(e) => asset_write_error(e, "template_not_found"),
    }
}

fn validate_template_source(root: &std::path::Path, key: &str, source: &str) -> Result<(), Resp> {
    nctool_tpl::parse(source, key)
        .map_err(|e| Resp::Json(400, err("validation", e.to_string())))?;
    let manifest = nctool_core::manifest::TemplateManifest::load(root)
        .map_err(|e| Resp::Json(400, err("manifest", e.to_string())))?;
    let library = nctool_core::variables::VariableLibrary::load(root)
        .map_err(|e| Resp::Json(400, err("variables", e.to_string())))?;
    let meta = nctool_core::manifest::ResolvedMeta::resolve(
        std::path::Path::new(key),
        source,
        manifest.get(key),
        &library,
    );
    let report = nctool_core::validate::check_spec_consistency(&meta.params);
    if report.has_errors() {
        return Err(Resp::Json(400, err("validation", report.summary())));
    }
    Ok(())
}

fn asset_write_error(e: nctool_core::asset::WriteError, fallback: &'static str) -> Resp {
    let (status, kind) = match &e {
        nctool_core::asset::WriteError::Conflict { .. }
        | nctool_core::asset::WriteError::LockBusy { .. } => (409, "write_conflict"),
        nctool_core::asset::WriteError::NotFound(_) => (404, fallback),
        nctool_core::asset::WriteError::PathEscape { .. } => (400, "bad_request"),
        _ => (500, "internal"),
    };
    Resp::Json(status, err(kind, e.to_string()))
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
    // ERR-NUM-UNDERFLOW：JSON 下溢字面量（如 `1e-400`）会被 serde_json 静默归零 →
    // 错误 G-code。在解析**之前**做文本层候选提取 + serde_json 实测确认（硬失败）。
    // 请求体按 UTF-8 解码失败时不在此报错——交下面的 from_slice 报合法错误。
    if let Ok(text) = std::str::from_utf8(body) {
        if let Some(resp) = json_underflow_resp(text) {
            return Err(resp);
        }
    }
    serde_json::from_slice(body)
        .map_err(|e| Resp::Json(400, err("bad_request", format!("请求体不是合法 JSON: {e}"))))
}

/// JSON 文本 → 下溢错误响应（HTTP 400，kind `"num_underflow"`）；无命中返回 `None`。
///
/// 复用 [`crate::output::json_underflow_error`] 的两段式（core 候选 + serde_json 确认），
/// 但 HTTP 通道用**自己的** kind（`num_underflow`）与状态码（400），与 CLI 的 `args`/2
/// 区分：前端可据此把「数值下溢」独立渲染，而非笼统的 bad_request。
fn json_underflow_resp(text: &str) -> Option<Resp> {
    crate::output::json_underflow_error(text, "请求体")
        .map(|e| Resp::Json(400, err("num_underflow", e.message)))
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
    // 必须把「键缺失」与「类型错误」分开：`and_then(as_str)` 对 `{"machine":123}`
    // 同样返回 None，合并处理就会**静默按 generic 生成** —— 机床参数错的 G-code
    // 正是项目零容忍的「静默产出错误程序」。与 `generation_options` 的
    // OPTIONS-SILENT-FALLBACK-001 同源（那里已修，此处是第二处漏网）。
    let explicit = match value.get("machine") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s.as_str()),
        Some(_) => {
            return Err(Resp::Json(
                400,
                err("bad_request", "machine 必须是字符串（省略时用默认机床）"),
            ))
        }
    };
    // 缺失 → 走 `resolve_machine(None)`：取 `--machine`/配置的默认机床，
    // 无配置时回落 generic（此前硬编码 "generic"，会无视已配置的默认机床）。
    ctx.resolve_machine(explicit).map_err(|e| {
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
    // P0-2：与 /api/render 同口径解析机床（请求体 `machine` 缺省 → 配置默认/
    // generic），转速上界按该机床联动——否则 validate 放行的参数会被 render
    // 拒绝，同一份参数在两个端点上给出两个结论。
    let machine = match api_machine(ctx, &value) {
        Ok(v) => v,
        Err(r) => return r,
    };
    match gen
        .registry()
        .validate_with_machine(&name, &params, Some(&machine))
    {
        Ok(report) => Resp::Json(
            200,
            ok(serde_json::json!({
                "report": crate::output::report_json(&name, &report),
            })),
        ),
        // P0-3：`RegistryError` 的 Display 可能携带模板文件绝对路径
        // （`RegistryError::Io` 来自 `registry.rs` 的读文件消息），原样回传等于
        // 把服务器目录结构交给浏览器。与 `registered_template` 的
        // `build_registry` 失败同一口径：详情进 stderr，响应体泛化。
        Err(e) => internal_error(CliError::from(e)),
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
            // HTTP 单模板渲染恒从 0 起编号；跨工序续编只在 `part generate`
            // 路径上发生（由 `part::PartSpec::generate` 传递游标）。
            line_number_start: 0,
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
    // P0-2：校验与渲染用同一台机床（`machine` 已在上方解析），转速上界按
    // `machine.max_spindle_rpm` 联动——报告里的结论就是渲染管线的结论。
    let report = match gen
        .registry()
        .validate_with_machine(&name, &params, Some(&machine))
    {
        Ok(v) => v,
        // P0-3：同 `validate`，不回显 `RegistryError` 正文（可能含绝对路径）。
        Err(e) => return internal_error(CliError::from(e)),
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
    // 两条路径都取 `*_outcome` 变体：只有它回传**后处理告警**（P2-1，行号撞上限
    // 后停止编号）。HTTP 通道把它放进 `warnings`，前端才能提示"程序生成了，
    // 但后半段没有行号"—— 否则浏览器里看到的是一份看起来完全正常的 G-code。
    let output = if lenient {
        gen.generate_lenient_outcome(&name, &params, &machine, &opts)
            .map(|(o, _report)| o)
    } else {
        gen.generate_outcome(&name, &params, &machine, &opts)
    };
    match output {
        Ok(outcome) => Resp::Json(
            200,
            ok(serde_json::json!({
                "blocked": false,
                "report": report_json,
                "output": outcome.text,
                "warnings": outcome.warnings,
                "template": name,
                "machine": machine.id,
            })),
        ),
        // P0-3 保留 400 + 回显正文：此处消息只描述**调用方传入的**模板
        // （HTTP 侧 `registered_template` 只接受注册表逻辑名 = 相对键，
        // 不复用 CLI 的文件路径解析），不含服务器磁盘路径（cli-review P0-1）。
        Err(e) => Resp::Json(400, err("render", e.to_string())),
    }
}

/// `POST /api/part/generate` —— 零件级批量生成。
///
/// # 契约（与前端 `ui/src/31_script_api.part.html` 的 `doPart` mock 逐字段对齐）
///
/// 请求体：`{"part": {"name","default_machine","params","ops":[{"template","params","machine","options"}]}}`
///
/// 响应体：`{"ok":true,"data":{"results":[{"index","name","output"} | {"index","name","error"}]}}`
///
/// **逐工序返回而不是整体成败**：与 CLI 的"事务语义"看似矛盾，实为两个通道的
/// 不同职责。CLI 写文件，半成品会被人误送上机床，故必须全成或全不成；HTTP 只
/// 返回文本给浏览器，前端需要知道"哪几道好了、哪几道坏了"才能高亮定位。
/// 端到端的一致性靠**同一个 core 编排**保证：参数继承、机床覆盖、行号续编
/// 三条语义在两条通道上完全相同。
///
/// `options` 走与 `/api/render` 同一套键名（`lineNumbers` / `addHeader` /
/// `stripBlank` / `ascii` / `lenient`），避免同一语义在两处各起一套名字。
/// 工序级 `options` 只覆盖显式给出的字段。
fn part_generate(ctx: &Ctx, body: &[u8]) -> Resp {
    let value = match api_body(body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let Some(part_value) = value.get("part") else {
        return Resp::Json(
            400,
            err("bad_request", "请求体缺少 `part` 字段（零件定义）"),
        );
    };
    let spec: nctool_core::part::PartSpec = match serde_json::from_value(part_value.clone()) {
        Ok(s) => s,
        Err(e) => return Resp::Json(400, err("bad_request", format!("解析零件定义失败: {e}"))),
    };
    let gen = match ctx.build_registry() {
        Ok(g) => g,
        Err(e) => return internal_error(e),
    };
    // 顶层布尔选项必须**严格**：`and_then(as_bool).unwrap_or(false)` 对
    // `{"lineNumbers":"true"}` 会静默关掉行号——调用方以为选项生效，实际输出
    // 少了行号（OPTIONS-SILENT-FALLBACK-001 的同类漏网；`/api/render` 的
    // `generation_options` 早已对类型错误硬 400，同一份请求体不该两套严格度）。
    let opts = (|| -> Result<nctool_core::part::PartOptions, Resp> {
        let opt_bool = |name: &str| -> Result<bool, Resp> {
            match value.get(name) {
                None | Some(serde_json::Value::Null) => Ok(false),
                Some(serde_json::Value::Bool(b)) => Ok(*b),
                Some(_) => Err(Resp::Json(
                    400,
                    err("bad_request", format!("{name} 必须是布尔值")),
                )),
            }
        };
        Ok(nctool_core::part::PartOptions {
            line_numbers: opt_bool("lineNumbers")?,
            add_header_comment: opt_bool("addHeader")?,
            strip_blank_lines: opt_bool("stripBlank")?,
            ascii_only: opt_bool("ascii")?,
            lenient: opt_bool("lenient")?,
        })
    })();
    let opts = match opts {
        Ok(o) => o,
        Err(r) => return r,
    };

    match spec.generate(&gen, ctx.default_machine.as_deref(), &opts) {
        Ok(outcome) => {
            let results: Vec<serde_json::Value> = outcome
                .ops
                .iter()
                .map(|o| {
                    serde_json::json!({
                        "index": o.index,
                        "name": o.template,
                        "output": o.output,
                        "endLineNumber": o.end_line_number,
                    })
                })
                .collect();
            Resp::Json(
                200,
                ok(serde_json::json!({
                    "results": results,
                    "program": outcome.program,
                    "okCount": outcome.ops.len(),
                    "failCount": 0,
                    // P2-1：程序生成了，但可能有工序的编号被上限截断
                    "warnings": outcome.warnings,
                })),
            )
        }
        // 形状不合法（ops 空、template 空）是**请求本身有问题**，回 400 且无 data
        // —— 前端 `runBatch` 走到这个分支只会提示"批量生成失败"，正合语义。
        Err(nctool_core::part::PartError::InvalidSpec(msg)) => {
            Resp::Json(400, err("bad_request", msg))
        }
        // 工序级失败仍回 **200**，因为前端需要 `data.results` 才能逐工序高亮
        // （`runBatch` 检查 `res.data.results` 存在才渲染标签页，不存在则整体
        // 丢弃 —— 那样用户只看到一句"批量生成失败"，完全不知道哪道坏了、为什么）。
        //
        // 这里与 CLI 的差异是**故意的**：CLI 写文件，半成品会被人误送上机床，
        // 必须全成或全不成；HTTP 只回文本给浏览器，逐工序详情才是用户要的信息。
        // 两通道共享同一个 core 编排，故参数继承/机床覆盖/行号续编语义完全一致。
        Err(nctool_core::part::PartError::OperationsFailed { failures }) => {
            // 成功工序的产出**按 index 对齐填回**，失败工序给 `error`：
            // 前端按 `results[i]` 逐标签渲染，缺项会让标签与内容错位。
            let failed: std::collections::BTreeMap<usize, &nctool_core::part::OpFailure> =
                failures.iter().map(|f| (f.index, f)).collect();
            let total = spec.ops.len();
            let results: Vec<serde_json::Value> = (0..total)
                .map(|i| match failed.get(&i) {
                    Some(f) => serde_json::json!({
                        "index": i,
                        "name": f.name,
                        "error": f.error,
                    }),
                    None => serde_json::json!({
                        "index": i,
                        "name": spec.ops[i].template,
                        // 该工序未失败但整体未交付：不发文本，避免被当成可用产出
                        "skipped": true,
                    }),
                })
                .collect();
            Resp::Json(
                200,
                ok(serde_json::json!({
                    "results": results,
                    "program": "",
                    "okCount": total - failures.len(),
                    "failCount": failures.len(),
                })),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// GET /api/machines
// ---------------------------------------------------------------------------

/// `MachineKeySchema` → 前端机床键规格 JSON。
///
/// 与 [`spec_json`] 同款做法：手写 `match`，**不**给 core 的类型加 `Serialize`
/// ——core 对这些类型零 serde 依赖，而派生形状是外部标签（`{"Choice":[…]}`），
/// 与前端要的 `"Choice"` + 候选列表对不上。
pub(crate) fn machine_schema_json(s: &MachineKeySchema) -> serde_json::Value {
    let (kind, options) = match s.kind {
        MachineKeyKind::String => ("String", Vec::new()),
        MachineKeyKind::Integer => ("Integer", Vec::new()),
        MachineKeyKind::Choice(opts) => (
            "Choice",
            opts.iter().map(|o| (*o).to_string()).collect::<Vec<_>>(),
        ),
    };
    serde_json::json!({
        "key": s.key,
        "kind": kind,
        "default": s.default,
        "description": s.description,
        // 候选值白名单：仅 Choice 非空，其余为 null（与 `spec_json` 的 options 同约定）
        "options": if options.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::json!(options)
        },
    })
}

/// 机床列表：内置预设 + 配置文件自定义机床（与 `machine list` 口径一致）。
///
/// `nctool ui` 的前端机床切换需要它；属于 ROADMAP C2 三端点之外的必要补充。
/// `schema` 字段供前端展示键类型、含义和候选项；自定义机床写入另由
/// `POST /api/machines` 与 `POST /api/machines/delete` 处理。
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
    // 键规格同样取自 core 的单一来源（`KNOWN_CONFIG_KEYS`），前端不另存一份。
    let schema: Vec<serde_json::Value> = nctool_core::machine::KNOWN_CONFIG_KEYS
        .iter()
        .map(machine_schema_json)
        .collect();
    let config_fingerprint =
        nctool_core::asset::WriteKernel::read_fingerprint(&ctx.project_config_path())
            .ok()
            .flatten()
            .map(|fp| fp.as_string());
    Resp::Json(
        200,
        ok(
            serde_json::json!({ "machines": machines, "schema": schema, "configFingerprint": config_fingerprint }),
        ),
    )
}

fn config_view(ctx: &Ctx) -> Resp {
    Resp::Json(
        200,
        ok(serde_json::json!({
            "templateDir": ctx.template_dir,
            "defaultMachine": ctx.default_machine,
            "globalPath": ctx.loaded.global_path,
            "projectPath": ctx.loaded.project_path,
            "customMachines": ctx.loaded.merged.machine.keys().collect::<Vec<_>>(),
            "warnings": ctx.loaded.warnings,
        })),
    )
}

fn machine_save(ctx: &Ctx, body: &[u8]) -> Resp {
    let path = ctx.project_config_path();
    machine_save_at(ctx, body, &path)
}

fn machine_save_at(ctx: &Ctx, body: &[u8], path: &std::path::Path) -> Resp {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", e.to_string())),
    };
    let (Some(id), Some(vendor), Some(model)) = (
        v.get("id").and_then(|x| x.as_str()),
        v.get("vendor").and_then(|x| x.as_str()),
        v.get("model").and_then(|x| x.as_str()),
    ) else {
        return Resp::Json(400, err("bad_request", "需要 id、vendor 与 model"));
    };
    if nctool_core::asset::is_builtin_machine(id) {
        return Resp::Json(
            400,
            err("bad_request", "内置机床不可修改；请另存为自定义机床"),
        );
    }
    let config: std::collections::BTreeMap<String, String> = match serde_json::from_value(
        v.get("config")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    ) {
        Ok(c) => c,
        Err(e) => return Resp::Json(400, err("bad_request", format!("config 格式错误：{e}"))),
    };
    let current = match nctool_core::asset::WriteKernel::read_fingerprint(path) {
        Ok(fp) => fp,
        Err(e) => return asset_write_error(e, "machine_not_found"),
    };
    let expect_hash = match v.get("expectHash") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(hash)) => Some(hash.as_str()),
        Some(_) => return Resp::Json(400, err("bad_request", "expectHash 必须是字符串或 null")),
    };
    if let Some(want) = expect_hash {
        if current.as_ref().map(|fp| fp.as_string()).as_deref() != Some(want) {
            return Resp::Json(
                409,
                err("write_conflict", "机床配置文件已变化，请刷新后重试"),
            );
        }
    }
    let machine = nctool_core::MachineConfig {
        id: id.into(),
        vendor: vendor.into(),
        model: model.into(),
        config,
    };
    let (required, mut validation_warnings) =
        match crate::commands::machine::required_machine_keys(ctx, id) {
            Ok(result) => result,
            Err(e) => return cli_error(e),
        };
    let report = nctool_core::asset::MachineWriter::preflight(&machine, &required);
    if !report.can_save() {
        return Resp::Json(400, err("validation", report.blocking.join("；")));
    }
    validation_warnings.extend(report.warnings);
    match nctool_core::asset::MachineWriter::upsert(path, &machine, current) {
        Ok(out) => Resp::Json(
            200,
            ok(
                serde_json::json!({"machine":machine,"action":out.action,"fileFingerprint":out.fingerprint,"warnings":validation_warnings}),
            ),
        ),
        Err(e) => asset_write_error(e, "machine_not_found"),
    }
}

fn machine_delete(ctx: &Ctx, body: &[u8]) -> Resp {
    let path = ctx.project_config_path();
    machine_delete_at(ctx, body, &path)
}

fn machine_delete_at(ctx: &Ctx, body: &[u8], path: &std::path::Path) -> Resp {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", e.to_string())),
    };
    let Some(id) = v.get("id").and_then(|x| x.as_str()) else {
        return Resp::Json(400, err("bad_request", "需要 id"));
    };
    if ctx.default_machine.as_deref() == Some(id) {
        return Resp::Json(
            409,
            err(
                "default_machine",
                "该机床是当前默认机床，请先修改默认机床配置再删除",
            ),
        );
    }
    if nctool_core::asset::is_builtin_machine(id) {
        return Resp::Json(400, err("bad_request", "内置机床不可删除"));
    }
    let current = match nctool_core::asset::WriteKernel::read_fingerprint(path) {
        Ok(fp) => fp,
        Err(e) => return asset_write_error(e, "machine_not_found"),
    };
    let expect_hash = match v.get("expectHash") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(hash)) => Some(hash.as_str()),
        Some(_) => return Resp::Json(400, err("bad_request", "expectHash 必须是字符串或 null")),
    };
    if let Some(want) = expect_hash {
        if current.as_ref().map(|fp| fp.as_string()).as_deref() != Some(want) {
            return Resp::Json(
                409,
                err("write_conflict", "机床配置文件已变化，请刷新后重试"),
            );
        }
    }
    match nctool_core::asset::MachineWriter::remove(path, id, current) {
        Ok(out) => Resp::Json(200, ok(serde_json::json!({"id":id,"action":out.action}))),
        Err(e) => asset_write_error(e, "machine_not_found"),
    }
}

fn lint_template(ctx: &Ctx, body: &[u8]) -> Resp {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", e.to_string())),
    };
    let Some(name) = v.get("template").and_then(|x| x.as_str()) else {
        return Resp::Json(400, err("bad_request", "需要 template"));
    };
    let source = match v.get("source") {
        Some(serde_json::Value::String(source)) => source.clone(),
        Some(_) => return Resp::Json(400, err("bad_request", "source 必须是字符串")),
        None => {
            let path = format!("/api/templates/{}", percent_encode(name));
            let Resp::Json(status, response) =
                template_detail(ctx, path.trim_start_matches("/api/templates/"))
            else {
                unreachable!()
            };
            if status != 200 {
                return Resp::Json(status, response);
            }
            let Some(source) = response
                .pointer("/data/template/source")
                .and_then(|x| x.as_str())
            else {
                return Resp::Json(500, err("internal", "模板详情缺少源码"));
            };
            source.to_string()
        }
    };
    match nctool_tpl::lint(&source, name) {
        Ok(findings) => Resp::Json(200, ok(serde_json::to_value(findings.iter().map(|f| serde_json::json!({"line":f.line,"col":f.col,"filter":f.filter,"suggestion":f.suggestion,"message":f.message})).collect::<Vec<_>>()).unwrap_or_default())),
        Err(e) => Resp::Json(400, err("render", e.to_string())),
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
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

/// 成功响应里的预设文件位置：`~/...` 脱敏形态（P1-9）。
///
/// 绝对路径必然含 Windows 用户名，`/api/presets*` 系列无鉴权，不得随每次
/// 成功响应无条件外泄（cli-review P1-3 / SUMMARY 批1-3）。`~` 前缀保留
/// 用户定位文件所需的全部信息；路径在家目录之外时退回**文件名**
/// （与 [`write_error_resp`] 的 Conflict 分支同口径），绝不回显目录部分。
fn preset_path_display(path: &std::path::Path) -> String {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from);
    if let Some(home) = home {
        if let Ok(rest) = path.strip_prefix(&home) {
            // 统一正斜杠：反斜杠在 JSON 里要转义，`~/` 形态跨平台一致
            let rest = rest.to_string_lossy().replace('\\', "/");
            return format!("~/{rest}");
        }
    }
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "presets.yaml".to_string())
}

/// 单个 [`ParamValue`] → **扁平** JSON 标量（`21.0` / `"粗加工"` / `true` / `[..]`）。
///
/// **为什么不直接用 `serde_json::to_value`**：`ParamValue` 的 `Serialize` 是
/// **带标签**形式（`{"type":"number","value":21.0}`，见 `core::model::ParamValue`
/// 的文档），而 `presets_save` 侧用 `parameter_set_from_json` 解析的是**扁平**形式
/// （`{"x":21.0}`，与 `--params-file` 同形）。若把带标签形式塞进列表项，消费方
/// 把它原样回喂 `POST /api/presets` 会撞上"参数 x 不支持对象类型"——列表项无法
/// 往返。故此处**显式按扁平标量构造**，与解析端严格互逆（不依赖 derive 的默认形态）。
fn param_value_flat_json(v: &ParamValue) -> serde_json::Value {
    match v {
        ParamValue::Number(n) => serde_json::json!(n),
        ParamValue::Integer(i) => serde_json::json!(i),
        ParamValue::String(s) => serde_json::json!(s),
        ParamValue::Bool(b) => serde_json::json!(b),
        // 列表递归扁平化：元素仍为裸标量，`parameter_set_from_json` 能原样还原。
        ParamValue::List(items) => {
            serde_json::Value::Array(items.iter().map(param_value_flat_json).collect())
        }
    }
}

/// [`ParameterSet`] → **扁平** JSON 映射（`{"x":21.0,"y":15.0}`）。
///
/// 供 `GET /api/presets` 列表项的 `params` 字段使用。形态与 `presets_save` 的入参
/// （`parameter_set_from_json`）**互为逆运算**：把列表里拿到的 `params` 原样作为
/// `POST /api/presets` 的 `params` 回喂，必须解析成功且产出同一参数集（往返约束）。
/// 键序取 `BTreeMap` 的字典序（确定性输出，便于逐字断言）。
fn params_flat_json(params: &ParameterSet) -> serde_json::Value {
    let mut map = serde_json::Map::with_capacity(params.values.len());
    for (k, v) in &params.values {
        map.insert(k.clone(), param_value_flat_json(v));
    }
    serde_json::Value::Object(map)
}

/// `GET /api/presets`：预设列表（含结构化陈旧字段**与扁平参数取值**）。`?template=<名>` 过滤。
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
                // ★ 扁平参数取值（`{"x":21.0}`），让消费方（Web UI / GUI）能从列表
                //   直接"应用"预设——此前只回 `paramCount`，消费方拿到名字却拿不到值，
                //   只能静默清空表单（Web UI 早已按 `p.params` 取值，正是此处缺字段）。
                //   形态与 `presets_save` 入参互逆（见 `params_flat_json`），可原样回喂保存。
                "params": params_flat_json(&p.params),
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
            // P1-9：`~/...` 脱敏形态，绝不回显含用户名的绝对路径
            "path": preset_path_display(&path),
            "pathRedacted": true,
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
    // ERR-NUM-UNDERFLOW：与 `api_body` 同一口径，在解析前拦下下溢字面量（400/num_underflow）。
    if let Ok(text) = std::str::from_utf8(body) {
        if let Some(resp) = json_underflow_resp(text) {
            return resp;
        }
    }
    let parsed: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", format!("请求体不是合法 JSON: {e}"))),
    };
    let name = match parsed.get("name").and_then(|t| t.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Resp::Json(400, err("bad_request", "请求体需要非空字符串字段 \"name\"")),
    };
    // 预设名与 CLI `preset save` / `preset rename` / `preset import` 同一条
    // `validate_asset_name` 口径：否则"import 拒绝的名字能经 save 落盘"，
    // 且含控制字符的名字会被 `preset list` 原样回显到终端（ANSI/OSC 注入）。
    // 也使 `write_error_resp` 的 PathEscape 分支重新可达（此前不可达）。
    if let Err(reason) = nctool_core::asset::validate_asset_name(&name) {
        // kind 与 `write_error_resp` 的 PathEscape 分支一致（bad_request），
        // 保证"早校验"与"core 落盘时校验"两条路径对同一份输入给出同一形状。
        return Resp::Json(400, err("bad_request", format!("预设名非法：{reason}")));
    }
    let template = match parsed.get("template").and_then(|t| t.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return Resp::Json(
                400,
                err("bad_request", "请求体需要非空字符串字段 \"template\""),
            )
        }
    };
    // `force` 必须是布尔值：`and_then(as_bool).unwrap_or(false)` 会把
    // `force:"true"` 静默读成 false → 409。方向虽安全（不覆盖），但把用户的
    // 明确意图静默改写成"拒绝"同样是口径漂移，与本文件其余选项的严格度对齐。
    let force = match parsed.get("force") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return Resp::Json(400, err("bad_request", "force 必须是布尔值")),
    };

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
    // 服务侧等待 0（try-lock）：serve 循环单线程，阻塞等锁会钉死整个 UI；
    // 拿不到锁即 LockBusy → write_error_resp 回 409，由前端稍后重试。
    let outcome = match PresetStore::upsert_with_wait(&path, preset, std::time::Duration::ZERO) {
        Ok(o) => o,
        Err(e) => return write_error_resp(e),
    };
    Resp::Json(
        200,
        ok(serde_json::json!({
            "name": name,
            "template": template,
            "paramCount": params.len(),
            // P1-9：`~/...` 脱敏形态，绝不回显含用户名的绝对路径
            "path": preset_path_display(&path),
            "pathRedacted": true,
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
    // 服务侧等待 0（try-lock）：与 upsert 同口径，绝不阻塞 serve 循环。
    let outcome = match PresetStore::remove_with_wait(&path, &name, std::time::Duration::ZERO) {
        Ok(o) => o,
        Err(e) => return write_error_resp(e),
    };
    Resp::Json(
        200,
        ok(serde_json::json!({
            "name": name,
            // P1-9：`~/...` 脱敏形态，绝不回显含用户名的绝对路径
            "path": preset_path_display(&path),
            "pathRedacted": true,
            "action": outcome.action,
            "fileFingerprint": outcome.fingerprint,
        })),
    )
}

fn presets_rename(ctx: &Ctx, body: &[u8]) -> Resp {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", e.to_string())),
    };
    let (Some(old), Some(new)) = (
        v.get("oldName").and_then(|x| x.as_str()),
        v.get("newName").and_then(|x| x.as_str()),
    ) else {
        return Resp::Json(400, err("bad_request", "需要 oldName 与 newName"));
    };
    let path = match presets_path(ctx) {
        Ok(p) => p,
        Err(e) => return cli_error(e),
    };
    match PresetStore::rename(&path, old, new) {
        Ok(out) => Resp::Json(
            200,
            ok(serde_json::json!({"oldName":old,"name":new,"action":out.action})),
        ),
        Err(nctool_core::asset::WriteError::NotFound(_)) => {
            Resp::Json(404, err("preset_not_found", format!("预设不存在：{old}")))
        }
        Err(nctool_core::asset::WriteError::PathEscape { reason, .. })
            if reason.contains("同名") =>
        {
            Resp::Json(409, err("name_conflict", reason))
        }
        Err(e) => write_error_resp(e),
    }
}

fn presets_export(ctx: &Ctx, body: &[u8]) -> Resp {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", e.to_string())),
    };
    let path = match presets_path(ctx) {
        Ok(p) => p,
        Err(e) => return cli_error(e),
    };
    let got = match PresetStore::load(&path) {
        Ok(g) => g,
        Err(e) => return write_error_resp(e),
    };
    let selected = v.get("name").and_then(|x| x.as_str());
    let presets: Vec<_> = match selected {
        Some(name) => match got.file.get(name) {
            Some(p) => vec![p.clone()],
            None => return Resp::Json(404, err("preset_not_found", format!("预设不存在：{name}"))),
        },
        None => got.file.presets.clone(),
    };
    if presets.is_empty() {
        return Resp::Json(400, err("args", "没有可导出的预设"));
    }
    let file = nctool_core::asset::PresetFile {
        version: nctool_core::asset::PRESET_SCHEMA_VERSION,
        presets: presets.clone(),
    };
    match serde_yaml::to_string(&file) {
        Ok(yaml) => Resp::Json(
            200,
            ok(
                serde_json::json!({"yaml":yaml,"count":presets.len(),"names":presets.iter().map(|p| p.name.clone()).collect::<Vec<_>>()}),
            ),
        ),
        Err(e) => Resp::Json(500, err("internal", format!("预设序列化失败：{e}"))),
    }
}

fn presets_import(ctx: &Ctx, body: &[u8]) -> Resp {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(e) => return Resp::Json(400, err("bad_request", e.to_string())),
    };
    let Some(yaml) = v.get("yaml").and_then(|x| x.as_str()) else {
        return Resp::Json(400, err("bad_request", "需要 yaml 文本"));
    };
    let force = match v.get("force") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        _ => return Resp::Json(400, err("bad_request", "force 必须是布尔值")),
    };
    let mut unvalidated_templates = std::collections::BTreeSet::new();
    let presets = match PresetStore::import_presets::<CliError, _>(yaml, |p| {
        let specs = match crate::commands::preset::specs_of(ctx, &p.template) {
            Ok(specs) => specs,
            Err(_) => {
                unvalidated_templates.insert(p.template.clone());
                Vec::new()
            }
        };
        let report = nctool_core::validate::check_param_values(&specs, &p.params);
        if report.has_errors() {
            return Err(CliError::new(
                "validation",
                format!("预设「{}」参数值非法：\n{}", p.name, report.summary()),
            ));
        }
        Ok(())
    }) {
        Ok(p) => p,
        Err(e) => return cli_error(e),
    };
    let path = match presets_path(ctx) {
        Ok(p) => p,
        Err(e) => return cli_error(e),
    };
    // 指纹必须在加载文件前取得。若加载期间其他进程改写了文件，旧内容与旧
    // 指纹的组合会在最终写入时冲突，而不会把并发更新静默覆盖。
    let expected = match nctool_core::asset::WriteKernel::read_fingerprint(&path) {
        Ok(x) => x,
        Err(e) => return write_error_resp(e),
    };
    let loaded = match PresetStore::load(&path) {
        Ok(x) => x,
        Err(e) => return write_error_resp(e),
    };
    if loaded.degraded {
        return write_error_resp(nctool_core::asset::WriteError::Corrupt(format!(
            "预设文件不可用（{}），为避免覆盖已拒绝导入",
            loaded.warnings.join("；")
        )));
    }
    let mut imported_names = std::collections::BTreeSet::new();
    if let Some(duplicate) = presets.iter().find_map(|p| {
        if imported_names.insert(p.name.as_str()) {
            None
        } else {
            Some(p.name.clone())
        }
    }) {
        return Resp::Json(
            400,
            err(
                "bad_request",
                format!("导入文件包含重复预设名：{duplicate}"),
            ),
        );
    }
    let conflicts: Vec<_> = presets
        .iter()
        .filter(|p| loaded.file.get(&p.name).is_some())
        .map(|p| p.name.clone())
        .collect();
    if !conflicts.is_empty() && !force {
        return Resp::Json(
            409,
            err(
                "name_conflict",
                format!("预设已存在：{}。确认覆盖后重试", conflicts.join(", ")),
            ),
        );
    }
    let mut merged = loaded.file;
    for p in &presets {
        merged.take(&p.name);
        merged.presets.push(p.clone());
    }
    match PresetStore::save_with_wait(&path, &merged, expected, std::time::Duration::ZERO) {
        Ok(out) => Resp::Json(
            200,
            ok(
                serde_json::json!({"count":presets.len(),"names":presets.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),"action":out.action,"warnings":unvalidated_templates.iter().map(|t| format!("模板 {t} 无法解析，导入时只执行有限性校验。" )).collect::<Vec<_>>()}),
            ),
        ),
        Err(e) => write_error_resp(e),
    }
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
    // 参数**闭包**提取（穿透 include/extends），与 CLI `inspect`、`registry.validate`
    // 同一口径；系统变量由 `extract_params` 内部剔除。
    let vars = match gen.registry().extract_params(tpl) {
        Ok(v) => v,
        Err(err) => return cli_error(CliError::from(err)),
    };
    // `issues` **不再是硬编码 `[]`**：填入不依赖用户参数的规格 default 自洽
    // 问题（类型不符 / 越界 / 非整数 / 不在白名单 / NaN）——写错的 default 会在
    // 渲染前被静默注入上下文，此前这条路径没有任何"不提交参数就能看到"的入口。
    // JSON 形状与 `/api/validate` 的 `report.issues` 同源（同一序列化视图）。
    let spec_report = nctool_core::validate::check_spec_defaults_report(&entry.params);
    let issues = report_json(&entry.name, &spec_report)
        .get("issues")
        .cloned()
        .unwrap_or(serde_json::json!([]));
    let v = vars_json(&vars);
    Resp::Json(
        200,
        ok(serde_json::json!({
            "template": entry.name,
            "issues": issues,
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
    serve_requests(&server, addr, &ctx)
}

/// 一次请求体的读取结果。
enum BodyOutcome {
    /// 读完（可能读出错）：`request` 必须**还回去**才能 `respond`。
    Read {
        /// 原请求（读完后仍要用来回响应）。
        request: Box<tiny_http::Request>,
        /// 读到的字节（出错时可能只有前缀）。
        body: Vec<u8>,
        /// `read_to_end` 的结果：`Err` 表示对端中途断开 / 重置 / 半包。
        result: std::io::Result<usize>,
    },
    /// 读超时：连接被放弃，主循环继续。
    TimedOut,
}

/// 读请求体，带**读超时**（P1-8），并把 IO 错误原样带出（P1-7）。
///
/// # 为什么必须用分离线程
///
/// `read` 阻塞在系统调用里，std 没有任何取消手段。报告初稿建议的
/// `std::thread::scope` + `recv_timeout` **不起作用**：`scope` 在块结束时会
/// **join 全部线程**，读线程仍卡在 `read` 上 → 主线程照样钉死，等于没加超时。
/// 只有 `std::thread::spawn`（分离）能让主循环脱身。
///
/// 代价是超时后那个线程与其持有的连接被**放弃**（读操作返回时 `send` 失败，
/// `Request` 随错误值被 drop —— tiny_http 会据此自动回一个 500 并关闭连接）。
/// 因此调用方必须用 [`MAX_IN_FLIGHT_BODY_READERS`] 给这种泄漏设上界。
fn read_body_with_timeout(
    request: tiny_http::Request,
    timeout: Duration,
    in_flight: &Arc<AtomicUsize>,
) -> BodyOutcome {
    let (tx, rx) = mpsc::channel();
    in_flight.fetch_add(1, Ordering::SeqCst);
    let counter = Arc::clone(in_flight);
    std::thread::spawn(move || {
        // 无论正常退出、早退还是 panic 都要把计数还回去，否则 8 次之后
        // 服务会永久回 503（比原来的挂死更难查）。
        struct Decr(Arc<AtomicUsize>);
        impl Drop for Decr {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _decr = Decr(counter);

        let mut request = request;
        let mut body = Vec::new();
        // `take(上限+1)`：多读一个字节就足以判定"超限"，不必把整份载荷读进内存。
        let result = request
            .as_reader()
            .take(MAX_BODY_BYTES as u64 + 1)
            .read_to_end(&mut body);
        // 接收端已超时放弃 → `send` 失败，`request` 随错误值被 drop。
        let _ = tx.send((request, body, result));
    });

    match rx.recv_timeout(timeout) {
        Ok((request, body, result)) => BodyOutcome::Read {
            request: Box::new(request),
            body,
            result,
        },
        Err(_) => BodyOutcome::TimedOut,
    }
}

/// [`serve`] 的实际请求循环，按**借用**接收服务实例。
///
/// 拆出借用版是为了可测：`serve` 按值接管后，测试进程无法再持有句柄调用
/// `Server::unblock()` 来让循环干净退出（而 llvm-cov 只在**干净退出**时才落盘
/// 覆盖数据——被 kill 的子进程数据全丢）。测试用借用版 + `unblock()` 覆盖
/// 请求循环（见 `serve_handles_real_request_then_unblocks_cleanly`）。
fn serve_requests(server: &tiny_http::Server, addr: SocketAddr, ctx: &Ctx) -> Result<(), CliError> {
    serve_requests_with_timeout(server, addr, ctx, BODY_READ_TIMEOUT)
}

/// 与 [`serve_requests`] 相同，但请求体读超时可注入 —— 单测用它把 10s 压到毫秒级，
/// 否则测一次"半包连接不钉死服务"要真等 10 秒（覆盖门禁不接受）。
fn serve_requests_with_timeout(
    server: &tiny_http::Server,
    addr: SocketAddr,
    ctx: &Ctx,
    body_timeout: Duration,
) -> Result<(), CliError> {
    serve_requests_with_timeout_and_counter(
        server,
        addr,
        ctx,
        body_timeout,
        Arc::new(AtomicUsize::new(0)),
        MAX_IN_FLIGHT_BODY_READERS,
    )
}

fn serve_requests_with_timeout_and_counter(
    server: &tiny_http::Server,
    addr: SocketAddr,
    ctx: &Ctx,
    body_timeout: Duration,
    in_flight: Arc<AtomicUsize>,
    max_in_flight_body_readers: usize,
) -> Result<(), CliError> {
    let allowed = allowed_origins(&addr);
    // 在读请求体的线程数（P1-8 第二道闸）。每个 serve 实例一份，理由见
    // [`MAX_IN_FLIGHT_BODY_READERS`]。
    // Keep the long-running server view in sync with guarded asset writes and edits made
    // by other tools while preserving explicit startup overrides.
    let mut active_ctx = ctx.clone();

    for request in server.incoming_requests() {
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

        // Host 校验先于一切（含静态页）：DNS rebinding 下连 `/` 也不该被读走。
        // 缺失 `Host` 时放行（见 host_guard 文档），故不改变 curl/HTTP1.0 用法。
        //
        // 三层短路全部先算完，再决定要不要读请求体：被拒的请求不该让服务为它
        // 缓冲 1 MiB，也不该进 `route()`（建注册表）。
        let early: Option<Resp> = match host_guard(&headers) {
            Some(blocked) => Some(blocked),
            None => match (method.as_str(), path.as_str()) {
                ("GET", "/") | ("GET", "/index.html") => Some(Resp::Html),
                _ => path
                    .starts_with("/api/")
                    .then(|| cross_site_guard(&headers, &allowed))
                    .flatten(),
            },
        };
        // `Content-Length` 预检（P1-7）：声明长度已超上限就直接 413，**不进读取**。
        // 此前是"先缓冲 1 MiB 再判 413"，声明 100 MiB 的请求也要先吃掉 1 MiB 内存。
        //
        // 用 `body_length()` 而不是自己解析头：tiny_http 已按 RFC 处理了
        // 「有 `Transfer-Encoding` 时忽略 `Content-Length`」（`request.rs:143-160`），
        // 自己再解析一遍就会把这条规则抄错。`None` 表示"未知长度"（chunked 或
        // 两者皆无），此时交给下面的读上限兜底。
        let precheck: Option<Resp> = match request.body_length() {
            Some(len) if len > MAX_BODY_BYTES => Some(Resp::Json(
                413,
                err("payload_too_large", "请求体超过 1 MiB 上限"),
            )),
            _ => None,
        };
        // 声明长度（`None` = chunked / 两者皆无）。读完要与实际字节数对账（P1-7）。
        let declared_len = request.body_length();
        // `headers` 里的 `&str` 借用 `request`，而 `Vec` 带 Drop → 借用活到作用域尾，
        // 不显式 drop 就**无法把 request 移交给读线程**。
        drop(headers);

        // 读线程数达到上限（P1-8 第二道闸）：说明已经堆了 `MAX_IN_FLIGHT_BODY_READERS`
        // 个"发一半就挂"的连接，此时对新请求直接回 503，而不是再赔一个线程进去。
        let overloaded = in_flight.load(Ordering::SeqCst) >= max_in_flight_body_readers;

        let mut respondable = request;
        let resp = if let Some(resp) = early {
            resp
        } else if let Some(resp) = precheck {
            resp
        } else if overloaded {
            Resp::Json(
                503,
                err(
                    "service_unavailable",
                    "服务正忙于处理挂起的连接，请稍后重试",
                ),
            )
        } else {
            match read_body_with_timeout(respondable, body_timeout, &in_flight) {
                // 超时：连接被放弃，**没有**可用于回响应的 `Request`，只能记 stderr
                // 后继续下一轮 —— 这正是 P1-8 要的效果：一个赖着不发的客户端不再
                // 能把整个 UI 服务钉死。
                BodyOutcome::TimedOut => {
                    eprintln!(
                        "warning: 读取请求体超时（{body_timeout:?}），已放弃该连接：{method} {path}"
                    );
                    continue;
                }
                BodyOutcome::Read {
                    request,
                    body,
                    result,
                } => {
                    respondable = *request;
                    match result {
                        // P1-7 前半：读失败（对端重置等）**必须**显式报错。
                        // 此前 `.is_ok()` 把 Err 直接吞掉，残缺的请求体被当完整请求交给
                        // `route()` —— 调用方看到的是"我的 JSON 写错了"，而不是"传输中断"，
                        // 排障方向全错。
                        Err(e) => Resp::Json(
                            400,
                            err(
                                "bad_request",
                                format!("读取请求体失败（对端中断或半包）: {e}"),
                            ),
                        ),
                        // P1-7 后半：**实际字节数必须等于声明的 `Content-Length`**。
                        //
                        // 只靠上面的 `Err` 抓不到"半包"：tiny_http 的 `EqualReader`
                        // 在底层 EOF 时返回 `Ok(0)`（`equal_reader.rs:52-58`，注释写明
                        // "if the limit is reached, it returns EOF"），也就是**短读不报错**。
                        // 不自己比长度，截断的 JSON 依旧会被当完整请求送进 `route()`。
                        Ok(n) => match declared_len {
                            Some(d) if n != d => Resp::Json(
                                400,
                                err(
                                    "bad_request",
                                    format!(
                                        "请求体不完整：声明 {d} 字节，实际读到 {n} 字节（对端中断或半包）"
                                    ),
                                ),
                            ),
                            // chunked（`body_length()` 为 `None`）时预检拦不住，这里兜底。
                            _ if body.len() > MAX_BODY_BYTES => {
                                Resp::Json(413, err("payload_too_large", "请求体超过 1 MiB 上限"))
                            }
                            _ => {
                                let template_override = active_ctx.template_dir != active_ctx.loaded.merged.template_dir;
                                let machine_override = active_ctx.default_machine != active_ctx.loaded.merged.default_machine;
                                if let Ok(loaded) = crate::config::load() {
                                    if !template_override { active_ctx.template_dir = loaded.merged.template_dir.clone(); }
                                    if !machine_override { active_ctx.default_machine = loaded.merged.default_machine.clone(); }
                                    active_ctx.loaded = loaded;
                                }
                                route(&active_ctx, &method, &path, &query, &body)
                            },
                        },
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
        let _ = respondable.respond(response);
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
/// 状态码与 `kind` 查 [`crate::output::classify_write_error`] 单一分类表
/// （P1-11，与 CLI 侧同源）；**正文**逐臂构建且一律脱敏：乐观锁冲突 / 锁争用
/// 409、越界/名称非法 400（重名 409 name_conflict）、条目不存在 404、
/// IO / 内容损坏 500（本地环境或磁盘问题，调用方无从修正，正文可能含
/// 绝对路径，按 [`internal_error`] 的口径不回显细节）。
///
/// **4xx 也不回显绝对路径**（P1-17 口径）：路径只进服务端 stderr，响应体给
/// 文件名——足够用户识别是哪份预设文件被外部改动了，又不把磁盘布局经 API 泄露。
/// `LockBusy` 更严格：正文连文件名都不带，固定文案（架构 §3.1 约束 A）。
///
/// `WriteError` 为 `#[non_exhaustive]`：未登记变体由分类表**留声**兜底
/// （eprintln 警告 + 500），不静默降级成 400。
fn write_error_resp(e: WriteError) -> Resp {
    // 状态码与 kind 查单一分类表（P1-11）；消息逐臂构建且**一律脱敏**——
    // 表的 `cli_msg_with_path` 只授权 CLI/stderr 通道带绝对路径，HTTP 响应体
    // 永远不含目录段（P1-17；文件名可留，供用户识别是哪份预设）。
    let cls = crate::output::classify_write_error(&e, "preset_not_found", "io");
    let message = match &e {
        WriteError::Conflict { path, .. } => {
            eprintln!(
                "error: 预设文件已被外部修改（写入冲突，未覆盖）: {}",
                path.display()
            );
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "预设文件".into());
            format!("写入冲突：{name} 已被外部修改，未覆盖。请重试")
        }
        // 锁争用（P0-1）：路径只进 stderr；HTTP 响应体用固定脱敏文案 ——
        // 不得照抄 Conflict 的形状（那会在 LockBusy 上新开一处路径泄漏）。
        WriteError::LockBusy { path } => {
            eprintln!(
                "error: 预设文件正被另一个 nctool 进程写入（未改动）: {}",
                path.display()
            );
            "预设文件正被另一个 nctool 进程写入，请稍后重试".to_string()
        }
        WriteError::PathEscape { rel, reason } => {
            if reason.contains("已存在") {
                format!("{reason}：{rel}")
            } else {
                format!("预设名非法：{rel}（{reason}）")
            }
        }
        WriteError::ReadOnly { path } => {
            eprintln!("error: 预设文件只读或无写入权限: {}", path.display());
            "目标文件只读或无写入权限".to_string()
        }
        // "预设不存在"：调用方问题，404（**不是** 500 —— 早期因为它被塞进
        // `Corrupt` 分支而报成服务端内部错误）
        WriteError::NotFound(m) => m.clone(),
        WriteError::Corrupt(m) => {
            eprintln!("error: 预设文件损坏: {m}");
            "预设文件内容损坏，详情见服务终端输出".to_string()
        }
        // 行/列/字面量足够定位；路径只进 stderr（表 flag=false，HTTP 不带）
        WriteError::NumUnderflow {
            literal,
            line,
            column,
            path,
        } => {
            eprintln!("error: 预设文件下溢: {}", path.display());
            format!(
                "第 {line} 行第 {column} 列：数值 yaml:{literal} 低于 f64 最小可表示正数\
                 （会被静默变 0，G-code 将产出错误坐标）。请改用可表示的数值。"
            )
        }
        WriteError::Io(e) => {
            eprintln!("error: 预设文件读写失败: {e}");
            "预设文件读写失败".to_string()
        }
        // `#[non_exhaustive]`：未登记变体由 classify 留声警告，HTTP 归 500
        _ => "预设文件读写失败".to_string(),
    };
    Resp::Json(cls.http_status, err(cls.http_kind, message))
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
    fn host_guard_allows_loopback_hosts() {
        for host in [
            "localhost",
            "localhost:8787",
            "LOCALHOST:8787",
            "127.0.0.1",
            "127.0.0.1:8787",
            "127.0.0.53:8787", // 回环网段整体（is_loopback），非精确 127.0.0.1
            "[::1]",
            "[::1]:8787",
            "::1",
        ] {
            assert!(
                host_guard(&[("host", host)]).is_none(),
                "回环 Host 应放行: {host}"
            );
        }
    }

    #[test]
    fn host_guard_rejects_rebinding_hosts() {
        // DNS rebinding：浏览器按地址栏主机名填 Host，同源 GET 不带 Origin，
        // cross_site_guard 对它完全无感——Host 是唯一能拦住的凭据。
        for host in [
            "attacker.example",
            "attacker.example:8787",
            "evil.local:8787",
            "0.0.0.0:8787",
            "192.168.1.10:8787",
            "[2001:db8::1]:8787",
            "",
            " ",
            "localhost.evil.example:8787", // 后缀伪装
            "127.0.0.1.evil.example:8787",
        ] {
            let Some(Resp::Json(status, payload)) = host_guard(&[("host", host)]) else {
                panic!("非回环 Host 应被拒绝: {host:?}");
            };
            assert_eq!(status, 403, "Host: {host:?}");
            assert_eq!(payload["error"]["kind"], "forbidden_host");
        }
    }

    #[test]
    fn host_guard_missing_host_is_allowed_but_case_insensitive() {
        // HTTP/1.0 / curl 显式去头：缺失放行（浏览器在 HTTP/1.1 下必然带 Host）
        assert!(host_guard(&[]).is_none());
        assert!(host_guard(&[("user-agent", "curl/8.0")]).is_none());
        // 头名大小写不敏感
        assert!(host_guard(&[("HOST", "localhost:8787")]).is_none());
        assert!(host_guard(&[("Host", "evil.example")]).is_some());
    }

    #[test]
    fn api_machine_rejects_non_string_instead_of_silent_generic() {
        let ctx = test_ctx();
        // 类型错误必须硬 400：静默回落 generic 会产出机床参数错的 G-code
        let Err(Resp::Json(status, payload)) =
            api_machine(&ctx, &serde_json::json!({ "machine": 123 }))
        else {
            panic!("machine 非字符串应被拒绝");
        };
        assert_eq!(status, 400);
        assert_eq!(payload["error"]["kind"], "bad_request");
        // bool / 数组 / 对象同样拒绝
        for bad in [
            serde_json::json!(true),
            serde_json::json!(["wfl_m65"]),
            serde_json::json!({"id": "generic"}),
        ] {
            let v = serde_json::json!({ "machine": bad });
            assert!(api_machine(&ctx, &v).is_err(), "machine={bad} 应被拒绝");
        }
        // 键缺失 → 默认机床（Ctx::for_test 无配置 → generic）
        let m = api_machine(&ctx, &serde_json::json!({})).expect("缺失应回落默认机床");
        assert_eq!(m.id, "generic");
        // 显式字符串照常解析
        let m = api_machine(&ctx, &serde_json::json!({ "machine": "wfl_m65" }))
            .expect("已知机床应解析成功");
        assert_eq!(m.id, "wfl_m65");
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

    // -----------------------------------------------------------------------
    // ERR-NUM-UNDERFLOW：HTTP 通道（400 / num_underflow）
    // -----------------------------------------------------------------------

    /// `api_body` 在解析前拦下下溢字面量：`{"x":1e-400}` → 400 / num_underflow。
    ///
    /// 未命中模板解析（下溢检查在 `api_body` 内、模板解析之前），故普通 ctx 即可。
    #[test]
    fn api_body_rejects_underflow_literal() {
        let ctx = test_ctx();
        let body = br#"{"template":"t.j2","params":{"x":1e-400}}"#;
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/validate", "", body) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 400, "{payload}");
        assert_eq!(payload["ok"], false);
        assert_eq!(
            payload["error"]["kind"], "num_underflow",
            "HTTP 通道 kind 应为 num_underflow: {payload}"
        );
        assert!(
            payload["error"]["message"]
                .as_str()
                .unwrap()
                .contains("json:1e-400"),
            "消息应带回显的字面量: {payload}"
        );
    }

    /// 字符串内容里的 `1e-400` 不是数值 → 不得误报（前端回退到模板解析）。
    #[test]
    fn api_body_string_content_not_underflow() {
        let ctx = test_ctx();
        let body = br#"{"template":"t.j2","params":{"note":"1e-400"}}"#;
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/validate", "", body) else {
            panic!("应返回 JSON")
        };
        // 未命中下溢 → 落到正常的模板解析（此处模板不存在 → 404），**不是** num_underflow
        assert_ne!(
            payload["error"]["kind"], "num_underflow",
            "字符串内容不应被判为下溢: {payload}"
        );
        assert_ne!(status, 400, "不应是下溢的 400: {payload}");
    }

    /// 合法次正规数 `5e-324` / `1e-323` 不得被判为下溢。
    #[test]
    fn api_body_accepts_legal_subnormals() {
        let ctx = test_ctx();
        for lit in ["5e-324", "1e-323"] {
            let body = format!(r#"{{"template":"t.j2","params":{{"x":{lit}}}}}"#);
            let Resp::Json(_status, payload) =
                route(&ctx, "POST", "/api/validate", "", body.as_bytes())
            else {
                panic!("应返回 JSON")
            };
            assert_ne!(
                payload["error"]["kind"], "num_underflow",
                "合法次正规数 {lit} 不应判为下溢: {payload}"
            );
        }
    }

    /// `presets_save` 与 `api_body` 同口径：下溢字面量 → 400 / num_underflow。
    #[test]
    fn presets_save_rejects_underflow() {
        let ctx = test_ctx();
        let body = br#"{"name":"p","template":"t.j2","params":{"x":1e-400}}"#;
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets", "", body) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 400, "{payload}");
        assert_eq!(payload["error"]["kind"], "num_underflow", "{payload}");
    }

    /// 非法预设名在**解析模板/参数之前**被早校验拒绝：400 / bad_request，
    /// kind 与 core `upsert` → `write_error_resp` 的 PathEscape 分支一致，
    /// 且完全不触碰磁盘（含控制字符名，防 `GET /api/presets` 回显注入）。
    #[test]
    fn presets_save_rejects_illegal_name() {
        let env = preset_endpoint_test("save_bad_name");
        let ctx = env.ctx();
        // (输入, 期望理由片段)：与 `validate_asset_name` 的拒绝理由同源。
        // 请求体经 `serde_json` 序列化，控制字符会被转义为 JSON 转义序列——
        // 否则**原始**控制字符直接违反 JSON 字符串语法，拦在解析层而非名字校验层。
        for (bad, why) in [("../evil", "路径分隔符"), ("p\u{7}1", "控制字符")] {
            let body = serde_json::to_vec(&serde_json::json!({
                "name": bad, "template": "t.j2", "params": {"x": 1.0}, "force": false,
            }))
            .unwrap();
            let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets", "", &body) else {
                panic!("应返回 JSON")
            };
            assert_eq!(status, 400, "bad={bad} {payload}");
            assert_eq!(payload["error"]["kind"], "bad_request", "bad={bad}");
            let msg = payload["error"]["message"].as_str().unwrap_or_default();
            assert!(msg.contains("预设名非法"), "bad={bad} {payload}");
            assert!(msg.contains(why), "应说明拒绝理由：{payload}");
        }
        assert!(
            !nctool_core::asset::default_preset_path().exists(),
            "非法名不得落盘"
        );
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

    /// 列表项**必须**带 `params` 取值（此前只回 `paramCount`）。
    ///
    /// 回归背景：`GET /api/presets` 曾只回 `paramCount` 不回 `params`，而 Web UI
    /// 的 `loadPreset` 早已按 `p.params` 取值 —— 于是点预设 chip 时参数被静默清空、
    /// 却提示"已载入预设"（本项目最忌讳的静默错误）。本测试钉住字段存在且取值正确。
    #[test]
    fn presets_list_includes_params() {
        let env = preset_endpoint_test("list_params");
        let ctx = env.ctx();
        let body = env.body("p1", r#"{"x":21,"y":15}"#, false);
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets", "", body.as_bytes())
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");

        let Resp::Json(status, payload) = route(&ctx, "GET", "/api/presets", "", &[]) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200);
        let one = &payload["data"]["presets"][0];
        let params = one
            .get("params")
            .unwrap_or_else(|| panic!("列表项应含 params 字段: {one}"));
        assert!(params.is_object(), "params 应为扁平对象: {params}");
        assert_eq!(params["x"], serde_json::json!(21.0), "x 取值: {params}");
        assert_eq!(params["y"], serde_json::json!(15.0), "y 取值: {params}");
        assert_eq!(
            params.as_object().unwrap().len(),
            2,
            "params 项数应与 paramCount 一致: {params}"
        );
    }

    /// **往返约束**：列表里拿到的 `params` **原样**回喂 `POST /api/presets` 必须成功。
    ///
    /// 这条是防"形态不一致"的关键测试：`ParameterSet` 的 `Serialize` 是
    /// `{"values":{...}}` 包裹形态、`ParamValue` 是带标签形态（`{"type":..,"value":..}`），
    /// 二者都与 `presets_save` 侧 `parameter_set_from_json` 期望的**扁平**形态不同。
    /// 若列表误用 derive 的默认形态，本测试会以 `bad_request`（"不支持对象类型"）失败。
    #[test]
    fn presets_list_params_round_trip() {
        let env = preset_endpoint_test("round_trip");
        let ctx = env.ctx();
        // ① 存一个源预设
        let body = env.body("src", r#"{"x":21,"y":15}"#, false);
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets", "", body.as_bytes())
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");

        // ② 从列表取回 params（**原样**）
        let Resp::Json(_, payload) = route(&ctx, "GET", "/api/presets", "", &[]) else {
            panic!("应返回 JSON")
        };
        let listed = payload["data"]["presets"][0]["params"].clone();
        assert!(listed.is_object(), "列表项 params 应为对象: {listed}");

        // ③ 把列表里的 params 原样作为 POST /api/presets 的 params 再存一次（换 name）
        let save_body = json_body(&serde_json::json!({
            "name": "copy",
            "template": "t.j2",
            "params": listed,
            "force": false,
        }));
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets", "", &save_body)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(
            status, 200,
            "列表 params 应能原样回喂保存（往返约束）: {payload}"
        );
        assert_eq!(payload["data"]["name"], "copy");
        assert_eq!(payload["data"]["paramCount"], 2, "回喂后参数个数应与源一致");

        // ④ 再列表：两个预设的 params 必须逐字段一致（值未被形态转换破坏）
        let Resp::Json(_, payload) = route(&ctx, "GET", "/api/presets", "", &[]) else {
            panic!("应返回 JSON")
        };
        let items = payload["data"]["presets"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        let params_of = |n: &str| {
            items
                .iter()
                .find(|p| p["name"] == n)
                .unwrap_or_else(|| panic!("缺少预设 {n}"))["params"]
                .clone()
        };
        assert_eq!(
            params_of("src"),
            params_of("copy"),
            "往返后 params 应逐字段一致"
        );
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
        let msg = payload["error"]["message"].as_str().unwrap();
        assert!(msg.contains("不得落在模板目录内"), "{payload}");
        // P1-17 口径：这条 reason 会被原样回显进 400 响应体，不得带绝对路径
        assert!(
            !msg.contains(&env.work.display().to_string()),
            "400 响应体不得回显绝对路径：{msg}"
        );
    }

    /// P1-9（cli-review P1-3）：三个预设端点的**成功**响应体不得回显含用户名
    /// 的绝对路径。`path` 统一 `~/...` 脱敏形态并以 `pathRedacted: true` 明示；
    /// 测试环境下 `USERPROFILE` 指向临时工作目录，任何 `nctool_apitest_` 残留
    /// 即代表目录部分泄漏。
    #[test]
    fn preset_success_responses_redact_absolute_path() {
        let env = preset_endpoint_test("p19");
        let ctx = env.ctx();
        let assert_redacted = |payload: &serde_json::Value, what: &str| {
            let text = payload.to_string();
            let p = payload["data"]["path"]
                .as_str()
                .unwrap_or_else(|| panic!("{what} 缺 data.path: {text}"));
            assert_eq!(payload["data"]["pathRedacted"], true, "{what}: {text}");
            assert!(
                p.starts_with("~/") || !p.contains('/'),
                "{what} 应为 ~/ 形态或文件名兜底：{p}"
            );
            assert!(!p.contains('\\'), "{what} 不得含反斜杠：{p}");
            assert!(!p.contains(':'), "{what} 不得含盘符：{p}");
            assert!(
                !text.contains("nctool_apitest_"),
                "{what} 不得回显工作目录（= 含用户名的绝对路径）：{text}"
            );
        };

        // POST /api/presets（保存）
        let body = env.body("p19", r#"{"x":21.0,"y":15.0}"#, false);
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets", "", body.as_bytes())
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");
        assert_redacted(&payload, "save");

        // GET /api/presets（列表）
        let Resp::Json(status, payload) = route(&ctx, "GET", "/api/presets", "", &[]) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");
        assert_redacted(&payload, "list");

        // POST /api/presets/delete（删除）
        let body = br#"{"name":"p19"}"#;
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets/delete", "", body)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");
        assert_redacted(&payload, "delete");
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
    fn presets_import_refuses_to_overwrite_corrupt_existing_file() {
        let env = preset_endpoint_test("import_corrupt");
        let ctx = env.ctx();
        let path = nctool_core::asset::default_preset_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let original = b"version: 1\npresets: [ {unclosed";
        std::fs::write(&path, original).unwrap();
        let yaml = r#"version: 1
presets:
  - name: imported
    template: t.j2
    params: {x: 21.0, y: 15.0}
    createdAt: "2026-09-29T00:00:00Z"
    specFingerprint: "fnv1a64:0000000000000000"
"#;
        let body = json_body(&serde_json::json!({"yaml": yaml, "force": true}));
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets/import", "", &body)
        else {
            panic!("应返回 JSON")
        };
        assert_ne!(status, 200, "损坏文件不得被导入覆盖：{payload}");
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn presets_import_rejects_duplicate_names_in_payload() {
        let env = preset_endpoint_test("import_duplicate");
        let ctx = env.ctx();
        let yaml = r#"version: 1
presets:
  - name: repeated
    template: t.j2
    params: {x: 21.0, y: 15.0}
    createdAt: "2026-09-29T00:00:00Z"
    specFingerprint: "fnv1a64:0000000000000000"
  - name: repeated
    template: t.j2
    params: {x: 30.0, y: 15.0}
    createdAt: "2026-09-29T00:00:00Z"
    specFingerprint: "fnv1a64:0000000000000000"
"#;
        let body = json_body(&serde_json::json!({"yaml": yaml, "force": true}));
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/presets/import", "", &body)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 400, "重复名称应明确拒绝：{payload}");
        assert_eq!(payload["error"]["kind"], "bad_request");
        assert!(!nctool_core::asset::default_preset_path().exists());
    }

    #[test]
    fn route_health() {
        let Resp::Json(status, payload) = route(&test_ctx(), "GET", "/health", "", &[]) else {
            panic!("/health 应返回 JSON")
        };
        assert_eq!(status, 200);
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["data"]["status"], "ok");
        // P1-4 配套②：内置模板缺失的结构化出口（纯新增；正常恒为 []）
        assert!(
            payload["data"]["builtinWarnings"].is_array(),
            "builtinWarnings 应恒为数组: {payload}"
        );
        assert_eq!(
            payload["data"]["builtinWarnings"]
                .as_array()
                .map(|a| a.len()),
            Some(0),
            "内置模板应全部安装成功: {payload}"
        );
    }

    // ---- POST /api/part/generate ----

    /// 成功路径：逐工序返回 `{index,name,output}`，并给出拼接后的 `program`。
    #[test]
    fn part_generate_returns_per_op_results() {
        let ctx = test_ctx();
        let body = serde_json::json!({
            "part": {
                "name": "P",
                "ops": [
                    {"template": "program_header", "params": {"prog": 1001}},
                    {"template": "safe_move", "params": {"x": 0, "y": 0, "z": 50}}
                ]
            }
        });
        let Resp::Json(status, payload) = route(
            &ctx,
            "POST",
            "/api/part/generate",
            "",
            body.to_string().as_bytes(),
        ) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");
        assert_eq!(payload["ok"], true);
        let results = payload["data"]["results"]
            .as_array()
            .expect("results 应为数组");
        assert_eq!(results.len(), 2, "两道工序应各有结果: {results:?}");
        assert_eq!(results[0]["index"], 0);
        assert_eq!(results[0]["name"], "program_header");
        assert!(
            results[0]["output"].as_str().unwrap().contains("O1001"),
            "第一道应含程序号: {}",
            results[0]["output"]
        );
        assert_eq!(results[1]["index"], 1);
        assert!(results[1]["output"].as_str().unwrap().contains("G0 X0.000"));
        assert_eq!(payload["data"]["failCount"], 0);
        assert!(
            payload["data"]["program"]
                .as_str()
                .unwrap()
                .contains("O1001"),
            "program 应是拼接产物"
        );
    }

    /// 参数继承：顶层 `params` 对每道工序可见（E5 局限 5.3）。
    #[test]
    fn part_generate_inherits_part_level_params() {
        let ctx = test_ctx();
        let body = serde_json::json!({
            "part": {
                "params": {"prog": 1001, "part_name": "FLANGE"},
                "ops": [
                    {"template": "program_header"},
                    {"template": "safe_move", "params": {"x": 0, "y": 0, "z": 50}}
                ]
            }
        });
        let Resp::Json(status, payload) = route(
            &ctx,
            "POST",
            "/api/part/generate",
            "",
            body.to_string().as_bytes(),
        ) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");
        let out0 = payload["data"]["results"][0]["output"].as_str().unwrap();
        assert!(
            out0.contains("O1001") && out0.contains("FLANGE"),
            "第一道应看到继承来的 prog 与 part_name: {out0}"
        );
    }

    /// 行号跨工序续编（E5 局限 5.1）：末行号随工序推进，整份程序无重复行号。
    #[test]
    fn part_generate_continues_line_numbers_across_ops() {
        let ctx = test_ctx();
        let body = serde_json::json!({
            "lineNumbers": true,
            "part": {
                "ops": [
                    {"template": "program_header", "params": {"prog": 1001}},
                    {"template": "safe_move", "params": {"x": 0, "y": 0, "z": 50}}
                ]
            }
        });
        let Resp::Json(status, payload) = route(
            &ctx,
            "POST",
            "/api/part/generate",
            "",
            body.to_string().as_bytes(),
        ) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");
        let results = payload["data"]["results"].as_array().unwrap();
        let last0 = results[0]["endLineNumber"].as_u64().unwrap();
        let last1 = results[1]["endLineNumber"].as_u64().unwrap();
        assert!(last0 >= 10, "第一道应已编号: {results:?}");
        assert!(
            last1 > last0,
            "第二道末行号应大于第一道（续编生效）: {last0} -> {last1}"
        );

        // 整份程序的行号必须唯一 —— 每段重来会在这里出现重复
        let program = payload["data"]["program"].as_str().unwrap();
        let nums: Vec<&str> = program
            .lines()
            .filter_map(|l| l.split_whitespace().next())
            .filter(|t| t.starts_with('N') && t.len() > 1)
            .collect();
        let mut uniq = nums.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(nums.len(), uniq.len(), "行号不得重复: {nums:?}");
    }

    /// 工序级失败仍回 **200 且带 `data.results`** —— 前端靠它逐工序高亮。
    ///
    /// 这是与 CLI 的关键差异：CLI 写文件必须全成或全不成，HTTP 只回文本，
    /// 逐工序详情才是用户要的信息。若这里回 400 且不给 `data`，
    /// 前端 `runBatch` 会整条丢弃，用户只看到一句"批量生成失败"。
    #[test]
    fn part_generate_partial_failure_keeps_results_for_per_op_highlighting() {
        let ctx = test_ctx();
        let body = serde_json::json!({
            "part": {
                "ops": [
                    {"template": "program_header", "params": {"prog": 1001}},
                    {"template": "no_such_template"}
                ]
            }
        });
        let Resp::Json(status, payload) = route(
            &ctx,
            "POST",
            "/api/part/generate",
            "",
            body.to_string().as_bytes(),
        ) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "部分失败也要回 200 带 results: {payload}");
        let results = payload["data"]["results"]
            .as_array()
            .expect("必须有 results");
        assert_eq!(
            results.len(),
            2,
            "失败工序也要占位，否则标签与内容错位: {results:?}"
        );
        assert_eq!(results[0]["index"], 0);
        assert!(
            results[0]["skipped"].as_bool().unwrap_or(false),
            "成功工序整体未交付应标 skipped: {}",
            results[0]
        );
        assert!(
            results[0].get("output").is_none(),
            "整体未交付时不该发半成品文本: {}",
            results[0]
        );
        assert_eq!(results[1]["index"], 1);
        assert_eq!(results[1]["name"], "no_such_template");
        assert!(
            results[1]["error"].as_str().unwrap().contains("模板不存在"),
            "{}",
            results[1]
        );
        assert_eq!(payload["data"]["failCount"], 1);
        assert_eq!(
            payload["data"]["program"], "",
            "整体未交付时 program 应为空"
        );
    }

    /// `ops` 为空是**请求本身**不合法 → 400 且无 data（前端提示"批量生成失败"）。
    #[test]
    fn part_generate_empty_ops_is_400_without_data() {
        let ctx = test_ctx();
        let body = serde_json::json!({"part": {"ops": []}});
        let Resp::Json(status, payload) = route(
            &ctx,
            "POST",
            "/api/part/generate",
            "",
            body.to_string().as_bytes(),
        ) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 400);
        assert_eq!(payload["ok"], false);
        assert!(
            payload.get("data").is_none(),
            "形状错误不应带 data: {payload}"
        );
        assert!(
            payload["error"]["message"]
                .as_str()
                .unwrap()
                .contains("至少要有一道工序"),
            "{payload}"
        );
    }

    /// 缺 `part` 字段给出可操作提示，而不是含糊的 400。
    #[test]
    fn part_generate_missing_part_field_is_actionable_400() {
        let ctx = test_ctx();
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/part/generate", "", b"{}")
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 400);
        assert!(
            payload["error"]["message"]
                .as_str()
                .unwrap()
                .contains("part"),
            "应指出缺哪个字段: {payload}"
        );
    }

    /// 零件定义里的 `params` 用**扁平形式**（与 `--params-file` 同形）。
    ///
    /// 回归：`ParameterSet` 只有派生 `Deserialize` 时，扁平输入会被静默解析成
    /// 空集 —— "参数传了却全部缺失"，报错还指向模板。这条守卫住那个坑。
    #[test]
    fn part_generate_accepts_flat_params_without_silent_loss() {
        let ctx = test_ctx();
        let body = serde_json::json!({
            "part": {
                "ops": [{"template": "program_header", "params": {"prog": 4242, "part_name": "X"}}]
            }
        });
        let Resp::Json(status, payload) = route(
            &ctx,
            "POST",
            "/api/part/generate",
            "",
            body.to_string().as_bytes(),
        ) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "扁平 params 应被识别: {payload}");
        let out = payload["data"]["results"][0]["output"].as_str().unwrap();
        assert!(
            out.contains("O4242"),
            "工序级 prog 必须真的送到渲染，不能静默变空集: {out}"
        );
        assert!(out.contains("X"), "{out}");
    }

    /// 旧键名 `operations` 不再被识别（前端契约定为 `ops`）。
    ///
    /// 这条是**防止悄悄兼容两种名字**：若哪天为兼容加了 alias，
    /// 前端与后端就会各写一套而无人发现漂移。
    ///
    /// 【2026-09-24 收紧，P2-3】`PartSpec` 加了 `deny_unknown_fields` 后，失败原因
    /// 从「解析成功但 `ops` 为空 → 形状校验报『至少要有一道工序』」变成
    /// **解析阶段就点名未知字段** —— 后者才是对的：用户写错的是**字段名**，
    /// 说「你没给工序」会把人引到错误的方向。断言改为要求错误点名 `operations`。
    #[test]
    fn part_generate_legacy_operations_key_is_not_silently_accepted() {
        let ctx = test_ctx();
        let body = serde_json::json!({
            "part": {"operations": [{"template": "program_header"}]}
        });
        let Resp::Json(status, payload) = route(
            &ctx,
            "POST",
            "/api/part/generate",
            "",
            body.to_string().as_bytes(),
        ) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 400, "`operations` 不是契约字段，应被拒: {payload}");
        let msg = payload["error"]["message"].as_str().unwrap();
        assert!(
            msg.contains("operations"),
            "错误应点名写错的字段名（而不是笼统说缺少工序）: {msg}"
        );
        assert!(
            msg.contains("unknown field") || msg.contains("未知字段"),
            "应明确这是未知字段而非缺工序: {msg}"
        );
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

    /// 进程内真实服务测试（补 `ui.rs` / `serve()` 覆盖率，A3）。
    ///
    /// 为什么不用 `cli/tests/` 里的 spawn-and-kill E2E：llvm-cov 只在**进程干净退出**
    /// 时才落盘 profile 数据；E2E 里服务被杀掉中途 kill，其 `run()` / `serve()` 的
    /// 覆盖永远丢失（实测 `serve()` 循环体 0 覆盖，而 E2E 确实发过 HTTP 请求）。
    /// 这里在**测试进程内**跑 `serve`，用 `Server::unblock()` 让它干净退出循环。
    #[test]
    fn serve_handles_real_request_then_unblocks_cleanly() {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        let (srv, actual) = bind("127.0.0.1:0".parse().unwrap()).expect("绑定回环应成功");
        let port = actual.port();
        // 借用版：serve 完成后测试侧仍持有 srv，才能调 unblock() 让循环干净退出
        // （scoped thread 借用 srv，作用域结束即 join，天然同步）。
        let handle = std::thread::scope(|scope| {
            let srv_ref = &srv;
            let t = scope.spawn(move || serve_requests(srv_ref, actual, &Ctx::for_test()));

            // 真实 HTTP 请求：GET /health（走完 route() 的 JSON 分支）
            let mut stream = TcpStream::connect_timeout(
                &format!("127.0.0.1:{port}").parse().unwrap(),
                Duration::from_millis(500),
            )
            .expect("应能连上服务");
            stream
                .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
                .unwrap();
            stream
                .set_read_timeout(Some(Duration::from_millis(1000)))
                .unwrap();
            let mut buf = String::new();
            let _ = stream.read_to_string(&mut buf);
            assert!(
                buf.contains("HTTP/1.1 200") && buf.contains("\"status\":\"ok\""),
                "应返回 200 与健康数据: {buf}"
            );

            // 干净关停：unblock 让 incoming_requests() 返回 None，循环正常结束
            srv.unblock();
            t.join().expect("serve 线程应正常结束")
        });
        assert!(handle.is_ok(), "serve 应返回 Ok: {handle:?}");
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

        // `schema`：机床键规格（键 / 类型 / 默认值 / 含义），供 UI 只读展示。
        let schema = payload["data"]["schema"]
            .as_array()
            .expect("schema 应为数组");
        assert!(!schema.is_empty(), "schema 不应为空");
        let prefix = schema
            .iter()
            .find(|s| s["key"] == "program_prefix")
            .expect("schema 应含 program_prefix");
        assert_eq!(prefix["kind"], "String");
        assert_eq!(prefix["default"], "O");
        assert!(prefix["description"]
            .as_str()
            .is_some_and(|d| !d.is_empty()));

        // Choice 键带候选值（其余键 `options` 为 null）。
        let units = schema
            .iter()
            .find(|s| s["key"] == "units")
            .expect("应含 units");
        assert_eq!(units["kind"], "Choice");
        assert_eq!(units["options"], serde_json::json!(["metric", "imperial"]));
        assert_eq!(prefix["options"], serde_json::Value::Null);

        // 键规格与 core 的单一来源逐条对齐（数目 + 键名都不得漂移）。
        assert_eq!(
            schema.len(),
            nctool_core::machine::KNOWN_CONFIG_KEYS.len(),
            "schema 应逐条覆盖 KNOWN_CONFIG_KEYS"
        );
        for (got, want) in schema
            .iter()
            .zip(nctool_core::machine::KNOWN_CONFIG_KEYS.iter())
        {
            assert_eq!(got["key"], want.key);
        }
    }

    /// 机床写端点只接受 POST 与严格 JSON；不支持的方法仍为 404。
    #[test]
    fn machine_write_routes_validate_methods_and_bodies() {
        for (method, path, expected) in [
            ("POST", "/api/machines", 400),
            ("PUT", "/api/machines", 404),
            ("DELETE", "/api/machines", 404),
            ("POST", "/api/machines/delete", 400),
        ] {
            let Resp::Json(status, payload) = route(&test_ctx(), method, path, "", b"{}") else {
                panic!("未命中路由应返回 JSON")
            };
            assert_eq!(status, expected, "{method} {path}: {payload}");
        }
    }

    #[test]
    fn machine_delete_rejects_current_default_machine() {
        let mut ctx = test_ctx();
        ctx.default_machine = Some("custom_lathe".into());
        let body = json_body(&serde_json::json!({"id":"custom_lathe"}));
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/machines/delete", "", &body)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 409, "默认机床必须先解除引用：{payload}");
        assert_eq!(payload["error"]["kind"], "default_machine");
    }

    #[test]
    fn machine_save_and_delete_share_the_guarded_writer_path() {
        let path = std::env::temp_dir().join(format!(
            "nctool_http_machine_{}_{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut machine = nctool_core::machine::MachinePreset::from_id("generic")
            .unwrap()
            .config();
        machine.id = "http_custom".into();
        machine.vendor = "Test".into();
        machine.model = "HTTP".into();
        let save = json_body(&serde_json::json!({
            "id": machine.id,
            "vendor": machine.vendor,
            "model": machine.model,
            "config": machine.config,
            "expectHash": null,
        }));
        let ctx = test_ctx();
        let Resp::Json(status, saved) = machine_save_at(&ctx, &save, &path) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "机床应通过共享预校验并保存：{saved}");
        let fingerprint = saved["data"]["fileFingerprint"].as_str().unwrap();
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("[machine.http_custom]"));

        let delete = json_body(&serde_json::json!({
            "id": "http_custom",
            "expectHash": fingerprint,
        }));
        let Resp::Json(status, deleted) = machine_delete_at(&ctx, &delete, &path) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "机床应通过共享删除用例：{deleted}");
        assert_eq!(deleted["data"]["action"], "deleted");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn template_create_reports_missing_manifest_as_partial_success() {
        let root = std::env::temp_dir().join(format!(
            "nctool_template_create_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut ctx = test_ctx();
        ctx.template_dir = Some(root.clone());
        let body = json_body(&serde_json::json!({"name":"new_part","category":"milling"}));
        let Resp::Json(status, payload) = route(&ctx, "POST", "/api/templates/create", "", &body)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "模板文件应已创建：{payload}");
        assert!(root.join("new_part.j2").is_file());
        assert_eq!(payload["data"]["name"], "new_part.j2");
        assert!(payload["data"]["manifestWarning"].as_str().is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn template_asset_workflow_uses_guarded_create_save_derive_and_rename() {
        let root = std::env::temp_dir().join(format!(
            "nctool_http_templates_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("templates.yaml"), "templates:\n").unwrap();
        let mut ctx = test_ctx();
        ctx.template_dir = Some(root.clone());

        let create = json_body(&serde_json::json!({"name":"part","category":"milling"}));
        let Resp::Json(status, created) = route(&ctx, "POST", "/api/templates/create", "", &create)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "创建应成功：{created}");
        assert_eq!(created["data"]["name"], "part.j2");
        assert!(created["data"]["manifestWarning"].is_null());
        let initial_hash = created["data"]["fingerprint"].as_str().unwrap();
        let listed = route(&ctx, "GET", "/api/templates", "", b"");
        let Resp::Json(_, listed) = listed else {
            panic!("应返回 JSON")
        };
        assert_eq!(
            listed["data"]["templates"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == "part.j2")
                .unwrap()["status"],
            "unreviewed"
        );
        let Resp::Json(status, detail) = route(&ctx, "GET", "/api/templates/part.j2", "", b"")
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "模板详情应返回评审状态：{detail}");
        assert_eq!(detail["data"]["template"]["status"], "unreviewed");

        let save = json_body(&serde_json::json!({
            "name": "part.j2",
            "source": "G0 X{{ x }}\n",
            "expectHash": initial_hash,
        }));
        let Resp::Json(status, saved) = route(&ctx, "POST", "/api/templates/save", "", &save)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "保存应成功：{saved}");
        let refreshed_hash = saved["data"]["fingerprint"].as_str().unwrap();

        let derive = json_body(&serde_json::json!({
            "sourceName": "part.j2",
            "newName": "copy",
        }));
        let Resp::Json(status, derived) = route(&ctx, "POST", "/api/templates/derive", "", &derive)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "派生应成功：{derived}");
        assert_eq!(derived["data"]["name"], "copy.j2");
        assert_ne!(refreshed_hash, "");

        let rename = json_body(&serde_json::json!({
            "oldName": "copy.j2",
            "newName": "renamed",
        }));
        let Resp::Json(status, renamed) = route(&ctx, "POST", "/api/templates/rename", "", &rename)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "重命名应成功：{renamed}");
        assert_eq!(renamed["data"]["name"], "renamed.j2");
        assert!(!root.join("copy.j2").exists());
        assert!(root.join("renamed.j2").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lint_accepts_unsaved_source_from_editor() {
        let body = json_body(&serde_json::json!({
            "template": "not_saved.j2",
            "source": "{{ 30 | sin }}",
        }));
        let Resp::Json(status, payload) = route(&test_ctx(), "POST", "/api/lint", "", &body) else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "无需磁盘模板即可检查编辑器内容：{payload}");
        assert_eq!(payload["data"][0]["filter"], "sin");
        assert_eq!(payload["data"][0]["suggestion"], "sin_d");
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

    /// P0-3（cli-review P0-1）回归：含绝对路径的内部错误经 `/api/validate`、
    /// `/api/render` 回传时，响应体必须是泛化文案——路径详情只进 stderr。
    /// 最典型的路径载体是 `build_registry` 的 `模板目录不存在: <绝对路径>`，
    /// 它在 `registered_template` 处经 `internal_error` 落地为 500；
    /// `validate`/`render` 里 `registry().validate()` 的 `Err` 分支（此前
    /// `err("registry", e.to_string())` 直接回传）已改为同一口径。
    #[test]
    fn http_error_bodies_never_contain_absolute_paths() {
        let mut ctx = test_ctx();
        let missing =
            std::env::temp_dir().join(format!("nctool_p03_missing_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&missing);
        assert!(!missing.exists(), "测试前提：目录不存在");
        ctx.template_dir = Some(missing.clone());
        let marker = missing
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .expect("temp_dir 应有最后一段");
        for path in ["/api/validate", "/api/render"] {
            let body = br#"{"template":"drill_cycle","params":{}}"#;
            let Resp::Json(status, payload) = route(&ctx, "POST", path, "", body) else {
                panic!("{path} 应返回 JSON")
            };
            assert_eq!(status, 500, "{path}: {payload}");
            assert_eq!(payload["error"]["kind"], "internal", "{path}: {payload}");
            let text = payload.to_string();
            assert!(!text.contains(&marker), "{path} 不得回显服务器路径: {text}");
            assert!(!text.contains(".j2"), "{path} 不得回显 .j2: {text}");
            // 裸 `C:\` 与 JSON 转义后的 `C:\\` 都要堵死
            assert!(
                !text.contains(":\\") && !text.contains("\\\\"),
                "{path} 不得回显 Windows 路径: {text}"
            );
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

    /// 选项对拍门禁（隐患修复）：CLI 的生成选项与 Web API 的 `options` 对象必须
    /// 映射到同一份 `GenerationOptions`。fixture 是唯一来源，两侧各有一份消费点：
    /// 本测试跑 JSON 侧（`generation_options`），并与 CLI 侧对照
    /// （`RenderArgs` 的字段由 `commands::render` 1:1 赋值）。
    ///
    /// 背景：`--line-step` / `--max-line` 曾只存在于 Web UI，CLI 无法复现
    /// 同一份带自定义步进的输出 —— 同一份参数在两个入口产出不同 G-code。
    #[test]
    fn option_mapping_matches_shared_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../scripts/option_parity_cases.json"))
                .expect("option_parity_cases.json 必须是合法 JSON");
        let cases = fixture["cases"]
            .as_array()
            .expect("fixture 需有 cases 数组");
        assert!(!cases.is_empty(), "fixture 不应为空");

        for (i, case) in cases.iter().enumerate() {
            let name = case["name"].as_str().unwrap_or("?");
            let json = &case["json"];

            // —— JSON 侧：走真实函数 —————————————————————————
            let body = serde_json::json!({ "options": json });
            let (opts, lenient) = generation_options(&body)
                .unwrap_or_else(|_| panic!("case #{i} {name}: generation_options 失败"));

            let e = &case["expect"];
            assert_eq!(
                opts.line_numbers,
                e["line_numbers"].as_bool().unwrap(),
                "case #{i} {name}: line_numbers"
            );
            assert_eq!(
                opts.line_number_step,
                e["line_number_step"].as_u64().unwrap() as u32,
                "case #{i} {name}: line_number_step"
            );
            assert_eq!(
                opts.max_line_number,
                e["max_line_number"].as_u64().unwrap() as u32,
                "case #{i} {name}: max_line_number"
            );
            assert_eq!(
                opts.add_header_comment,
                e["add_header_comment"].as_bool().unwrap(),
                "case #{i} {name}: add_header_comment"
            );
            assert_eq!(
                opts.strip_blank_lines,
                e["strip_blank_lines"].as_bool().unwrap(),
                "case #{i} {name}: strip_blank_lines"
            );
            assert_eq!(
                opts.ascii_only,
                e["ascii_only"].as_bool().unwrap(),
                "case #{i} {name}: ascii_only"
            );
            assert_eq!(
                lenient,
                e["lenient"].as_bool().unwrap(),
                "case #{i} {name}: lenient"
            );

            // —— CLI 侧：从 cli 字段构出同一结构，必须与 JSON 侧一致 ——————
            // （cli 字段名 = RenderArgs 的 snake_case；commands::render 把
            //   它们 1:1 赋给 GenerationOptions，因此这里直接构造即代表 CLI 行为）
            let cli = &case["cli"];
            let cli_opts = GenerationOptions {
                format: OutputFormat::Gcode,
                line_numbers: cli["line_numbers"].as_bool().unwrap_or(false),
                line_number_step: cli["line_step"].as_u64().unwrap_or(10) as u32,
                max_line_number: cli["max_line"].as_u64().unwrap_or(9999) as u32,
                // 两个入口都是单段渲染，起始行号恒为 0（续编游标只属于 `part generate`）
                line_number_start: 0,
                add_header_comment: cli["header"].as_bool().unwrap_or(false),
                strip_blank_lines: cli["strip_blank"].as_bool().unwrap_or(false),
                ascii_only: cli["ascii"].as_bool().unwrap_or(false),
            };
            let cli_lenient = cli["lenient"].as_bool().unwrap_or(false);

            assert_eq!(
                cli_opts, opts,
                "case #{i} {name}: CLI 与 API 映射到不同的 GenerationOptions\n\
                 cli={cli_opts:?}\napi={opts:?}"
            );
            assert_eq!(cli_lenient, lenient, "case #{i} {name}: lenient 不一致");
        }
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

    // =======================================================================
    // 覆盖率补测（P1-2c）
    //
    // 背景：`cli` 库化（lib + bin）后 `server.rs` 成为**公开 API 面**被完整插桩，
    // 一批此前在 bin 目标里被死代码消除、从未进过分母的**生产**分支（错误路径、
    // 边界、`serve_requests` 的真实 HTTP 分支）现在需要覆盖。以下用例全部沿用
    // 本文件既有的测试风格（直接调 `route()` / 进程内真实 HTTP），不引入新依赖。
    // =======================================================================

    /// 写内核错误 → HTTP 响应的**全部**分支（含 `#[non_exhaustive]` 兜底语义）。
    ///
    /// 状态码按语义而非一律 400：乐观锁冲突 409、名称非法 400、只读/IO/损坏 500、
    /// 条目不存在 404。这些分支此前只在真实写盘异常时才走到，常规用例碰不到。
    #[test]
    fn write_error_resp_maps_every_variant() {
        let Resp::Json(s, p) = write_error_resp(WriteError::Conflict {
            path: PathBuf::from("C:/secret/dir/presets.yaml"),
            expected: None,
            actual: None,
        }) else {
            panic!("应为 JSON")
        };
        assert_eq!(s, 409, "乐观锁冲突应可重试（409）");
        assert_eq!(p["error"]["kind"], "write_conflict");
        // P1-17 口径：4xx 响应体也不回显绝对路径——保留文件名供识别，目录段不得出现
        let msg = p["error"]["message"].as_str().unwrap_or_default();
        assert!(msg.contains("presets.yaml"), "应保留文件名：{msg}");
        assert!(!msg.contains("secret"), "不得回显绝对路径：{msg}");

        // LockBusy（P0-1 锁争用）：同 Conflict 归 409 write_conflict，但正文
        // 更严格 —— 连文件名都不带，固定脱敏文案（架构 §3.1 约束 A：绝不照抄
        // Conflict 的 path 形状，路径只进服务端 stderr）
        let Resp::Json(s, p) = write_error_resp(WriteError::LockBusy {
            path: PathBuf::from("C:/secret/dir/presets.yaml"),
        }) else {
            panic!("应为 JSON")
        };
        assert_eq!(s, 409, "锁争用应可重试（409）");
        assert_eq!(p["error"]["kind"], "write_conflict");
        let msg = p["error"]["message"].as_str().unwrap_or_default();
        assert!(msg.contains("另一个 nctool 进程"), "应说明锁争用：{msg}");
        assert!(msg.contains("稍后重试"), "应给出重试指引：{msg}");
        assert!(!msg.contains("secret"), "不得回显绝对路径：{msg}");
        assert!(!msg.contains("presets.yaml"), "锁争用正文不带路径段：{msg}");

        // PathEscape：reason 含「已存在」→ 409 name_conflict
        let Resp::Json(s, p) = write_error_resp(WriteError::PathEscape {
            rel: "p1".into(),
            reason: "同名预设已存在".into(),
        }) else {
            panic!("应为 JSON")
        };
        assert_eq!(s, 409);
        assert_eq!(p["error"]["kind"], "name_conflict");

        // PathEscape：其它原因 → 400 bad_request（调用方能改名重试）
        let Resp::Json(s, p) = write_error_resp(WriteError::PathEscape {
            rel: "../evil".into(),
            reason: "路径越界".into(),
        }) else {
            panic!("应为 JSON")
        };
        assert_eq!(s, 400);
        assert_eq!(p["error"]["kind"], "bad_request");

        // ReadOnly → 500 internal（本地环境问题，调用方无从修正）
        let Resp::Json(s, p) = write_error_resp(WriteError::ReadOnly {
            path: PathBuf::from("presets.yaml"),
        }) else {
            panic!("应为 JSON")
        };
        assert_eq!(s, 500);
        assert_eq!(p["error"]["kind"], "internal");

        // NotFound → 404 preset_not_found（**不是** 500：这是调用方问题）
        let Resp::Json(s, p) = write_error_resp(WriteError::NotFound("没有这个预设".into()))
        else {
            panic!("应为 JSON")
        };
        assert_eq!(s, 404);
        assert_eq!(p["error"]["kind"], "preset_not_found");

        // Corrupt / Io → 500 internal（正文不回显细节）
        let Resp::Json(s, p) = write_error_resp(WriteError::Corrupt("内容坏了".into())) else {
            panic!("应为 JSON")
        };
        assert_eq!(s, 500);
        assert_eq!(p["error"]["kind"], "internal");

        let Resp::Json(s, p) = write_error_resp(WriteError::Io(std::io::Error::other("磁盘错误")))
        else {
            panic!("应为 JSON")
        };
        assert_eq!(s, 500);
        assert_eq!(p["error"]["kind"], "internal");

        // NumUnderflow → 400 num_underflow（P1-11 前掉进 `_` 被报成 500）；
        // 正文给行/列/字面量，**不**带文件路径（表 cli_msg_with_path=false）
        let Resp::Json(s, p) = write_error_resp(WriteError::NumUnderflow {
            path: PathBuf::from("C:/secret/presets.yaml"),
            literal: "1e-400".into(),
            line: 7,
            column: 12,
        }) else {
            panic!("应为 JSON")
        };
        assert_eq!(s, 400, "{p}");
        assert_eq!(p["error"]["kind"], "num_underflow", "{p}");
        let msg = p["error"]["message"].as_str().unwrap_or_default();
        assert!(msg.contains("1e-400"), "应带字面量：{msg}");
        assert!(!msg.contains("secret"), "不得回显绝对路径：{msg}");
    }

    /// `presets_save` / `presets_delete` / `inspect` 的**请求体校验**分支。
    ///
    /// 三者的共同契约：非法 JSON / 缺字段 / 空字段都要 400 且点名缺哪个字段 ——
    /// 若某个端点漏了这道校验，它会把 `""` 当名字/模板名去查注册表 → 报 404 而非
    /// 400，前端拿到的错误分类就错了。
    #[test]
    fn post_endpoints_reject_malformed_bodies() {
        let env = preset_endpoint_test("badbody");
        let ctx = env.ctx();

        // 非法 JSON：三个 POST 端点各自报 bad_request
        for path in ["/api/presets", "/api/presets/delete", "/api/inspect"] {
            let Resp::Json(s, p) = route(&ctx, "POST", path, "", b"{not json") else {
                panic!("{path} 应返回 JSON")
            };
            assert_eq!(s, 400, "{path}: {p}");
            assert_eq!(p["error"]["kind"], "bad_request", "{path}: {p}");
            assert!(
                p["error"]["message"].as_str().unwrap().contains("JSON"),
                "{path} 应说明请求体不是合法 JSON: {p}"
            );
        }

        // 缺 name（presets_save / presets_delete）
        for path in ["/api/presets", "/api/presets/delete"] {
            let Resp::Json(s, p) = route(&ctx, "POST", path, "", br#"{"template":"t.j2"}"#) else {
                panic!("{path} 应返回 JSON")
            };
            assert_eq!(s, 400, "{path}: {p}");
            assert!(
                p["error"]["message"].as_str().unwrap().contains("name"),
                "{path} 应点名缺 name: {p}"
            );
        }

        // presets_save：缺 template
        let Resp::Json(s, p) = route(&ctx, "POST", "/api/presets", "", br#"{"name":"n"}"#) else {
            panic!("应返回 JSON")
        };
        assert_eq!(s, 400, "{p}");
        assert!(
            p["error"]["message"].as_str().unwrap().contains("template"),
            "应点名缺 template: {p}"
        );

        // inspect：空 template 视同缺（否则会拿 "" 去查注册表 → 404）
        let Resp::Json(s, p) = route(&ctx, "POST", "/api/inspect", "", br#"{"template":""}"#)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(s, 400, "{p}");
        assert_eq!(p["error"]["kind"], "bad_request");
    }

    /// `part_generate` 的请求体错误：形状不合法 → **反序列化失败**点名（400，不是 500）；
    /// 形状合法但 `ops` 为空 → **语义错误**（400 `InvalidSpec`）。
    ///
    /// 两者都必须是 400：形状错是调用方发错了结构，语义错是调用方发了空零件 ——
    /// 都是调用方能自己修正的问题，不该报 500（那会让前端显示"服务端故障"）。
    #[test]
    fn part_generate_bad_part_shape_is_400() {
        let ctx = test_ctx();
        // ① 数字 / 字符串：`serde_json::from_value::<PartSpec>` 失败
        for body in [r#"{"part":123}"#, r#"{"part":"x"}"#] {
            let Resp::Json(status, payload) =
                route(&ctx, "POST", "/api/part/generate", "", body.as_bytes())
            else {
                panic!("应返回 JSON")
            };
            assert_eq!(status, 400, "body={body}: {payload}");
            assert_eq!(payload["error"]["kind"], "bad_request", "body={body}");
            assert!(
                payload["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("零件定义"),
                "应说明是零件定义解析失败: {payload}"
            );
        }

        // ② `[]` 实测可反序列化成"空 ops"，属**语义**错误（InvalidSpec），仍是 400
        let Resp::Json(status, payload) =
            route(&ctx, "POST", "/api/part/generate", "", br#"{"part":[]}"#)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 400, "{payload}");
        assert_eq!(payload["error"]["kind"], "bad_request", "{payload}");
        assert!(
            payload["error"]["message"]
                .as_str()
                .unwrap()
                .contains("工序"),
            "空零件应点名缺少工序: {payload}"
        );
    }

    /// `render` 的**宽松分支**：`lenient: true` 时走 `generate_lenient_outcome`。
    ///
    /// 严格/宽松是两条不同的路径：严格先做校验、有错就 `blocked:true` 拦下；宽松
    /// **跳过校验拦截**、直接尝试生成（`map(|(o, _report)| o)` 丢掉报告）。但
    /// 「跳过校验」≠「保证成功」—— 生成阶段遇到不可用的参数（缺失 / 类型错）仍会
    /// 报错（400 `render`）。此前只测了严格分支，宽松分支的产出路径从未执行。
    #[test]
    fn render_lenient_uses_lenient_outcome() {
        // ① 合法参数 + 宽松 → 正常产出（覆盖 `if lenient { generate_lenient_outcome(..) }`）
        let body = br#"{"template":"drill_cycle","params":{"x":21,"y":15,"depth":-10,"feed":100},"options":{"lenient":true}}"#;
        let Resp::Json(status, payload) = route(&test_ctx(), "POST", "/api/render", "", body)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");
        assert_eq!(payload["data"]["blocked"], false);
        assert!(
            payload["data"]["output"]
                .as_str()
                .is_some_and(|s| s.contains("X21.000")),
            "宽松模式同样要产出 G-code: {payload}"
        );

        // ② 缺参数 + 宽松 → 跳过校验拦截，但生成仍失败 → 400 render（**不是** blocked）
        let lenient_missing =
            br#"{"template":"drill_cycle","params":{},"options":{"lenient":true}}"#;
        let Resp::Json(status, payload) =
            route(&test_ctx(), "POST", "/api/render", "", lenient_missing)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 400, "宽松模式不保证生成成功: {payload}");
        assert_eq!(payload["error"]["kind"], "render", "{payload}");

        // ③ 对照：同样缺参数但**不**宽松 → 200 + blocked:true（走校验拦截路径）
        let strict_missing = br#"{"template":"drill_cycle","params":{}}"#;
        let Resp::Json(status, payload) =
            route(&test_ctx(), "POST", "/api/render", "", strict_missing)
        else {
            panic!("应返回 JSON")
        };
        assert_eq!(status, 200, "{payload}");
        assert_eq!(payload["data"]["blocked"], true, "{payload}");
    }

    /// `serve_requests` 的真实 HTTP 分支：内嵌页面（`Resp::Html`）、query 串、
    /// 跨站拦截（403）、超大请求体（413）。
    ///
    /// 既有用例只发了一条 `GET /health`。这里把**同一进程内**的请求循环按分支补齐，
    /// 用 `Server::unblock()` 让它干净退出（llvm-cov 只在进程干净退出时落盘数据）。
    ///
    /// 两处**必须**这么写，否则测试会假红或直接挂死（都是实测踩过的坑）：
    ///
    /// 1. **按原始字节读**，不用 `read_to_string`：`GET /` 的响应体走
    ///    `Transfer-Encoding: chunked`，分块边界可能落在多字节 UTF-8 字符中间，
    ///    于是 `read_to_string` 判定为非法 UTF-8、**丢弃整个缓冲区**（返回 Err 且
    ///    buf 为空）—— 断言就会看到"空响应"。改读 `Vec<u8>` 再 lossy 解码即可。
    /// 2. **断言移到作用域之外 + `unblock` 兜底守卫**：若在 `thread::scope` 内断言
    ///    失败，scope 会先 join 那个仍阻塞在 `incoming_requests()` 上的服务线程，
    ///    而 `unblock()` 还没被调用 → 整个测试进程挂死（实测挂了 24 分钟）。
    #[test]
    fn serve_requests_covers_html_query_guard_and_limits() {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        // 无论后面怎样退出（含 panic），都保证 unblock，避免 scope 在 join 时挂死。
        struct UnblockOnDrop<'a>(&'a tiny_http::Server);
        impl Drop for UnblockOnDrop<'_> {
            fn drop(&mut self) {
                self.0.unblock();
            }
        }

        let (srv, actual) = bind("127.0.0.1:0".parse().unwrap()).expect("绑定回环应成功");
        let port = actual.port();

        // 发一条原始 HTTP 请求并读回**全部字节**（直到对端关闭或读超时）。不 panic：
        // 连接/读写失败时回一个带前缀的说明串，交给外层断言报错。
        let send = |raw: &str| -> String {
            let mut buf: Vec<u8> = Vec::new();
            match TcpStream::connect_timeout(
                &format!("127.0.0.1:{port}").parse().unwrap(),
                Duration::from_millis(1000),
            ) {
                Ok(mut stream) => {
                    let _ = stream.write_all(raw.as_bytes());
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(2000)));
                    let mut chunk = [0u8; 8192];
                    loop {
                        match stream.read(&mut chunk) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                }
                Err(e) => return format!("<connect error: {e}>"),
            }
            String::from_utf8_lossy(&buf).into_owned()
        };

        let (html, idx, q, cross, too_large, joined) = std::thread::scope(|scope| {
            let srv_ref = &srv;
            let t = scope.spawn(move || serve_requests(srv_ref, actual, &Ctx::for_test()));
            let _guard = UnblockOnDrop(&srv);

            // ① GET / → 内嵌前端（`Resp::Html` 分支 + 安全响应头）
            let html = send("GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
            // ② GET /index.html → 同一分支
            let idx =
                send("GET /index.html HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
            // ③ query 串（url.split_once('?') 的 Some 分支）
            let q = send(
                "GET /health?verbose=1 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            );
            // ④ 跨站 Origin → 403（cross_site_guard 先于路由）
            let cross = send(
                "GET /api/templates HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: http://evil.example\r\nConnection: close\r\n\r\n",
            );
            // ⑤ 请求体恰好超过 1 MiB → 413（Content-Length 与实际等长，避免半读死锁）
            let n = MAX_BODY_BYTES + 1;
            let body = "x".repeat(n);
            let raw = format!(
                "POST /api/validate HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {n}\r\n\r\n{body}"
            );
            let too_large = send(&raw);

            srv.unblock();
            let joined = t.join();
            (html, idx, q, cross, too_large, joined)
        });

        assert!(html.contains("HTTP/1.1 200"), "根路径应回内嵌页面: {html}");
        assert!(
            html.contains("Content-Security-Policy"),
            "静态页面也要带安全头: {html}"
        );
        assert!(
            idx.contains("HTTP/1.1 200"),
            "index.html 应回内嵌页面: {idx}"
        );
        assert!(q.contains("\"status\":\"ok\""), "query 串应被正确切分: {q}");
        assert!(cross.contains("403"), "跨站应被拒: {cross}");
        assert!(cross.contains("forbidden_origin"), "{cross}");
        assert!(too_large.contains("413"), "超大请求体应回 413: {too_large}");
        assert!(too_large.contains("payload_too_large"), "{too_large}");
        assert!(joined.is_ok(), "serve 应返回 Ok: {joined:?}");
    }

    // -----------------------------------------------------------------------
    // P1-7 / P1-8：请求体读取的**错误归因**与**读超时**
    // -----------------------------------------------------------------------

    /// 发一条原始 HTTP 请求并读回全部响应字节。
    ///
    /// - `half_close = true`：写完 `body` 后 `shutdown(Write)`，让服务端读到 EOF。
    ///   这是"半包"的形态，也是**唯一**能让服务端读操作确定性地返回的方式
    ///   （不关就变成"挂着"，那是另一条用例）。
    /// - `half_close = false`：保持连接，服务端靠 `Connection: close` 自行收尾。
    fn raw_http(port: u16, head: &str, body: &[u8], half_close: bool) -> String {
        use std::io::{Read, Write};
        use std::net::{Shutdown, TcpStream};

        let mut stream = TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(1000),
        )
        .expect("应能连上服务");
        stream.write_all(head.as_bytes()).expect("写请求头");
        if !body.is_empty() {
            stream.write_all(body).expect("写请求体");
        }
        if half_close {
            let _ = stream.shutdown(Shutdown::Write);
        }
        let _ = stream.set_read_timeout(Some(Duration::from_millis(3000)));
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// 起一个进程内服务，把 `f(port)` 的返回值与 `serve` 的返回值一起交回。
    fn with_server<T>(timeout: Duration, f: impl FnOnce(u16) -> T) -> (T, Result<(), CliError>) {
        with_server_and_reader_limit(timeout, MAX_IN_FLIGHT_BODY_READERS, |port, _| f(port))
    }

    fn with_server_and_reader_limit<T>(
        timeout: Duration,
        max_in_flight_body_readers: usize,
        f: impl FnOnce(u16, Arc<AtomicUsize>) -> T,
    ) -> (T, Result<(), CliError>) {
        let (srv, actual) = bind("127.0.0.1:0".parse().unwrap()).expect("绑定回环应成功");
        let port = actual.port();
        let in_flight = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            let srv_ref = &srv;
            let counter = Arc::clone(&in_flight);
            // 测试中的断言若失败，也必须解除 incoming_requests()，否则 scope 会等
            // 一个仍阻塞在服务循环里的线程，CI 表现为永久卡在 Test 步骤。
            struct UnblockOnDrop<'a>(&'a tiny_http::Server);
            impl Drop for UnblockOnDrop<'_> {
                fn drop(&mut self) {
                    self.0.unblock();
                }
            }
            let _unblock = UnblockOnDrop(&srv);
            let t = scope.spawn(move || {
                serve_requests_with_timeout_and_counter(
                    srv_ref,
                    actual,
                    &Ctx::for_test(),
                    timeout,
                    counter,
                    max_in_flight_body_readers,
                )
            });
            let out = f(port, in_flight);
            srv.unblock();
            let joined = t.join().expect("serve 线程应正常结束");
            (out, joined)
        })
    }

    /// P1-7：**半包请求体必须回 400，绝不能当完整请求交给 `route()`**。
    ///
    /// 声明 2000 字节、只发 16 字节后关掉写端。tiny_http 的 `EqualReader` 在底层
    /// EOF 时返回 `Ok(0)`（**短读不报错**），所以只靠 `read_to_end` 的 `Err`
    /// 抓不到这种请求 —— 修复前它会被原样交给 `route()`，用户看到的是
    /// "模板不存在: {"template":"x"}"（把截断的 JSON 当成了模板名），
    /// 排障方向完全错。
    #[test]
    fn truncated_body_is_rejected_with_400_not_routed() {
        let (resp, joined) = with_server(BODY_READ_TIMEOUT, |port| {
            raw_http(
                port,
                "POST /api/inspect HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: 2000\r\n\r\n",
                br#"{"template":"x"}"#,
                true,
            )
        });
        assert!(joined.is_ok(), "serve 应返回 Ok: {joined:?}");
        assert!(resp.contains("400"), "半包必须回 400: {resp}");
        assert!(resp.contains("bad_request"), "{resp}");
        assert!(
            resp.contains("请求体不完整"),
            "应明确说是请求体不完整（而不是让下游报模板不存在）: {resp}"
        );
        assert!(
            !resp.contains("template_not_found"),
            "截断的请求体不得被路由: {resp}"
        );
    }

    /// P1-7：`Content-Length` 超限要在**读取之前**就拒（413）。
    ///
    /// 声明 8 MiB 却一个字节都不发、随即关掉写端。有预检 → 立刻 413；
    /// 没有预检 → 走进读取路径，读到 0 字节后因长度对不上回 400
    /// （修复前更是会拿空 body 去 `route()`）。断言 413 就能区分两者。
    #[test]
    fn declared_length_over_limit_is_rejected_before_reading() {
        let declared = 8 * 1024 * 1024;
        let (resp, joined) = with_server(BODY_READ_TIMEOUT, |port| {
            raw_http(
                port,
                &format!(
                    "POST /api/inspect HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {declared}\r\n\r\n"
                ),
                b"",
                true,
            )
        });
        assert!(joined.is_ok(), "serve 应返回 Ok: {joined:?}");
        assert!(resp.contains("413"), "声明超限应立刻 413: {resp}");
        assert!(resp.contains("payload_too_large"), "{resp}");
    }

    /// P1-7 的反面守卫：**合法的大请求体（> 1024 字节）不能被新加的长度对账误伤**。
    ///
    /// 用尾部空白把 body 撑到 3000 字节（JSON 允许尾随空白，`serde_json` 照常解析），
    /// `Content-Length` 与实际等长。这条走的是 tiny_http 的 `EqualReader` 惰性读
    /// 分支（`content_length > 1024`），正是本次改动碰到的路径。
    #[test]
    fn legit_large_body_still_reaches_route() {
        let mut body = br#"{"template":"definitely_not_a_template"}"#.to_vec();
        body.extend(std::iter::repeat_n(b' ', 3000));
        let declared = body.len();
        assert!(declared > 1024, "本用例要覆盖惰性读分支，body 必须 > 1024");

        let (resp, joined) = with_server(BODY_READ_TIMEOUT, move |port| {
            raw_http(
                port,
                &format!(
                    "POST /api/inspect HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {declared}\r\n\r\n"
                ),
                &body,
                true,
            )
        });
        assert!(joined.is_ok(), "serve 应返回 Ok: {joined:?}");
        assert!(
            resp.contains("template_not_found"),
            "请求体应被完整读到并进入路由（预期 404 未知模板）: {resp}"
        );
        assert!(
            !resp.contains("请求体不完整"),
            "合法长度不得被判为半包: {resp}"
        );
    }

    /// P1-8：一个"发一半就挂着"的连接**不得钉死**整个服务循环。
    ///
    /// `serve_requests` 是单条顺序循环，tiny_http 0.12 又没有读超时 —— 修复前
    /// `read_to_end` 会永远阻塞，`nctool ui` 就此永久无响应。这里注入 300ms 超时，
    /// 先挂一个半包连接，再在窗口内发一条正常请求：正常请求必须被服务。
    ///
    /// 修复前（无超时）本用例会**挂住**而不是失败 —— 所以它同时是"超时真的生效"
    /// 的守卫，而不只是"没报错"。
    #[test]
    fn half_sent_body_does_not_pin_the_serve_loop() {
        use std::io::Write;
        use std::net::TcpStream;

        let (resp, joined) = with_server(Duration::from_millis(300), |port| {
            // ① 半包连接：声明 2000 字节、只发 3 字节，**不关连接**（读操作无限阻塞）
            let mut stuck = TcpStream::connect_timeout(
                &format!("127.0.0.1:{port}").parse().unwrap(),
                Duration::from_millis(1000),
            )
            .expect("应能连上服务");
            stuck
                .write_all(b"POST /api/inspect HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 2000\r\n\r\nabc")
                .expect("写半包请求");
            // 给服务端一点时间把 ① 排到队首（否则 ② 可能先被处理，用例就失去意义）
            std::thread::sleep(Duration::from_millis(50));

            // ② 正常请求：必须在超时窗口之后仍被服务（读超时 3s，足够覆盖 300ms 等待）
            let resp = raw_http(
                port,
                "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
                b"",
                true,
            );

            // 清场：关掉半包连接，被放弃的读线程读到 EOF 后退出并归还计数
            drop(stuck);
            resp
        });
        assert!(joined.is_ok(), "serve 应返回 Ok: {joined:?}");
        assert!(
            resp.contains("HTTP/1.1 200") && resp.contains("\"status\":\"ok\""),
            "半包连接不得钉死服务循环，后续请求必须被正常处理: {resp}"
        );
    }

    /// P1-8 的第二道闸：被放弃的读线程数达到上限后，新请求回 503 而不是继续赔线程。
    ///
    /// 用 100ms 超时和测试注入的 1 个读线程上限堆满服务，第 2 个请求必须拿到 503。
    /// 生产服务仍使用 `MAX_IN_FLIGHT_BODY_READERS`（8）；小阈值让测试不依赖 runner
    /// 的监听线程数和 socket 调度顺序。
    /// 没有这道闸时，这个测试会拿到 200（服务照旧再赔一个线程）。
    #[test]
    fn too_many_abandoned_body_readers_yield_503() {
        use std::io::Write;
        use std::net::TcpStream;

        // 用 1 个 slot 验证饱和分支；生产入口仍将上限设为 8。小阈值避免测试依赖
        // runner 的监听线程数和 socket 调度顺序。
        let reader_limit = 1;
        let (resp, joined) = with_server_and_reader_limit(
            Duration::from_millis(100),
            reader_limit,
            |port, in_flight| {
                // 堆满：声明 2000 字节、只发 1 字节且保持连接打开。
                let mut stuck = TcpStream::connect_timeout(
                    &format!("127.0.0.1:{port}").parse().unwrap(),
                    Duration::from_millis(1000),
                )
                .expect("应能连上服务");
                stuck
                    .write_all(
                        b"POST /api/inspect HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 2000\r\n\r\nx",
                    )
                    .expect("写半包请求");

                // 等待服务端确实接收并启动全部读线程。固定 sleep 会受慢 runner 的
                // socket accept / 调度速度影响，容易在只堆积了部分连接时误取到 200。
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                while in_flight.load(Ordering::SeqCst) < reader_limit
                    && std::time::Instant::now() < deadline
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert_eq!(
                    in_flight.load(Ordering::SeqCst),
                    reader_limit,
                    "超时窗口内服务端应接收并启动读线程"
                );

                let resp = raw_http(
                    port,
                    "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
                    b"",
                    true,
                );
                drop(stuck);
                resp
            },
        );
        assert!(joined.is_ok(), "serve 应返回 Ok: {joined:?}");
        assert!(
            resp.contains("503") && resp.contains("service_unavailable"),
            "被放弃的读线程堆到上限后应回 503: {resp}"
        );
    }
}
