# nctool 桌面 GUI — Phase 1 (MVP) 实施方案

> 依据：`docs/gui-design/nctool-gui-设计方案.html`（v1.2 修订稿，2026-09-25）。
> 本方案所有技术判断均已回源码交叉验证（见文末〈勘误〉），不照抄设计稿。
> 范围：设计稿 §5 的 **Phase 1（MVP）**；不含 Phase 2/3。

- 目标仓库：`D:\Desktop\nctool\rustjinja`（下称"仓库根"）
- 交付物：`nctool-gui.exe`（Tauri 2 + React），完成"选模板 → 填参数 → 校验 → 预览 → 保存"闭环，输出与 CLI **逐字节一致**（含 JSON 数值下溢硬失败）
- 本机环境：Windows 开发机、**无头**（不能弹 GUI）、rustc/cargo 1.98.0、node v22 / npm 10.9、WebView2 已装、**未装 tauri CLI**

---

## 0. 结论摘要（先读这段）

1. **cli 库化**：新增 `cli/src/lib.rs`，声明 `pub mod server/context/config/output/cli`，`args`/`commands` 保持私有；`cli/src/main.rs` 删掉全部 `mod` 声明改为 `use nctool_cli::…`。**不改 `nctool-core`**。唯一需要补的公开构造器是 `Ctx::for_embedded(template_dir, default_machine, loaded)`。
2. **GUI 复用契约层**：所有 Tauri command 内部构造 `Ctx` → 拼 HTTP 请求体 → 调 `nctool_cli::server::route(&ctx, method, path, query, body)`（纯函数，无网络）→ 把返回的 `{ok,data}`/`{ok,error}` 包络映射成 `Result<serde_json::Value, CommandError>`。**不新增端点，不改 `scripts/api_routes.json`。**
3. **参数通道安全**：command 入参为 `params_json: String`（前端 `JSON.stringify` 后传入），Rust 侧把该字符串**文本拼接**进请求体（**绝不** `from_str`→`to_string`），下溢守卫因此原样生效。
4. **AppState 只存纯数据**（`template_dir` + `default_machine` + `LoadedConfig`，均 `Send+Sync`），每命令在 `spawn_blocking` 里现建 `Ctx`（`Ctx` 是 `!Send`，绝不跨 `await`）。
5. **六页形态**：渲染页可用；模板/检查/机床/预设/设置五页为占位。
6. **本机可验证**：L1 `cargo check -p nctool-gui`、L2 `cargo build -p nctool-gui`、L3 前端 `npm run build`、L4 `cargo test --workspace`、L5 `route()` 与 CLI golden 逐字节对拍。**不可验证**：真实弹窗、NSIS 打包（需 tauri CLI，本机没有）。
7. **设计稿有 12 处与实际代码不符/遗漏**（见 §8），其中 3 处照抄会直接编译不过或行为错误，1 处（Windows 构建强制要求 `icons/icon.ico`）会让 `cargo build` 直接失败。

---

## 1. 范围界定

### 1.1 Phase 1 功能项（对齐设计稿 §5 Phase 1 的 11 项）

| # | 功能项 | 本环境可验证性 | 备注 |
|---|--------|----------------|------|
| 1 | 契约复用（cli 加 lib.rs + 公开 `Ctx` 构造器，跑通 inspect + render） | ✅ 可验证 | `cargo test -p nctool-cli --lib` + gui 侧 `route()` 单测 |
| 2 | 参数通道安全（`params_json` 文本层 + 下溢对抗用例进 CI） | ✅ 可验证 | **门槛项**；见 §3.5、§7 L5 |
| 3 | Tauri 2 项目脚手架 | ⚠️ 只能保证"可编译" | 无头，无法启动窗口 |
| 4 | AppState 设计 | ✅ 可验证（编译 + 单测） | `Send+Sync` 由编译期保证 |
| 5 | 侧边栏 + 路由布局 | ⚠️ 只能保证"可编译/可构建" | 视觉需人工 |
| 6 | 渲染页：模板选择 + 变量表单 + NC 预览 | ⚠️ 部分可验证 | 逻辑可单测；交互需人工 |
| 7 | 实时预览（300ms 防抖） | ⚠️ 部分可验证 | 防抖逻辑可单测 |
| 8 | 文件保存对话框 | ❌ 不可验证 | 需真实窗口 + 用户点选 |
| 9 | NC 语法高亮组件 | ✅ 可验证（纯函数 tokenizer） | 不引入 Monaco/CodeMirror |
| 10 | 深色工业主题 CSS | ⚠️ 只能保证"可构建" | 视觉需人工 |
| 11 | Windows .exe 构建 + NSIS 安装包 | ❌ 不可验证 | **需 tauri CLI + NSIS**，本机没有 |

> **门槛项**：只有 #2（参数通道安全）是门槛项。设计稿 v1.2 已把"契约复用"降级为常规落地（本方案确认该降级成立，见 §2）。

### 1.2 GUI 六页在 Phase 1 的形态

| 页面 | 路由 id | Phase 1 形态 |
|------|---------|--------------|
| 渲染页 | `render` | **可用**：模板下拉（`GET /api/templates`）→ 变量表单（`GET /api/templates/{name}`）→ 机床下拉（`GET /api/machines`）→ 300ms 防抖预览（`POST /api/render`）→ 保存（dialog + `save_nc_file`） |
| 模板管理页 | `templates` | 占位（显示"Phase 2"） |
| 静态检查页 | `checks` | 占位 |
| 机床配置页 | `machine` | 占位 |
| 参数预设页 | `preset` | 占位 |
| 设置页 | `config` | 占位（可在状态栏透出 `get_config` 的只读摘要，可选） |

### 1.3 明确不做（Phase 1）

- 不新增任何 HTTP 端点；不改 `scripts/api_routes.json`；不改 Web UI（`ui/`、`cli/ui/`）。
- 不改 `nctool-core`（设计稿硬约束，本方案遵守）。
- 不做模板/机床/预设的**写**操作（`new_template` / `save_template_source` / `save_machine` / 预设 CRUD）——全在 Phase 2。
- 不做键盘快捷键系统、多窗口对比、错误 toast 体系（Phase 2）。
- 不做 NSIS 安装包的**验证**（只写进构建脚本/文档，交由有 tauri CLI 的 Windows 机器执行）。

---

## 2. cli 库化（bin → bin + lib）的确切改法

### 2.1 现状（源码事实）

- `cli/Cargo.toml`：`name = "nctool-cli"` v0.3.0，只有 `[[bin]] name = "nctool" path = "src/main.rs"`，**无 lib target**。
- `cli/src/main.rs`（37 行）：7 行私有 `mod` 声明（`args`/`cli`/`commands`/`config`/`context`/`output`/`server`）+ `clap` 解析 + 退出码。
- `cli/src/context.rs`：
  - `pub struct Ctx { pub style, pub verbose, pub template_dir, pub default_machine, pub loaded, pub(crate) registry_cache: RefCell<Option<(RegistryKey, Rc<GCodeGenerator>)>> }`，`RegistryKey` 是 `pub(crate)`。
  - 公开构造器只有 `Ctx::from_global(&GlobalArgs)`（读配置 + 向 stderr 打 warning）。
  - `pub fn build_registry_fresh(&self) -> Result<GCodeGenerator, CliError>`（返回 owned，不碰缓存）。
  - `pub fn resolve_machine(&self, Option<&str>) -> Result<MachineConfig, CliError>`。
  - `#[cfg(test)] pub(crate) fn for_test()` + `#[cfg(test)] #[ignore] fn spike_registry_build_cost()`（内部调 `crate::server::route`）。
- `cli/src/server.rs`：`pub fn route(ctx: &Ctx, method, path, query, body) -> Resp`；`pub enum Resp { Json(u16, Value), Html }`。响应包络为 `{ok:true,data}` / `{ok:false,error:{kind,message}}`。
- `cli/src/commands/mod.rs`：`impl Command { pub fn run(&self, g: &GlobalArgs) -> Result<(), CliError> }`（`Command` 是 `pub enum`，位于 `cli.rs`）。

### 2.2 新增 `cli/src/lib.rs`（完整模块声明与可见性决策）

```rust
//! nctool-cli —— nctool 命令行工具的**库形态**。
//!
//! 7 个模块中仅 4 个对外公开：GUI 复用 `server::route` 契约层、
//! `context::Ctx`、`config` 配置层、`output::CliError`/下溢守卫；
//! `args` / `commands` 仅供 crate 内部（含 bin）使用。

pub mod cli;      // Cli / GlobalArgs / Command / CategoryArg / FormatArg —— main.rs 用
pub mod config;   // LoadedConfig / load / init_config
pub mod context;  // Ctx / Ctx::for_embedded
pub mod output;   // CliError / OutputStyle / json_underflow_error / report_json
pub mod server;   // route / Resp —— GUI 复用的契约层

mod args;         // parameter_set_from_json / read_text_capped —— 仅 crate 内部
mod commands;     // Command::run 的落地 —— 仅 crate 内部
```

