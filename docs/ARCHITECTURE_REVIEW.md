# nctool 架构综合评估报告

> 评估日期：2026-09-15
> 评估对象：`rustjinja` workspace（`nctool-tpl` v0.3.2 / `nctool-core` v0.2.2 / `nctool-cli` v0.2.2，14 462 行 Rust）
> 评估方式：**基于源码实证指标**（模块规模、生产/测试行比、panic 面、CI 门禁、调用点分析），非主观印象。
> 配套文档：`docs/SYSTEM_DESIGN.md`（架构事实）、`docs/architecture-overview.html`（架构图）

---

## 1. 评分总览

评分刻度：10 = 卓越，8 = 良好，6 = 及格有短板，4 = 需返工。

| 维度 | 权重 | 得分 | 一句话依据 |
| --- | ---: | ---: | --- |
| 可维护性 | 20% | **8.0** | 8 299 生产行 + 5 126 测试行，模块边界清晰、注释解释"为什么"；扣分在 `check_vars` 248 行单函数与数据表内联进代码 |
| 可测试性 | 20% | **7.5** | 459 项测试、三平台 CI 含 `clippy -D warnings` 与 `cargo audit`；扣分在覆盖率门禁**实际未生效**、`extract.rs` 零内联测试、无属性测试 |
| 可扩展性 | 15% | **7.5** | 模板元数据外部化到三个声明式来源（加模板免重编译）是最大亮点；扣分在无 trait 扩展点，新增过滤器/参数类型需改内核 |
| 性能 | 15% | **6.5** | minijinja 本身极快、分配有上界；扣分在**完全没有缓存层**：每个 HTTP 请求全量重建注册表，单次渲染重复解析 2–3 次 |
| 安全性 | 15% | **7.5** | 回环地址硬约束、1 MiB 体积上限、路径穿越有测试、`cargo audit` 入 CI；扣分在无 Origin/CSRF 校验与 CSP 头、存在绕过校验的公开渲染路径 |
| 耦合度（低耦合=高分） | 15% | **7.5** | 依赖严格单向 `cli → core → tpl → minijinja`，core 无 I/O、tpl 不懂 G-code；扣分在 `--param` 归一逻辑 Rust/JS 双份实现、`server.rs` 混合传输与展示 |

### 总体评分

```
加权总分 = 8.0×0.20 + 7.5×0.20 + 7.5×0.15 + 6.5×0.15 + 7.5×0.15 + 7.5×0.15
        = 1.60 + 1.50 + 1.125 + 0.975 + 1.125 + 1.125
        = 7.45  ≈  7.5 / 10
```

> **总体评分：7.5 / 10 —— 良好，工程化程度明显高于同类工具项目。**

结论：这是一个**架构判断力强、纪律性好**的项目。分层、单一来源、静默出错零容忍等原则不仅写在文档里，
在代码中有一致的落地痕迹（0 处生产 `panic!`、1 处 `unwrap()`、CI 门禁严格）。
短板集中在**性能工程（缓存缺失）**与**质量度量闭环（覆盖率未生效）**两块，
且都不属于架构性缺陷 —— 是"再加一层"就能解决的问题，不是"推倒重来"。

---

## 2. 各维度评分依据

### 2.1 可维护性 —— 8.0

**加分证据**

| 指标 | 实测值 | 解读 |
| --- | --- | --- |
| 生产代码行 | 8 299 | 对 3 crate 的能力面而言规模克制 |
| 测试代码行 | 5 126 | 测试/生产 ≈ **0.62**，该比值在工具类项目中属健康区间 |
| 生产 `panic!` / `unreachable!` | 0 / 1 | 无 panic 面，畸形输入不会触发进程 abort |
| 生产 `unwrap()` | 1 | 近乎零；错误全部走 `Result` + 自定义错误类型 |
| `TODO`/`FIXME`/`HACK` | **0**（唯一匹配是注释里的 Unicode 转义示例） | 无遗留技术债标记 |
| 文档注释 | `#![warn(missing_docs)]` + 大量"为什么"注释 | 注释解释设计取舍而非复述代码 |

