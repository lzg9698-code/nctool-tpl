# `nctool` CLI 层深度代码审查报告

- **审查对象**：`D:\Desktop\nctool\rustjinja\cli`（crate `nctool-cli`）
- **审查范围**：`server.rs`(3556) / `context.rs`(1071) / `output.rs`(687) / `cli.rs`(691) / `args.rs`(728) / `config.rs`(307) / `commands/*`(13 文件) / `main.rs` / `lib.rs`
- **审查维度**：HTTP 服务安全、代码质量与可读性、错误处理、性能、CLI 正确性
- **审查方式**：逐文件通读 + `grep` 交叉核实；**所有行号均经 Read/Grep 实际确认，未编造**
- **性质**：纯审查，**未修改任何代码文件**

---

## 0. 总体结论

### 0.1 先说做得好的（这些不是套话，是逐点核实过的）

| 项 | 结论 | 证据 |
|---|---|---|
| **监听地址是否默认绑 0.0.0.0** | **否**，默认 `127.0.0.1`；非回环 IP **硬拒绝**而非告警放行 | `cli.rs:626` `default_value = "127.0.0.1"`；`server.rs:1228` `if !ip.is_loopback() { return Err(...) }` |
| **命令注入** | **不存在**。两处 spawn 外部进程均用 `Command::new(prog).arg(x)`，不经 shell | `ui.rs:56/60/64`（explorer/open/xdg-open）；`templates.rs:582-585`（`$EDITOR`） |
| **路径遍历（HTTP）** | **不存在**。模板名只查注册表逻辑名，`../Cargo.toml` → 404，且有测试钉住 | `server.rs:477-486` `registered_template`；`server.rs:338` `template_detail`；测试 `server.rs:2718` |
| **路径遍历（CLI 模板目录）** | 三重防护：隐藏项跳过 + `canonicalize` 后 `starts_with(root)` + `visited` 断环 | `context.rs:451-497` |
| **静态文件穿越** | **无此面**。前端是 `include_str!` 内嵌单文件，没有静态目录服务 | `server.rs:54` `pub const UI_HTML` |
| **CORS** | 不设 `Access-Control-Allow-Origin`，改用 `Origin` 白名单 + `Sec-Fetch-Site` + `Host` 三道校验；响应带 CSP/nosniff/Referrer-Policy | `server.rs:75-90 / 116-148 / 171-187` |
| **DNS rebinding** | 有专门防护（`Host` 回环白名单），且文档写清了为什么 `Origin` 挡不住 | `server.rs:150-227` + 测试 `1590-1611` |
| **请求体上限** | 1 MiB，超限 413 | `server.rs:57 / 1303-1308` |
| **生产代码 panic 面** | 全 CLI 生产代码（排除 `#[cfg(test)]`）中 `.unwrap()/.expect()/panic!/unreachable!` **仅 10 处**，且绝大多数作用在静态常量或固定结构上 | 见 §3.1 |

### 0.2 主要问题分布

| 严重程度 | 数量 | 主题 |
|---|---|---|
| **P0-致命** | 1 | HTTP 500/400 回显内部绝对路径（与本模块自定红线冲突） |
| **P1-严重** | 6 | 请求体读取吞错、服务可被单连接挂死、预设绝对路径外泄、文件路径模板丢弃参数规格、`WriteError` 映射 6 份分裂、`preset apply` 陈旧检测静默通过 |
| **P2-一般** | 14 | 读取上限不一致、硬编码默认值多处副本、`unwrap_or_default` 静默产出空包络、两套模板解析、下溢守卫漏 2 个端点、CLI 子命令一致性等 |

### 0.3 关于"主轴转速 6000 硬编码"

**在 CLI 层未发现。** 对 `cli/src/**` 全量检索 `6000|spindle|RPM|rpm`，唯一命中是 `config.rs:198`——示例配置里的注释行 `# max_spindle_rpm = "8000"`（非代码）。该硬编码若存在，应在 `core/`（机床预设 `MachinePreset` / `MachineConfig`）而非 CLI 层，**已超出本次审查范围，建议交由 core 层审查确认**。

---

## 1. P0-致命

### P0-1　HTTP 500/400 响应原样回显内部错误正文，泄漏模板绝对路径

- **位置**：
  - `cli/src/server.rs:535`（`POST /api/validate`）
  - `cli/src/server.rs:637`（`POST /api/render` 校验阶段）
  - `cli/src/server.rs:672`（`POST /api/render` 渲染失败）
  - `cli/src/server.rs:354`（`GET /api/templates/{name}`，`cli_error(CliError::from(err))`）
  - `cli/src/server.rs:1196`（`POST /api/inspect`，同上）
- **问题**：这五处绕过本模块自己建立的 `internal_error`，直接把 `RegistryError` / `PipelineError` 的 `to_string()` 塞进响应体；而这些错误的正文**含模板文件的绝对路径**。
- **证据**：

```rust
// server.rs:528-536  —— /api/validate
match gen.registry().validate(&name, &params) {
    Ok(report) => Resp::Json(200, ok(...)),
    Err(e) => Resp::Json(500, err("registry", e.to_string())),
}
```

```rust
// server.rs:1364-1370  —— 本模块自己写下的红线（正是这五处违反的政策）
/// `build_registry` 的失败消息里含模板文件的绝对路径（形如
/// `读取模板失败 \\?\C:\Users\<用户名>\Desktop\...\a.j2: …`），原样回给浏览器
/// 等于把用户名与目录结构一并泄露
fn internal_error(e: CliError) -> Resp {
```

`context.rs:250` 与 `core/src/registry.rs:399-403` 证实路径确实被拼进了错误消息文本（`read_text_capped` 的 `读取模板失败 {path}`、`add_file` 明确注释"把路径并进 io::Error 的消息里"）。

- **影响**：
  1. 用户名、项目目录结构、磁盘布局外泄给任何能连到 `127.0.0.1:<port>` 的**本地进程**（零权限、零认证）；在 DNS rebinding 场景下还可被网页读走。
  2. 与 `internal_error`（`server.rs:1371`）的既定策略**自相矛盾**——同一类错误在不同端点一会儿脱敏、一会儿裸奔，脱敏就失去意义。
  3. 已有测试 `server.rs:3188-3203` 专门钉住"500 正文不得含 `Users`/`a.j2`/`Desktop`"，但它只测了 `internal_error` 本身，**没有覆盖这五条泄漏路径**，所以门禁是绿的。

- **改进建议**（代码级，逐点替换）：