**可见性依据**：
- `main.rs` 需要 `cli::Cli`、`output::OutputStyle` → `cli`、`output` 必须 `pub`。
- GUI 需要 `server::route`、`context::Ctx`、`config::LoadedConfig`、`output::CliError` → 这 4 个 `pub`。
- `args` / `commands` 无需对外：`Command::run` 是 `pub` 方法、`Command` 是 `pub` 类型，**inherent impl 的方法可见性只取决于方法自身与类型的可达性，与 impl 所在模块无关** —— 因此 `mod commands;` 保持私有，`main.rs` 仍可 `cli.command.run(...)`。
- **不要**给 `lib.rs` 加 `#![warn(missing_docs)]`（现有 bin 没有；加上会引入大量 doc warning，`cargo doc -D warnings` 会红）。新公开项（`Ctx::for_embedded`）仍按惯例写 doc 注释。

### 2.3 唯一需要补的公开构造器

约束：`registry_cache` 与 `RegistryKey` 都是 `pub(crate)`，**外部 crate 无法用结构体字面量构造 `Ctx`**；`from_global` 会读盘 + 打 stderr warning（GUI 无 stderr）。故在 `cli/src/context.rs` 内（能访问 `pub(crate)` 字段）新增：

```rust
impl Ctx {
    /// 嵌入式（GUI / 库）构造器：**不读盘、不向 stderr 打印 warning**。
    ///
    /// - `template_dir` / `default_machine`：调用方已解析好的值（显式参数优先于配置）。
    /// - `loaded`：一次性加载的层叠配置（GUI 侧在启动时 `config::load()` 得到）。
    ///
    /// 配置层的降级 warning 不打印，由调用方从 `ctx.loaded.warnings` 读取并自行呈现。
    pub fn for_embedded(
        template_dir: Option<PathBuf>,
        default_machine: Option<String>,
        loaded: config::LoadedConfig,
    ) -> Self {
        Self {
            style: OutputStyle::Text,
            verbose: false,
            template_dir,
            default_machine,
            loaded,
            registry_cache: RefCell::new(None), // 每命令新建 → 首次 build_registry 即重建
        }
    }
}
```

- 该构造器是**纯数据填充**，无 IO、无 `Result`。配置读取与"显式参数覆盖配置"的优先级逻辑放在 GUI 的 `AppState::load()` 里（见 §3.3）。
- 不新增字段、不改 `Ctx` 派生（`Debug, Clone` 保持）。**不动 core**。
- `Ctx::for_test()`（`#[cfg(test)] pub(crate)`）**不受影响**：库化后它仍在 lib crate 内、测试模块仍在同一 crate，`Ctx::for_test()` 照旧可用。

### 2.4 `cli/src/main.rs` 改法

**必须删掉全部 7 行 `mod` 声明**，否则 7 个模块会被 lib 与 bin **各编译一次**、测试**跑两遍**（见 §2.5）。改为：

```rust
use std::process::ExitCode;
use clap::Parser;
use nctool_cli::cli::Cli;
use nctool_cli::output::OutputStyle;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let style = OutputStyle::from(&cli.global.format);
    match cli.command.run(&cli.global) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            style.print_error(&err);
            ExitCode::from(err.exit_code())
        }
    }
}
```

依赖的可见性均已满足：`Cli`/`GlobalArgs`/`Command`/`FormatArg` 为 `pub`，`Cli.global`/`Cli.command` 为 `pub`，`Command::run` 为 `pub`，`OutputStyle`/`print_error`/`CliError::exit_code` 为 `pub`。`clap` 已是 `[dependencies]`，bin 与 lib 均可 `use clap::Parser`。

`cli/Cargo.toml`：**无需改动**（`src/lib.rs` 存在时 cargo 自动生成 lib target）。可选：显式加 `[lib] name = "nctool_cli" path = "src/lib.rs"` 以求清晰（lib crate 名 `nctool_cli`）。

### 2.5 `cargo test` 三种调用面的差异（关键，必须写进改动）

| 命令 | 库化前 | 库化后 |
|------|--------|--------|
| `cargo test -p nctool-cli --bin nctool` | 跑全部单测（模块属于 bin） | **跑 0 个测试**（bin 已无测试） |
| `cargo test -p nctool-cli --lib` | 无 lib target，报错 | 跑**全部**单测（含 `spike_registry_build_cost`） |
| `cargo test --workspace --all-targets` | 单测在 bin target 跑一遍 | 单测在 **lib target 跑一遍**，bin target 0 个 → **不重复** |

结论：
- 单测的"家"从 `--bin nctool` 迁到 `--lib`，**总量不变、不重复**。
- **必须更新**两处已失效的复现命令（否则工程师会以为探针还在跑）：
  - `cli/src/context.rs` 第 804–806 行注释：`cargo test -p nctool-cli --bin nctool --release -- --ignored --nocapture spike_registry_build_cost` → 改为 `--lib`。
  - 设计稿 §4.6 的同一命令（勘误 E6）。
- `cli/tests/*.rs` 的 7 个集成测试全部走 `assert_cmd`（`Command::new(env!("CARGO_BIN_EXE_nctool"))`），**不 `use nctool_cli::`**，故 lib 化不破坏它们；`CARGO_BIN_EXE_nctool` 仍由 bin target 提供。
- `spike_registry_build_cost` 内的 `crate::server::route` / `crate::args` / `tree_stamp` 引用在 lib crate 内全部解析正常，无需改动函数体。

---

## 3. gui/ crate 精确设计

### 3.1 目录结构（Phase 1 落地）

```
gui/
├── Cargo.toml
├── build.rs                      # tauri_build::build()
├── tauri.conf.json
├── capabilities/default.json     # 权限：core:default + dialog:allow-save
├── icons/icon.ico                # ★ Windows 构建强制要求存在（见 §3.8）
├── src/
│   ├── main.rs                   # tauri::Builder + invoke_handler 注册
│   ├── state.rs                  # AppState（纯数据，Send+Sync）
│   └── commands/
│       ├── mod.rs                # 模块声明 + re-export
│       ├── shared.rs             # CommandError / run_route / build_render_body / snapshot
│       ├── render.rs             # render_template
│       ├── templates.rs          # list_templates / get_template
│       ├── checks.rs             # validate_template
│       ├── machines.rs           # list_machines
│       ├── config.rs             # get_config
│       └── nc.rs                 # save_nc_file
└── frontend/                     # 见 §4
```

### 3.2 `gui/Cargo.toml`（完整）

```toml
[package]
name = "nctool-gui"
version = "0.1.0"
edition = "2021"
rust-version = "1.85"
description = "nctool 桌面 GUI（Tauri 2 + React），进程内复用 nctool-cli 的服务契约"
license = "MIT"
publish = false

# Phase 1 为桌面-only：只用 [[bin]]，不建 lib target。
# （若日后做移动端/被其它 crate 依赖，再加 [lib] crate-type = ["staticlib","cdylib","rlib"]）
[[bin]]
name = "nctool-gui"
path = "src/main.rs"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }          # 默认 features（含 wry / compression / common-controls-v6 …）
tauri-plugin-dialog = "2"                         # 保存对话框（前端 save() + 权限）
serde = { version = "1", features = ["derive"] }
serde_json = "1"
nctool-core = { path = "../core", version = "0.3.0" }
nctool-cli  = { path = "../cli",  version = "0.3.0" }

[features]
# 生产构建（把前端资源嵌进二进制）用；tauri CLI 构建时会启用。
# ⚠️ CI coverage 命令若带 `--all-features` 会误启用它 → 需要 frontend/dist 存在。
#    本方案已把 coverage 改为不带 --all-features（见 §7.3）。
custom-protocol = ["tauri/custom-protocol"]
```

要点：
- `tauri = { version = "2" }` 解析到 ≥2.11.5（缓存已有 2.11.5；如需严格复用缓存，可写 `=2.11.5` / `tauri-build = "=2.6.3"`）。
- `tauri-plugin-dialog` **不在** `~/.cargo/registry/cache`（本机缓存 622 个 crate 中无此包）→ 首次构建需联网拉取（网络可用）。
- **`nctool-gui` 作为 workspace 成员后，根 crate 的 `exclude` 与 `check_package_contents.py` 必须同步改**（见 §7.3 gate 10）：根 `Cargo.toml` 的 `exclude` 加 `"gui/"`；`scripts/check_package_contents.py` 的 `FORBIDDEN` 加 `"gui/"`。虽然 `cargo package` 对子包通常会自动跳过，但**用黑名单守卫显式登记更稳**，避免 `gui/frontend/**` 被打进 `nctool-tpl` 的 `.crate`。