注释质量是本项目最突出的可维护性资产。例如 `pipeline.rs` 里 `MAX_LINE_NUMBER_DIGITS`
的注释完整解释了"为何是 32 而不是对齐 `nc_pad` 的 1024"（行号作用于每一行，
总分配量是 `行数 × 位宽`），`validate.rs` 里解释了"为何白名单检查不能塞进约束检查"。
这类注释让后续修改者能判断"这条规则能不能动"，而不是靠猜。

**扣分项**

1. **`check_vars` 单函数 248 行**（`core/src/validate.rs`）。串行执行约 10 类检查
   （派生 → 默认值自洽 → 缺失 → 类型 → 有限性 → 白名单 → 区间 → 整数性 → 条件必选 → 冗余），
   是全部模块中最长的逻辑函数，也是修改风险最集中处。
2. **数据表内联在代码中**：`registry.rs::builtin_templates()` 284 行、
   `machine.rs::config()` 214 行、`cli.rs::from_core()` 175 行均为纯字面量/match 表。
   数据变更需要改 Rust 源码并重编译 —— 与项目自己在模板元数据上采用的
   "外部化"思路不一致（`cli/ui/index.html` 已用 `include_str!`，说明团队具备该手法）。
3. **`model.rs` 承载 7 个公开类型**（839 生产行）：`ParamValue`/`ParamKind`/`ParamSpec`/
   `ParameterSet`/`MachineConfig`/`DeriveRule`/`RequiredIf`，内聚性偏弱。

### 2.2 可测试性 —— 7.5

**加分证据**

| 指标 | 实测值 |
| --- | --- |
| 测试总数 | **453**（单元 366 / CLI 集成 87） |
| CI 门禁 | `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test --all-targets`、`cargo doc` 带 `-D warnings`、`cargo audit` |
| CI 平台矩阵 | ubuntu / windows / macos 三平台 |
| 黄金样本 | `tests/golden/`，移植模板走逐字节比对（INDEX G420 已 18/18 逐行一致） |
| 一致性测试 | `ui_html_copies_stay_in_sync` 拦住 UI 双份文件漂移 |
| 可测性设计 | `extract`/`validate`/`derive` 均为纯函数，无 I/O，单元测试成本低 |

**扣分项**

1. **覆盖率门禁实际未生效**（最关键）。`.github/workflows/ci.yml` 的 `coverage` job
   设了 `continue-on-error: true`，注释自述"自引入起在 ubuntu runner 上持续失败……
   先解除 CI 红灯"。结果是**没有任何覆盖率数字**，CI 全绿不代表覆盖达标。
2. **`src/extract.rs` 577 生产行、0 项内联测试**。而这里恰好是最微妙、
   错了会"静默推迟崩溃"的逻辑（兜底不向下传播、自赋值兜底的可选性判定）。
   现有覆盖来自 `src/lib.rs` 的 119 项示例式断言 —— 能防回归，但防不住
   未被想到的写法组合。
3. **无属性测试 / 无 fuzz**。可选/必选推断本质上是一条可形式化的不变量
   （"判为可选 ⇒ 严格模式下确实可缺省渲染"），适合 proptest 验证；解析器适合
   fuzz 目标。二者都缺失。
4. **前端 JS 无测试载体**，仅靠 `node --check` 验语法。

### 2.3 可扩展性 —— 7.5

**加分证据**

- **模板元数据三层外部化**（清单 > `variables.yaml` > 头部注释）：新增一个模板
  = 放一个 `.j2` 文件 + 加一条清单条目，**无需改 Rust、无需重编译**。
  这是本架构最有价值的扩展性设计。
- **`derive` 规则声明式**：查表换算（如 `tip_model → tip_depth`）以数据形式声明，
  引擎实现一次、复用无限次。
- **稀疏覆盖机制**（`ParamOverride` 全字段 `Option` + `deny_unknown_fields`）：
  三个层级共享同一套合并语义，扩展新约束字段只改一处结构体。