```rust
// server.rs:535
Err(e) => internal_error(CliError::from(e)),

// server.rs:637
Err(e) => return internal_error(CliError::from(e)),

// server.rs:672 —— 这是渲染失败，属"调用方参数触发"，可回显但需确认不含路径；
//   保守起见统一走 cli_error（400 + 正文），并让 core 侧保证 PipelineError 不含绝对路径
Err(e) => Resp::Json(400, err("render", e.to_string())),

// server.rs:354 / 1196 —— 模板解析错误属"调用方传进来的模板"，保留 400 回显是符合政策的，
//   但必须确保 CliError::from(RegistryError) 不带路径；建议改为：
Err(err) => cli_error(CliError::from(err)),
```

并**补一条回归测试**：构造一个含绝对路径的 `CliError`/`RegistryError`，分别对 `/api/validate`、`/api/render` 发请求，断言响应体**不含** `Users` / `:\\` / `.j2`。

---

## 2. P1-严重

### P1-1　请求体读取用 `.is_ok()` 吞掉 IO 错误，残缺 body 被当完整请求处理

- **位置**：`cli/src/server.rs:1300-1306`
- **问题**：`read_to_end` 的 `Err` 被 `.is_ok()` 直接丢弃；只要读失败，`too_large` 恒为 `false`，于是**残缺的请求体**被原样交给 `route()`。同时也没有校验 `Content-Length` 与实际读得字节数是否一致。
- **证据**：

```rust
let mut body = Vec::new();
let too_large = request
    .as_reader()
    .take(MAX_BODY_BYTES as u64 + 1)
    .read_to_end(&mut body)
    .is_ok()                       // ← Err 被吞掉
    && body.len() > MAX_BODY_BYTES;
if too_large { Resp::Json(413, ...) } else { route(ctx, &method, &path, &query, &body) }
```

- **影响**：客户端中途断开 / 重置 / 半包时，服务端把截断的 JSON 当完整请求解析。对本项目最坏情形不是崩溃，而是**"静默按错的方式处理"**——例如 `{"template":"drill_cycle","params":{"depth":-1` 被解析失败后回 400，调用方看到的是"我的 JSON 写错了"而非"传输中断"，排障方向全错。超大请求体还要先缓冲 1 MiB 才判 413。

- **改进建议**：

```rust
let mut body = Vec::new();
// ① 先用 Content-Length 预检，避免为超大载荷白白缓冲
if let Some(len) = content_length_of(&headers) {
    if len > MAX_BODY_BYTES as u64 {
        return_response(Resp::Json(413, err("payload_too_large", "请求体超过 1 MiB 上限")));
    }
}
// ② 读取失败必须显式报错，不能落到 route
match request.as_reader().take(MAX_BODY_BYTES as u64 + 1).read_to_end(&mut body) {
    Err(e) => Resp::Json(400, err("bad_request", format!("读取请求体失败: {e}"))),
    Ok(_) if body.len() > MAX_BODY_BYTES =>
        Resp::Json(413, err("payload_too_large", "请求体超过 1 MiB 上限")),
    Ok(_) => route(ctx, &method, &path, &query, &body),
}
```

（`content_length_of` 从 `request.headers()` 里取，解析失败或缺失即视为非法，回 411/400。）

---

### P1-2　单线程顺序循环 + tiny_http 0.12 无读超时 → 一个不发完请求体的连接即可永久挂死 UI 服务

- **位置**：`cli/src/server.rs:1272`（`for mut request in server.incoming_requests()`）、`server.rs:1301-1304`
- **问题**：`serve_requests` 是**单条顺序循环**，一次只处理一个请求；而 tiny_http 0.12 **根本没有读超时配置项**。
- **证据**：

```rust
// server.rs:1269
fn serve_requests(server: &tiny_http::Server, addr: SocketAddr, ctx: &Ctx) -> Result<(), CliError> {
    ...
    for mut request in server.incoming_requests() {   // ← 顺序、单线程
```

```
// tiny_http-0.12.0/src/lib.rs:174-183（实测确认）
pub struct ServerConfig {
    pub addr: ConfigListenAddr,
    pub ssl: Option<SslConfig>,      // ← 只有这两个字段，无任何 timeout
}
```

`Server::http()` 内部只构造 `ServerConfig { addr, ssl: None }`（`lib.rs:190-203`），因此**无法**通过配置给 socket 设 `SO_RCVTIMEO`。

- **影响**：攻击者（或仅仅是一个写错的客户端）发起 `POST /api/render` 并声明巨大的 `Content-Length` 后只发极少字节、然后挂住连接，`read_to_end` 会无限阻塞在 `server.rs:1304`。由于循环是顺序的，**整个 `nctool ui` 服务就此永久无响应**，且 Ctrl-C 也未必能从阻塞的 `read` 中干净恢复（进程卡在系统调用里）。对一个常驻的本地 UI 服务，这是可用性硬伤。

- **改进建议**（按成本递增）：
  1. **立即**：先做 §P1-1 的 `Content-Length` 预检——至少让"声明超大长度"不再进入读取；对缺失 `Content-Length` 的 POST 直接 411。
  2. **短期**：把 body 读取包进带超时的执行单元，例如 `std::thread::scope` 内读 + `mpsc::channel` + `recv_timeout(Duration::from_secs(10))`，超时即回 408 并 `drop` 该 request（tiny_http 会在 `Request` 被 drop 时关闭连接）。
  3. **长期**：`server.rs` 换 axum/hyper（有内建超时、连接数限制与并发）；`Ctx` 用 `Rc/RefCell` 是单线程约束，届时把 `build_registry` 的结果改成 `Arc<GCodeGenerator>`（先消除 core 里两处 `OnceCell`，见 `context.rs:856-858` 的实测注记）。

---

### P1-3　`/api/presets` 全系列端点把预设文件**绝对路径**回传浏览器

- **位置**：
  - `server.rs:972`（`GET /api/presets` 响应体 `"path"`）
  - `server.rs:1120`（`POST /api/presets` 响应体 `"path"`）
  - `server.rs:1155`（`POST /api/presets/delete` 响应体 `"path"`）
  - `server.rs:1415`（`WriteError::Conflict` 409 正文含 `path.display()`）
- **问题**：预设文件默认位于 `%APPDATA%\nctool\presets.yaml`，其绝对路径**必然包含 Windows 用户名**。这些端点无鉴权，任何本地进程都能拿到。
- **证据**：

```rust
// server.rs:969-976
Resp::Json(200, ok(serde_json::json!({
    "path": path.display().to_string(),     // ← C:\Users\<用户名>\AppData\Roaming\nctool\presets.yaml
    "presets": items, "warnings": got.warnings,
}))),
```

```rust
// server.rs:1411-1417  —— 409 正文同样带路径
WriteError::Conflict { path, .. } => Resp::Json(409, err("write_conflict",
    format!("写入冲突：{} 已被外部修改，未覆盖。请重试", path.display()))),
```

- **影响**：与 P0-1 同一类信息泄漏，且这条是**主动、无条件、每次成功响应都发**。对比 `context.rs:262-270` 为了让 `/api/templates` 不泄漏路径，特意改用相对键作描述——说明团队清楚这条红线，只是预设端点漏了。