### 3.3 `AppState`（只存纯数据，满足 `Send + Sync`）

```rust
// gui/src/state.rs
#[derive(Clone)]
pub struct AppState {
    /// 已解析的模板目录（显式参数优先于配置；None = 仅内置模板）
    pub template_dir: Option<std::path::PathBuf>,
    /// 已解析的默认机床
    pub default_machine: Option<String>,
    /// 一次性加载的层叠配置（只读；`resolve_machine` 查自定义机床要用）
    pub loaded: nctool_cli::config::LoadedConfig,
    /// 配置层降级 warning（透出到状态栏）
    pub warnings: Vec<String>,
}

impl AppState {
    /// 启动时加载：读配置（不打印 warning，交由前端呈现）。
    pub fn load(
        template_dir: Option<std::path::PathBuf>,
        default_machine: Option<String>,
    ) -> Result<Self, nctool_cli::output::CliError> {
        let loaded = nctool_cli::config::load()?;
        Ok(Self {
            template_dir: template_dir.or_else(|| loaded.merged.template_dir.clone()),
            default_machine: default_machine.or_else(|| loaded.merged.default_machine.clone()),
            warnings: loaded.warnings.clone(),
            loaded,
        })
    }

    /// 每命令现建一个 Ctx（`registry_cache` 为空 → 首个 build_registry 即重建注册表）。
    pub fn ctx(&self) -> nctool_cli::context::Ctx {
        nctool_cli::context::Ctx::for_embedded(
            self.template_dir.clone(),
            self.default_machine.clone(),
            self.loaded.clone(),
        )
    }

    /// 取出可跨线程的纯数据快照（`Ctx` 是 `!Send`，绝不跨 await）。
    pub fn snapshot(&self) -> (Option<std::path::PathBuf>, Option<String>, nctool_cli::config::LoadedConfig) {
        (self.template_dir.clone(), self.default_machine.clone(), self.loaded.clone())
    }
}
```

- `LoadedConfig` = `{ Option<PathBuf>, Option<PathBuf>, NctoolConfig(Option<PathBuf>+Option<String>+BTreeMap<String,MachineConfig>), Vec<String> }`，**全部 `Send + Sync`** → `AppState: Send + Sync`，满足 `State<'_, AppState>` 约束。
- `Ctx` 含 `RefCell<…Rc<…>>`，是 `!Send`/`!Sync` —— **绝不放进 State**，只在命令体内临时构造。
- 若 `AppState::load()` 失败（配置 IO 错），`main.rs` 应回退到 `LoadedConfig::default()` 并把错误写进 `warnings`，而不是 panic（保证窗口能起来）。

### 3.4 Tauri commands —— 完整签名与"调 cli 的哪一行"

**统一集成模式**（每命令一致）：

```
命令入参（标量，Tauri 自动把 JS camelCase 映射到 Rust snake_case）
  → state.snapshot() 取出 Send 纯数据
  → tauri::async_runtime::spawn_blocking(move || {
        let ctx = Ctx::for_embedded(td, dm, loaded);   // 工作线程内建 Ctx
        拼请求体 body（params_json 文本拼接）
        run_route(&ctx, METHOD, PATH, QUERY, body.as_bytes())
    }).await
```

**关键设计决策：`route()` 返回的是 `serde_json::Value` 包络 —— 直接转发 data，不再投影成强类型。**

- **决定**：命令返回 `Result<serde_json::Value, CommandError>`；成功时把包络的 `data` 原样转发给前端，失败时把 `error:{kind,message}` 转成 `Err(CommandError{kind,message,status})`。
- **理由**：① 包络本身**就是**被 parity 门禁约束的契约（Web UI 的 `API.request` 也消费同一份），再投影一遍等于在 GUI 侧复制一份"契约影子"，新增字段会被静默丢弃；② 前端 TS 类型只需声明一次（`types.ts`），同时可用于未来 Web UI；③ 错误用 `Err` 走 Tauri 的 Promise reject，符合 JS 习惯，且保留 `kind` 供前端分支。
- **不采用**：把 `data` 反序列化成 `RenderResult`/`InspectResult` 等 Rust 结构再序列化 —— 徒增一次往返与漂移面。

#### 3.4.1 `commands/shared.rs`

```rust
#[derive(Debug, serde::Serialize)]
pub struct CommandError { pub kind: String, pub message: String, pub status: u16 }

pub(crate) fn run_route(
    ctx: &nctool_cli::context::Ctx,
    method: &str, path: &str, query: &str, body: &[u8],
) -> Result<serde_json::Value, CommandError> {
    use nctool_cli::server::{route, Resp};
    match route(ctx, method, path, query, body) {
        Resp::Json(status, env) => {
            if env.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                Ok(env.get("data").cloned().unwrap_or(serde_json::Value::Null))
            } else {
                let e = env.get("error");
                Err(CommandError {
                    kind:    e.and_then(|e| e.get("kind")).and_then(|v| v.as_str()).unwrap_or("internal").into(),
                    message: e.and_then(|e| e.get("message")).and_then(|v| v.as_str()).unwrap_or("未知错误").into(),
                    status,
                })
            }
        }
        Resp::Html => Err(CommandError { kind: "internal".into(), message: "非 API 响应".into(), status: 500 }),
    }
}
```

#### 3.4.2 `commands/render.rs`

```rust
/// 选项结构：字段名与 HTTP `options` 键**逐字对齐**（camelCase）。
#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GenOptions {
    pub line_numbers: bool,              // → lineNumbers
    #[serde(default = "step_default")] pub line_step: u32,   // → lineStep   (默认 10)
    #[serde(default = "max_default")]  pub max_line: u32,    // → maxLine    (默认 9999)
    pub add_header: bool,                // → addHeader
    pub strip_blank: bool,               // → stripBlank
    pub ascii: bool,                     // → ascii
    pub lenient: bool,                   // → lenient
    pub machine: Option<String>,         // 顶层字段（构造 body 时从 options 里取出）
}
fn step_default() -> u32 { 10 }
fn max_default() -> u32 { 9999 }

/// ★ 关键：`params_json` 以**文本**拼接进请求体，绝不 from_str→to_string。
pub(crate) fn build_render_body(template: &str, params_json: &str, opts: &GenOptions) -> String {
    let mut o = serde_json::to_value(opts).expect("GenOptions 序列化");
    let machine = o.as_object_mut().and_then(|m| m.remove("machine"));
    let opts_json = serde_json::to_string(&o).expect("options 序列化");
    let mut body = format!(
        "{{\"template\":{t},\"params\":{p},\"options\":{o}",
        t = serde_json::to_string(template).unwrap(),
        p = params_json,       // ← 原样文本
        o = opts_json,
    );
    if let Some(m) = machine.filter(|m| !m.is_null()) {
        body.push_str(&format!(",\"machine\":{}", serde_json::to_string(&m).unwrap()));
    }
    body.push('}');
    body
}

#[tauri::command]
pub async fn render_template(
    template: String,
    params_json: String,
    options: Option<GenOptions>,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, CommandError> {
    let (td, dm, loaded) = state.snapshot();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = nctool_cli::context::Ctx::for_embedded(td, dm, loaded);
        let body = build_render_body(&template, &params_json, &options.unwrap_or_default());
        run_route(&ctx, "POST", "/api/render", "", body.as_bytes())
    })
    .await
    .map_err(|e| CommandError { kind: "internal".into(), message: format!("任务调度失败: {e}"), status: 500 })?
}
```

- **前端调用**：`invoke('render_template', { template, paramsJson, options })`（Tauri 把 `paramsJson` 映射到 `params_json`）。
- 对应的 cli 调用：`nctool_cli::server::route(&ctx, "POST", "/api/render", "", body.as_bytes())` —— 与 Web UI 完全同一行。
- 响应 `data` = `{blocked, report, output, warnings?, template?, machine}`（**blocked 时无 `warnings`/`template`**，前端需判空）。

#### 3.4.3 `commands/templates.rs`

