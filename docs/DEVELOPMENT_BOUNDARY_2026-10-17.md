# NCtool 当前开发边界报告（AI 复刻用）

- 对象：`feat/shared-parameter-inputs` 分支工作树，HEAD `08ab296`，含未提交改动。
- 目的：让另一个 AI 在**不看本仓库源码**的前提下，复刻本项目的重要功能。所有内容均以当前源码为事实依据，逐条标注来源文件。
- 复刻口径：本文按「契约」写（结构、字段、数值、规则、顺序、错误码）。凡与仓库现有 `docs/` 冲突，**以源码为准**；文档描述滞后处在第 14 节单独列出。
- 日期：2026-10-17。

## 目录

1. [一句话边界](#1-一句话边界)
2. [Workspace 结构与依赖方向](#2-workspace-结构与依赖方向)
3. [根引擎 `nctool-tpl`](#3-根引擎-nctool-tpl公开-api-契约)
4. [资产层 `nctool-assets`](#4-资产层-nctool-assets)
5. [插件 SDK `nctool-plugin-sdk`](#5-插件-sdk-nctool-plugin-sdk)
6. [运行时 `nctool-runtime`](#6-运行时-nctool-runtime)
7. [CLI 与 HTTP](#7-cli-与-httpcli)
8. [模板插件 `nctool-plugin-template`](#8-模板插件-nctool-plugin-template)
9. [共享参数插件 `nctool-plugin-parameters`](#9-共享参数插件-nctool-plugin-parameters)
10. [数学插件 `nctool-plugin-math`](#10-数学插件-nctool-plugin-math)
11. [前端 WebUI](#11-前端-webui)
12. [复刻时必须保留的硬约束](#12-复刻时必须保留的硬约束架构门禁)
13. [完整验证命令](#13-完整验证命令复刻后照跑)
14. [文档与源码的已知偏差](#14-文档与源码的已知偏差复刻时以源码为准)
15. [复刻顺序建议](#15-复刻顺序建议)

---

## 1. 一句话边界

一个**领域中立**的通用 Jinja 文本渲染平台：模板资产管理 + 共享参数解析 + 可撤销的通用插件运行时 + 独立可选数学扩展 + 本地回环 WebUI。**不含**任何加工/NC 领域模型；领域能力只能通过通用插件机制从外部提供。

版本：CLI/WebUI `2.2.0`，其余 workspace crate `2.0.0`，插件协议/服务版本 `1`（三者独立维护，不要统一数字）。

---

## 2. Workspace 结构与依赖方向

8 个 crate（[Cargo.toml](../Cargo.toml)）：

| crate | 版本 | 路径 | 职责 |
| --- | --- | --- | --- |
| `nctool-tpl` | 2.0.0 | `src/` | Jinja 解析、变量分析、渲染；**生产依赖只有 minijinja** |
| `nctool-assets` | 2.0.0 | `assets/` | JSON 数值校验、资产存储、锁、指纹、原子写、集合导入回滚 |
| `nctool-plugin-sdk` | 2.0.0 | `sdk/` | 插件/动作/服务/诊断/取消/扩展契约 + Python 适配器 |
| `nctool-runtime` | 2.0.0 | `runtime/` | 插件组合、Schema 校验、配置、服务依赖、外部进程 RPC |
| `nctool-plugin-template` | 2.0.0 | `plugins/template/` | 模板/预设资产、检查、渲染、冻结快照 |
| `nctool-plugin-parameters` | 2.0.0 | `plugins/parameters/` | 共享参数定义/预置/绑定、来源解析、受管渲染 |
| `nctool-plugin-math` | 2.0.0 | `plugins/math/` | 可选数学过滤器 |
| `nctool-cli` | 2.2.0 | `cli/` | 装配根、CLI 命令、回环 HTTP、任务/历史、资产包 |

`default-members = [".", "cli"]`；`resolver = "2"`。

**允许的内部依赖**（`scripts/check_v2.py` 的 `NEUTRAL_DEPENDENCIES` 硬编码断言）：

```
nctool-assets         -> {}
nctool-plugin-sdk     -> {nctool-assets, nctool-tpl}
nctool-runtime        -> {nctool-assets, nctool-plugin-sdk}
nctool-plugin-template-> {nctool-assets, nctool-plugin-sdk, nctool-tpl}
nctool-plugin-parameters -> {nctool-assets, nctool-plugin-sdk}
nctool-tpl            -> {minijinja}   # 且仅此一个
```

CLI 依赖全部 6 个内部 crate，其中 `nctool-plugin-math` 为 `optional`。`cli` features：`default = []`，`math = ["dep:nctool-plugin-math"]`（**唯一**可选 feature，不存在 `nc-bundle`）。

根引擎 minijinja 依赖固定为：

```toml
minijinja = { version = "~2.24.0", features = ["unstable_machinery", "loop_controls", "debug", "fuel"] }
```

`unstable_machinery` 用于拿 AST，`debug` 用于错误字节范围定位，`fuel` 用于指令预算。

MSRV **Rust 1.89**（`File::try_lock`/`unlock` 在 1.89 稳定，锁实现依赖它）。

**仓库根目录的本地运行数据不属于交付物**：`/templates/`、`/presets/`、`/parameters/`、`/.nctool/`、`/.nctool-backups/`、`*.nctool.lock` 均在 `.gitignore` 中。当前工作树里 `parameters/nc/`（materials/profiles/standards/tool_types/tools）与大量 `v3-param-*` 是历史本机数据，复刻时**不要**把它们当成当前功能或示例。当前 `examples/` 包含 6 个文件：Rust 引擎示例 `demo.rs`、两个 v2 JSON 请求，以及 Python 插件的 `plugin.json`、`plugin.py`、`nctool_plugin.py`。

---

## 3. 根引擎 `nctool-tpl`（公开 API 契约）

源码：[src/lib.rs](../src/lib.rs)、[src/extract.rs](../src/extract.rs)、[src/renderer.rs](../src/renderer.rs)、[src/error.rs](../src/error.rs)。

### 3.1 再导出

```rust
pub use error::TplError;
pub use extract::{extract_member_accesses, extract_template_refs,
                  extract_undeclared, extract_variables, parse, Ast, Variable};
pub use renderer::Renderer;
pub use minijinja::functions::Function;
pub use minijinja::value::{FunctionArgs, FunctionResult};
pub use minijinja::{Error as EngineError, ErrorKind as EngineErrorKind, Value};
pub use minijinja::value::ValueKind;
```

下游**不得**自己声明 minijinja 依赖，统一从本 crate 取 `Value`，保证 workspace 内只有一个 `minijinja::Value` 类型。

### 3.2 类型

```rust
pub struct Variable {              // #[derive(Debug, Clone, PartialEq, Eq)]
    pub name: String,
    pub line: usize,               // 1 起
    pub col: usize,                // 1 起
    pub start: usize,              // 字节偏移
    pub end: usize,                // 字节偏移（不含）
    pub optional: bool,            // 全部引用都处于兜底上下文 → true
}

pub struct Ast<'a> {               // #[derive(Debug)]
    name: &'a str,                 // 私有
    source: &'a str,               // 私有
    pub(crate) stmt: Stmt<'a>,     // 私有
}
impl<'a> Ast<'a> {
    pub fn statement(&self) -> &minijinja::machinery::ast::Stmt<'_>;
    pub fn name(&self) -> &str;
    pub fn source(&self) -> &str;
}
```

### 3.3 函数

```rust
pub fn parse<'a>(source: &'a str, name: &'a str) -> Result<Ast<'a>, TplError>;
pub fn extract_variables<'a>(ast: &Ast<'a>) -> Vec<Variable>;
pub fn extract_undeclared<'a>(ast: &Ast<'a>) -> Vec<Variable>;
pub fn extract_template_refs(ast: &Ast) -> Vec<String>;
pub fn extract_member_accesses(ast: &Ast, root: &str) -> Vec<String>;
```

**变量提取精确规则**（`extract.rs`，全部有测试钉住）：

- 结果按**首次出现顺序去重**。
- 排除引擎内置名：`RESERVED_NAMES = ["loop","self","super","caller"]`。
- 排除 Jinja 自动注入全局：`BUILTIN_GLOBALS = ["range","dict","namespace","debug"]`。**只列 minijinja 真提供的**（不含 Python 侧的 `lipsum`/`cycler`/`joiner`）。
- 模板内 `set`/`for` 目标、宏名与宏参数、`with` 绑定、`namespace` kwarg 名 → 局部，不算未声明。
- `optional` 判定 = **全部**引用都处于兜底上下文。兜底上下文仅指：`default`/`d` 过滤器的**裸变量**直接操作数，或 `is defined`/`is undefined` 测试的**裸变量**直接操作数。
- **兜底不向下传播**（关键回归）：
  - `{{ (a+b) | default(1) }}` → `a`、`b` 均**必选**（先求值加法）。
  - `{{ a.b | default(1) }}` → `a` **必选**（先取属性）。
  - `{% if a.b is defined %}` → `a` **必选**。
  - 但 `{{ x | default(1) | upper }}` → `x` **可选**（裸变量先被 default 兜住）。
  - `{{ missing ~ "x" }}` 的 span 覆盖整条表达式，仍需能恢复出 `missing`。
- 同一变量既有兜底引用又有非兜底引用 → 整体**必选**。
- `{% set R1 = R1 | default(4000) %}` 自赋值惯用法 → `R1` **可选**；`{% set R1 = R1 + 1 %}` → **必选**。
- `{% with %}` 后续绑定能看到前一个绑定；`set` 块体内先求值 RHS 再声明目标。
- `for` 的 `filter_expr` 能看到循环变量；块体内 `set` 不外泄。

**`extract_template_refs`**：只枚举 `{% include %}`/`{% extends %}`/`{% import %}`/`{% from %}` 的**字符串字面量**，按序去重；动态名（变量/表达式）不收集。遍历所有嵌套语句体。

**`extract_member_accesses`**：只收 `root.<key>` 与 `root["常量字符串"]`，按序去重，空键忽略；属性链只取紧邻 `root` 的第一段；动态下标不收集；`root` 裸出现不产生键。

### 3.4 `Renderer`

```rust
pub struct Renderer {              // #[derive(Debug)]
    env: Environment<'static>,
    lenient: bool,
    filters: HashSet<String>,
    output_limit: usize,
}
impl Renderer {
    pub fn new() -> Self;
    pub fn with_limits(mut self, instructions: u64, output_bytes: usize) -> Result<Self, TplError>;
    pub fn with_whitespace(mut self, trim_blocks: bool, lstrip_blocks: bool) -> Self;
    pub fn add_filter<F, Rv, Args>(&mut self, name: &str, filter: F) -> Result<(), TplError>;
    pub fn with_lenient(mut self) -> Self;
    pub fn with_strict(mut self) -> Self;
    pub fn is_lenient(&self) -> bool;
    pub fn render(&self, source: &str, name: &str, context: &Value) -> Result<String, TplError>;
    pub fn add_template(&mut self, name: impl Into<String>, source: impl Into<String>) -> Result<(), TplError>;
    pub fn set_loader<F>(&mut self, loader: F) where F: Fn(&str) -> Result<Option<String>, EngineError> + Send + Sync + 'static;
    pub fn set_path_loader(&mut self, dir: impl AsRef<Path>);
    pub fn render_template(&self, name: &str, context: &Value) -> Result<String, TplError>;
}
```

**默认值（`new()`）**：`UndefinedBehavior::Strict`、`fuel = 1_000_000` 指令、`output_limit = 16 MiB`。`with_limits` 两个参数都必须非零，否则 `TplError::Render{name:"limits", message:"render budgets must be nonzero"}`。

**`add_filter` 拒绝名单**：重复注册 + 覆盖以下 **49** 个 Jinja 内置过滤器 → `TplError::Render{message:"duplicate filter registration"}`：

```
abs attr batch bool capitalize chain count d default dictsort e escape first float
format groupby indent int items join last length lines list lower map max min pprint
reject rejectattr replace reverse round safe select selectattr slice sort split
string sum title tojson trim unique upper urlencode zip
```

**同名语义**：`add_template` 同名**静默替换**；内存模板优先于 `set_path_loader`/`set_loader` 的同名文件。

**`set_path_loader` 安全规则**：拒绝空名、含 `:`、含 `\`、含非 `Component::Normal` 段的名字（`Ok(None)`）；root 与拼接结果都 canonicalize 并校验 root 前缀；单文件上限 **1 MiB**（读 `1024*1024+1` 字节判断）。

**`set_loader` 契约**：只有「模板不存在」才返回 `Ok(None)`；其它错误即使 `ignore missing` 也中止加载。

**输出上限实现**：`LimitedOutput` 实现 `std::io::Write`，超过 limit 即报错，最终 `into_string()`。

### 3.5 错误模型

```rust
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum TplError {
    Parse          { name: String, message: String, line: usize, col: usize },
    TemplateNotFound{ name: String, template: String, message: String },
    UndefinedVariable{ name: String, variable: String, message: String },
    UnknownFilter  { name: String, filter: String, message: String },
    UnknownTest    { name: String, test: String, message: String },
    Render         { name: String, message: String },
}
```

`Display` 格式（中文，逐变体）：

```
{name}:{line}: 模板语法错误: {message}
{name}: 模板未找到: {message}                                  // template 为空
{name}: 模板未找到 '{template}': {message}
{name}: 未定义变量: {message}                                  // variable 为空
{name}: 未定义变量 '{variable}': {message}
{name}: 未知过滤器: {message} / {name}: 未知过滤器 '{filter}': {message}
{name}: 未知测试: {message}   / {name}: 未知测试 '{test}': {message}
{name}: 渲染错误: {message}
```

**行列换算**：优先用 minijinja 错误 `range()` 的字节起点 → `line_col_at`；取不到时 `(err.line().unwrap_or(1), 1)`。列号按**字符**计而非字节（有回归）。

**错误名恢复**（`extract_undefined_var_name`）：span 起点恰好是裸标识符时才认定为缺失变量名。三条判据：
- span 前最后一个非空白字符是 `|` → 是过滤器名，**不**当变量名；
- span 覆盖属性/下标链 → 返回 `None`（宁缺毋错）；
- 但 `{{ missing ~ "x" }}` 这种整条表达式 span 仍要恢复首标识符。

### 3.6 引擎测试钉住的不变量（节选，复刻时应逐条实现）

`extract_basic`、`extract_dedup_and_order`、`extract_with_local_and_macro`、`parse_syntax_error_line`、`parse_error_column_is_located`（`{{ (1 + 2 }}` → col=11）、`parse_error_column_on_multiline`、`extract_undeclared_*`（default 链 / defined 测试 / 自赋值 / 混合引用 / 兜底不传播 / 过滤器链）、`render_template_include/extends/import_macro`、`from_import_alias_binds_alias_not_original`、`set_inside_block_does_not_leak`、`undefined_var_in_included_template_not_recovered_from_main_source`、`opaque_render_error_reports_offending_expression`、`detailed_error_does_not_add_snippet`、`reserved_names_not_undeclared`、`builtin_globals_are_exactly_what_minijinja_provides`、`member_accesses_dot_and_const_subscript_only`、`very_long_variable_name`、`many_distinct_variables`、`deeply_nested_ifs`、`raw_block_ignores_template_syntax`、`unicode_content_renders`。

---

## 4. 资产层 `nctool-assets`

源码：[assets/src/lib.rs](../assets/src/lib.rs)、[guard.rs](../assets/src/guard.rs)、[lock.rs](../assets/src/lock.rs)、[atomic.rs](../assets/src/atomic.rs)、[path.rs](../assets/src/path.rs)、[json_num.rs](../assets/src/json_num.rs)。

### 4.1 指纹（乐观锁）

```rust
pub struct FileFingerprint { pub hash: u64, pub len: u64, pub mtime: SystemTime }  // Copy
impl FileFingerprint {
    pub fn of_bytes(bytes: &[u8], mtime: SystemTime) -> Self;
    pub fn matches(&self, other: &Self) -> bool;   // hash && len && mtime 三者全等
    pub fn as_string(&self) -> String;             // "fnv1a64:{:016x}"
}
```

- 哈希 = **FNV-1a 64**（零依赖自实现）：`OFFSET_BASIS = 0xcbf29ce484222325`，`PRIME = 0x100000001b3`，`wrapping_mul`。
- 三元组任一不同即视为改动。**不用纯 mtime**：Windows 时间分辨率粗，同一秒内两次改动能漏检。
- `as_string()` 只含 hash，供 CLI 提示用；完整指纹以 `serde_json` 序列化字符串在 API 中传递（即 API 里的 `fingerprint` 字段）。

### 4.2 写内核

```rust
pub const CLI_LOCK_WAIT: Duration = Duration::from_secs(2);
pub struct WriteKernel;                     // 无状态，全关联函数
impl WriteKernel {
    pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), WriteError>;
    pub fn write_guarded(path, bytes, expect: Option<FileFingerprint>) -> Result<WriteOutcome, WriteError>;      // 等 2s
    pub fn write_guarded_with_wait(path, bytes, expect, wait: Duration) -> Result<WriteOutcome, WriteError>;
    pub fn remove_guarded(path, expect: Option<FileFingerprint>) -> Result<WriteOutcome, WriteError>;            // 等 2s
    pub fn read_fingerprint(path) -> Result<Option<FileFingerprint>, WriteError>;
}
```

**`write_guarded_with_wait` 流程**：取锁 → `read_snapshot` → 比对 `expect` → 不一致返回 `Conflict` 且**不写** → 写 → 释放（`Drop` 兜底）。

动作分类：目标不存在 → `Created`；内容与 `bytes` 相同 → `Unchanged`（跳过落盘）；否则 → `Updated`。

`expect` 语义：`Some(fp)` 要求写前指纹恰为 `fp`；`None` 要求写前**文件不存在**（新建语义）。

**按具体入口区分等待策略**：`JsonStore::save` 传 `Duration::ZERO`（try-lock，拿不到即 `LockBusy`）；`WriteKernel::write_guarded` 与 `remove_guarded` 默认等 2s，`GroupGuard::acquire` 也等 2s。不能概括为所有 HTTP 写入均不等待；当前 HTTP 使用 Axum/Tokio，通过 `spawn_blocking` 调用同步逻辑。

```rust
pub enum WriteAction { Created, Updated, Unchanged, Deleted }   // serde rename_all="lowercase"
pub struct WriteOutcome { pub path: PathBuf, pub action: WriteAction,
                          pub fingerprint: Option<String>, pub revision: Option<FileFingerprint> }
```

**原子写**（`atomic.rs`）：同目录临时文件 `<name>.nctool-tmp-<pid>-<nanos>.tmp` → `flush` + `sync_all` → `rename` → 父目录 fsync（尽力，拿不到句柄则静默跳过）。写盘包在 `with_retry` 里，线性退避 10/20/30/40ms（累计 ≤100ms），用于跨过杀毒扫描窗口。保证强度 = **原子可见 + 尽力持久**。

**跨进程锁**（`lock.rs`）：OS advisory lock（`File::try_lock`/`unlock`），锁文件 = `<stem>.nctool.lock`，与目标同目录。**只 unlock、不 unlink**（Unix 上 unlink 会让他人已持有的 fd 失效）。Windows 首次创建必须 `create_new` 两步走，否则并发首次创建会返回 `ERROR_ACCESS_DENIED` 被误判为只读。

```rust
pub enum WriteError {   // #[non_exhaustive]
    Conflict { path: PathBuf, expected: Option<FileFingerprint>, actual: Option<FileFingerprint> },
    LockBusy { path: PathBuf },
    PathEscape { rel: String, reason: String },
    ReadOnly { path: PathBuf },
    Io(std::io::Error),
    NotFound(String),
    Corrupt(String),
    NumRange { literal: String, line: usize, column: usize },
    NumUnderflow { path: PathBuf, literal: String, line: usize, column: usize },
}
```

`map_io`：`PermissionDenied` → `ReadOnly`；其余 → `Io`。

### 4.3 组锁

```rust
pub struct GroupGuard { _guard: lock::DirLock }
impl GroupGuard { pub fn acquire(root: &Path) -> Result<Self, WriteError> }   // 锁 <root>/.asset-group，等 2s
```

用于「多资产一致快照」：`capture`、`changed`、`import` 都在同一把组锁内完成，避免看到半应用的并发导入。

### 4.4 JsonStore

```rust
pub struct JsonStore { root: SafePath }              // Clone
impl JsonStore {
    pub fn new(root: &Path) -> Result<Self, WriteError>;               // create_dir_all + SafePath::from_root
    pub fn list(&self) -> Result<Vec<String>, WriteError>;             // *.json，排序
    pub fn read(&self, name: &str) -> Result<(Value, String), WriteError>;  // (资产, 指纹 JSON 串)
    pub fn remove(&self, name: &str, expected: &str) -> Result<(), WriteError>;
    pub fn save(&self, name: &str, value: &Value, expected: Option<&str>) -> Result<String, WriteError>;
}
```

- 磁盘布局：`<root>/<name>.json`。
- `save` 用 `to_vec_pretty`；**单资产上限 1 MiB**；`JsonStore` 一律传 `Duration::ZERO`（服务侧语义）。
- `save` 返回新指纹 JSON 串；`read` 返回的指纹串可直接回传给 `expected`。
- 资产名必须是单个合法路径段（见 4.6）。

### 4.5 集合导出/导入/回滚

```rust
pub fn export_collection(stores: &[(&str, &JsonStore)]) -> Result<Value, WriteError>;
pub fn import_collection(stores, data: &Value, validate_only: bool,
                         validate: impl Fn(&str,&str,&Value) -> Result<(),WriteError>) -> Result<Value, WriteError>;
pub fn rollback_collection(stores: &[(&str,&JsonStore)], receipt: &Value) -> Result<(), WriteError>;
```

导出格式：`{"version":1,"records":[{"group":..,"name":..,"asset":..}]}`；合计超 **64 MiB** 拒绝。

导入规则（顺序即拒绝顺序）：
1. `data["version"] != 1` → `Corrupt("不支持的资产集合版本")`；
2. `records` 非数组 → `Corrupt("缺少资产列表")`；
3. `records.len() > 2048` → 拒绝；
4. 每条：`group` 必须已知、`name` 过 `validate_asset_name`、`(group,name)` 不得重复；
5. 目标已存在（有指纹）→ `Conflict`（**首版导入拒绝同名冲突，不自动重命名**）；
6. `asset` 必须是对象；单资产 ≤ 1 MiB；
7. 调用方 `validate(group,name,asset)` 回调；
8. `validate_only` → 返回 `{"count":N,"mapping":[{"group","from","to"}]}`；
9. 否则逐条 `save(name, asset, None)`（新建语义），成功累积 receipt `{"group","name","fingerprint"}`；
10. 任一条失败 → `rollback_collection` **逆序**按 receipt 删除；回滚成功则报原错误，回滚失败则报 `Corrupt("导入失败，回滚受冲突阻止: {error}")`。

返回 `{"count":N,"receipt":[...]}`。

### 4.6 名称与路径

```rust
pub fn validate_asset_name(name: &str) -> Result<(), String>;
pub struct SafePath { root: PathBuf }             // 构造时 canonicalize
impl SafePath {
    pub fn from_root(root: &Path) -> Result<Self, WriteError>;   // root 必须存在且为目录
    pub fn root(&self) -> &Path;
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, WriteError>;
}
```

`validate_asset_name` 唯一合法形态 = **恰好一个 `Component::Normal`**。依次拒绝：空串、控制字符、`.`/`..`、含 `\`、盘符相对前缀（首字符 ASCII 字母 + 第二字符 `:`，跨平台统一判断）。

`SafePath::resolve` 双层校验：① 用 `symlink_metadata` 探测，目标是符号链接**直接拒绝**（消除 `exists()→canonicalize()` 的 TOCTOU）；② 归一（存在→目标自身，不存在→父目录 + 文件名）后断言 `starts_with(&self.root)`。**校验对象与返回对象是同一条路径**。

### 4.7 数值校验（`json_num.rs`，复刻重点）

```rust
pub struct UnderflowCandidate { pub literal: String, pub byte_offset: usize, pub line: usize, pub column: usize }
pub fn scan_json_integer_overflows(json: &str) -> Vec<UnderflowCandidate>;
pub fn scan_underflow_candidates(json: &str) -> Vec<UnderflowCandidate>;
pub fn parse_json(bytes: &[u8]) -> Result<Value, WriteError>;   // 在 lib.rs
```

`parse_json` 三步：
1. `serde_json::from_slice` 解析；
2. `scan_json_integer_overflows(text)`：整数字面量 `parse::<i64>().is_err() && parse::<u64>().is_err()` → 取第一个报 `NumRange`。即**接受 `[-2^63, 2^64-1]`**，超出即 `numeric_range`；
3. `scan_underflow_candidates(text)`：候选 = 语法上是数字、十进制真值非零、`|真值| < 2^-1000`（≈`9.33e-302`）。对每个候选用 `serde_json::from_str::<f64>(literal) == Some(0.)` **确认**，确认才报 `NumUnderflow`。

预筛实现要点：
- 字面量拆成 `(sig: Vec<u8>, exp: i64)`，`sig` 为大端十进制位、无前导零/无末尾零、**空表示真值 0**；
- 指数用 `i64` + **saturating** 累加，`i64::MIN` 取负特判为 `i64::MAX`；
- 尾随垃圾（`1e-400e`、`1.`、`1e`、`1e+`、空串、纯符号）→ 返回 `None`，交给解析器报语法错（扫描器不越权）；
- 前导零整数 `01` → `None`（JSON 语法不允许，交给解析器拒绝）；
- 阈值常量 `2^-1000 = 5^1000/10^1000`，运行时 `pow5` 生成十进制位串，`OnceLock` 缓存；
- **合法次正规数（`5e-324`、`1e-323`）也是候选**，但确认环节会放行（解析结果非零）；
- **字符串内容和键名一律跳过**（`{"note":"1e-400"}` 不报错）。

`line` 从原文现算；`column` 按**字符**计（`json_num` 的 column），而 `NumUnderflow` 的文档注释说按字节 —— 复刻时以代码为准：`scan_number_tokens` 产出的 `column` 是字符计。

---

## 5. 插件 SDK `nctool-plugin-sdk`

源码：[sdk/src/lib.rs](../sdk/src/lib.rs)、[sdk/python/nctool_plugin.py](../sdk/python/nctool_plugin.py)。

```rust
pub const PROTOCOL_VERSION: u32 = 1;
pub type PluginResult<T> = Result<T, PluginError>;

pub struct PluginError { pub code: String, pub message: String, pub diagnostics: Vec<Diagnostic> }  // deny_unknown_fields
pub struct Diagnostic { pub level: String, pub code: String, pub message: String, pub path: Option<String> }  // path skip_if none
pub struct Artifact { pub name: String, pub media_type: String, pub text: String }
pub struct ActionResult { pub data: Value, pub artifacts: Vec<Artifact>, pub diagnostics: Vec<Diagnostic> }
pub struct Service { pub id: String, pub version: u32, pub action: String }              // Service::new 默认 version=1
pub struct Requirement { pub id: String, pub version: u32 }
pub struct Panel { pub id: String, pub title: String, pub actions: Vec<String>, pub view: String }  // view 默认 "form"
pub struct PluginDescriptor {          // deny_unknown_fields
    pub id: String, pub version: String, pub protocol_version: u32,
    pub requires: Vec<Requirement>,           // default []
    pub provides: Vec<Service>,               // default []
    pub config_schema: Value,                 // default {"type":"object"}
    pub panels: Vec<Panel>,                   // default []
    pub asset_collections: Vec<AssetCollection>,  // default []，空则 skip_serializing
}
pub struct ActionDescriptor {          // deny_unknown_fields
    pub id: String, pub title: String,
    pub input_schema: Value, pub output_schema: Value,
    pub example: Value,               // default {}
    pub ui_schema: Option<Value>,     // None 则 skip_serializing
}
pub struct AssetCollection { pub id: String, pub export_action: String, pub import_action: String }

pub struct RenderExtension { pub id: String, pub configure: RendererCallback }
pub struct Cancellation(Arc<AtomicBool>);       // Clone + Default
pub trait Host { fn call(...); fn extend(...); }
pub struct Context { pub owner: String, pub config: Value, pub cancellation: Cancellation,
                     pub stack: Vec<String>, pub host: Arc<dyn Host> }
pub trait Plugin: Send + Sync {
    fn descriptor(&self) -> PluginDescriptor;
    fn actions(&self) -> Vec<ActionDescriptor> { vec![] }
    fn extensions(&self) -> Vec<RenderExtension> { vec![] }
    fn listeners(&self) -> Vec<EventListener> { vec![] }
    fn activate(&self, _ctx: &Context) -> PluginResult<()> { Ok(()) }
    fn invoke(&self, action: &str, input: Value, ctx: &Context) -> PluginResult<ActionResult>;
    fn shutdown(&self) {}
}
pub struct TemplateSnapshot {
    pub format_version: u32,      // 缺省 = 1
    pub template: String,
    pub templates: BTreeMap<String,String>,
    pub asset: Option<Value>,     // default None
    pub metadata: Option<BTreeMap<String,Value>>,  // v2 才有；None 则 skip_serializing
}
pub enum EventKind { ActionStarted, ActionFinished }
pub struct Event { pub kind: EventKind, pub action: String, pub owner: String, pub succeeded: Option<bool> }
pub struct EventListener { pub kind: EventKind, pub callback: ... }
pub fn object_schema() -> Value;                       // {"type":"object"}
pub fn parse_json(bytes: &[u8]) -> PluginResult<Value>;
pub fn compile_schema(schema: &Value) -> PluginResult<jsonschema::Validator>;
pub fn validate_schema(schema: &Value, value: &Value, code: &str) -> PluginResult<()>;
```

`PluginError::new` 只填 code/message；`From<io::Error>` → `"io"`；`From<serde_json::Error>` → `"invalid_json"`；`From<TplError>` → `"template_error"`。

`parse_json` 错误映射：`NumUnderflow` → `"numeric_underflow"`，`NumRange` → `"numeric_range"`，其余 → `"invalid_json"`。

`compile_schema` 先 `reject_remote_refs`：任何 `$ref` 值不是 `#` 开头 → `invalid_schema`（禁止远程引用，不发起网络请求）。

`validate_schema` 失败时：`PluginError::new(code, e.to_string())` 并 push 一条 `Diagnostic{level:"error", code, message, path: Some(e.instance_path)}`。

### ID 合法性

```rust
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.as_bytes()[0].is_ascii_alphanumeric() && id.len() <= 128
    && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    && id != "." && id != ".."
}
```

---

## 6. 运行时 `nctool-runtime`

源码：[runtime/src/lib.rs](../runtime/src/lib.rs)、[config.rs](../runtime/src/config.rs)、[external.rs](../runtime/src/external.rs)。

### 6.1 启动流程（`Runtime::start`）

```
validate_descriptor(descriptor, actions)         # 每个插件
插入 available，重名 → duplicate_plugin
definitions = 所有 descriptor
plan = resolve_composition(&definitions, &providers)
for id in plan.order:
    cfg = configs.get(id).unwrap_or({})
    validate_schema(descriptor.config_schema, cfg, "invalid_config")
    context = runtime.context(id, Cancellation::default(), vec![])
    state.descriptors.insert(id, descriptor)      # 先发布，激活期依赖调用才可用
    plugin.activate(&context)                      # 失败 → plugin.shutdown(); runtime.shutdown(); return Err
    注册（同一把 state 锁内）:
      - actions: 重名 → duplicate_action；compile input/output schema
      - extensions: 重名 → duplicate_extension；用 probe Renderer 试注册（含重复内置过滤器即失败）
      - services: 仅当 selected[service.id] == id 才登记
      - listeners, active
```

**失败即整体失败**，已启动插件通过 `runtime.shutdown()` 释放。

### 6.2 调用链

`invoke(action, input)` → `invoke_inner(action, input, Cancellation::default(), &[])`。

`invoke_inner`：
1. `cancellation.check()`；
2. `stack.len() >= 32 || stack.contains(action)` → `call_cycle`（嵌套深度上限 32，重复动作链直接失败）；
3. 查 actions，未注册 → `action_not_found`；
4. `input` 过 `input_schema`，失败 → `invalid_input`；
5. emit `ActionStarted{action, owner, succeeded:None}`；
6. `plugin.invoke(action, input, &context(owner, cancellation, stack+[action]))`；
7. `cancellation.check()`；
8. `result.data` 过 `output_schema`，失败 → `invalid_output`；
9. emit `ActionFinished{..., succeeded: Some(ok)}`。

事件**不得**改正文、不得吞错误。

`invoke_cancellable_request(id, action, input, cancellation)`：把 token 登记进 `requests`（`duplicate_request` 若 id 已存在），调用后无论成败都移除。

`cancel(id)` → 找到 token 则 `cancel()` 并返回 true。

`shutdown()`：`closing.swap(true)`；取消所有在途 token；**逆序** `active.pop()` 逐个 `plugin.shutdown()` 并摘除它的 actions/services/extensions/listeners/descriptors；最后清空全部 state（含激活失败插件的部分注册）。

### 6.3 服务解析（`resolve_composition`，纯函数，启动与配置预检共用）

```
owners: service_id -> [(plugin_id, version)]
对每个 service：
  providers.get(service):
    Some(id) 且 id 是候选 → 选它
    Some(id) 但非候选     → provider_missing
    None 且唯一候选       → 选它
    None 且多候选         → duplicate_service（"请为 {service} 选择服务提供方"）
providers 里的 key 必须都是已知 service，否则 provider_missing
对每个 descriptor.requires:
  owner = selected[req.id]，缺失 → service_missing
  version 不匹配 → service_version
  deps.insert(owner)
拓扑排序（Kahn，BTreeMap 保证确定性）：无就绪节点 → dependency_cycle
返回 CompositionPlan { order, providers }
```

**多提供方必须显式选择，不默认选最后一个。**

### 6.4 描述校验（`validate_descriptor`）

依次检查并返回对应错误码：
- `valid_id(id)` 失败 → `invalid_plugin_id`
- `is_retired_plugin_id(id)` → `retired_plugin`
- `semver::Version::parse(version)` 失败 → `plugin_version`
- `protocol_version != 1` → `protocol_version`
- `compile_schema(config_schema)` 失败 → `invalid_schema`
- 每个 action：`valid_id` 或 id 重复 → `invalid_action_id`；`is_retired_namespace_id` → `retired_identity`；两个 schema 均可编译
- 每个 service：`valid_id`、`version != 0`、`action` 必须在本插件 actions 内、service id 不重复 → `invalid_service`；`is_retired_namespace_id` → `retired_identity`
- 每个 requirement：`valid_id`、`version != 0` → `invalid_requirement`
- 每个 collection：`valid_id`、export/import action 都在本插件、id 不重复 → `invalid_collection`；`is_retired_collection_id` → `retired_identity`
- 每个 panel：`valid_id`、所有 action id 都存在 → `invalid_panel`

### 6.5 配置（`config.rs`）

```rust
pub const BUILTIN_PLUGIN_IDS: &[&str] = &["template","parameters","math"];
pub const RETIRED_PLUGIN_IDS: &[&str] = &["nc","nc-parameters","nc-managed","sync-turning","undercut","process","process-parameters"];
const RETIRED_DOMAIN_TOKENS: &[&str] = &["nc","machine","process","sync-turning","undercut"];
const RETIRED_COLLECTION_IDS: &[&str] = &["nc","nc-parameters","sync-turning-assets","undercut-assets"];

pub struct Config {              // serde default + deny_unknown_fields
    pub profile: String,          // default "template"
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
    pub providers: BTreeMap<String,String>,
    pub plugins: BTreeMap<String,Value>,
}
```

`is_retired_namespace_id(id)` = 存在 token 使 `id.strip_prefix(token)` 且剩余部分以 `.` 开头（即 `nc.`、`machine.`、`process.`、`sync-turning.`、`undercut.` 前缀）。

`load(home)`：读 `home/config.json`，不存在 → `(Config::default(), None)`；存在则读指纹、解析、`validate`。

`validate`：`profile != "template"` → `invalid_profile`；`enabled/disabled/plugins.keys()/providers.values()` 全部过 `ensure_plugin_allowed`；`providers` 的 key 不得是退役命名空间。

`save`：`validate` → `create_dir_all(home)` → `write_guarded(config.json, to_vec_pretty, expected)`。

**安装**（`install`）：source canonicalize → `ExternalManifest::read` → `ensure_plugin_allowed` → 内置 ID 拒绝（`reserved_plugin_id`）→ `plugins/<id>` 已存在则 `conflict` → 暂存目录 `.stage-<pid>-<nanos>` → `copy_tree` → 再次 `read` 校验 → `rename` 发布。`copy_tree` 拒绝符号链接（`plugin_symlink`）、非普通文件（`plugin_file_type`）、超 2048 文件或 64 MiB（`package_limit`）。source 包含 plugins 目录 → `recursive_install`。

**扫描**（`scan_installed`）：读不完的插件目录记为 `InstalledError` 记录而不致命；目录名与 descriptor.id 不一致也记录不致命；权限错误不掩盖。已启用但加载失败 → **fail-closed**（启动失败）。

`write_output(path, bytes)`：见 7.2。

### 6.6 外部进程协议（`external.rs`）

```rust
pub struct Limits { pub timeout_ms: u64, pub max_message_bytes: usize, pub max_concurrency: usize }
impl Default for Limits { timeout_ms: 30_000, max_message_bytes: 1 MiB, max_concurrency: 8 }

pub struct ExternalManifest { pub descriptor: PluginDescriptor, pub command: Vec<String>,
                              pub actions: Vec<ActionDescriptor>, pub limits: Limits }
impl ExternalManifest {
    pub fn read(root: &Path) -> PluginResult<Self>;   // plugin.json 必须是普通文件且 < 1 MiB
    pub fn validate(&self) -> PluginResult<()>;
}
```

`validate`：走 `validate_descriptor`；`command` 非空且无 NUL（`invalid_command`）；limits 范围：`timeout_ms ∈ [10, 3_600_000]`、`max_message_bytes ∈ [1 KiB, 16 MiB]`、`max_concurrency ∈ [1, 64]`（`invalid_limits`）。

**传输层**：
- 每条消息 = 一行 UTF-8 JSON + `\n`；`stdout` 只承载协议，日志走 `stderr`。
- 独立写线程（`sync_channel(capacity = max_concurrency * 2)`）和读线程（`BufReader` + `read_frame`）。
- 读线程按 id 路由：`method` 字段存在 → 必须是 `host.call`（否则 `rpc_protocol`）；无 `method` → 按字符串 id 找 pending，`result`/`error` 必须**恰好一个**（`rpc_protocol`），未知 id → `rpc_protocol`。
- `send`：`failed` 时 `plugin_exited`；超 `max_message_bytes` → `message_limit`；`try_send` 失败 → `plugin_busy`。
- `request(method, params, ctx)`：id = `host-{n}`；`pending.len() >= max_concurrency` → `plugin_busy`；循环里每 20ms 检查取消与超时；取消/超时先发 `cancel` 通知，再把该 pending 标记为 abandoned 并只返回本请求的错误，**不** `fail()`。abandoned 条目继续计入并发上限，直到插件迟到响应到达；连续两轮并发槽位全部为 abandoned 时才 `fail()` 重启进程。
- `fail(error)`：`swap(true)` 一次性把所有 pending 以该错误唤醒并 `child.kill()` + `wait()`。仅用于协议错误、进程退出与上述两轮放弃；收到 abandoned id 的迟到响应会丢弃，abandoned 父请求的 `host.call` 在本地回 `cancelled`。
- `host.call`：`parent_id` 必须对应活动宿主请求；`callbacks` 计数超 `max_concurrency` → 回一条 `-32000` 错误而不是失败整个进程；回调在**新线程**里执行 `context.call(service, version, input)`，所以等待宿主回调时不堵读线程。

**握手**（`ExternalPlugin::start`）：
1. `initialize` params `{protocol_version:1, plugin_id, config: ctx.config}`；返回必须 `protocol_version == 1` 且 `plugin_id` 匹配，否则 `protocol_version` 并 `fail()`；
2. `describe` params `{}`；返回解析后的 JSON 值必须**结构相等于** `{"descriptor":manifest.descriptor,"actions":manifest.actions}`，否则 `capability_mismatch` 并 `fail()`；
3. `invoke` params `{action, input}`，返回 `ActionResult`。
- `command[0]` 以 `./` 或 `.\` 开头 → 相对插件根解析；`current_dir` = 插件根；`stderr` 继承。
- 失效进程下一次调用重新握手，**不自动重试**失败动作。
- `shutdown` 发 `{"jsonrpc":"2.0","method":"shutdown","params":{}}` 然后 `fail("closed")`。

### 6.7 Python 适配器契约（复刻要点）

- `sys.stdin`/`sys.stdout` 重配为 UTF-8。
- `send`：`json.dumps(..., ensure_ascii=False, allow_nan=False)` + `print(flush=True)`，`write_lock` 保护。
- 主循环逐行读：`method == "shutdown"` → break；`method == "cancel"` → 记入 cancelled；有 `method` → 提交线程池；否则按 `id` 找 pending future 并 set_result/set_exception。
- `ThreadPoolExecutor(max_workers=8)`。
- `initialize` 校验 `protocol_version == 1`；`describe` 原样回 manifest；`invoke` 调用户的 `invoke(action, input, server, request_id)`。
- 错误回包固定形状：`{"jsonrpc":"2.0","id":..,"error":{"code":-32000,"message":str(e),"data":{"code":"plugin_error","message":..,"diagnostics":[]}}}`。
- `call(parent_id, service, input_data, version=1)`：id = `plugin-{n}`，`future.result(timeout=30)`。
- 仓库要求 `sdk/python/nctool_plugin.py` 与 `examples/plugins/python-report/nctool_plugin.py` **字节相同**（`check_v2.py` 断言）。

---

## 7. CLI 与 HTTP（`cli/`）

源码：[main.rs](../cli/src/main.rs)、[composition.rs](../cli/src/composition.rs)、[server.rs](../cli/src/server.rs)、[workbench.rs](../cli/src/workbench.rs)。

### 7.1 命令面

```
nctool [--home <dir>] [--workspace <dir>] [--profile <str>] <subcommand>
```

全局参数：`--home`（可选）、`--workspace`（默认 `.`）、`--profile`（可选），全部 `global = true`。

子命令：

| 子命令 | 参数 | 说明 |
| --- | --- | --- |
| `ui` | `--host <IpAddr>` 默认 `127.0.0.1`、`--port <u16>` 默认 `8788` | 启动 WebUI |
| `actions` | 无 | 打印动作数组 JSON |
| `run <action>` | `--input <PathBuf>`（`-` 读 stdin）、`--out <PathBuf>`、`--out-dir <PathBuf>`（与 `--out` 互斥） | 执行动作 |
| `config check` | 无 | 只读迁移预检 |
| `config migrate` | `--expected <String>`（指纹 JSON） | 应用迁移 |
| `plugins list` / `install <source>` / `enable <id>` / `disable <id>` / `uninstall <id>` | | 插件管理 |

`config` 子命令**必须显式 `--home`**，否则 `explicit_home_required`。

**home 解析顺序**（`default_home`）：`NCTOOL_HOME` → Windows `%LOCALAPPDATA%/nctool` → `$XDG_CONFIG_HOME/nctool` → `$HOME/.config/nctool` → 都不存在时 `./.config/nctool`。

**退出码**：成功 `ExitCode::SUCCESS`，失败 `ExitCode::FAILURE`，stderr 打印 `{"ok":false,"error":{...}}`（只有 0/1 两种，没有细分退出码）。

**`run` 细节**：输入上限 **1 MiB**（`take(1024*1024+1)` 后判断，超了 `input_limit`）；`--out` 要求 `artifacts.len() <= 1` 且 `data.text` 是字符串，走 `config::write_output` 原子写（**动作完整成功后才打开输出文件**）；`--out-dir` 走 `deliver_artifacts`；最后 stdout 打印完整 `ActionResult` 的 pretty JSON。

**`deliver_artifacts`**：目录已存在 → `conflict`；产物名不得为空/`.`/`..`/含 `/`、`\`、`:`/含控制字符/重复（`invalid_artifact`）；父目录下建 `.nctool-output-<pid>-<nanos>` 暂存目录；每个文件 `create_new(true)` 独占创建（`AlreadyExists` → `invalid_artifact`，用于让文件系统识别 `A.txt`/`a.txt` 别名）；`write_all` + `sync_all`；再次确认目标目录未出现；`rename(staging, directory)`；失败清暂存。

### 7.2 组合根（`composition.rs`）

```rust
pub struct App { pub runtime: Arc<Runtime>, pub home: PathBuf, pub profile: String,
                 pub boot_config: Config, pub workbench: Arc<Workbench>, next_request: AtomicU64 }
```

`App::boot(home, workspace, profile)`：
1. `config::load(home)`，`boot_config = config.clone()`；
2. `--profile` 覆盖 `config.profile`；
3. `config::validate`；
4. `selected_ids(config)`；
5. 按 id 构造内置插件：`template` → `TemplatePlugin::new(workspace)`、`parameters` → `ParametersPlugin::new(workspace)`；`math` 在 `#[cfg(feature="math")]` 下构造，否则若被选中返回 `feature_unavailable`；
6. `config::external_plugins(home, &config)`；
7. `Runtime::start(plugins, config.plugins, config.providers)`；
8. `Workbench::new(runtime, workspace)`。

`selected_ids`：基础集合 = `{"template","parameters"}` ∪ `config.enabled`，再移除 `config.disabled`；若 `template` 不在且 `parameters` 不是显式启用，则移除 `parameters`（二者同进同退的兜底）。

`definitions_at` 顺序固定：`parameters` → `template` → (`math`) → 外部已安装。

`validate_config_at`：`config::validate` → 每个启用的 id 必须有对应 definition（`plugin_missing`）→ `validate_descriptor` → `validate_schema(config_schema, config.plugins[id] ?? {}, "invalid_config")` → action id 不得重复（`duplicate_action`）→ `resolve_composition` → 返回 `{"valid":true,"plan":..,"plugins":..,"applies":"next_start"}`。

`inventory()`：内置三项（`available = ["template","parameters"].contains(id) || cfg!(feature="math")`）+ 退役 ID（`retired:true, available:false, descriptor:null`）+ 外部插件；`restart_required = saved != boot_config`。

**配置迁移**（`migration_at`）：读 `config.json` 指纹；读两次指纹不一致 → `conflict`；解析旧配置（只读）；`profile == "template"` 不动；`profile == "nc"` → 改为 `template` 并记录 change；**其它 profile → `invalid_profile`**（不静默改写）；从 `enabled`/`disabled` 移除退役插件 ID；`providers` 里 provider 是退役 ID 或 service key 是退役命名空间的，整项移除；`plugins` 移除退役 ID；`validate(proposed)`。
`migration_apply_at`：`expected != fingerprint` → `conflict`；`changes` 为空 → `{"migrated":false,...}`；否则写备份 `config.backup-<pid>-<nanos>.json`（`write_guarded(..., None)` 新建语义），再 `config::save(home, proposed, expected)`；save 失败则删除备份。

`export_bundle`：遍历 `runtime.descriptors()` 的 `asset_collections`，调用 `export_action` 得到 data，指纹 = `FileFingerprint::of_bytes(to_vec(data), UNIX_EPOCH).as_string()`；集合 id 重复 → `duplicate_collection`；`{"format":"nctool-workspace","version":1,"collections":{...},"manifest":[{"collection","provider","version","fingerprint"}]}`；超 64 MiB → `bundle_limit`。

`import_bundle`：`format`/`version` 校验（`invalid_bundle`）；`collections` 缺失 → `invalid_bundle`；**任一集合 id 是退役集合 → 整体拒绝 `collection_missing`**（在任何提供方被咨询、任何文件被写之前）；构建可用集合表（重复 → `duplicate_collection`）；每个集合在 `manifest` 里找条目（缺失 → `invalid_bundle`），重算指纹比对（不符 → `bundle_fingerprint`）；调用 `import_action` 带 `{collection, validate_only:true}`；`validate_only` → `{"valid":true,"reports":[...]}`；否则逐个 `validate_only:false`，失败时按 `applied` **逆序**调用 import_action 传 `{"rollback":receipt}`，回滚失败则在 error.diagnostics 追加 `rollback_failed`。

`source_path`：Unix 上把 `C:\...` 形态转换成 `/mnt/c/...` 并检查存在（WSL 场景）。

### 7.3 HTTP API（完整清单）

`server.rs` 只做安全检查，然后全部转发给 `App::dispatch(method, path, body)`。

**安全层**（顺序即拒绝顺序）：
1. `Host` 必须等于 `state.address.to_string()` 或 `localhost:<port>`，否则 `invalid_host`；
2. `Sec-Fetch-Site: cross-site` → `cross_site`；
3. 有 `Origin` 头时必须等于 `http://{host}`，否则 `cross_site`；
4. `GET /` → 返回内嵌 `index.html`；
5. `GET` 其它 → 查内嵌 `FILES` 表（`/assets/...`、`/favicon.svg`），命中则带 mime 返回；
6. 非 `/api/v2/` 前缀 → `not_found`；
7. `POST` 的 `Content-Type` 第一段（`;` 前）必须是 `application/json`，否则 `content_type`；
8. 读 body：`/api/v2/bundle/` 前缀 64 MiB，其余 1 MiB，超时 **10 秒**；超限 → 413 `body_limit`，超时 → `body_timeout`；
9. `parse_json` 解析；
10. `/api/v2/cancel/` 前缀**直接 dispatch，不占并发槽**；
11. 其它请求 `Semaphore::try_acquire_owned`（容量 **8**），拿不到 → `plugin_busy`；
12. `spawn_blocking` 里 dispatch。

响应包装：成功 `{"ok":true,"data":...}`；错误 `{"ok":false,"error":{code,message,diagnostics}}`。

安全响应头（每个响应都加）：`x-content-type-options: nosniff`、`x-frame-options: DENY`、`cache-control: no-store`、`referrer-policy: no-referrer`、`content-security-policy: default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'`。

**错误码 → HTTP 状态映射**：

| 状态 | code |
| --- | --- |
| 404 | `not_found`、`action_not_found`、`not_found_asset` |
| 409 | `conflict`、`duplicate_request` |
| 503 | `plugin_busy` |
| 504 | `plugin_timeout` |
| 502 | `plugin_exited`、`rpc_protocol`、`invalid_output` |
| 408 | `cancelled` |
| 400 | 其余全部 |

`serve`：`!host.is_loopback()` → `bind_address`；打印 `NCtool {version} profile: http://{address}`；`with_graceful_shutdown` 监听 Ctrl-C，先 `runtime.shutdown()` 再退出，退出前再 shutdown 一次。

**路由表**（`App::dispatch`）：

| 方法 | 路径 | 处理 |
| --- | --- | --- |
| GET | `/api/v2/workspace` | `{"id":workbench.workspace_id,"profile":..}` |
| GET | `/api/v2/runs` | `workbench.list()` |
| POST | `/api/v2/runs/query` | `workbench.query(body)` |
| POST | `/api/v2/runs` | body `{action,input,title?}` → `workbench.start`，返回 `{"id"}` |
| GET | `/api/v2/runs/<id>/replay-check` | `replay_check` |
| GET | `/api/v2/runs/<id>` | `workbench.get` |
| POST | `/api/v2/runs/<id>/rerun` | `{"id":new_id}` |
| POST | `/api/v2/runs/<id>/cancel` | `{"cancelled":bool}` |
| POST | `/api/v2/runs/<id>/remove` | `{"removed":id}` |
| GET | `/api/v2/plugins` | `inventory()` |
| GET | `/api/v2/capabilities` | `{actions, document_actions, services, panels}` |
| GET | `/api/v2/config` | `{config, fingerprint, applies:"next_start"}` |
| POST | `/api/v2/config` | body `{config, expected}` |
| POST | `/api/v2/config/validate` | body `{config}` |
| POST | `/api/v2/plugins/inspect` | body `{source}` |
| POST | `/api/v2/plugins/install` | body `{source}` → `{installed,enabled:false,restart_required:true}` |
| POST | `/api/v2/plugins/uninstall` | body `{id}` |
| POST | `/api/v2/plugins/<id>/enable` / `disable` | `{restart_required:true}` |
| GET | `/api/v2/bundle` | 导出 |
| POST | `/api/v2/bundle/validate` | body `{bundle}` |
| POST | `/api/v2/bundle/import` | body `{bundle}` |
| POST | `/api/v2/actions/<action-id>` | body `{input, request_id?}` |
| POST | `/api/v2/cancel/<request-id>` | `{"cancelled":bool}` |

`capabilities.actions` 来自当前运行时的可执行动作；`document_actions` 则从 `definitions()` 中筛出所有含 `ui_schema.document_input` 的动作声明，包含已安装但未启用的提供方。前端用后一集合识别模板意图，并在提供方未激活时阻止执行，不能把它当作可执行动作列表。

`POST /api/v2/actions/<id>`：`request_id` 缺省用 `App::request_id()`（`request-{n}`）；`valid_id` 校验失败 → `invalid_request_id`。

`plugins/<id>/enable|disable`：先查 inventory，`available != true` → `plugin_missing`；然后 `config.enabled/disabled` 去重后 push 目标值 → `validate_config` → `config::save`。

`uninstall`：内置 ID → `builtin_plugin`；仍在 runtime 里 → `plugin_active`。

### 7.4 工作台（`workbench.rs`）

```rust
pub struct Run {
    pub id: String, pub action: String, pub input: Value, pub status: String,
    pub started_at: u64, pub finished_at: Option<u64>,
    pub result: Option<ActionResult>, pub error: Option<PluginError>,
    pub plugins: Vec<PluginDescriptor>, pub title: String,
    pub environment: Option<ExecutionEnvironment>,   // skip_if none
    pub replay: Option<ReplayOrigin>,                // skip_if none
}
pub struct ExecutionEnvironment { pub format_version: u32, pub host_version: String,
                                  pub plugins: BTreeMap<String,PluginVersion>, pub services: Value }
pub struct PluginVersion { pub version: String, pub protocol_version: u32 }
pub struct ReplayCheck { pub status: String, pub differences: Vec<String> }
pub struct ReplayOrigin { pub source_run_id: String, pub check: ReplayCheck }
```

- 磁盘：`<workspace>/.nctool/runs/<id>.json`（权威记录）+ `.nctool/runs/_index/<id>.json`（派生摘要）+ `<id>.nctool.lock`（运行锁）。
- 摘要字段：`{id,title,action,status,started_at,finished_at,diagnostic_count}`。
- 单记录上限 **64 MiB**（写前判断 + 读时 `take(64MiB+1)` 再判断）；单摘要上限 1 MiB。
- `workspace_id` = `FileFingerprint::of_bytes(canonical workspace path, UNIX_EPOCH).as_string().replace(':','-')`（Windows 盘符冒号替换成 `-`）。
- 记录 id 格式 `run-{ms}-{pid}-{counter}`。

**`Workbench::new` 启动恢复**：遍历摘要，跳过非 `queued`/`running`；`valid_id` 校验（畸形条目直接跳过，不阻断启动）；`guard(id)` 后 `try_lock()`，拿不到（别的进程在跑）→ 跳过；`get(id)` 返回 `not_found` → `prune_summary` 并继续；其它读错误 → **返回错误（不掩盖）**；否则置 `status="interrupted"`、`finished_at=now()`、`result=None`、`error=PluginError("interrupted", ...)`，`save_run`（失败必须上抛），`save_summary` 失败**忽略**（索引可重建）。

**`start_with_replay`**：
1. 动作必须在 `capabilities()` 里，否则 `action_not_found`；
2. `input` 过 `input_schema`（`invalid_input`）；
3. `active.fetch_add(1) >= 8` → `plugin_busy`（最多 8 并发）并回退计数；
4. 建 id、`Run`（`status:"queued"`、`environment: Some(...)`、`replay`）；
5. `guard(id)` + `guard.lock()`（阻塞，拿不到 → `lock`）；
6. `save(&run)`；失败回退 active 计数；
7. 登记 `Cancellation`；
8. `std::thread::spawn`：置 `running` → `cancellation.check()` → **若 `action == "template.render"` 且无 `snapshot`**：注入 `include_snapshot=true` 调 `template.inspect`，取 `data.snapshot`，从 input 删掉 `source`/`template`/`templates`，写入 `snapshot`（冻结模板集合，`invalid_output` 若无快照）→ `cancellation.check()` → `invoke_cancellable_request(run.id, action, input, cancellation)`；
9. 结果分类：成功且未取消 → `succeeded`；成功但已取消 → `cancelled` + error；失败且 `error.code=="cancelled"` → `cancelled`，否则 `failed`；
10. `finished_at=now()`；`save` 失败则把 status 改 `failed` 再存一次；
11. 移除 controls、`active` 减一。`_guard`（锁文件）在线程结束时 Drop。

**`query`**（`POST /api/v2/runs/query`）：body 必须是对象；`query` 必须是字符串（小写化后匹配 title/id/action）；`status` 必须是 `queued|running|succeeded|failed|cancelled|interrupted` 之一；`offset` 非负整数（缺省 0）；`limit` 缺省 25、范围 1–100；排序 = `started_at` 降序 + `id` 升序；返回 `{runs,total,offset,limit}`；**不读完整记录，但仍扫描全部索引**，offset 分页在并发增删时无快照一致性。

**`list`**：`started_at` 降序，取前 100 条（兼容入口）。

**`replay_check` / `compare_environment`**：记录缺环境或 `format_version` 不匹配 → `unknown` + `["历史记录缺少可比较的执行环境信息"]`；否则逐项比对 `host_version`、plugins（版本/协议变化、当前未启用、本次新增启用）、`services`，有差异 → `changed`，无 → `matching`。

**`rerun`**：`compare_environment`；输入优先 `result.data.replay_input`，否则 `recorded.input`；`start_with_replay` 带 `ReplayOrigin{source_run_id, check}`。

**`cancel(id)`**：`controls` 里找到就 `token.cancel()` + `runtime.cancel(id)` → true，否则 false。

**`remove(id)`**：`controls` 里还有该 id（本进程在跑）→ `conflict`「运行中的任务不能删除」；`guard(id).try_lock()` 失败（别的进程在跑）→ `conflict`「任务正在其他进程中运行」；否则删 `<id>.json`，再删 `_index/<id>.json`（若存在）。

---

## 8. 模板插件 `nctool-plugin-template`

源码：[plugins/template/src/lib.rs](../plugins/template/src/lib.rs)。

```rust
pub struct TemplatePlugin { templates: JsonStore, presets: JsonStore, access: Mutex<()>, root: PathBuf }
impl TemplatePlugin {
    pub fn new(workspace: &Path) -> PluginResult<Self>;   // templates/, presets/
    pub fn definition() -> PluginDescriptor;
    pub fn action_definitions() -> Vec<ActionDescriptor>;
}
pub fn ensure_generic_template(asset: &Value) -> PluginResult<()>;
```

### 8.1 快照构建（`sources(input)`）

输入优先级：
1. 有 `snapshot` 字段 → 若同时存在 `source`/`template`/`templates` → `invalid_input`；反序列化为 `TemplateSnapshot` 并 `check_snapshot`；
2. 否则取 `GroupGuard`（组锁），扫全部已存模板：
   - 每个资产必须有 `source` 字符串，否则 `invalid_asset`；
   - `sources.len() >= 1024` 或累计字节 > **16 MiB** → `template_limit`；
   - `metadata` 收集：`explicit_root.is_none() && name == root_name` → 存 `Null`（**inline 哨兵不继承已存资产的元数据**），否则存 `asset["metadata"]`；
   - `explicit_root == Some(name)` → `root_asset = Some(asset)`；
3. `input.templates`（内存集合）覆盖同名；
4. `input.source` 覆盖根名（**只换源码，不清元数据/意图**）；
5. 其余名字补 `metadata: Null`；
6. 产出 `TemplateSnapshot{format_version:2, template: root_name, templates, asset, metadata}`。

`root_name` = `input.template` 或 `"__inline__"`。

### 8.2 `check_snapshot`

- `format_version == 1` 且 `metadata.is_none()` → 通过；
- `format_version == 2` 且 `metadata` 存在、`metadata.len() == templates.len()` 且 keys 完全一致 → 通过；
- 其余 → `snapshot_invalid`；
- 其它版本号 → `snapshot_version`；
- `templates` 不含 `template` → `template_not_found`；
- `templates.len() > 1024` 或总字节 > 16 MiB → `template_limit`。

### 8.3 `inspect`

栈式深度优先（`queue = vec![root]`，`while let Some(name) = queue.pop()`）遍历快照：`parse` → `extract_undeclared` → 汇总 `{name, optional, template, line, column}`（**同一变量已有 `optional==false` 记录则不被覆盖**）→ `queue.extend(extract_template_refs(&ast))`。返回 `{template, variables, references(访问过的名字集合，BTreeSet 序列化为有序数组), source}`；`include_snapshot:true` 时附加 `snapshot`。

### 8.4 `render`

1. `sources(input)`；
2. 若有 `asset` → `ensure_generic_template(asset)`（**当前发行版无领域渲染器，所以总是检查**）；
3. `Renderer::new()`；`lenient:true` → `with_lenient()`；`with_whitespace(trim_blocks, lstrip_blocks)`；
4. `extensions` 数组 → `ctx.extend(&mut renderer, &extensions)`（未注册 → `extension_missing`，重复 → `duplicate_extension`）；
5. 装 loader：名字在快照里才返回 `Some(source)`；否则对每个被加载的名字做意图检查 —— 根名跳过（已在上面查过）；`format_version == 1` → `snapshot_compatibility`（旧快照缺依赖元数据，禁止依赖重放）；否则用 `snapshot.metadata[name]` 过 `ensure_generic_template`。错误通过 `Arc<Mutex<Option<PluginError>>>` 共享槽穿过 minijinja 的错误包装；
6. `context = input.context ?? {}`；`defaults = input.defaults ?? asset.defaults`，按**顶层 `entry().or_insert()`** 合并（显式 context 覆盖同名默认值）；`context` 非对象 → `invalid_input`；
7. `schema = input.schema ?? asset.schema` → `validate_schema(schema, context, "validation")`；
8. `cancellation.check()` → `render_template(name, Value::from_serialize(context))` → 若有共享错误先返回它 → `cancellation.check()`；
9. 返回 `ActionResult::text("{name}.txt", "text/plain", text)`。

### 8.5 `ensure_generic_template`

```rust
let domain_intent = metadata.get("nc").is_some()
    || metadata["execution"]["action"].as_str().is_some_and(|action|
        ["nc.","machine.","process.","sync-turning.","undercut."].iter().any(|p| action.starts_with(p)));
// → PluginError::new("domain_action_required", "...")
```

### 8.6 动作分发与锁顺序

`template.render` **不持** `access` 锁（长且可取消），只在 `sources()` 内取组锁。
其余动作：`access` 锁先取，`template.inspect` 不取组锁（它自己在 `sources()` 里取），其它动作再取组锁。
**顺序固定为 `access → group`**，反序会与 inspect 死锁/伪冲突。

动作清单：`template.collection.export/import`、`template.inspect`、`template.catalog`、`template.render`，以及 `template`/`preset` 两组的 `list/read/save/remove/copy`。

- `template.remove`：先扫其它模板的 `extract_template_refs`，若有静态引用 → `asset_referenced`；
- `template.copy`：读源资产 → 可选改 `metadata.title` → `save(new_name, asset, None)`（新建语义，不覆盖）；
- `template.save`：`source` 必须存在且 `parse` 通过；`schema` 可编译；`tags` 必须全是字符串；然后 `store.save(name, asset, expected)`；
- `preset.save` 只做通用保存；
- `template.catalog` 返回 `{items:[{id,title,description,tags,fingerprint}]}`，读失败的资产进 `diagnostics`（`level:"error", code:"invalid_asset", path:name`）而不是整体失败；
- `asset_error` 映射：`Conflict|LockBusy → conflict`、`NotFound → not_found`、`PathEscape → path_escape`、其余 → `asset_error`。

`template.render` 的 `input_schema`（复刻要点）：

```json
{"type":"object","properties":{
  "source":{"type":"string","maxLength":1048576},
  "template":{"type":"string"},
  "include_snapshot":{"type":"boolean"},
  "snapshot":{"type":"object","required":["template","templates"],
    "properties":{"template":{"type":"string"},
      "templates":{"type":"object","additionalProperties":{"type":"string"}},
      "asset":{"type":["object","null"]},
      "format_version":{"type":"integer","enum":[1,2]},
      "metadata":{"type":"object"}},"additionalProperties":false},
  "context":{"type":"object"},
  "templates":{"type":"object","additionalProperties":{"type":"string"}},
  "lenient":{"type":"boolean"},
  "extensions":{"type":"array","items":{"type":"string"}},
  "schema":{"type":["object","boolean"]},
  "defaults":{"type":"object"},
  "trim_blocks":{"type":"boolean"},
  "lstrip_blocks":{"type":"boolean"}},
 "anyOf":[{"required":["source"]},{"required":["template"]},{"required":["snapshot"]}],
 "additionalProperties":false}
```

`template.render` 的 `ui_schema`：

```json
{"document_input":{"title":"普通文本","default":true,
  "description":"通用 Jinja 渲染，不注入领域上下文。",
  "bindings":{"source":"source","template":"id","context":"context",
              "schema":"schema","defaults":"defaults","extensions":"extensions"}}}
```

descriptor：`asset_collections = [{id:"template", export:"template.collection.export", import:"template.collection.import"}]`；`provides = [template.render, template.inspect, template.read]`（v1）；panels = `templates`（view `template-editor`，动作 render/inspect/list/read/save）+ `presets`（view `form`，动作 list/read/save）。

---

## 9. 共享参数插件 `nctool-plugin-parameters`

源码：[plugins/parameters/src/lib.rs](../plugins/parameters/src/lib.rs)。

### 9.1 资产

- 目录：`parameters/definitions/<id>.json`、`parameters/presets/<id>.json`。
- 定义：`{"format_version":1,"id":"part-depth","title":"加工深度","schema":{...},"unit":"mm","group":"零件","description":"...","default":<可选>}`
- 预置：`{"format_version":1,"id":"rough","title":"...","values":{"part-depth":5}}`
- 绑定（在模板 `metadata.parameters.bindings`）：`{"/part/depth":{"id":"part-depth","unit":"mm"}}`（`id` 也可直接写成字符串）

`validate_parameter`：`format_version == 1` 且 `id == 文件名`；definitions 组 `compile_schema(schema)` 且有 `default` 时用该 schema 校验 default；presets 组 `values` 必须是对象。

### 9.2 JSON Pointer 绑定规则（`tokens`）

- 必须以 `/` 开头且长度 ≤ 1024，否则 `invalid_binding`；
- `~1` → `/`、`~0` → `~`（RFC 6901）；
- token 为空、或为 `__proto__`/`constructor`/`prototype` → `invalid_binding`；
- **路径重叠拒绝**：任一分组 `{pointer}/` 是另一个的前缀 → `invalid_binding`。

`set_pointer(value, pointer, field)`：数组段必须能 `parse::<usize>()`（`invalid_binding`）且下标不越界；非容器段穿过 → `invalid_binding`；中间段自动建对象。

### 9.3 解析优先级（`resolve`，复刻核心）

输入：`{bindings, fallback_defaults, defaults, values, schema, preset_id, snapshot, defer_validation}`。

顺序（后者覆盖前者）：

1. **definition**：每个绑定找到定义；定义有 `default` → `set_pointer(values, pointer, default)`，`origins[pointer]="definition"`；
2. **fallback_defaults**：`merge(values, layer)`，命中 pointer 的 `origins="spec_default"`；
3. **defaults**（模板默认）：同样 merge，命中则 `origins="template_default"`；
4. **preset**：`preset_id` 是字符串时取预置，对每个 `definitions[pointer]["id"]` 在 `preset["values"]` 里有值的 → `set_pointer`，`origins="preset:{id}"`；
5. **values**（人工输入）：`merge(values, supplied)`，命中 pointer 的 `origins="manual"`。

然后：
- 每个有值的 pointer 用**定义的 schema** 校验，失败时 message 前加 `"{pointer}: "`，code `parameter_validation`；
- `defer_validation != true` 且有 `schema` → 用请求 schema 校验整个 `values`。

`merge` 语义：source 是对象而 target 不是 → target 变 `{}`；两侧都是对象则**递归**合并（对象逐键，非对象整体替换）。

返回 `{"values","origins","definitions","snapshot"}`。

单位与类型约束（每个绑定）：
- `binding.unit` 存在且 `definition.unit` 不同 → `unit_conflict`；
- 请求 schema 在该 pointer 处的 `x-unit` 或 `unit` 与定义 unit 不同 → `unit_conflict`；
- 类型交集为空 → `type_conflict`（`number` 与 `integer` 视为相容）。

### 9.4 `effective_bindings`

模板资产绑定 + 请求绑定合并；**若同一 path 上请求绑定与已存绑定不同 → `binding_conflict`**（不能替换模板的共享定义绑定）。

### 9.5 动作

| 动作 | 输入 | 输出 |
| --- | --- | --- |
| `parameter.snapshot` | `{}` | `capture()`：`{"version":1,"records":[{group,name,asset,fingerprint}]}` |
| `parameter.dependencies.check` | `{snapshot, template_snapshot?}` | `{current: bool, changes: [..]}` |
| `parameter.resolve` | 见 9.3 | `{values,origins,definitions,snapshot}` |
| `template.render.managed` | 见 9.6 | 渲染结果 + `parameters` + `replay_input` |
| `parameter.catalog/read/save/remove` | `{group,name,asset,expected}` | 通用 |
| `parameter.collection.export/import` | | 通用 |

`changed(snapshot)`：组锁内逐条比对 `asset == row["asset"]`；不匹配或 `not_found` → `changes.push("{group}/{id}")`。

`dependencies.check` 额外逻辑（有 `template_snapshot` 时）：
- 用 `template.inspect` 带 `{template:name, source:"", include_snapshot:true}` 取**当前**磁盘快照；
- 用 `template.inspect` 带 `{snapshot: frozen}` 取**冻结**快照的 `references`（可达集合）；
- 对每个可达名（除根名）：`current.templates[key] != frozen.templates[key]` → `changes.push("templates/{key}")`；
- `current.asset != frozen.asset` → `changes.push("template_metadata")`；
- 冻结集合没引用的新资产**不报**（不是依赖）。

`parameter.resolve` 与 `template.render.managed` 都有：`snapshot`/`parameter_snapshot` 与**字符串** `preset_id` 互斥 → `invalid_input`。

### 9.6 `template.render.managed` 流程

1. 组装 `inspect = {include_snapshot:true, source|template|templates|snapshot}`；
2. `ctx.call("template.inspect", 1, inspect)` → `snapshot`；
3. `parameter_snapshot` 缺省则 `library.capture()`；
4. `request.bindings = effective_bindings(asset.metadata.parameters.bindings, input.bindings)`；
5. `request.values = input.context ?? {}`；
6. `request.defaults = input.defaults ?? asset.defaults ?? {}`；
7. `schema`/`preset_id`：取 `input.<key>`，缺省回落 `asset[key]`（schema）或 `asset.metadata.parameters[key]`（preset_id）。**`or_else` 只在字段缺省时跑，所以显式 `preset_id: null` 能覆盖资产上的保存选择**；
8. 若有 `parameter_request`（冻结形式）：必须同时有 `snapshot` 和 `parameter_snapshot`，且**不得**出现 `context/schema/defaults/bindings/preset_id`，否则 `invalid_input`；用 `parameter.resolve` 的 input_schema 校验它；然后 `request = parameter_request`，仅 `bindings` 重新过 `effective_bindings`；
9. `resolved = resolve(request, parameters)`；
10. 若资产有 `schema`，再用 `{bindings, values: resolved.values, schema: asset.schema}` 跑一次 `resolve`（请求 schema 不能隐藏资产里的类型/单位标注）；
11. `rendered_input = {snapshot, context: resolved.values, schema: true, defaults: {}, extensions: input.extensions ?? []}`，加 `trim_blocks`/`lstrip_blocks`（若有）；
12. `ctx.call("template.render", 1, rendered_input)`；
13. 从 `rendered_input` 删掉 `context`/`schema`/`defaults`，加 `parameter_snapshot` 和 `parameter_request` → 作为 `result.data["replay_input"]`；
14. `result.data["parameters"] = resolved`。

`ui_schema`：

```json
{"document_input":{"title":"共享参数模板","match_metadata":"parameters",
  "bindings":{"source":"source","template":"id","context":"context","schema":"schema",
              "defaults":"defaults","bindings":"metadata.parameters.bindings",
              "preset_id":"parameter_preset_id","extensions":"extensions"}}}
```

descriptor：`requires = [template.inspect@1, template.render@1]`；`provides = [parameter.snapshot, parameter.resolve, parameter.dependencies.check, template.render.managed]`；`asset_collections = [{id:"parameters", export/import: parameter.collection.*}]`；panel `parameters` view `parameter.library`。

`Library` 的 `invoke(verb, input, validate)` 也持组锁；verb ∈ `export/import/catalog/read/save/remove`。

---

## 10. 数学插件 `nctool-plugin-math`

源码：[plugins/math/src/lib.rs](../plugins/math/src/lib.rs)。

`configure(renderer)` 注册 **19** 个过滤器，每个都过 `checked_math`（非有限 → `EngineError{InvalidOperation, "{name}: non-finite result (NaN/Inf)"}`）：

| 弧度制 | 角度制（`_d` 后缀） | 其它 |
| --- | --- | --- |
| `sin` `cos` `tan` | `sin_d` `cos_d` `tan_d` | `sqrt` `exp` `ln` `log10` |
| `asin` `acos` `atan` | `asin_d` `acos_d` `atan_d` | `pow(v,e)` `floor` `ceil` |

角度制：`sin_d = v.to_radians().sin()`；反三角以度输出（`asin_d(0.5) → 30`）。

`MathPlugin`：`provides = [Service::new("math.ready","math.info")]`；`extensions() = [RenderExtension::new("math", configure)]`；`invoke` 恒返回 `{"extension":"math"}`；动作 `math.info`（`object_schema`）。

**三重显式门槛**（缺一不可）：
1. 编译期 `--features math`（否则启用时 `feature_unavailable`）；
2. 配置 `enabled` 含 `math`；
3. 单次请求 `extensions: ["math"]`。

---

## 11. 前端 WebUI

源码：`ui/src/**`（当前工作树：生产 TS/TSX/CSS **46 个文件、11,760 行**；测试 28 个文件）。

### 11.1 技术栈与构建

- React 19.3.0 + TypeScript 5.9.3 + Vite 7.3.6 + Vitest 5.0.3；Node 22.12+。
- 直接依赖 14 个：CodeMirror 系列（`codemirror`、`@codemirror/{commands,lang-json,language,legacy-modes,state,view}`）、`@radix-ui/react-dialog`、`@radix-ui/react-dropdown-menu`、`diff`、`lossless-json`、`lucide-react`、`react`、`react-dom`。
- 脚本：`dev` = `vite --host 127.0.0.1`；`build` = `tsc --noEmit && vite build`；`lint` = `tsc --noEmit --noUnusedLocals --noUnusedParameters`；`test` = `vitest run`。
- `vite.config.ts`：`base:"/"`，`outDir:"dist"`，`manualChunks` 把 CodeMirror 相关打 `editor` chunk、react 相关打 `react` chunk；dev server 把 `/api` 代理到 `127.0.0.1:8788` 并在 `proxyReq` 里强制 `Origin: http://127.0.0.1:8788`。

**嵌入流程**（`scripts/build_ui.mjs`）：
1. 非 `--check` 时先跑 `npm run build`（Windows 用 `npm.cmd` + `shell:true`）；
2. 遍历 `ui/dist` 全部文件；
3. 生成 `cli/ui/assets.rs`：`pub const FILES: &[(&str,&str,&[u8])]`，每项 `("/"+相对路径, mime, include_bytes!(路径))`，按路径 `localeCompare` 排序，排除 `index.html`；
4. mime：`.js` → `text/javascript; charset=utf-8`，`.css` → `text/css; charset=utf-8`，`.svg` → `image/svg+xml`，其余 `application/octet-stream`；
5. `source.sha256` = sha256，输入顺序固定：`ui/{package.json, package-lock.json, tsconfig.json, vite.config.ts, index.html}`，然后 `sourceHash(ui/src)` 与 `sourceHash(ui/public)`（若存在）—— 递归按 `name.localeCompare` 排序，只收 `*.ts|tsx|css|svg|png|ico` 且**文件名不含 `.test.`**，每个文件先 update 相对路径再 update 内容；
6. `--check` 校验三项：`source.sha256` 相等、`assets.rs` 文本相等、`cli/ui/` 下每个文件字节相等；任一不符即报错。**不手改 `cli/ui` 下任何文件。**

### 11.2 API 客户端

```ts
export class ApiError extends Error { constructor(public detail: Data) { super(detail.message || "请求失败") } }
export async function api(path: string, body?: Data): Promise< Data>
export async function action(id: string, input: Data): Promise<Data>
```

`api` 拼 `/api/v2/` + path；有 body 才 POST 且带 `Content-Type: application/json`；body 用 `encode(body)`（lossless-json 序列化，保留原始数字字面量）；响应 `text()` 后 `parseData`；`!data.ok` → 抛 `ApiError(data.error)`；返回 `data.data`。
`action(id, input)` = `api("actions/" + encodeURIComponent(id), {input})`。

### 11.3 核心类型（`lib/data.ts`）

```ts
export interface Asset { source: string; schema?: Schema; defaults?: Data; tags?: string[];
                        metadata?: Record<string, Data>; [key: string]: Data }
export interface Document {
  key: string; id: string | null; title: string; asset: Asset; context: Data;
  fingerprint: string | null; saved: string; mode: "use" | "edit";
  tab: "source" | "schema" | "settings"; outputName: string;
  generationAction?: string; parameterSelection?: Data; extensions?: string[];
  replayInput?: Data; jsonInput?: string; lastRunId?: string; lastRunSignature?: string;
}
export interface Run { id; title; action; input; status; started_at; finished_at?;
  result?: { data; artifacts: {name,media_type,text}[]; diagnostics };
  error?: {message, code, diagnostics}; environment?; replay?: {source_run_id, check} }
export interface ReplayCheck { status: "matching"|"changed"|"unknown"; differences: string[] }
export class DraftNumber { constructor(public raw: string) {} }
const NUMBER = /^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/;
```

- `parseData` = lossless-json `parse`；`encode(data, pretty?)` 把 `DraftNumber` 转 `LosslessNumber`，**raw 不匹配 NUMBER 就抛错**（未完成数字阻止执行）；`encode` 返回 `undefined` → 抛 "输入数据为空"。
- `clone(v)` = `parseData(encode(v))`。
- `fieldErrors(schema, value, path)` 递归校验：`x-untyped` → "请先在编辑模式定义参数类型"；object 查 required（undefined / null 而属性类型不含 null / 空串 → "请填写<title>"）；array 逐项；number/integer 用 `numberText` 过 NUMBER，integer 还要过 `/^-?\d+$/`。
- `documentValue(doc)` = `encode({title, asset, outputName, generationAction})`；`isDirty` = `documentValue(doc) !== doc.saved`，抛错时视为脏。
- `newDocument()`：`key = crypto.randomUUID()`、`id:null`、`title:"未命名模板"`、`source:""`、`schema:{type:"object",properties:{},required:[]}`、`mode:"edit"`、`outputName:"output.txt"`。

### 11.4 草稿持久化（`lib/drafts.ts` + `hooks/useDraftPersistence.ts`）

- key = `"nctool:workbench:" + workspaceId`；值 = `{version:1, active, documents: pack(documents)}`。
- `pack` 给每个节点打标签：`["n", raw]`（DraftNumber/LosslessNumber）、`["u"]`（undefined）、`["a", [...]]`、`["o", {...}]`、`["v", value]`。
- `unpack` 反向；`"n"` 先试 `parseData(raw)`，失败退回 `new DraftNumber(raw)`。
- 加载校验：`version === 1` 且 `documents` 是数组且每个 `doc.key` 存在且 `doc.asset.source !== undefined`，否则返回空。
- `useDraftPersistence(workspaceId, documents, active, onStorageFailure)`：
  - 防抖 **350ms** 写 localStorage；失败调 `onStorageFailure()`（提示存储空间不足）；
  - `beforeunload`：先 flush，若有脏文档 `preventDefault()` + `returnValue = ""`；
  - `pagehide`：flush（无确认）。

### 11.5 执行输入与历史恢复（`lib/documentExecution.ts`）

- `domainAction(id)` = `/^(nc|machine|process|sync-turning|undercut)\./.test(id)`；
- `domainTemplate(asset)` = `metadata` 有 `nc` 自有属性，或 `domainAction(metadata.execution?.action)`；
- `documentActions(actions)` = 过滤掉领域动作，保留 `ui_schema.document_input.bindings` 存在的；
- `activePanels(panels, actions)` = 过滤领域 panel/view、排除 `view==="template-editor"` 和 `id==="presets"`，只留 `view==="parameter.library"` 或还有可用动作的；
- `selectDocumentAction(doc, actions, knownActions)` 优先级：
  1. 领域模板/动作 → `{error: machiningRemovedMessage}`；
  2. `doc.generationAction || asset.metadata.execution.action` 显式选择：找到 → 用它；找不到 → error（**不降级**）；
  3. `knownActions` 里 `match_metadata` 键在 asset.metadata 上存在的：恰好 1 个 → 用它；>1 → error "多个插件支持这个模板，请明确选择生成方式。"；
  4. `document_input.default === true` 的：恰好 1 个 → 用它；否则 error。
- `documentInput(doc, entry)`：构造 `values`（source/id/context/`effective_context`=mergeDefaults(defaults,context)/schema/defaults/extensions/`parameter_selection`/`parameter_preset_id`），按 `entry.ui_schema.document_input.bindings` 映射（`metadata.` 前缀走元数据路径；`undefined` 不发送）；对 `template.render`/`template.render.managed` 强制补 `extensions`；
- **replay 路径**：`doc.replayInput.snapshot` 存在且动作是那两个渲染动作时 → `input.snapshot = clone(replay.snapshot)`，把根源码替换为当前草稿源码、`snapshot.asset = clone(doc.asset)`，删 `source`/`template`，补 `extensions/trim_blocks/lstrip_blocks/lenient`；受管动作还带 `parameter_snapshot` + `parameter_request`（`values` 用当前 context，schema/defaults/bindings 依次回落 replay→input→asset），并删掉 `context/schema/defaults/bindings/preset_id`；
- 最后 `encode(input)`：未完成数值必须抛错，**不能回退到旧的合法输入**；
- `documentSignature(doc, action)` = `encode({id, source, schema, defaults, metadata, context, parameter_selection, extensions, replay_input, action})`，抛错返回 `"invalid"`；
- `historyExecutionReason(run, actions)`：领域动作/领域模板 → 拒绝；动作当前无提供方 → 拒绝；
- `restoreDocument(run, actions)`：只支持 `template.render` / `template.render.managed`；从 `replay_input ?? input` 恢复 asset/source/schema/defaults/metadata.parameters.bindings/context；`parameterSelection = input.preset_id === undefined ? {} : {preset_id: input.preset_id ?? null}`（**保留显式拒绝**）；`replayInput = snapshot ? input : undefined`；`mode:"use"`；记 `lastRunId`/`lastRunSignature`。

### 11.6 共享参数前端（`lib/managedParameters.ts` + `components/SharedParameterInputs.tsx`）

- `pointerTokens` / `pointerEscape`（RFC 6901）；
- `overridePointer(values, pointer)`：路径任一段落在数组上，就返回到该数组层的 pointer（**数组只能整数组覆盖**）；
- `setParameterOverride(overrides, effective, pointer, value)`：只记录用户改动的叶子；数组内清字段安全，清数组项则恢复整数组；
- `removePointer`：删叶子并剪掉变空的父层；路径经过数组下标 → 拒绝；
- `applyOverrides(previous, next, overrides)`：只把用户真正改过的叶子并入 overrides（表单回传的是整个可见对象，直接存会把继承值全冻结成 manual）；
- `mergeParameterLayers(base, overrides)`：对象递归合并，数组与数字 token 原子；
- `previewParameters(request)` → `action("parameter.resolve", payload)`；`preset_id !== undefined` 才放进 payload（保留显式 null）；
- `managedRequest(doc)` = 后端会推导的同一个请求（bindings/values/defaults/schema/preset_id，preset 回落 `parameterSelection` → `metadata.parameters.preset_id`）；
- `originLabel`：`definition`→"共享定义默认值"、`spec_default`→"规格默认值"、`template_default`→"模板默认值"、`manual`→"本次填写"、`preset:x`→"共享预置：x"；
- `SharedParameterInputs`：共享预置下拉（`""` = 不使用，当前选择不在目录里也显示）、"清除全部本次覆盖"、"刷新共享参数"；表单显示**继承值**但只保存 manual 覆盖；JSON 模式只编辑覆盖层；`parameter.resolve` 预览 200ms 防抖，用 `key`（`encodeDraft([doc.key, request, doc.jsonInput, refresh])`）丢弃迟到响应；继承基线单独一次请求（`values:{}`），与编辑无关。

### 11.7 依赖过期（`lib/managedDependencies.ts`）

- `managedDependencyRequest(run)`：`run.status === "succeeded"` 且有 `replay_input` 时才检查；`replay.parameter_snapshot` 存在 → `parameter.dependencies.check`，input = `{snapshot, template_snapshot}`；
- 同一 run 的多个消费者共享一次在途检查和 3000ms 轮询；`result.data.current === true` → 不过期，否则 `"参数或模板依赖已变化，请重新生成。"`；请求失败 → `"无法核对当前参数依赖，请检查资产后重新生成。"`；
- 订阅者全部消失即清 timer 并删条目。

### 11.8 任务轮询（`hooks/useRunPolling.ts`）

- `pending` = status ∈ `queued|running` 的 run；`pendingSignature` = `id:status` 排序拼接；
- effect 只依赖 `pendingSignature`（**不依赖整个 runs 对象**，避免结果更新取消在途响应）；
- 每 500ms 逐个 `api("runs/"+id)`；有终态时额外 `api("runs")` 刷新列表；`failed` 的调 `onRunFailure`；失败的 GET 下轮重试。

### 11.9 保存与冲突（`lib/templateSave.ts` + App）

- `templateSaveAsset(doc, generationId)`：metadata 写 `title`/`output_name`，有 generationId 时写 `execution.action`；
- `applyTemplateSaveResult(current, submitted, id, asset, fingerprint)`：`savedSnapshot = {...submitted, id, asset, fingerprint}`；`saved = documentValue(savedSnapshot)`；**仅当 `documentValue(current) === documentValue(submitted)` 才替换 asset**（保存期间的编辑不被覆盖）；
- App `saveDocument()`：`id = doc.id || "tpl-"+randomUUID()`；`action("template.save", {name, asset, expected: doc.fingerprint})`；`conflict` 时读磁盘版本弹 `ConflictDialog`（比较 / 加载磁盘 / 另存副本）。

### 11.10 App 壳（`App.tsx`，2236 行）

- 页面状态 `page: "workbench" | ...`；`runs` / `runList` / `execution` 三类状态由 App 持有，**没有平行 store**；
- 懒加载：`SharedBindingEditor`、`SyntaxGuide`、`PluginPage`、`PluginSettings`、`HistoryPage`，均包 `Suspense`；
- 启动 effect：并行 `api("workspace"/"capabilities"/"plugins")` → 读 `localStorage["nctool:sidebar:"+w.id]`、`["nctool:layout:"+w.id]`（use/edit 各 25–70）→ `loadDrafts(w.id)` → 恢复 `execution` → 对每个 `lastRunId` 拉一次 run → `catalog()` → `api("runs")`；`cancelled` 守卫丢弃迟到响应；
- 快捷键：Ctrl/Cmd+S 保存、Ctrl/Cmd+Enter 生成；有 `[role="dialog"]`/`[role="menu"]` 时禁用；
- `generate()`：`busy` 门禁（queued/running 期间快捷键无法再排队）→ `jsonError/inputError` 拦截 → 非 managed 动作跑 `fieldErrors` → 源码非空 → `documentInput(documentForGeneration(doc), generation)` → `api("runs", {action, input, title})` → 乐观插入 pending run + 记 `execution[key]` 与 `lastRunId/lastRunSignature`；`numeric_underflow` 特殊提示；
- `stale` = `managedDependencies.stale || execution[key].signature !== documentSignature(doc, generation.id)`。

### 11.11 历史页（`components/HistoryPage.tsx`）

- `api("runs/query", {query?, status?, offset, limit:25})`；`offset >= total` 时回退到最后一页；
- `listRevision` / `detailRevision` / `compareListRevision` / `compareRevision` 四个单调序号丢弃乱序响应；
- 图标删除按钮（`aria-label`）+ 确认；运行中被锁保护的记录不可删。

### 11.12 入门示例（`lib/starterExamples.ts`）

两个示例：`plain-text`（`你好，{{ user.name }}！` → `你好，Ada！`）与 `shared-parameters`（`报告：{{ report.name }}（{{ report.unit }}）` → `报告：零件 A（件）`，含 definition + preset + 绑定 `/report/name`）。
`buildStarterExample(id)` 构造完整 `{format:"nctool-workspace", version:1, collections:{template:{version:1,records:[...]}, parameters:{...}}, manifest:[{collection, provider, version:1, fingerprint}]}`；id 带 5 位随机后缀避免覆盖；fingerprint = 对 canonical（键排序）JSON 做 FNV-1a 64（与 Rust 端同算法）。

### 11.13 共享绑定编辑面板（`components/SharedBindingEditor.tsx`）

- 读 `parameter.catalog`（definitions + presets）；
- `bindingShapeError`：`metadata.parameters` / `.bindings` / `.selection` 形状不符 → **禁用编辑**（避免丢数据）；
- `pointerError`：必须以 `/` 开头、≤1024、不能等于 `/`、token 非空且 `~` 只能写作 `~0`/`~1`、禁 token、已绑定、与现有路径重叠；
- 启用/停用、新增/移除绑定、更换定义（`withDefinition` 同步 unit）、选模板默认预置（`selection.preset_id`，空串 → `null`）；
- "检查共享绑定" 调 `parameter.resolve`（`values:{}`），请求序号 + key 双丢弃迟到响应；修改后旧检查作废。

---

## 12. 复刻时必须保留的硬约束（架构门禁）

`scripts/check_v2.py` 把这些写成断言，复刻后必须同样通过：

1. workspace 不含 `nctool-gui`、`nctool-core`、`nctool-plugin-{nc,process,sync-turning,undercut}`；
2. 三棵依赖树（workspace all-features、cli no-default、cli math）都**不得**出现 `tauri*`/`nctool-gui` 或退役包名；
3. 各 neutral crate 的 `nctool-*` 生产依赖集合必须精确等于白名单；
4. `nctool-tpl` 生产依赖集合 == `{minijinja}`；
5. `cli` `features.default == []`、无 `nc-bundle`、`math == ["dep:nctool-plugin-math"]` 且该依赖 optional；
6. math 包只在 math 树里出现；
7. `template`/`parameters` 依赖非 optional 且两棵树都有；
8. 不存在 `gui/` 目录与 `启动GUI.bat`；
9. 不存在 12 个退役源文件路径（`plugins/nc`、`plugins/process`、`plugins/sync-turning`、`plugins/undercut`、`ui/src/plugins/{NcViews,SyncTurningView,TurningWorkbench,UndercutView}.tsx`、`ui/src/lib/{turningModel,syncAp2}.ts`、`ui/src/components/{SpecializedTemplateEditor,TrajectoryPresetEditor}.tsx`）；
10. `src/renderer.rs` 不含 `with_v3_whitespace`、不含 `add_filter("nc_"/"sin"/"cos"/"tan"/"sqrt"`、`src/lint.rs` 不存在；
11. `sdk/python/nctool_plugin.py` 与 `examples/plugins/python-report/nctool_plugin.py` 字节相同；
12. `node scripts/build_ui.mjs --check` 通过；
13. `ui/dist/assets` 与 `cli/ui/assets` 里不得有 `NcViews-/TurningWorkbench-/SyncTurningView-/UndercutView-/SpecializedTemplateEditor-/TrajectoryPresetEditor-` 前缀的 chunk，且每个 `.js` 过 `node --check`；
14. 4 个脚本过 `node --check`；
15. `cli/ui/index.html` 含 `/assets/` 与 `type="module"`；`ui/src/App.tsx` 存在；`ui/src/lib/api.ts` 含 `/api/v2/` 与 `encode(body)`；`cli/ui/assets.rs` 存在；
16. `cargo package -p nctool-tpl --list` 不含 `cli/ core/ plugins/ gui/ templates/ docs/ ui/ runtime/ assets/ sdk/ shaft_programmer/` 前缀；
17. 示例 `plugin.json` 的 `descriptor.requires == [{"id":"template.render","version":1}]`。

---

## 13. 完整验证命令（复刻后照跑）

```bash
npm ci --prefix ui
npm run --prefix ui lint
npm run --prefix ui test
node scripts/build_ui.mjs
python3 scripts/check_v2.py
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --workspace --all-features --doc --locked
cargo +1.89.0 check --workspace --all-features --locked
```

浏览器验收三组（需 Playwright 1.62.1 + Chromium，`NCTOOL_BINARY` 指定固定二进制）：

```bash
cargo build -p nctool-cli --features math
node scripts/browser_v2.cjs
node scripts/browser_review_fixes.cjs
node scripts/browser_generic_parameters.cjs
```

源码快照：`python3 scripts/source_baseline.py capture <新目录>` / `verify <目录>`。
依赖审计：`python3 scripts/audit_dependencies.py`（失败即失败，不用 `npm audit fix`，不自动放宽版本）。

**既有验证记录（本次未重跑）**：`PROJECT_STATUS.md` 的 P2 阶段记录 Windows Rust 默认/math 各 282 项、Clippy、fmt、MSRV 1.89、前端 27 文件 174 项、Python 33 项及三组浏览器通过；之后的共享参数 UI 会话摘要记录前端 28 文件 188 项通过。它们分别对应当时工作树，不构成本报告对当前完整工作树的重新验收。本次仅检查文档内容、链接和锚点，未运行业务测试、远端 CI、联网审计或发行流程，未提交或推送。

---

## 14. 文档与源码的已知偏差（复刻时以源码为准）

1. `assets/src/lib.rs` 与 `assets/src/lock.rs` 的**注释**仍按 `tiny_http`/`serve_requests` 单线程循环解释 try-lock 的理由；实际服务端已是 **axum 0.8 + tokio + Semaphore(8) + spawn_blocking**（[cli/src/server.rs](../cli/src/server.rs)）。具体等待策略以调用点为准：JsonStore 保存不等待，组锁及部分删除/配置操作有 2s 等待；注释里的历史服务实现名已经过时。
2. `docs/SYSTEM_DESIGN.md` 的 HTTP 段落已更新为 Axum/Tokio，与源码一致；无需按旧版描述复刻。
3. `assets/src/json_num.rs` 的 `NumUnderflow.column` 文档注释说按字节计，代码产出的是**字符**计。
4. `assets/src/lib.rs` 的 `WriteError::NumRange`/`NumUnderflow` 文档里还提到 YAML 解析器；当前 `parse_json` 只处理 JSON。
5. [docs/PROJECT_STATUS.md](PROJECT_STATUS.md) 与 [docs/CODE_AND_FUNCTION_STATUS_2026-10-17.md](CODE_AND_FUNCTION_STATUS_2026-10-17.md) 的行数统计基于各自时点的工作树，与当前 HEAD+未提交改动的精确行数会有出入（例如前端生产源码现为 46 文件 / 11,760 行，早先报告记为 33 / 10,250）。
6. `cli/src/main.rs` 的 `--help` 描述字符串是英文（`about = "Jinja template platform with optional domain plugins"`），运行时消息是中文。

---

## 15. 复刻顺序建议

1. **引擎**：`parse` + `extract_*` + `Renderer` + `TplError`（含全部 optional 判定与三条错误名恢复判据）。无外部依赖，可独立测。
2. **资产层**：`FileFingerprint`(FNV-1a) → `DirLock` → `atomic` → `SafePath`/`validate_asset_name` → `WriteKernel` → `JsonStore` → `json_num` → 集合三函数。
3. **SDK**：类型 + `Plugin` trait + `TemplateSnapshot` + schema 编译校验 + Python 适配器。
4. **运行时**：`resolve_composition`（纯函数，先写它）→ `Runtime::start` 生命周期 → `invoke_inner` → `Bridge` → `external` 传输层。
5. **模板插件**：`sources` → `check_snapshot` → `inspect` → `render` → `ensure_generic_template` → 资产 CRUD。
6. **参数插件**：`tokens`/`set_pointer` → `resolve` 优先级 → `effective_bindings` → `render.managed` → `dependencies.check`。
7. **math 插件**（19 个过滤器 + 三重显式门槛）。
8. **CLI**：`default_home` → `App::boot` → 子命令 → `deliver_artifacts` → `Workbench`（含启动恢复与 query）→ `server`（安全层 + dispatch）。
9. **前端**：`api/data/drafts` → `documentModel/documentExecution/managedParameters` → hooks → 各页面 → `App.tsx`。
10. **门禁**：`check_v2.py` + `build_ui.mjs --check` 最后接入。