- **改进建议**：
  - 响应体里的 `"path"` 改为**对配置目录的相对显示**：`path.strip_prefix(config_dir).unwrap_or(&相对化失败标记)`，或统一输出 `~/<...>` 形态的脱敏串；需要真实路径的场景（`--file` 显式指定）再给全路径。
  - 新增 `"pathRedacted": true` 字段明示脱敏，避免消费方误以为是完整路径。
  - `write_error_resp` 的 `Conflict` 分支改为 `"预设文件已被外部修改，未覆盖。请重试"`（去掉 `path.display()`），与 `Corrupt`/`Io`/`ReadOnly` 三个分支（已脱敏，见 `server.rs:1428-1442`）保持一致。

---

### P1-4　按**文件路径**渲染时，模板的参数规格被整份丢弃 → 类型/区间/白名单校验静默失效

- **位置**：`cli/src/commands/render.rs:209-217`（经 `resolve_registry` 影响 `render` / `generate` / `validate`）
- **问题**：文件路径模板注册进临时注册表时，`params` 传的是**空切片**，完全不解析清单 `params` / 变量库 / 头部 `{# PARAMS: #}` 三层规格。
- **证据**：

```rust
// render.rs:209-217
let mut fresh = ctx.build_registry_fresh()?;
fresh.registry_mut().add_file(
    key.clone(),
    TemplateCategory::General,
    format!("文件模板: {}", path.display()),
    &path,
    vec![],                      // ← 参数规格恒为空
)?;
```

对比**同一份文件**走目录注册时的处理（`context.rs:249-286`）：

```rust
let meta = ResolvedMeta::resolve(rel_path, &source_text, manifest.get(&rel_key), &library);
...
TemplateEntry::new(rel_key.clone(), meta.category, description, TemplateSource::File(canonical),
    meta.params.clone(),        // ← 这里规格是完整的
    source_text)
```

- **影响**：**同一份模板文件，两种引用方式校验强度不同**——`nctool render turning/a.j2`（注册表名）会校验类型/区间/白名单，`nctool render /abs/path/turning/a.j2`（路径）不校验。用户看到的都是"校验通过"，实际后者少了一整层约束。这是本项目零容忍的"静默少校验"，错误参数会一路走到 G-code（错误坐标 = 撞刀风险）。同一缺口也存在于 `templates.rs:112-137` 的 `resolve_source`（路径分支返回 `None` 规格），因此 `inspect <路径>` / `lint <路径>` 也看不到规格。

- **改进建议**：把规格解析抽成单一函数并在两条路径上复用：

```rust
// 新增（放在 commands/templates.rs 或 context.rs）
pub(crate) fn specs_for_file(root: &Path, rel_key: &str, source: &str)
    -> Vec<nctool_core::ParamSpec>
{
    let manifest = TemplateManifest::load(root).unwrap_or_else(|e| { eprintln!("warning: {e}"); TemplateManifest::empty() });
    let library  = VariableLibrary::load(root).unwrap_or_else(|e| { eprintln!("warning: {e}"); VariableLibrary::empty() });
    ResolvedMeta::resolve(Path::new(rel_key), source, manifest.get(rel_key), &library).params
}

// render.rs:210 改为
let specs = crate::commands::templates::specs_for_file(&root, &key, &src_text);
fresh.registry_mut().add_file(key.clone(), .., &path, specs)?;
```

（`resolve_specs` 已存在于 `templates.rs:603-624`，逻辑几乎一致，直接复用即可，注意它有 `root` 参数，而 `resolve_registry` 需先拿到 `ctx.template_dir`。）
**并补一条对拍测试**：同一份带 `min/max/options` 的文件，分别用注册表名和绝对路径渲染同一组越界参数，断言两者 `validate` 结论必须一致。

---

### P1-5　`WriteError → 错误分类` 有 6 份并行实现，且分类口径互相冲突

- **位置**：
  1. `output.rs:70-107` `CliError::from_write_error(err, not_found_kind)`
  2. `output.rs:251-277` `impl From<WriteError> for CliError`
  3. `server.rs:1409-1446` `write_error_resp(e) -> Resp`（HTTP 通道）
  4. `preset.rs:114-119` `map_write_err`（覆写 `Corrupt`）
  5. `templates.rs:711-732` `map_write_err`
  6. `templates.rs:735-743` `map_create_err`（覆写 `Conflict`）

- **问题**：`output.rs` 里**同一类型的两个映射对同一变体给出不同分类**，注释还把它们描述成"有意不同"：

```rust
// output.rs:99  —— 映射 ①
WriteError::Corrupt(m) => CliError::new("config", m),      // 退出码 4
// output.rs:270 —— 映射 ②（同一个 WriteError::Corrupt）
WriteError::Corrupt(m) => CliError::new("io", m),          // 退出码 3
```

```rust
// output.rs:84-91 —— 映射 ① 对 PathEscape 细分出 name_conflict
if reason.contains("已存在") { CliError::new("name_conflict", ...) }
else { CliError::new("args", ...) }
// output.rs:263-265 —— 映射 ② 一律归 args，且文案是"路径越界被拒绝"
WriteError::PathEscape { rel, reason } => CliError::new("args", format!("路径越界被拒绝：{rel}（{reason}）")),
```

`templates.rs:724-726` 与 `output.rs:263-265` 又是**逐字重复**的两份。

- **影响**：
  1. 调用方选错一个 `From`/`from_write_error` 就得到不同退出码，而**编译器不会报错**（`output.rs:248-250` 的注释已经承认这一点）。
  2. HTTP 侧（`write_error_resp`）与 CLI 侧对同一错误的分类/状态码是**手写对齐**的，没有共享表；`output.rs:60-69` 的文档列出的口径表正是 `write_error_resp` 的注释在 `server.rs:1402-1408` 又抄了一遍。
  3. `#[non_exhaustive]` 新增变体时，6 处都要手工同步，漏一处就是静默错分。

- **改进建议**：把分类表收敛为**一张静态表**，6 处全部改为查表：

> **2026-09-XX 更新**：下表已按与架构师（software-architect）定稿的 core 侧"跨进程锁"契约补齐 —— 变体全集为 **8 个**（新增 `LockBusy`），并加入 **CLI / HTTP 双通道消息分离**（见下方说明与 P1-3）。