```rust
#[tauri::command]
pub async fn list_templates(category: Option<String>, state: tauri::State<'_, AppState>)
    -> Result<serde_json::Value, CommandError> {
    let (td, dm, loaded) = state.snapshot();
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = Ctx::for_embedded(td, dm, loaded);
        let query = match category.filter(|c| !c.trim().is_empty()) {
            Some(c) => format!("category={}", percent_encode(&c)),
            None => String::new(),
        };
        run_route(&ctx, "GET", "/api/templates", &query, b"")
    }).await.map_err(internal_spawn)?
}

#[tauri::command]
pub async fn get_template(name: String, state: tauri::State<'_, AppState>)
    -> Result<serde_json::Value, CommandError> {
    // GET /api/templates/{percent_encode(name)}；name 含 '/' → 必须把 '/' 编成 %2F
    // route 内部对 raw 段做 percent_decode，故编码后能还原 "turning/undercut.j2"
}
```

- 响应：`list_templates` → `data = {templates:[{name,category,description}]}`；`get_template` → `data = {template:{name,category,description,builtin,source,params,variables}}`。
- `category` 取值是 core 的中文标签（`TemplateCategory::label()`：通用/铣削/车削/钻孔/切槽/机床）；`parse_category` 走 `FromStr`，中文与英文 id 均可。

#### 3.4.4 `commands/checks.rs`

```rust
#[tauri::command]
pub async fn validate_template(
    template: String, params_json: String, state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, CommandError> {
    // body = {"template":<t>,"params":<params_json 原样文本>}
    // route(ctx, "POST", "/api/validate", "", body) → data = {report: {...}}
}
```

#### 3.4.5 `commands/machines.rs`

```rust
#[tauri::command]
pub async fn list_machines(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, CommandError> {
    // route(ctx, "GET", "/api/machines", "", b"") → data = {machines:[...]}
}
```

#### 3.4.6 `commands/config.rs`

无对应 HTTP 端点，直接从 `state.loaded` 投影只读视图：

```rust
#[tauri::command]
pub async fn get_config(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, CommandError> {
    Ok(serde_json::json!({
        "globalPath":     state.loaded.global_path,
        "projectPath":    state.loaded.project_path,
        "templateDir":    state.loaded.merged.template_dir,
        "defaultMachine": state.loaded.merged.default_machine,
        "customMachines": state.loaded.merged.machine.keys().collect::<Vec<_>>(),
        "warnings":       state.warnings,
    }))
}
```

#### 3.4.7 `commands/nc.rs`

```rust
#[tauri::command]
pub async fn save_nc_file(path: String, content: String) -> Result<(), CommandError> {
    std::fs::write(&path, content.as_bytes()).map_err(|e| CommandError {
        kind: "io".into(), message: format!("写入失败 {path}: {e}"), status: 500,
    })
}
```
> NC 输出是**用户选定路径**的成品文件，不是模板/机床/预设资产，故走 `std::fs::write` 合法（设计稿 §1.3 的"写盘走 `core::asset`"只约束资产）。对话框由**前端** `@tauri-apps/plugin-dialog` 的 `save()` 取得路径，再传路径字符串进来。

#### 3.4.8 `main.rs`

```rust
fn main() {
    let state = AppState::load(None, None).unwrap_or_else(|e| {
        eprintln!("warning: 加载配置失败，使用默认值: {e}");
        AppState { template_dir: None, default_machine: None,
                   loaded: Default::default(), warnings: vec![e.to_string()] }
    });
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::render_template,
            commands::list_templates,
            commands::get_template,
            commands::validate_template,
            commands::list_machines,
            commands::get_config,
            commands::save_nc_file,
        ])
        .run(tauri::generate_context!())
        .expect("运行 nctool-gui 失败");
}
```

### 3.5 `inspect` 端点信息不足的问题 —— 方案

**事实核对**：
- 真实 `POST /api/inspect` 只返回 `{template, issues:[], required:[...], optional:[...]}` —— **不含 `params`（`ParamSpec` 列表）、不含 `source`、不含清单字段**。
- 渲染页表单需要 `ParamSpec` 列表 → 真实来源是 **`GET /api/templates/{name}`**（`template_detail`），它返回
  `{template:{name, category, description, builtin, source, params:[…], variables:{required:[…],optional:[…]}}}`。
- 设计稿 §4.2 的 `InspectResult`（含 `params/variables/refs` + 清单字段 `output_filename/output_extension/machine/visible/status`）**两个端点都给不全** —— 清单字段与 `refs` 需新端点。

**决定（Phase 1）**：
1. 渲染页表单改用 **`GET /api/templates/{name}`**（命令名 `get_template`）。该端点已存在、已登记在 `api_routes.json`（`covers: "/api/templates/"`），**不需要改 `api_routes.json`，不需要新增端点**。
2. **不实现** `inspect_template` 命令（Phase 1 用不到 `refs`/清单字段）。
3. 设计稿 §4.2 的丰富 `InspectResult`（清单字段 + `refs`）留到 Phase 2：届时要么新增 `GET /api/templates/{name}/meta`（同时改 4 处 + parity），要么在 cli 侧加一个返回 `ResolvedMeta` 的内部函数 —— **Phase 1 不做**（勘误 E3）。

### 3.6 错误映射：HTTP `kind` / 状态码 → 前端 `{kind,message,status}`

| HTTP | 包络 `error.kind` | 触发点（源码） | 前端建议处理 |
|------|-------------------|----------------|--------------|
| 400 | `bad_request` | `api_body` 非法 JSON / `api_template_params` 缺 `template` / `options.*` 类型错 / `options.format` 非法 | 表单/请求错误提示 |
| 400 | `num_underflow` | `json_underflow_resp`（`api_body` 内，文本层守卫命中） | **独立红色错误**："数值下溢"（门槛项对抗用例） |
| 400 | `render` | `render` 处理器 `generate_*_outcome` 返回 `Err` | 预览区红色错误 |
| 400 | `cli_error.kind`（如 `template_compile`） | `template_detail`/`inspect` 的 `extract_variables` 失败 → `cli_error` | 错误提示 |
| 404 | `template_not_found` | `template_detail` / `registered_template` / `inspect` | 提示 + 刷新模板列表 |
| 404 | `machine_not_found` | `api_machine` → `resolve_machine` | 提示并回退 generic |
| 404 | `preset_not_found` | `cli_error_mapped` / `write_error_resp`（Phase 2） | — |
| 404 | `not_found` | `route` 未知路由 | 内部错误（不应出现） |
| 409 | `write_conflict` | `write_error_resp`（乐观锁，Phase 2） | "文件已被外部修改" |
| 409 | `name_conflict` | `write_error_resp`（名称已存在，Phase 2） | "名称已存在" |
| 500 | `registry` | `validate`/`render` 的 `registry.validate` 失败 | 状态栏错误 |
| 500 | `internal` | `internal_error`（IO/内部，正文不回显） | 状态栏错误 |

> **勘误 E5**：设计稿 §4.6 写 `kind` 取 `args/validate/io/conflict` —— 与实际 HTTP 通道不符。GUI 侧实际只会看到上表的 kind（`conflict` 的真实名是 `write_conflict`）。`CliError.kind` 仅用于 `get_config` 这类直调 cli 函数的命令（`config`/`io`/`args`）。

前端 `CommandError`：`{ kind: string; message: string; status: number }`；`api/nctool.ts` 统一 `try/catch`，把 Tauri 的 reject 规范化为该形状后抛出。