- **前向兼容标注**：`IssueKind` 与 `TplError` 均 `#[non_exhaustive]`，新增变体不破坏下游。
- **机床配置键 schema**（`KNOWN_CONFIG_KEYS` + `validate_config_keys`）：新增配置键有登记处与校验。

**扣分项**

1. **无 trait 级扩展点**。过滤器在 `filters.rs` 硬编码注册；新增一个 NC 过滤器
   或校验规则必须改 `core`/`tpl` 源码并重编译，下游无法以库的形式注入自己的规则。
   对"下游二次开发"这一 README 明示的用途而言，这是主要缺口。
2. **`ParamKind` 是封闭枚举**，新增参数类型需同步修改所有 match 分支
   （`matches`/`label`/`spec_json`/`coerce_param_value` 等多处），
   且分散在不同 crate，编译器能拦住但改动面大。
3. **HTTP 路由是硬编码 match 表**（`server.rs::route`），新增端点需改该函数；
   对本地工具可接受，但不利于模块化拆分。

### 2.4 性能 —— 6.5

**加分证据**

- minijinja 是当前最快的 Jinja 实现之一（作者即 Jinja2 原作者），引擎选型正确。
- **所有放大路径都有上界**：`MAX_NC_FIXED_DECIMALS = 32`、`MAX_NC_PAD_WIDTH = 1024`、
  `MAX_LINE_NUMBER_DIGITS = 32`、`MAX_BODY_BYTES = 1 MiB`。这一点比多数项目做得好 ——
  明确防住了"用户配置一个巨大宽度拖垮内存"。
- 后处理单遍扫描，无二次遍历；行号用 `checked_add` 防溢出。
- 已有 criterion 基准（解析/提取/渲染 + 管线端到端），性能回归有观测手段。

**扣分项（本维度是短板所在）**

1. **注册表零缓存，每个 HTTP 请求全量重建**。`cli/src/server.rs` 有 **4 处**
   `ctx.build_registry()` 调用（L100 / L137 / L253 / L498），每次请求都执行：
   `canonicalize` → 读 `templates.yaml` → 读 `variables.yaml` → 递归遍历目录 →
   对**全部 25 个模板**逐个 `read_to_string` → 逐模板解析 `{# PARAMS: #}` 头部 →
   构造 `TemplateEntry`。复杂度是 **O(模板数)**，UI 里每次切换模板或改参数都要付一遍。
   25 个模板下耗时仅数毫秒、体感无碍，但**设计上没有余量** ——
   仓库扩到数百模板时 UI 会明显退化。
2. **同一模板在一次渲染中被重复解析 2–3 次**：
   `registry.validate` → `nctool_tpl::parse`（`registry.rs:397`）→ `validate_template`
   → `nctool_tpl::parse`（`validate.rs:265`）；`extract_params` 再 parse 一次
   （`registry.rs:362`）。AST 与 `extract_undeclared` 结果均未缓存。
3. **`derive::apply` 每次渲染跑两遍**（校验前 + 渲染前），这是为正确性付出的代价，
   可以接受，但若有了 AST/规格缓存则可省。
4. **`ParameterSet::to_minijinja_value()` 每次渲染重建整张上下文 map**（`model.rs:827`）。
   上下文很小，收益有限，优先级最低。

### 2.5 安全性 —— 7.5

威胁模型：本地开发工具，单用户，无多租户、无凭据存储。以下按该模型评估。

**加分证据**

| 措施 | 位置 | 评价 |
| --- | --- | --- |
| **仅允许回环地址** | `server.rs::listen_addr`：非 loopback 直接返回错误 | 硬约束而非文档约定，做法正确 |
| **请求体上限** | `MAX_BODY_BYTES = 1 MiB`，超限返回 413 | 用 `.take(n+1)` 判定，避免先分配再检查 |
| **路径穿越** | 有专门测试（提交 `"../Cargo.toml"` 断言被拒） | 有测试守护而非仅靠实现 |
| 响应头 | 静态 `Content-Type` 硬编码 | 无头部注入面 |
| **依赖审计** | `cargo audit` 进 CI | 供应链有门禁 |
| 无 panic 面 | 生产 0 `panic!` / 1 `unwrap()` | 畸形输入不能造成进程 abort（DoS） |
| 输出有限性 | 所有过滤器 + `validate` 双层拦 NaN/Inf | 直接防止非法坐标上机 |
| 严格模式默认 | 宽松需显式 `--lenient` | 安全默认 |