```rust
// output.rs 新增：单一来源。变体全集 8 个，与 core 侧定稿一致。
pub(crate) struct WriteErrClass {
    pub cli_kind: &'static str,
    pub http_status: u16,
    pub http_kind: &'static str,
    /// 仅 CLI / stderr 通道可以在消息里带 `path.display()`；
    /// **HTTP 通道一律 false**（见 P1-3：响应体不得含绝对路径）。
    pub cli_msg_with_path: bool,
}

pub(crate) fn classify_write_error(
    e: &WriteError,
    not_found_kind: &'static str,
    corrupt_kind: &'static str, // preset → "io"(3)；machine 的 nctool.toml → "config"(4)
) -> WriteErrClass {
    use WriteError::*;
    match e {
        Conflict { .. }     => WriteErrClass { cli_kind: "write_conflict", http_status: 409, http_kind: "write_conflict", cli_msg_with_path: true },
        // ★ core 侧新增：跨进程锁争用（等待超时）。可重试，与 Conflict 同 kind、同 HTTP 409。
        LockBusy { .. }     => WriteErrClass { cli_kind: "write_conflict", http_status: 409, http_kind: "write_conflict", cli_msg_with_path: true },
        PathEscape { reason, .. } if reason.contains("已存在")
                            => WriteErrClass { cli_kind: "name_conflict",  http_status: 409, http_kind: "name_conflict",  cli_msg_with_path: false },
        PathEscape { .. }   => WriteErrClass { cli_kind: "args",            http_status: 400, http_kind: "bad_request",   cli_msg_with_path: false },
        ReadOnly { .. }     => WriteErrClass { cli_kind: "io",              http_status: 500, http_kind: "internal",      cli_msg_with_path: true },
        NotFound(_)         => WriteErrClass { cli_kind: not_found_kind,    http_status: 404, http_kind: not_found_kind,  cli_msg_with_path: false },
        Corrupt(_)          => WriteErrClass { cli_kind: corrupt_kind,      http_status: 500, http_kind: "internal",      cli_msg_with_path: false },
        NumUnderflow { .. } => WriteErrClass { cli_kind: "args",            http_status: 400, http_kind: "num_underflow", cli_msg_with_path: false },
        Io(_)               => WriteErrClass { cli_kind: "io",              http_status: 500, http_kind: "internal",      cli_msg_with_path: false },
        // 兜底：新增变体**不得**静默退化 —— 必须显式登记并留声
        #[allow(unreachable_patterns)]
        _ => {
            eprintln!("warning: 未登记的 WriteError 变体，已按 io(3) 兜底：{e}");
            WriteErrClass { cli_kind: "io", http_status: 500, http_kind: "internal", cli_msg_with_path: false }
        }
    }
}
```

`Corrupt` 的 "config(4) vs io(3)" 分歧改由**显式入参** `corrupt_kind` 表达，不再靠"preset 调 `From`、machine 调 `from_write_error`"这种隐式约定。迁移分两步：先纯重构（行为不变，靠现有 `map_write_err_*` 系列测试保底），再统一语义。

#### P1-5 补充：与 core 侧"跨进程锁"改动的契约（已与架构师定稿）

- core 将新增第 8 个变体 `WriteError::LockBusy { path }`（跨进程锁争用，有界等待 2s 后仍未获得锁）。
- **分类定稿**：`kind = "write_conflict"`（CLI 退出码 6 / HTTP 409），**不新开 kind**。退出码矩阵（`output.rs:119-133`）、README 退出码表、测试 `output.rs:492-515` `exit_code_matrix` 全部零改动。归 `io`(3) 已被明确排除——`output.rs:104` 的消息是"读写失败"，会把用户引向查权限/磁盘，且 3 在脚本里会被当硬失败、误杀本可重试的场景。
- **消息文案**（CLI）：`写入冲突：{path} 正被另一个 nctool 进程写入（等待超时仍未获得锁）。请稍后重试同一命令。` —— **不要**复用 `output.rs:76-83` 那条乐观锁文案。
- ⚠️ **HTTP 通道必须去掉 `{path}`**（`cli_msg_with_path` 只作用于 CLI/stderr）：若 `LockBusy` / `Conflict` 的 `Display` 含 `path.display()`，而 `server.rs:1411-1417` 把消息原样塞进响应体，就会在 HTTP 侧**新增一处绝对路径泄漏**（与 P0-1 同类）。HTTP 一律用脱敏文案（如"预设文件正被另一个 nctool 进程写入，请稍后重试"）。
- **`create_exclusive`（`O_EXCL`）方案已由架构师撤回**：`TemplateWriter::create` / `derive` 保持现状签名、继续返回 `WriteError::Conflict`（理由是 `O_EXCL` 需直接写目标文件，会破坏 `atomic.rs:6-8` 自己承诺的"不留半成品"）。故 `templates.rs:256-266`（`template_duplicate` → 6）与 `templates.rs:735-743`（`name_conflict` → 6）**一行都不用改**，退出码契约零改动。
- ⚠️ **绝不能让 `LockBusy` 掉进 `output.rs:105` / `output.rs:271` 的 `_ => io(3)` 兜底** —— 那会把"可重试的锁争用"静默变成退出码 3 的硬失败，且编译器不会报错。上表已显式登记。
- ⚠️ **HTTP 通道建议用非阻塞 try-lock**：`serve_requests` 是单线程顺序循环（`server.rs:1272`），且 tiny_http 无读超时（见 P1-2）。若 HTTP 请求在 `route()` 内**阻塞**等锁 2s，整个 UI 会随之停顿 2s。建议 `asset/lock.rs` 提供 `try_lock()` 变体，服务侧等待时间设为 0，拿不到锁立即返回 `LockBusy`（409）。

---

### P1-6　`preset apply` 的陈旧检测在目标模板不可解析时**静默通过**

- **位置**：`cli/src/commands/preset.rs:695`（另 `preset.rs:581`、`preset.rs:653` 同类）
- **问题**：`template_vars(...)` / `specs_of(...)` 的 `Err` 被 `unwrap_or_default()` 吞成空集，于是陈旧检测拿到**空变量集 / 空必选集**，"检测通过"是假象。
- **证据**：

```rust
// preset.rs:695-697
let (target_vars, target_required) = template_vars(ctx, &target).unwrap_or_default();
let stale = PresetStore::stale_report_full(&p, &target_specs, Some(&target_vars), &target_required);
```

```rust
// preset.rs:178-183 —— template_vars 在模板解析失败时返回 Err（不是空集）
let analysis = entry.analysis().map_err(|e| {
    CliError::new("validation", format!("模板 {template} 解析失败，无法确认参数归属：{e}"))
})?;
```

`preset.rs:139-148` 的 `stale_of` 明明用了正确写法（`specs_of(...).ok()?` → 返回 `None` → 调用方标记"模板缺失"），说明正确口径已知，只是 `apply` 这条路径没跟上。

- **影响**：目标模板有语法错误 / 被删 / 被改名时，`preset apply` 会输出"陈旧检测: 通过（规格指纹一致，无失效参数）"，用户据此把预设参数直接送上机床——**这正是本项目定义的静默误报**。
- **改进建议**：`apply` 复用 `stale_of` 的 `Option` 语义：