### 3.7 `tauri.conf.json` 关键字段

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "nctool",
  "version": "0.1.0",
  "identifier": "com.nctool.gui",
  "build": {
    "beforeDevCommand": "npm --prefix frontend run dev",
    "beforeBuildCommand": "npm --prefix frontend run build",
    "devUrl": "http://localhost:1420",
    "frontendDist": "frontend/dist"
  },
  "app": {
    "windows": [
      { "title": "nctool — NC 程序模板工具", "width": 1200, "height": 800,
        "minWidth": 900, "minHeight": 600, "resizable": true }
    ],
    "security": { "csp": null }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "icon": ["icons/icon.ico"],
    "windows": { "webviewInstallMode": { "type": "downloadBootstrapper" } }
  }
}
```

- `frontendDist` / `devUrl` 路径**相对 `tauri.conf.json` 所在目录**（`gui/`）解析（源码：`tauri-build` 用 `config_parent.join(p)`）。故 `frontendDist = "frontend/dist"` → `gui/frontend/dist`。
- `beforeDevCommand` / `beforeBuildCommand` **只被 tauri CLI 执行**（已由 `tauri-build` 2.6.3 源码确认：其代码中不存在 `before_*` 字段处理）。纯 `cargo build` **不会**构建前端 → 前端必须单独 `npm run build`（见 §7）。
- `bundle.icon` 只列 `.ico`，避免引用不存在的 png 触发打包期缺文件。

### 3.8 ★ Windows 构建强制要求 `gui/icons/icon.ico`

**源码事实**（`tauri-build` 2.6.3 `src/lib.rs`）：在 `target` 含 windows 时，构建脚本会为 exe 编译 Windows Resource，并**硬性要求** `.ico` 存在：

```
if window_icon_path.exists() { res.set_icon_with_id(...) }
else { return Err("`icons/icon.ico` not found; required for generating a Windows Resource file during tauri-build") }
```

默认路径 `icons/icon.ico`（相对 `gui/`）。**缺失则 `cargo build -p nctool-gui` 直接失败。** 故 Phase 1 必须提供 `gui/icons/icon.ico`（占位即可）。生成一个最小合法 ICO 的最简做法（工程步骤，非应用代码）：

```bash
python - <<'PY'
import struct
# 16x16 32bpp 全透明占位 ICO（BITMAPINFOHEADER + BGRA 像素 + AND 掩码）
w=h=16; px=bytes(0 for _ in range(w*h*4)); and_mask=bytes(0 for _ in range(h*((w+31)//32)*4))
bmp=struct.pack('<IiiHHIIiiII',40,w,h*2,1,32,0,len(px),0,0,0,0)+px+and_mask
ico=struct.pack('<HHH',0,1,1)+struct.pack('<BBBBHHII',w,h,0,0,1,32,len(bmp),22)+bmp
open('gui/icons/icon.ico','wb').write(ico)
PY
```

（或任意合法 `.ico` 均可；NSIS 打包可另用 `tauri icon` 生成完整图标集，属可选。）

### 3.9 `capabilities/default.json`

```json
{
  "identifier": "default",
  "description": "nctool GUI 默认能力：核心 + 保存对话框",
  "windows": ["main"],
  "permissions": ["core:default", "dialog:allow-save"]
}
```
- 自定义命令（`#[tauri::command]` + `generate_handler!`）无需 capability 授权；只有**插件/核心**命令需要 —— 故只需为 dialog 插件授权。
- capability 文件由 Tauri 从 `gui/capabilities/` 自动发现。

---

## 4. 前端设计（gui/frontend/）

### 4.1 文件清单

```
gui/frontend/
├── package.json
├── tsconfig.json
├── tsconfig.node.json
├── vite.config.ts
├── index.html
└── src/
    ├── main.tsx
    ├── App.tsx                 # 侧边栏 + 页面切换 + 状态栏
    ├── types.ts                # 包络与领域类型
    ├── api/nctool.ts           # invoke 封装
    ├── stores/appStore.ts      # zustand
    ├── pages/RenderPage.tsx    # 可用
    ├── pages/TemplatesPage.tsx # 占位
    ├── pages/ChecksPage.tsx    # 占位
    ├── pages/MachinePage.tsx   # 占位
    ├── pages/PresetPage.tsx    # 占位
    ├── pages/ConfigPage.tsx    # 占位
    ├── components/Sidebar.tsx
    ├── components/VarField.tsx
    ├── components/NcPreview.tsx
    ├── lib/highlight.ts        # NC tokenizer（轻量）
    └── lib/debounce.ts
    └── styles/industrial.css   # 深色工业主题（§4.7）
```

### 4.2 `package.json`（依赖与版本）

```json
{
  "name": "nctool-frontend",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "react": "^18.3.1",
    "react-dom": "^18.3.1",
    "@tauri-apps/api": "^2.0.0",
    "@tauri-apps/plugin-dialog": "^2.0.0",
    "zustand": "^5.0.2"
  },
  "devDependencies": {
    "@vitejs/plugin-react": "^4.3.4",
    "vite": "^5.4.11",
    "typescript": "^5.6.3",
    "@types/react": "^18.3.12",
    "@types/react-dom": "^18.3.1"
  }
}
```
> `@tauri-apps/cli` **不列为依赖**（Phase 1 不用 tauri CLI，见 §7.2）；若要本地打包，再以 devDependency 形式加入（预编译二进制，避免 `cargo install tauri-cli` 的长时间编译）。

### 4.3 `vite.config.ts` 要点（Tauri 约定）

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,                 // Tauri 约定：别清掉 CLI 日志
  server: {
    port: 1420,
    strictPort: true,                 // 端口被占则失败，不静默换端口（否则 devUrl 对不上）
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"], // Tauri 约定
  build: {
    target: "chrome105",              // WebView2 基线
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
```
> `server.port` 必须与 `tauri.conf.json` 的 `devUrl` 一致（1420）。

### 4.4 组件树

```
App
├── Sidebar                # 6 项导航（渲染/模板/检查/机床/预设/设置），点击改 appStore.page
├── <main>
│   ├── RenderPage         # Phase 1 唯一可用页
│   │   ├── TemplateSelect     # 来自 listTemplates()
│   │   ├── MachineSelect      # 来自 listMachines()
│   │   ├── OptionsBar         # 行号/步长/最大行号/header/stripBlank/ascii/lenient
│   │   ├── VarField[]         # 由 getTemplate().params 生成（必填标红）
│   │   └── NcPreview          # 高亮后的 output
│   └── TemplatesPage | ChecksPage | MachinePage | PresetPage | ConfigPage   # 占位
└── StatusBar              # 渲染状态 / 错误 / 配置 warning
```

### 4.5 `stores/appStore.ts` state 形状

```ts
type PageId = "render" | "templates" | "checks" | "machine" | "preset" | "config";

interface AppStore {
  page: PageId; setPage(p: PageId): void;

  // 模板 / 机床
  templates: TemplateSummary[];          // {name, category, description}
  machines: MachineSummary[];            // {id, vendor, model, builtin, ...}
  loadTemplates(): Promise<void>;
  loadMachines(): Promise<void>;

  // 渲染上下文
  selected: string | null;
  spec: ParamSpec[];                     // 来自 get_template().template.params
  variables: { required: VarLoc[]; optional: VarLoc[] };
  selectTemplate(name: string): Promise<void>;

  params: Record<string, unknown>;       // 表单值（字符串/数字/布尔/数组）
  setParam(name: string, value: unknown): void;   // 内部调用 scheduleRender()
  resetParams(): void;

  options: GenOptions;                   // {lineNumbers,lineStep,maxLine,addHeader,stripBlank,ascii,lenient,machine}
  setOption<K extends keyof GenOptions>(k: K, v: GenOptions[K]): void;  // 内部调用 scheduleRender()

  // 渲染输出
  output: string;
  blocked: boolean;
  warnings: string[];
  renderError: { kind: string; message: string } | null;
  rendering: boolean;
  renderNow(): Promise<void>;            // 立即渲染（首次/保存前/模板切换）
  scheduleRender(): void;                // 300ms 防抖 → renderNow()

  status: { level: "ok" | "warn" | "err"; message: string } | null;
}
```

- **300ms 防抖落点**：`setParam` / `setOption` 末尾调 `scheduleRender()`；`scheduleRender` 用 `lib/debounce.ts` 的定时器（模块级 `let timer`），300ms 内多次变更只触发最后一次 `renderNow()`。模板/机床切换调 `renderNow()`（不防抖）。
- `renderNow()`：`JSON.stringify(params)` 得到 `paramsJson` → `api.renderTemplate(selected, paramsJson, options)` → 写 `output/blocked/warnings/renderError`。**渲染失败只更新 `renderError`，不打断编辑**。

### 4.6 `api/nctool.ts` 封装签名

```ts
import { invoke } from "@tauri-apps/api/core";

export interface CommandError { kind: string; message: string; status: number; }

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try { return await invoke<T>(cmd, args); }
  catch (e) { throw normalize(e); }   // 规范化为 CommandError
}

export const api = {
  listTemplates: (category?: string) =>
    call<{ templates: TemplateSummary[] }>("list_templates", { category }),
  getTemplate: (name: string) =>
    call<{ template: TemplateDetail }>("get_template", { name }),
  listMachines: () =>
    call<{ machines: MachineSummary[] }>("list_machines"),
  renderTemplate: (template: string, paramsJson: string, options: GenOptions) =>
    call<RenderData>("render_template", { template, paramsJson, options }),
  validateTemplate: (template: string, paramsJson: string) =>
    call<{ report: unknown }>("validate_template", { template, paramsJson }),
  getConfig: () => call<ConfigDump>("get_config"),
  saveNcFile: (path: string, content: string) =>
    call<void>("save_nc_file", { path, content }),
};
```

> **硬约束（设计稿 §3.4）**：`renderTemplate` / `validateTemplate` 的 `paramsJson` 参数类型是 **`string`**；调用点必须先 `JSON.stringify(params)` 再传入。**绝不**把 `params` 对象直接交给 `invoke`。

保存流程（前端）：`const p = await save({ defaultPath: fileName }); if (p) await api.saveNcFile(p, output);`（`save` 来自 `@tauri-apps/plugin-dialog`）。

### 4.7 深色工业主题 CSS 变量表（抄自设计稿 §2 原型的 `:root`）

```css
:root{
  --bg-0:#0d1117; --bg-1:#161b22; --bg-2:#1c2330; --bg-3:#252d3d;
  --border:#30363d; --border-l:#485059;
  --txt-0:#e6edf3; --txt-1:#8b949e; --txt-2:#6e7681;
  --accent:#0078d4; --accent-l:#2b9fff; --accent-glow:rgba(0,120,212,.15);
  --ok:#3fb950; --warn:#d29922; --err:#f85149; --info:#58a6ff;
  --mono:'Cascadia Code','Fira Code','JetBrains Mono','Consolas','Courier New',monospace;
  --sans:'Segoe UI','Noto Sans SC','Microsoft YaHei',sans-serif;
}
```
NC 预览 token 配色（同原型）：注释 `.cmt #6a9955`、关键字 `.kw #c586c0`、数字 `.num #b5cea8`、变量 `.var #9cdcfe`、函数 `.fn #dcdcaa`。

### 4.8 NC 语法高亮的实现方式（不引重依赖）

- **决定**：自研轻量 tokenizer（`lib/highlight.ts`），输入 NC 文本，输出转义后的 HTML 字符串，用 `<pre dangerouslySetInnerHTML>` 渲染。
- **理由**：① Monaco（~2 MB+，含 worker、需 CSP 放宽）与 CodeMirror（~300 KB+，多包）对"**只读预览**"是重依赖，工业 PC 离线部署下体积/内存/启动成本都不划算；② 预览无需编辑、无需 LSP、无需 diff；③ 高亮规则极简单（G/M 代码、坐标、注释 `;…` 与 `(…)`、行号、模板变量），纯函数易于单测。
- **规则（Phase 1）**：按行切分 → 先提取注释（`;` 到行尾、`(...)` 内）→ 再匹配 `G\d+(\.\d+)?` / `M\d+` / `[A-Z][-+]?\d*\.?\d+` / `#\d+` / 括号表达式 → 用 span 包裹。行号用独立列（原型 `.ln` 样式）。
- 逃逸：所有非高亮文本先做 HTML 转义（`& < > "`），杜绝模板内容注入。

---

## 5. 有序任务列表（按实现顺序，可逐项勾掉）

> 依赖列里的 `Txx` 表示"必须先完成"。`[ ]` 便于工程师勾选。

- [ ] **T01 — cli 契约层库化（bin → bin + lib）**
  - 文件：`cli/src/lib.rs`（新增）、`cli/src/main.rs`（改）、`cli/src/context.rs`（新增 `Ctx::for_embedded`；更新 `spike_registry_build_cost` 复现命令注释为 `--lib`）、`cli/Cargo.toml`（可选显式 `[lib]`）
  - 依赖：无
  - 验收：`cargo build -p nctool-cli` 成功且 `nctool --version` 不变；`cargo test -p nctool-cli --lib` 全绿；`cargo test -p nctool-cli --lib -- --ignored --nocapture spike_registry_build_cost` 能跑；`cargo test --workspace --all-targets` 无重复；`cargo clippy -p nctool-cli --all-targets -- -D warnings` 通过；`RUSTDOCFLAGS=-D warnings cargo doc -p nctool-cli --no-deps` 通过

- [ ] **T02 — gui crate 骨架与构建配置**
  - 文件：`Cargo.toml`（根：`members += "gui"`、`exclude += "gui/"`）、`gui/Cargo.toml`、`gui/build.rs`、`gui/tauri.conf.json`、`gui/capabilities/default.json`、`gui/icons/icon.ico`、`gui/src/main.rs`、`gui/src/state.rs`、`gui/src/commands/mod.rs`（先空壳）、`.gitignore`（+`gui/frontend/node_modules/`、`gui/frontend/dist/`）、更新并提交 `Cargo.lock`
  - 依赖：T01
  - 验收：`cargo check -p nctool-gui` 通过（首次会拉取/编译 tauri 依赖，建议后台跑）

- [ ] **T03 — Tauri commands（契约复用 + 错误映射 + 下溢守卫）**
  - 文件：`gui/src/commands/{shared,render,templates,checks,machines,config,nc}.rs`、`gui/src/main.rs`（注册 handler + dialog 插件）
  - 依赖：T02
  - 验收：`cargo check -p nctool-gui` 通过；`cargo test -p nctool-gui` 中以下用例通过：① `build_render_body` 产出与期望字符串逐字相等；② `drill_cycle` + `{"x":21,"y":15,"depth":-10,"feed":100}` → `route()` 的 `data.output` 与 `tests/golden/drill_cycle_generic.nc` 逐字节相等；③ 对抗用例 `{"template":"drill_cycle","params":{"x":1e-400}}` → `Err(CommandError{kind:"num_underflow", status:400})` 且**不产出 G-code**

- [ ] **T04 — 前端脚手架（配置 + 布局 + 主题 + 占位页）**
  - 文件：`gui/frontend/{package.json,tsconfig.json,tsconfig.node.json,vite.config.ts,index.html}`、`src/main.tsx`、`src/App.tsx`、`src/components/Sidebar.tsx`、`src/styles/industrial.css`、`src/pages/{TemplatesPage,ChecksPage,MachinePage,PresetPage,ConfigPage}.tsx`
  - 依赖：T02（与 T03 并行）
  - 验收：`npm --prefix gui/frontend install` 后 `npm --prefix gui/frontend run build` 成功产出 `gui/frontend/dist/`；`tsc --noEmit` 无错

- [ ] **T05 — 前端 API 封装 + store**
  - 文件：`gui/frontend/src/{types.ts,api/nctool.ts,stores/appStore.ts,lib/debounce.ts}`
  - 依赖：T03、T04
  - 验收：`npm --prefix gui/frontend run build` 通过；类型与 §3.4 的 data 形状一致

- [ ] **T06 — 渲染页（表单 + 实时预览 + 保存）**
  - 文件：`gui/frontend/src/pages/RenderPage.tsx`、`src/components/{VarField,NcPreview}.tsx`、`src/lib/highlight.ts`
  - 依赖：T05
  - 验收：`npm --prefix gui/frontend run build` 通过；`highlightNc()` 单测（纯函数）通过；表单由 `params` 驱动、必填标红；变更 300ms 防抖后调 `render_template`；保存走 `save()` + `save_nc_file`；**`paramsJson` 以 `JSON.stringify(params)` 传入**

- [ ] **T07 — CI 与门禁适配（workspace 成员的连带改动）**
  - 文件：`.github/workflows/ci.yml`（ubuntu 加 WebKitGTK/GTK 系统依赖；coverage 命令去 `--all-features` 并加 `--ignore-filename-regex`）、`scripts/check_package_contents.py`（`FORBIDDEN += "gui/"`）
  - 依赖：T02
  - 验收：本地 `python3 scripts/check_package_contents.py` 通过；文档记录 CI 新增步骤（见 §7.3）

- [ ] **T08 — 文档与最终验收**
  - 文件：本文件（如需补充实测结果）、`README.md`（可选：说明 GUI crate 存在）
  - 依赖：T01–T07
  - 验收：§7 的 L0–L5 全绿；本机门禁（fmt/clippy/test/doc/package/parity/docs-links）全部通过

---

## 6. 依赖包清单

### 6.1 Rust 侧

| crate | 版本 | 用途 | 状态 |
|-------|------|------|------|
| `tauri` | 2（≥2.11.5） | 桌面壳 + IPC + `generate_context!` | **缓存已有 2.11.5** |
| `tauri-build` | 2（≥2.6.3） | build script（读 `tauri.conf.json`、Windows 资源） | **缓存已有 2.6.3** |
| `tauri-plugin-dialog` | 2 | 保存对话框（前端 `save()` + `dialog:allow-save`） | ⚠️ **不在缓存，需联网拉取** |
| `serde` | 1（derive） | command 入参/错误序列化 | 已在 workspace |
| `serde_json` | 1 | 包络 Value 处理 | 已在 workspace |
| `nctool-core` | path `../core` 0.3.0 | （间接：Ctx 内部） | 已有 |
| `nctool-cli` | path `../cli` 0.3.0 | **契约层 `server::route` / `Ctx` / `config` / `output`** | 已有（T01 后暴露 lib） |

传递依赖（tauri 树）：`wry` / `tao` / `muda` / `tray-icon` / `webkit2gtk`(linux) / `windows` 等 —— 缓存中已有 `wry 0.55.1` / `tao 0.35.3`，其余按需联网。

### 6.2 前端侧（npm）

| 包 | 版本 | 用途 | 状态 |
|----|------|------|------|
| `react` / `react-dom` | ^18.3.1 | UI 框架 | 新增 |
| `@tauri-apps/api` | ^2 | `invoke` | 新增 |
| `@tauri-apps/plugin-dialog` | ^2 | `save()` 保存对话框 | 新增 |
| `zustand` | ^5 | 全局状态 | 新增 |
| `vite` | ^5.4 | 构建 | 新增 |
| `@vitejs/plugin-react` | ^4.3 | React 支持 | 新增 |
| `typescript` | ^5.6 | 类型检查 | 新增 |
| `@types/react` / `@types/react-dom` | ^18.3 | 类型 | 新增 |

> 不引入 Monaco / CodeMirror（§4.8）。Phase 1 不引入 `react-router-dom`（用 `appStore.page` 内部切换）。

---

## 7. 构建与验证策略（无头环境）

### 7.1 分层验证矩阵

| 层 | 命令 | 目的 | 说明 |
|----|------|------|------|
| **L0** | `cargo fmt --all -- --check` | 格式 | 新增 Rust 代码必须先格式化 |
| **L1** | `cargo check -p nctool-gui` | 快检 | 首次需拉取+编译 tauri 依赖（数分钟），**建议后台跑** |
| **L2** | `cargo build -p nctool-gui` | 编出 exe | 默认 features = dev 模式，**不需要 `frontend/dist` 存在**（源码确认：`tauri/build.rs` 中 `dev = !has_feature("custom-protocol")`，dev 模式不嵌入资源） |
| **L2b** | `npm --prefix gui/frontend run build` 然后 `cargo build -p nctool-gui --features custom-protocol` | 校验生产嵌入路径 | 验证 `frontendDist` 路径正确、资源可嵌入（此时需要 `frontend/dist`） |
| **L3** | `npm --prefix gui/frontend run build`（含 `tsc --noEmit`） | 前端构建 | 不需要浏览器；产出 `gui/frontend/dist` |
| **L4** | `cargo test --workspace --all-targets --locked` | 全量测试 | 含 gui 的 `#[cfg(test)]`（bin target） |
| **L5** | gui crate 单测（`cargo test -p nctool-gui`） | **逐字节一致性 + 门槛项** | 见下 |

### 7.2 是否安装 tauri CLI —— 结论：**Phase 1 不装**

- `cargo check/build -p nctool-gui` 与 `cargo test` **都不需要** tauri CLI：`tauri-build` 是 build-dependency，cargo 自动编译（已由 `tauri-build` 2.6.3 源码确认）。
- `tauri.conf.json` 的 `beforeBuildCommand`/`beforeDevCommand` **只在 tauri CLI 下执行**；纯 `cargo build` 不构建前端 → 前端**必须单独** `npm run build`（源码确认 `tauri-build` 不处理 `before_*`）。
- 唯一需要 tauri CLI 的是 **Phase 1 第 11 项（NSIS 安装包）** 与 `tauri dev` 热重载。本机无头、无 tauri CLI → **这两项在本机不可验证**。
- 若确需打包：**用 npm devDependency `@tauri-apps/cli@^2`（预编译二进制，`npx tauri build`）**，而不是 `cargo install tauri-cli`（从源码编译，耗时长）。且 NSIS 需另装 NSIS 工具链。→ 留待有 Windows GUI 的机器执行。

### 7.3 现有 CI 门禁的影响（逐条）

> 现有 `.github/workflows/ci.yml` 的 gate 与本次改动的关系：

| Gate | 是否受影响 | 必须的动作 |
|------|-----------|-----------|
| `build_ui.mjs --check` | ❌ 不受影响 | 只读 `ui/`、`cli/ui/`、`ui/src/*.part.html` |
| `check_param_parity.mjs` | ❌ 不受影响 | 只读 `ui/`、`cli/ui/`、`cli/src/args.rs` |
| `check_option_parity.mjs` | ❌ 不受影响 | 只读 `ui/`、`cli/ui/`、`cli/src/server.rs` |
| `check_api_parity.mjs` | ❌ 不受影响（**前提：不新增端点**） | 只读 `ui/`、`cli/ui/`、`api_routes.json`；GUI 走 `invoke`（非 `/api/` 字面量），Phase 1 不纳管 |
| `check_docs_links.py` | ⚠️ 受影响 | 扫描 `docs/` → **本文件不得含坏链/坏锚点**（本文件刻意只用反引号路径，不用 markdown 链接） |
| Golden refresh guards | ❌ 不受影响 | |
| `cargo fmt --all -- --check` | ⚠️ 受影响 | 新增 Rust 代码需格式化 |
| `cargo clippy --workspace --all-targets -- -D warnings` | ⚠️ 受影响 | **gui 代码必须 clippy-clean**（含 `--all-targets`） |
| `cargo test --workspace --all-targets --locked` | ⚠️ **受影响（关键）** | ① **必须更新并提交 `Cargo.lock`**（新增 tauri 依赖树；`--locked` 会因此报错）；② **ubuntu/macOS runner 需装系统依赖**，否则编译 tauri 直接失败（见下） |
| `check_package_contents.py` | ⚠️ 受影响 | `FORBIDDEN += "gui/"`；根 `Cargo.toml` `exclude += "gui/"`（防 `gui/**` 混进 `nctool-tpl` 的 `.crate`） |
| `cargo test --workspace --doc` | ⚠️ 受影响 | cli 新增 lib doc target；确保无新 intra-doc 坏链 |
| `cargo doc --workspace --no-deps`（`-D warnings`） | ⚠️ 受影响 | 同上；`lib.rs` **不要**加 `#![warn(missing_docs)]` |
| `cargo audit --deny warnings` | ⚠️ 受影响 | 新增 tauri 依赖树需审计；若命中 advisory，用 `.cargo/audit.toml` 的 `ignore`（附理由）或升级 |
| **coverage**（`cargo llvm-cov --workspace --all-features --lcov` + `check_coverage_caliber.py --min 91`） | ⚠️ **受影响（关键）** | ① **gui 未覆盖的生产代码会把 91% 拉低**（当前生产口径 4672/5068≈92.19%，余量仅 ~60 行；gui 数百行未覆盖会直接击穿）；② **`--all-features` 会启用 gui 的 `custom-protocol`** → 编译期要求 `frontend/dist` 存在 → coverage job 会失败。**对策**：把命令改为 `cargo llvm-cov --workspace --lcov --output-path lcov.info --ignore-filename-regex '(^|/)gui/'`（**去掉 `--all-features`**，已确认三个现有 crate 均无 `[features]`，去掉无副作用）+ ubuntu 加系统依赖 |
| **msrv**（`cargo check --workspace --locked` on 1.85） | ⚠️ 受影响 | 需 webkit 系统依赖；`tauri` MSRV 1.77.2 / `tauri-build` 1.77.2 ✓（<1.85），但传递依赖 MSRV 需实测 |

**ubuntu runner 需要的系统依赖**（加到 quality / coverage / msrv 三个 job）：

```yaml
- name: Install Linux GUI deps (Tauri)
  if: matrix.os == 'ubuntu-latest'   # coverage / msrv job 无 matrix，条件去掉
  run: |
    sudo apt-get update
    sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
      libayatana-appindicator3-dev librsvg2-dev patchelf \
      libsoup-3.0-dev libjavascriptcoregtk-4.1-dev
```

> **备选（若不愿动 CI 系统依赖）**：把 gui 移出默认 workspace（`default-members = [".", "core", "cli"]`）并给它独立 job —— 但 `cargo test --workspace` 仍会拉入 gui，故 **`--workspace` 的命令必须改 `-p` 选择或加系统依赖**；独立 job 更复杂。**本方案推荐加系统依赖**（改动小、语义清晰）。

### 7.4 L5：GUI 输出与 CLI 逐字节一致的验证方法

在 `gui/src/commands/render.rs`（bin target 的 `#[cfg(test)] mod tests`）写**不走 Tauri** 的单测：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nctool_cli::context::Ctx;
    use nctool_cli::config::LoadedConfig;

    fn repo_templates() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("templates")
    }
    fn ctx() -> Ctx {
        Ctx::for_embedded(Some(repo_templates()), None, LoadedConfig::default())
    }

    // ① 请求体构造：camelCase / machine 顶层 / params 文本拼接
    #[test]
    fn build_render_body_shape() {
        let opts = GenOptions { line_numbers: true, line_step: 10, max_line: 9999,
                                machine: Some("generic".into()), ..Default::default() };
        let body = build_render_body("drill_cycle", r#"{"x":21,"y":15,"depth":-10,"feed":100}"#, &opts);
        assert_eq!(body, r#"{"template":"drill_cycle","params":{"x":21,"y":15,"depth":-10,"feed":100},"options":{"lineNumbers":true,"lineStep":10,"maxLine":9999,"addHeader":false,"stripBlank":false,"ascii":false,"lenient":false},"machine":"generic"}"#);
    }

    // ② 逐字节一致性：与 CLI golden 基线对拍（tests/golden/drill_cycle_generic.nc）
    #[test]
    fn output_matches_cli_golden() {
        let body = build_render_body("drill_cycle", r#"{"x":21,"y":15,"depth":-10,"feed":100}"#, &GenOptions::default());
        let data = run_route(&ctx(), "POST", "/api/render", "", body.as_bytes()).unwrap();
        let out = data["output"].as_str().unwrap();
        let golden = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("tests/golden/drill_cycle_generic.nc")
        ).unwrap().replace("\r\n", "\n");
        assert_eq!(out, golden, "GUI 输出必须与 CLI golden 逐字节一致");
    }

    // ③ 门槛项：JSON 下溢必须硬失败且不产出 G-code
    #[test]
    fn underflow_is_hard_error() {
        let body = build_render_body("drill_cycle", r#"{"x":1e-400,"y":15,"depth":-10,"feed":100}"#, &GenOptions::default());
        let err = run_route(&ctx(), "POST", "/api/render", "", body.as_bytes()).unwrap_err();
        assert_eq!(err.kind, "num_underflow");
        assert_eq!(err.status, 400);
    }
}
```

- golden 基线来自 `core/tests/integration.rs::golden_cases()`（`drill_cycle` 用 `{x:21,y:15,depth:-10,feed:100}`，machine `generic`）→ `tests/golden/drill_cycle_generic.nc`，**参数与上表逐字一致**。
- 该测试**不依赖 tauri 运行时**（只调 `route()`），因此在无头环境可跑。
- ③ 同时覆盖"CLI / Web UI / GUI 三条入口的下溢守卫"这一门槛项（GUI 入口即此用例）。

---

## 8. 风险与待明确项

### 8.1 发现的设计稿错误 / 遗漏（**本项目最看重的一项**）

| # | 设计稿位置 | 问题（对照源码） | 正确做法 |
|---|-----------|------------------|----------|
| **E1** | §4.1 / §4.2 | 伪代码调用 `shared::render_from_params_json(&state, args)` / `shared::inspect_entry` / `shared::lint_source` —— 这些函数**在仓库中不存在** | 真实集成点是 `nctool_cli::server::route(&ctx, method, path, query, body)`；包络是 `{ok,data}`/`{ok,error}`，**不是** `RenderResult` |
| **E2** | §4.1 `GenOptions` | 把 `machine` 放进 `options`，且字段名（`line_number_step` 等）未映射到 HTTP 键；`max_line_number` 的 camelCase 是 `maxLineNumber` 而非 `maxLine` | `machine` 在请求体**顶层**；`options` 键为 `lineNumbers/lineStep/maxLine/addHeader/stripBlank/ascii/lenient/format`（见 §3.4.2） |
| **E3** | §4.2 `InspectResult` | 声称含 `params/variables/refs` + 清单字段；实际 `POST /api/inspect` 只回 `{template,issues,required,optional}`，**无 params/source/清单字段/refs** | 渲染页表单改用 `GET /api/templates/{name}`（`params` 齐备）；丰富 `InspectResult` 留 Phase 2（需新端点）。**不改 `api_routes.json`** |
| **E4** | §4.3 `TemplateSummary` | 含 `param_count/visible/output_extension/machine/status`；实际 `GET /api/templates` 只回 `{name,category,description}` | Phase 1 只用这三个字段；其余留 Phase 2 |
| **E5** | §4.6 错误分类 | 写 `kind` 取 `args/validate/io/conflict`；实际 HTTP 通道 kind 是 `bad_request/num_underflow/template_not_found/machine_not_found/preset_not_found/render/registry/internal/not_found/write_conflict/name_conflict`，409 的 kind 是 `write_conflict`（**非** `conflict`） | 用 §3.6 对照表 |
| **E6** | §4.6 + `context.rs` 注释 | 复现命令 `cargo test -p nctool-cli --bin nctool … spike_registry_build_cost` 在库化后**失效**（测试已迁到 lib target） | 改为 `--lib` |
| **E7** | §1.4 | 只写 `gui/icons/`，未提 **Windows 构建强制要求 `icons/icon.ico` 存在**，缺失则 `cargo build` 直接失败（`tauri-build` 源码硬报错） | Phase 1 必须提供 `gui/icons/icon.ico`（见 §3.8） |
| **E8** | §1.4 / §1.5 | 未提"gui 作为 workspace 成员"的 **CI 连带影响**：ubuntu/macOS 需系统依赖；coverage 91% 会被拉低；`--all-features` 会因 `custom-protocol` 要求 `frontend/dist` 而失败；cargo-audit 审计新依赖树 | 见 §7.3 逐条对策 |
| **E9** | §5 Phase 1 第 11 项 | "Windows .exe + NSIS" 依赖 tauri CLI；未说明 `cargo build` **不执行** `beforeBuildCommand`、前端需单独 `npm run build`；也未说明 dev 模式不需 `dist` | 见 §7.2；本机不可验证 |
| **E10** | §3.4 / §4 | "前端先 `JSON.stringify` 再放进 invoke"正确，但**漏了 Rust 侧必须文本拼接**：若 GUI 把 `params_json` `from_str`→`to_string` 再放进 body，下溢守卫失效 | §3.4.2 的 `build_render_body` 原样拼接 |
| **E11** | §1.5 | "把现有 7 个私有 `mod` 声明为 `pub mod`"——**不应全 pub**，且**未提 `main.rs` 必须删掉 `mod` 声明**（否则 lib/bin 各编译一次、测试跑两遍） | 见 §2.2 / §2.4 |
| **E12** | §1.4 / `check_package_contents.py` | 未提根 crate `exclude` 与 `FORBIDDEN` 需补 `gui/`，否则 `gui/**` 可能混进 `nctool-tpl` 的 `.crate` | `exclude += "gui/"`；`FORBIDDEN += "gui/"` |

### 8.2 其余风险

1. **`tauri-plugin-dialog` 不在缓存** → 首次构建需联网（网络已确认可用；若离线则失败）。
2. **ubuntu/macOS CI 编译 tauri 需系统依赖** → 不加 apt 步骤则 `cargo test/clippy/doc/msrv/coverage --workspace` 全红（§7.3）。
3. **`cargo audit` 可能因 tauri 依赖树命中 advisory 而红** → 需实测；命中则用 `.cargo/audit.toml` 忽略（附理由）或升级。
4. **Cargo.lock 必须更新并提交**（`--locked` 会因此报错）。
5. **首次 `cargo build -p nctool-gui` 编译 tauri 依赖可能 10–30 分钟** → 后台跑，不要误判超时。
6. **`beforeDevCommand` 的工作目录**（tauri CLI 从何处执行）需在有 tauri CLI 的机器上实测确认；本方案用 `npm --prefix frontend …` 降低敏感性，但最终需验证。
7. **同步 vs 异步命令**：本方案用 `async fn` + `spawn_blocking`（`Ctx` 在工作线程内构造，绝不跨 `await`）。若工程师误改成"在 async 体内直接 `let ctx = state.ctx(); … .await`"，会因 `Ctx: !Send` **编译不过** —— 这是刻意的编译期护栏。
8. **`.gitignore`**：需加 `gui/frontend/node_modules/`、`gui/frontend/dist/`。
9. **本机无头**：真实弹窗、拖拽、快捷键、NSIS 打包均**不可验证**，只能保证"可编译/可构建"；这些项的验收需在有 GUI 的 Windows 机器补做。

---

## 附：Phase 1 交付判定（Definition of Done）

- [ ] `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --all-targets --locked`、`cargo test --workspace --doc`、`RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps` 全绿
- [ ] `cargo check -p nctool-gui` / `cargo build -p nctool-gui` 成功（`gui/icons/icon.ico` 已提供）
- [ ] `npm --prefix gui/frontend run build` 成功产出 `gui/frontend/dist`
- [ ] L5 三个单测通过：请求体形状、`drill_cycle` 与 golden 逐字节一致、下溢硬失败（`num_underflow`/400，无输出）
- [ ] `python3 scripts/check_package_contents.py` 通过；`node scripts/check_api_parity.mjs` 通过（未新增端点）
- [ ] CI 已加 ubuntu 系统依赖；coverage 命令已去 `--all-features` 并排除 `gui/`；`Cargo.lock` 已提交
- [ ] 六页形态符合 §1.2（渲染页可用，其余占位）
- [ ] 本方案 §8.1 的 12 条勘误已在实现中规避