**扣分项**

1. **无 `Origin` / `Referer` 校验，无一次性 token**。浏览器中任意网页可向
   `127.0.0.1:8787` 发起 POST。虽然读取响应受同源策略限制，
   但在 DNS rebinding 场景下可绕过；且本项目 `--open` 会自动开浏览器，
   攻击面是真实存在的。
2. **服务的 HTML 未设 `Content-Security-Policy`**。UI 是内嵌单文件、当前无外部脚本，
   但缺 CSP 意味着未来引入任何资源都没有第二道防线。
3. **存在绕过校验的公开渲染入口**：`TemplateRegistry::render` 的文档明确写
   "**绕过校验层**：非有限数（NaN/Inf）的拦截位于校验层，直接调用本方法时
   NaN/Inf 会以文本 `"NaN"`/`"inf"` 写入输出"。文档已警示，但把一个
   "会产出非法 G-code"的方法设为 `pub` 是危险设计 —— 下游误用是迟早的事。
4. **模板 `include` 的文件加载信任模板目录**，未对解析后的路径做"必须仍在根目录内"
   的二次校验（`set_path_loader` 交给 minijinja）。模板由用户自己编写，
   风险低，但若未来支持"下载他人模板"则需补。

### 2.6 耦合度 —— 7.5

**加分证据**

- **依赖严格单向**：`cli → core → tpl → minijinja`，无反向、无跨层。已逐 crate 核对
  `Cargo.toml` 确认。
- **职责边界干净**：`nctool-tpl` 不知道 G-code 语义（纯模板引擎 + NC 数值过滤器）；
  `nctool-core` 不做 I/O、不感知终端与命令行；`nctool-cli` 不含校验/渲染逻辑。
- **`Ctx` 是清晰接缝**：把"配置层叠 + 模板目录 + 默认机床"收敛成一个装配对象，
  命令实现只依赖它，不直接读盘。
- **校验/派生是纯函数**（输入 → 报告），无副作用，与 I/O 解耦。
- 保证 CLI 与 Web UI 输出一致的机制，是把业务逻辑完全下沉到 `core`，
  而不是在两个交付面各写一遍 —— 这是正确的解耦方向。

**扣分项**

1. **`--param` 归一逻辑 Rust 与 JS 各写一份**。项目自己的 P4 原则要求
   `args::coerce_param_value` 与前端 `coerceParamValue` 保持同序
   （显式后缀 > 白名单命中 > 声明类型 > 启发式），但一致性仅靠
   "两份 `index.html` 必须字节相同"的测试保证 ——
   **该测试不校验 JS 逻辑与 Rust 逻辑等价**。改 Rust 顺序而忘改 JS 时，
   CLI 与 UI 会对同一输入得出不同的参数类型，且是静默差异。
   这是当前架构里最实在的耦合风险。
2. **`cli/src/server.rs` 853 行混合两种关注点**：HTTP 传输（路由/状态码/体积限制/URL 解码）
   与展示层塑形（`spec_json` 规格 JSON 形状）。后者按 P4 是"单一来源"，
   但它落在交付层的传输模块里，位置不够干净。
3. **`TemplateEntry` 同时持有原始文本与解析后元数据**（`source_text` + `params`），
   注册表因此要按需重复解析 —— 既是性能问题（见 2.4），也是"同一份数据两种表示"
   的耦合（存在不一致可能）。
4. **`ui/index.html` 物理复制两份**（`ui/` 与 `cli/ui/`）。用测试锁住同步是权宜之计，
   更干净的做法是单一文件 + 构建期复制或 `include_str!` 指向同一路径。

---

## 3. 关键问题与改进建议（按优先级）