```rust
match stale_of_on(ctx, &p, &target, &target_specs) {   // 返回 Option<StaleReport>
    Some(r)  => { text.push_str(&stale_line(&r)); data["stale"] = json!(r); }
    None     => {
        text.push_str(&format!("陈旧检测: 跳过（目标模板 {target} 当前不可解析；可能已被重命名或删除）\n"));
        data["stale"] = serde_json::Value::Null;
    }
}
```

`preset.rs:581`（import 时 `specs_of` 失败）与 `preset.rs:653`（apply 的 `src_specs`）至少应 `eprintln!("warning: ...")` 说明"已按无规格处理"，不要无声无息。

---

## 3. P2-一般

### 3.1 错误处理：生产代码的 panic / 静默吞错

| 位置 | 问题 | 建议 |
|---|---|---|
| `server.rs:1358` | `.unwrap_or_else(\|_\| panic!("安全头 {name} 的键值应为合法 ASCII"))` —— **`panic!` 位于请求处理循环内**。虽作用于静态常量（实际不可达），但一旦 `SECURITY_HEADERS` 被改成含非 ASCII 值，就是每个请求都崩。 | 改用 `expect` 之外的兜底：`unwrap_or_else(\|_\| { eprintln!(...); return; })`，或把 3 个头在 `lazy` 里**启动时**构造一次并校验（fail-fast）。 |
| `server.rs:1324` / `1341` | `.expect("静态 Content-Type 头合法")` —— 每个响应都重新构造 + `expect` | 与上面同理，提到模块级常量缓存 |
| `output.rs:21` | `report_json` 里 `.expect("校验报告视图结构固定…")` —— 被 CLI 与 HTTP 共用，位于 `server.rs` 的 `validate`/`render` 路径上 | 改为 `unwrap_or_else` 返回 `json!({"error":"报告序列化失败"})`；或至少在 `Resp` 层兜 500 |
| `output.rs:377` / `386`、`lint.rs:59` / `94`、`validate.rs:52` | `serde_json::to_string_pretty(&obj).unwrap_or_default()` —— 序列化失败时静默产出 `"\n"` / `{}\n`，即**一个不含 `ok` 字段的畸形包络** | 改为 `match`：`Err(e) => { eprintln!(...); return; }`，或至少回退成 `{"ok":false,"error":{"kind":"internal","message":"序列化失败"}}` |
| `commands/mod.rs:47` | `Command::Completion(_) => unreachable!()` | 直接 `Command::Completion(a) => completion::run(a)`，删掉 unreachable 分支与上方特判（行为等价，少一处 panic 面） |
| `server.rs:1346` | `let _ = request.respond(response);` —— 发送失败被静默忽略 | 至少 `eprintln!` 一行，便于诊断"浏览器收不到响应" |
| `context.rs:387-407` `tree_stamp` | 任一 `?` 失败即返回 `None` → `build_registry` **静默放弃缓存**（`context.rs:154-157`），本次请求退化为全量重建，**无任何提示** | 首次失败时 `eprintln!("warning: 目录指纹获取失败，本次不使用注册表缓存：{e}")`；或引入连续失败计数，超过阈值才提示 |

> **正面记录**：`cli/src` 生产代码（排除 `#[cfg(test)]`）中 `.unwrap()/.expect()/panic!/unreachable!/unwrap_or_default()` 合计仅 **10 处**（`config.rs:159`、`output.rs:21/377/386`、`server.rs:1324/1341/1358`、`commands/mod.rs:47`、`inspect.rs:75/154`、`preset.rs:581/653/695`、`render.rs:21/181`、`validate.rs:24`），**没有在关键路径上滥用**——这是本次审查中最值得肯定的一点。

### 3.2 1 MiB 读取上限只覆盖了不到一半的读文件点

- **已覆盖**：`args.rs:178`（参数文件）、`context.rs:250`（模板）、`config.rs:93`（配置）、`part.rs:23`（零件定义）
- **未覆盖（直调 `std::fs::read_to_string`）**：
  - `templates.rs:125`（`resolve_source` 读模板源码 → 影响 `show` / `inspect` / `lint`）
  - `templates.rs:290`（读清单）、`templates.rs:441`（derive 读源模板）
  - `templates.rs:548`（`--from-file`）、`templates.rs:566`（编辑前快照）、`templates.rs:594`（编辑结果读回）
  - `preset.rs:574`（`preset import` 输入文件）、`preset.rs:570`（stdin `read_to_string`，**完全无界**）
  - 另：`core/src/registry.rs:399` 的 `add_file` 内部也是无界 `read_to_string`
- **影响**：`nctool preset import - ` 从 stdin 读无上限；`templates edit --from-file <GB 级日志>` 会一次性吃满内存，而报错会指向后续莫名其妙的解析失败，指不到病因——正是 `args.rs:140-143` 写这条上限时要防的场景。
- **建议**：新增 `args::read_stdin_capped(what)`；把上述 8 处全部改为 `read_text_capped`。`templates.rs:566/594` 是临时文件编辑场景，可放宽但应有上限。
- **跨层注记（避免重复计数）**：`core/src/registry.rs:399` 的 `add_file` 内部同样是**无界 `read_to_string`**。该项已由架构师认领并并入 **core 侧 P1-1**（把 `core/src/asset/preset.rs` 的 `Take` 逻辑上提成共用的 `read_text_capped`）。**与本节同源，汇总时请按一条计**，CLI 侧无需改动。

### 3.3 硬编码默认值在多处各存一份

| 值 | 副本位置 | 风险 |
|---|---|---|
| 行号步进 `10` | `cli.rs:314`、`server.rs:601`、测试 `server.rs:2999` | CLI 改默认而 HTTP 不改 → 两个入口产出不同 G-code |
| 行号上限 `9999` | `cli.rs:318`、`server.rs:602`、测试 `server.rs:3000` | 同上 |
| `1 MiB` | `args.rs:144` `MAX_CLI_FILE_BYTES`、`server.rs:57` `MAX_BODY_BYTES`（两个独立字面量） | 文档（`args.rs:137`）称"同一口径"，但没有任何编译期约束 |
| 端口 `8787` | `cli.rs:629` | 可接受（单一处） |

现有 `option_parity_cases.json` + `option_mapping_matches_shared_fixture`（`server.rs:2937`）只测**映射**，不测**默认值本身**（测试里自己又写了一遍 `unwrap_or(10)`）。

- **建议**：`pub const DEFAULT_LINE_STEP: u32 = 10; pub const DEFAULT_MAX_LINE: u32 = 9999;` 放到 `cli.rs`（或 core），`clap` 用 `default_value_t = DEFAULT_LINE_STEP`，`server.rs` 与测试统一引用；`MAX_BODY_BYTES` 直接 `= crate::args::MAX_CLI_FILE_BYTES as usize`。并在 fixture 里加一条"不传该字段"的 case，断言两侧默认值一致。