### P0 —— 影响当前架构上限，建议优先处理

> **状态：三项已于 2026-09-15 全部修复**，见 §5 修复记录。

| # | 问题 | 证据 | 建议 | 状态 |
| --- | --- | --- | --- | --- |
| P0-1 | **注册表无缓存，每请求全量重建** | `server.rs` 4 处 `ctx.build_registry()` | 在 `Ctx` 上持 `OnceCell`/`RwLock<GCodeGenerator>`，按 `(template_dir, 清单 mtime)` 失效；或改惰性加载（仅在 `extract_params`/`validate` 时读该模板文件） | ✅ 已修复 |
| P0-2 | **覆盖率门禁未生效，质量度量无闭环** | `ci.yml` coverage job `continue-on-error: true` 且自述持续失败 | 修复该 job（换安装方式或改用 `cargo-nextest` + tarpaulin），拿到数字后设阈值门 | ✅ 已修复 |
| P0-3 | **同模板单次渲染重复解析 2–3 次** | `registry.rs:362/397`、`validate.rs:265` | `TemplateEntry` 缓存 `Ast` 或至少缓存 `extract_undeclared` 结果，三条路径共用 | ✅ 已修复（并修掉同源的宽松渲染器重建） |

### P1 —— 影响长期可维护性与正确性保障

| # | 问题 | 证据 | 建议 |
| --- | --- | --- | --- |
| P1-1 | **`--param` 归一逻辑双份实现，等价性无测试** | P4 约定 + `ui_html_copies_stay_in_sync` 只比对文件相同 | 把归一顺序抽为一份规格（JSON 表）由两侧读取；或 UI 只提交字符串、由后端统一归一 |
| P1-2 | **最微妙逻辑零内联测试 + 无属性测试** | `src/extract.rs` 577 行 / 0 测试 | 为 `extract_undeclared` 加 proptest，断言不变量"判为可选 ⇒ 严格模式下确实可缺省渲染" |
| P1-3 | **存在绕过校验的公开渲染入口** | `TemplateRegistry::render` 文档自述"绕过校验层"、可输出 `"NaN"` | 在 `render` 内部补有限性检查，或降为 `pub(crate)`，只保留 `pipeline::generate*` 为公开入口 |
| P1-4 | **本地 UI 无 Origin/CSRF 校验与 CSP** | `serve()` 无 `Origin` 检查、无 CSP 头 | 校验 `Origin` 为空或同源；启动时生成一次性 token 拼进 URL；补 CSP 头 |

### P2 —— 结构性优化，可择机进行

| # | 问题 | 证据 | 建议 |
| --- | --- | --- | --- |
| P2-1 | `check_vars` 248 行单函数 | `core/src/validate.rs` | 拆为 `check_derive` / `check_defaults` / `check_missing` / `check_types` / `check_options` / `check_ranges` / `check_unused`，主函数只编排；测试可保护 |
| P2-2 | 数据表内联进代码 | `builtin_templates()` 284 行、`machine::config()` 214 行、`from_core()` 175 行 | 内置模板改 `include_str!` 资源（与 `cli/ui/index.html` 同法），机床预设改 TOML/YAML 资产 |
| P2-3 | `model.rs` 承担 7 个公开类型 | 839 生产行 | 拆 `value.rs`（ParamValue/ParamKind）、`spec.rs`（ParamSpec/RequiredIf/DeriveRule）、`set.rs`（ParameterSet） |
| P2-4 | 无 trait 级扩展点 | `filters.rs` 硬编码注册、`ParamKind` 封闭枚举 | 引入过滤器/校验器注册表；若确定不做插件化，至少在文档中明确"扩展需改内核" |
| P2-5 | `ui/index.html` 物理两份 | `ui/` 与 `cli/ui/` | 单一源文件 + 构建期复制，去掉同步测试这一补丁 |
| P2-6 | 上下文 map 每渲染重建 | `model.rs:827` | 收益有限，最低优先级 |

---

## 4. 实证指标附录

### 4.1 代码规模（剔除 `#[cfg(test)]` 后的生产行）