### 3.4 两套模板解析实现，语义已出现分歧

- `render.rs:161-224` `resolve_registry` —— 返回 `(Rc<GCodeGenerator>, 名称, 源路径)`，**路径模板会注册进临时注册表**
- `templates.rs:112-137` `resolve_source` —— 返回 `(名称, 源码, Option<规格>, 系统变量)`，**路径模板直接读文件、规格为 `None`**
- 使用者：`render`/`generate`/`validate` 走前者；`show`/`inspect`/`lint` 走后者
- 已出现的分歧：`machine.rs:310` 的 `machine test --template` **只接受注册表名**（`gen.registry().get(&args.template)`），而 `render`/`validate` 接受文件路径——同一套工具里 `--template` 的语义不统一。
- **建议**：统一为"先解析到 `(名称, 源码, 规格, 源路径, 系统变量)` 一个结构，再由各自决定要不要注册进注册表"；`machine test` 复用 `resolve_registry`。

### 3.5 下溢守卫漏了 2 个端点

- `server.rs:986-990`（`presets_save`）与 `api_body`（`server.rs:430-441`）都做了 `json_underflow_resp` 前置检查
- 但 `server.rs:1135`（`presets_delete`）与 `server.rs:1171`（`inspect`）直接用裸 `serde_json::from_slice`，**没有下溢守卫**
- `part_generate`（`server.rs:694`）走 `api_body`，已覆盖
- **影响**：当前这两个端点请求体只有字符串字段，实际打不出下溢；但一旦将来扩展数值字段，就是静默归零。且这属于"同一份代码两份严格度"。
- **建议**：这两个端点改调 `api_body(body)`（或至少补 `json_underflow_resp` 前置），消除分歧。

### 3.6 `presets_delete` 缺名称合法性校验

- `server.rs:1139-1142` 只校验非空；`presets_save` 在 `server.rs:1003-1007` 额外调用了 `validate_asset_name`（注释明确说明是为了防止含控制字符的名字被 `preset list` 原样回显到终端造成 ANSI/OSC 注入）
- **影响**：`POST /api/presets/delete` 可提交含控制字符/路径分隔符的名字。虽然 `PresetStore::remove` 按名查表不会造成文件穿越，但**写入再列表**的路径可能被污染。
- **建议**：`presets_delete` 同样先过 `validate_asset_name`，与 save 对齐。

### 3.7 CLI/HTTP 两侧"预设保存"的校验顺序不一致

- CLI `preset.rs`：**① L3 值校验**（`preset.rs:213`）→ **② 参数归属**（`preset.rs:228`）→ ③ 同名拒绝
- HTTP `server.rs`：**① 参数归属**（`server.rs:1050`）→ **② L3 值校验**（`server.rs:1077`）→ ③ 同名拒绝
- **影响**：同一份"既是未知参数、又值非法"的输入，CLI 报 `validation`（退出码 1），HTTP 报 `bad_request`（400）——两条通道对同一输入给出不同结论。
- **建议**：把整段保存逻辑抽成不依赖通道的 `fn save_preset(ctx, name, template, params, force) -> Result<SaveOutcome, PresetSaveError>`（见 §5 拆分方案第 5 项），两侧只负责把 `PresetSaveError` 映射成各自的呈现。

### 3.8 HTTP 模板列表暴露隐藏模板（与 CLI 口径不一致）

- `server.rs:309` 用 `gen.registry().list(cat_core)`；CLI `templates.rs:47-49` 默认用 `list_visible`（需 `--all` 才全列）
- 且 HTTP 响应项（`server.rs:313-317`）**不带 `visible` 字段**，前端无从区分
- **建议**：`/api/templates` 默认 `list_visible`，加 `?all=true` 放开；响应项补 `"visible"` 字段（与 `templates.rs:61` 对齐）。

### 3.9 无 `OPTIONS` 预检处理

- `route`（`server.rs:254-278`）无 `OPTIONS` 分支 → `OPTIONS /api/render` 落到 `_ => 404 "未知接口"`
- 当前前端同源（`server.rs:1290` 只服务 `/` 与 `/index.html`），不会触发预检；但若将来把 UI 放到别的源，`Content-Type: application/json` 的 POST 必然触发预检并失败
- **建议**：要么显式加 OPTIONS 分支（回 204 + 允许同源），要么在 `route` 文档里写明"本服务仅供同源访问，不支持跨域预检"。

### 3.10 `completion` 未走统一输出口

- `completion.rs:12` 直接 `let mut out = std::io::stdout();` 交给 `clap_complete::generate`
- 其余输出全走 `output::write_stdout_quiet`（`output.rs:339-348`，专门处理 `BrokenPipe` 不 panic）
- **影响**：`nctool completion bash \| head -1` 存在 BrokenPipe panic 风险（退出码 101，偏离退出码契约）
- **建议**：改为 `generate(shell, &mut cmd, "nctool", &mut Vec<u8>)` 缓冲后交给 `write_stdout_quiet`（clap_complete 的 `generate` 接受任意 `io::Write`）。

### 3.11 `$EDITOR` 按空白切分，含空格路径的编辑器打不开

- `templates.rs:579-585`：`editor.split_whitespace()`，首段为程序
- **影响**：`EDITOR="C:\Program Files\...\gvim.exe"` 会被切成 `C:\Program` → 启动失败；用户只能改用短路径
- **建议**：Windows 上支持引号切分（如用 `shell-words` 风格的最小解析器，或先判 `Path::new(&editor).is_file()` 整体当程序），并对切分结果给出可操作的错误文案。

### 3.12 非 UTF-8 路径（Windows）处理

- `render.rs:180` `path.file_name().map(|n| n.to_string_lossy().to_string())` —— 非 UTF-8 文件名被替换成 `U+FFFD`，并作为**注册键**
- `templates.rs:127-130` 同类（`to_string_lossy`）
- **影响**：非 UTF-8 文件名的模板会得到一个"看起来像但匹配不上"的注册名；后续按原名查找失败
- **建议**：注册键统一走 `path_to_rel_key`（与 `context.rs:496` 一致，基于 `OsStr`），仅在**显示**时 `to_string_lossy`；或显式报"模板文件名非 UTF-8，暂不支持"而非静默降级。

### 3.13 `preset_path` 的模板根收集是 O(模板数)，且每个 HTTP 请求都跑

- `preset.rs:82-106` `candidate_template_roots`：`build_registry()` + `list(None)` 全量遍历 + 逐个 `parent()`
- 调用方 `preset.rs:71` `preset_path` 被**每个** preset 子命令和**每个** `/api/presets*` 请求调用（`server.rs:885`）
- **影响**：量级不大（32 模板实测 ~0.1ms），但属于"每请求白跑一遍"；与 `tree_stamp` 叠加后是缓存命中路径上的主要开销
- **建议**：把 roots 结果随 `Ctx` 缓存（键同 `RegistryKey`），或在 `Ctx` 里算一次并复用。

### 3.14 `server.rs` 内嵌 2053 行测试，与本仓库惯例不一致

- `server.rs` 共 3556 行，其中 `#[cfg(test)] mod tests` 从 **1504 行**开始 → 生产 1503 行 / 测试 2053 行（**57.7%**）
- 而 `cli/tests/` 下已有 7 个 `cli_*.rs` 集成测试文件（共 5737 行），说明本仓库**有**把测试外置的惯例
- **影响**：单文件过大直接拖慢 IDE 与增量编译；`PRESET_ENV_LOCK`（`server.rs:1692`）这类"进程级 env 串行锁"放在 src 内，会和同进程其它测试互相牵制
- **建议**：见 §5 拆分方案第 8 项。

### 3.15 内置模板注册失败将从 panic 降级为"静默缺失"——CLI 必须把警告呈现出来

- **core 侧方案**（架构师定稿，`registry.rs:733`）：`GCodeGenerator::new()` 不再 panic，改为**跳过注册失败的内置模板**并记入 `TemplateRegistry::builtin_warnings()`；`new()` / `Default` 签名不变 → CLI 零改动。（不改签名的理由成立：`Default` 被 `GCodeGenerator::default()` 依赖，改 `Result` 会连带打掉 `Default`。）
- ⚠️ **降级方向对本项目是更危险的失败模式**：崩溃会立刻被发现，而"某个内置模板悄悄不存在"不会——`templates list` 只是少一项，`render drill_cycle` 只报 `template_not_found`(5)，引用它的组合模板在 `extract_params` 闭包提取时可能给出难以定位的错误。这属于"静默产出错误程序"红线，比 panic 更难察觉。故**警告必须够响**。
- **CLI 侧建议（新增、非破坏）**：
  1. `Ctx` 内加一次性标记，**每个进程**在首次 `build_registry` 时把 `builtin_warnings()` 打到 stderr 一次 —— HTTP 是每请求构建，不能每请求打印；
  2. `/health` 响应加 `"builtinWarnings"` 字段（**纯新增、不破坏契约**，GUI 可据此提示）；
  3. 新增守卫测试 `builtin_registry_has_no_warnings()`；现有 `route_templates_list`（断言 `drill_cycle` 存在，`server.rs:2612`）与 `part_generate_*`（依赖 `program_header` / `safe_move`）已是天然哨兵，内置模板消失会立刻变红。
- **需向 core 确认**：用户模板 `include` 一个缺失的内置模板时，应给出**明确的解析错误**，而不是产出空/残缺内容。

---

## 4. 重复代码块清单（给出具体位置）

| # | 重复内容 | 出现位置 | 建议 |
|---|---|---|---|
| 1 | `WriteError` 分类映射 | `output.rs:70-107`、`output.rs:251-277`、`server.rs:1409-1446`、`preset.rs:114-119`、`templates.rs:711-732`、`templates.rs:735-743` | 收敛为一张 `classify_write_error` 表（见 P1-5） |
| 2 | 预设保存全链路（归属→值校验→同名→upsert） | `preset.rs:195-309` vs `server.rs:984-1126`（其中"未知参数"块 `preset.rs:228-251` vs `server.rs:1050-1075` 近乎逐字重复） | 抽 `save_preset()` 纯业务函数 |
| 3 | `map_write_err` 函数体 | `templates.rs:724-726` 与 `output.rs:263-265` 逐字相同 | 同上，删掉 templates 版 |
| 4 | "请求体不是合法 JSON" 解析块 | `server.rs:439-440`、`991-993`、`1135-1137`、`1171-1173`（4 份） | 统一走 `api_body()` |
| 5 | `ResolvedMeta::resolve` 规格解析 | `context.rs:252-253` 与 `templates.rs:608-623` | 抽 `specs_for_file()`（见 P1-4） |
| 6 | 模板解析（注册表名 vs 文件路径） | `render.rs:161-224` 与 `templates.rs:112-137` | 统一（见 §3.4） |
| 7 | `1 MiB` / `10` / `9999` 常量 | `args.rs:144` / `server.rs:57`；`cli.rs:314,318` / `server.rs:601,602` / `server.rs:2999,3000` | 提为共享 `pub const` |
| 8 | `PartOptions` 布尔严格解析 | `server.rs:716-733`（`opt_bool`）与 `server.rs:546-557`（`get_bool`）两套写法 | 合为一个 `strict_bool(opts, name)` |
| 9 | `ensure_outside_template_root` 循环 | 仅一份（`preset.rs:71-79`），✓ 已共享 | — （正面记录） |
| 10 | 机床枚举 | `machine.rs:52` 与 `server.rs:853` 均走 `MachinePreset::entries`，✓ 已共享 | — （正面记录） |

---

## 5. `server.rs` 是否应该拆分？——**是**，且应分两步

### 判断依据

1. **绝对值过大**：3556 行，是工程最大文件，是第二名 `context.rs`(1071) 的 3.3 倍。
2. **生产部分也偏大**：扣除测试后仍有 1503 行，含 37 个函数，职责横跨 5 个领域（安全判定、HTTP 粘合、模板 API、生成 API、预设 API、URL 编解码）。
3. **已出现超长函数**：`presets_save` **150 行**（`984-1134`）、`part_generate` **128 行**（`693-821`）、`serve_requests` **84 行**（`1269-1353`）、`render` **79 行**（`614-693`）。
4. **职责边界清晰**（这点很关键）：现有函数几乎不共享可变状态（只传 `&Ctx`），拆分只需搬移 + 调 `pub(crate)`，**不需要重构数据流**。
5. 文件自己的注释已在多处承认"两份会漂移"（`server.rs:852`、`server.rs:818-820`），说明维护压力真实存在。

### 拆分方案（保持对外 API 零变更）

> **约束**：GUI 依赖 `server::route` / `server::Resp` / `server::listen_addr` / `server::bind` / `server::serve` / `server::browser_url`；`inspect.rs:99` 依赖 `server::spec_json`。拆分后由 `mod.rs` 重导出，签名与路径**完全不变**。