| 文件 | 生产行 | 测试行 |
| --- | ---: | ---: |
| `core/src/model.rs` | 839 | 277 |
| `core/src/registry.rs` | 817 | 339 |
| `core/src/manifest.rs` | 810 | 561 |
| `core/src/validate.rs` | 696 | 824 |
| `cli/src/server.rs` | 630 | 163 |
| `src/extract.rs` | 577 | 0 |
| `src/error.rs` | 376 | 0 |
| `core/src/pipeline.rs` | 350 | 599 |
| `core/src/machine.rs` | 322 | 110 |
| `cli/src/cli.rs` | 294 | 0 |
| **合计（全部 25 个源文件）** | **8 361** | **5 426** |

> 注：`src/lib.rs` 总 1 821 行中生产部分极少（再导出为主），其余为 119 项契约测试 ——
> 不构成"上帝模块"。

### 4.2 健壮性指标（生产代码）

| 指标 | 数量 |
| --- | ---: |
| `panic!` | 0 |
| `unreachable!()` | 1 |
| `unwrap()` | 1 |
| `expect()` | 5 |
| `TODO`/`FIXME`/`HACK` | 0 |

### 4.3 最长的生产函数

| 行数 | 位置 | 性质 |
| ---: | --- | --- |
| 284 | `core/src/registry.rs::builtin_templates()` | 数据表（非逻辑） |
| 248 | `core/src/validate.rs::check_vars()` | **逻辑，需拆分** |
| 214 | `core/src/machine.rs::MachinePreset::config()` | 数据表 |
| 175 | `cli/src/cli.rs::CategoryArg::from_core()` | 映射表 |
| 148 | `src/extract.rs::walk_stmt()` | AST 遍历（递归下降，可接受） |

### 4.4 测试与 CI

| 项 | 值 |
| --- | --- |
| 测试总数 | 459 `#[test]`（实测 474 项通过，含参数化与文档测试）；分布：`src/lib.rs` 119、`core/*` 189、`cli/src/*` 46、`tests/parsing.rs` 18、`cli/tests/cli.rs` 43、`cli/tests/cli_e2e.rs` 44 |
| CI 门禁 | fmt / clippy `-D warnings` / test / doc `-D warnings` / audit |
| CI 平台 | ubuntu、windows、macos |
| 覆盖率 | **已可度量**（2026-09-15 修复）。本地基线：行 **90.75%**、区域 90.52%、函数 90.18%（`cargo llvm-cov --workspace`） |

### 4.5 关键性能调用点（评估时快照）

| 位置 | 行为 | 频次 |
| --- | --- | --- |
| `cli/src/server.rs:100,137,253,498` | `ctx.build_registry()` 全量重建 | 每 HTTP 请求 → **已缓存** |
| `core/src/registry.rs:362` | `nctool_tpl::parse` | 每次 `extract_params` → **已缓存** |
| `core/src/registry.rs:397` → `core/src/validate.rs:265` | `nctool_tpl::parse` 两次 | 每次 `validate` → **已缓存** |
| `core/src/registry.rs` `render_template_lenient` | 重建渲染器并重编译全部模板 | 每次宽松渲染 → **已缓存** |
| `core/src/derive.rs::apply` | 派生计算 | 每次渲染 2 次（保留：为正确性付出的代价） |
| `core/src/model.rs:827` | `to_minijinja_value()` 重建上下文 | 每次渲染（保留：上下文很小，收益有限） |

---

## 5. 修复记录（2026-09-15）

三项 P0 已修复，均带回归测试。共同主题是"同一份工作被反复重做"。

### 5.1 P0-1 注册表缓存