```
cli/src/server/
├── mod.rs            (~200 行) 模块声明 + 重导出 + Resp/ok/err + route 路由表
│                                + listen_addr / browser_url / bind / serve
├── guard.rs          (~230 行) MAX_BODY_BYTES / SECURITY_HEADERS / allowed_origins
│                                / cross_site_guard / host_guard / host_allowed
│                                / with_security_headers
├── http.rs           (~160 行) serve_requests / internal_error / cli_error
│                                / cli_error_mapped / write_error_resp
├── url.rs            (~60 行)  parse_query / percent_decode
├── templates_api.rs  (~200 行) templates_list / template_detail / parse_category
│                                / spec_json / vars_json
│                                / machine_schema_json / machines_list
├── generate_api.rs   (~330 行) api_body / json_underflow_resp / api_template_params
│                                / registered_template / api_machine
│                                / generation_options / validate / render / part_generate
├── presets_api.rs    (~400 行) presets_path / presets_list / presets_save / presets_delete
│                                / param_value_flat_json / params_flat_json
│                                / ★ save_preset（与 CLI preset::save 共享，见下）
└── （测试外置，见第 8 项）
```

`mod.rs` 保留完整的模块级安全文档（`server.rs:1-23`），并把 `#![allow(rustdoc::private_intra_doc_links)]` 一并搬过去。

### 第 1 步：纯移动（零行为变更）

- 逐文件搬运，**不改一行逻辑**，只把跨模块调用改为 `use super::*` 或加 `pub(crate)`。
- 回归网（现有即可覆盖，无需新增）：
  - `api_routes_are_routable`（`server.rs:2494`）——守卫路由表不漏端点
  - `option_mapping_matches_shared_fixture`（`server.rs:2937`）——守卫选项映射
  - `serve_requests_covers_html_query_guard_and_limits`（`server.rs:3468`）——端到端覆盖 Html/query/403/413
  - `ui.rs` 的 8 个启动序列用例
- 验收：`cargo test -p nctool-cli` 全绿 + `git diff --stat` 只有移动（可用 `git log -M --follow` 验证 rename 识别）。

### 第 2 步：抽公共业务函数（消除重复，行为会有意变更）

- 在 `presets_api.rs` 抽出与通道无关的核心：

```rust
pub(crate) enum PresetSaveError { BadRequest(String), Validation(String), NotFound(CliError), Write(WriteError) }

pub(crate) fn save_preset(
    ctx: &Ctx, name: &str, template: &str, params: ParameterSet, force: bool,
) -> Result<SaveOutcome, PresetSaveError> { /* 归属 → L3 → 同名 → upsert，顺序唯一 */ }
```

  然后 `commands/preset.rs::save` 与 `server::presets_save` 各自只做「取参 → 调 `save_preset` → 映射错误到 CliError/Resp」。这同时修掉 §3.7 的顺序漂移。

- 在 `output.rs` 落地 `classify_write_error`（P1-5），`http.rs::write_error_resp` 改为查表。

### 第 3 步（可选）：测试外置

- 把 `server.rs:1504-3556` 的 **2053 行测试**迁到 `cli/tests/`，与本仓库 `cli/tests/cli_*.rs` 惯例一致：
  - 纯函数/不需磁盘的（`host_guard`、 `cross_site_guard`、`percent_decode`、`write_error_resp`、`generation_options`）→ 留在各子模块 `#[cfg(test)]`，跑得快
  - 需 `PresetEnv` / 改 `APPDATA` 的（约 1200 行）→ `cli/tests/server_presets_api.rs`
  - 真实 HTTP 循环的（`serve_requests_*`，2 个）→ `cli/tests/server_http.rs`
- 迁走后 `server/mod.rs` + 6 个子模块生产代码合计约 **1580 行**，单文件最大 `presets_api.rs` 约 400 行，可读性与增量编译都会明显改善。

### 不建议做的事

- ❌ **不要**把 `route()` 改成 `async`/多线程：`Ctx` 用 `Rc<GCodeGenerator>` + `RefCell` 缓存，是 `!Send + !Sync`（`context.rs:856-858` 有实测注记）。要并发必须先改 core 的两处 `OnceCell` → `OnceLock`，属独立改造。
- ❌ **不要**在拆分时顺手删文档注释：`server.rs` 里大量"为什么"（如 `host_guard` 关于 DNS rebinding 的推理、`presets_path` 关于红线 9 的推理）是这份代码最有价值的部分，搬运时必须原样保留。

---

## 6. 本模块 TOP 3 必须优先修复

### 🥇 TOP 1　P0-1：HTTP 500/400 回显内部绝对路径（`server.rs:535 / 637 / 672 / 354 / 1196`）

**为什么第一**：这是本次审查中唯一的**安全漏洞**，而且它违反的是本模块**自己写在文件头**（`server.rs:10-11`）和 `internal_error` 文档里的红线。现有测试 `server.rs:3188-3203` 只钉住了 `internal_error` 本身，让这条漏洞在"有门禁"的错觉下长期存在。修复成本极低（5 行替换 + 1 条测试），收益是堵住用户名/目录结构外泄。

### 🥈 TOP 2　P1-4：按文件路径渲染时参数规格被整份丢弃（`render.rs:215`）

**为什么第二**：这是**会产出错误 G-code 的静默缺陷**——同一份模板用注册表名渲染会校验类型/区间/白名单，用绝对路径渲染则完全不校验，而两者对用户呈现的都是"校验通过"。按本项目自己的红线定义（"静默产出错误程序"），这类问题优先于一切可维护性议题。修复方式是复用已有的 `resolve_specs`（`templates.rs:603`），成本很低。

### 🥉 TOP 3　P1-3：`/api/presets` 全系列回传预设绝对路径（`server.rs:972 / 1120 / 1155 / 1415`）

**为什么第三**：与 TOP 1 同类，但为**主动、无条件、每次成功响应都发**，且路径必然含 Windows 用户名。修复只需把 `path.display()` 换成相对化/脱敏串，并让 `write_error_resp` 的 `Conflict` 分支与同函数内已脱敏的 `Corrupt`/`Io`/`ReadOnly` 三个分支对齐。

> **紧随其后（建议同一批次处理）**：P1-1 请求体吞错、P1-2 单连接挂死服务——两者都在 `serve_requests` 的同一段代码里（`server.rs:1300-1306` 与 `1272`），一次改动可一起修完；P1-5 的 `WriteError` 6 份映射分裂是最大的可维护性债，建议随 `server.rs` 拆分一并收敛。

---

## 附录：审查覆盖率说明

- 已逐行通读：`server.rs`(3556)、`context.rs`(1071)、`output.rs`(687)、`cli.rs`(691)、`args.rs`(728)、`config.rs`(307)、`commands/` 全部 13 个文件、`main.rs`、`lib.rs`
- 行号核实方式：Read 工具实际读取 + Grep 定位；涉及第三方行为（tiny_http 0.12 `ServerConfig` 无超时字段、`num_connections()` 为 `unimplemented!()`、`add_file` 签名、core 的 `ensure_outside_template_root`/`normalize`）均已查阅 `~/.cargo/registry` 与 `core/src` 源码确认
- 未覆盖范围：`cli/tests/`（7 个集成测试文件，5737 行）——按任务约定不在本次范围内