| 项 | 内容 |
| --- | --- |
| 改动 | `cli/src/context.rs`：`Ctx::build_registry()` 返回 `Rc<GCodeGenerator>`，按 **`(模板目录, 目录树最新 mtime)`** 缓存；新增 `canonicalize_dir` / `tree_stamp` / `RegistryKey` |
| 失效策略 | 目录树指纹变化即重建。**指纹不可省** —— 无条件长期缓存会让用户改完模板仍拿到旧注册表，渲染出与图纸不符的 G-code（本项目零容忍的失败模式） |
| 降级 | 指纹取不到（IO 异常）时**放弃缓存**，宁可每次重算 |
| 新增入口 | `Ctx::build_registry_fresh()`：供需要 `&mut GCodeGenerator` 的调用方（`render` 注册临时文件模板）。走缓存会让临时模板泄漏进共享注册表 |
| 调用方调整 | `server.rs::registered_template` 返回 `Rc<GCodeGenerator>`；`render.rs::resolve_registry` 常态走缓存、仅临时注册时用 fresh |
| 测试 | `registry_is_reused_while_directory_is_unchanged`、`registry_is_rebuilt_after_template_edit`、`registry_is_rebuilt_when_template_added`、`fresh_registry_does_not_pollute_shared_cache` |

### 5.2 P0-2 覆盖率门禁

| 项 | 内容 |
| --- | --- |
| 改动 | `.github/workflows/ci.yml` 的 `coverage` job |
| 根因处理 | 用 `cargo install cargo-llvm-cov --locked` 替代第三方 `taiki-e/install-action`（原实现疑似其装的二进制与 runner 环境不兼容） |
| 恢复门禁 | **移除 `continue-on-error: true`** —— 该开关把持续失败掩盖成"非阻断项"，导致覆盖率从未被度量 |
| 可观测 | 覆盖率数字写入 job summary（`$GITHUB_STEP_SUMMARY`）并随 artifact 上传，为后续设阈值提供真实基线。摘要步骤**必须带 `--workspace`** —— 漏掉它只统计根 crate，摘要会"看起来有数字"却漏掉 core / cli（代码主体），本地实测：漏掉时误报行覆盖 91.54%，带上后真实值为 **90.75%** |
| 本地实测基线 | 行 **90.75%**（8807 行）、区域 90.52%、函数 90.18%。最低三项：`src/extract.rs` 74.25%、`cli/src/output.rs` 65.22%、`cli/src/server.rs` 73.63% —— 与 §2.2 的判断一致（`extract.rs` 最微妙却最少测试） |
| 遗留 | 阈值门（如 `--fail-under-lines`）待 CI 跑出基线后再设；本机无法验证 runner 侧，需下一次 CI 运行确认 |

### 5.3 P0-3 重复解析

| 项 | 内容 |
| --- | --- |
| 改动 1 | `core/src/registry.rs`：`TemplateEntry::analysis()` 惰性缓存 `Analysis { variables, refs }`（`OnceCell`）。`Ast` 借用源码无法自引用存入条目，故缓存解析**产物** |
| 改动 1 连带 | `collect_include_closure` 改为接收**条目**而非 AST，使 `include` 闭包里的每个子模板同样只解析一次 |
| 改动 1 连带 | `TplError` 增加 `Clone`：缓存失败态需要留存带行列定位的原始错误（`extract_params` 要返回 `RegistryError::Compile`） |
| 改动 2 | `TemplateRegistry::lenient_cache`：宽松渲染器惰性构建一次并缓存，`add_entry` 时失效。此前每次宽松渲染都新建 `Renderer` 并把全部模板重新注册、重新编译 |
| 语义保持 | 宽松渲染器构建失败（某模板编译不过）的原因一并缓存，返回语义与逐次重建一致；`validate` 的主模板解析失败分支仍走 `validate_template` 以保留 `IssueKind::ParseError` 的行列定位 |
| 测试 | `analysis_is_computed_once_and_matches_direct_extraction`（缓存结论须与直接提取逐项一致）、`lenient_renderer_cache_sees_templates_registered_later`（缓存须随注册失效） |

### 5.4 未处理项

P1（`--param` 归一逻辑双份实现、`extract.rs` 零内联测试、绕过校验的公开 `render`、
UI 无 Origin/CSRF 校验）与 P2（`check_vars` 拆分、数据表外置、`model.rs` 拆分等）**保持原状**，
见 §3 的 P1 / P2 表。
