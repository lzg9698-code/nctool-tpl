# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/) 格式和 [语义化版本](https://semver.org/)。

## 版本策略

- **0.x 阶段**：API 仍在演进，`minor` 版本升级可能包含破坏性变更（如本版本的 API 收窄、错误细分）。
- **1.0.0**：公共 API 稳定后发布，此后严格遵循语义化版本（`major` = 破坏性变更，`minor` = 向后兼容新功能，`patch` = 向后兼容修复）。
- 破坏性变更均在 `Changed` 节明确标注，并在 README 中说明迁移方式。

---

## [nctool-tpl 0.3.2] - 2026-09-03

"第三轮彻底代码审查修复"（核心/CLI 配套改动见 [nctool-core 0.2.1] / [nctool-cli 0.2.1]）。

### Fixed

- **`{% from %}` 别名语义修复**：`{% from "m" import a as b %}` 此前把导入名与别名的
  角色弄反——别名被误报为外部必选参数（上层校验要求用户提供根本不需要的参数）、
  导入名被误判为模板局部（漏报真实引用）。现对齐 minijinja 绑定语义（有别名绑定
  别名，无别名绑定导入名），两者均不再进入未声明集合
- **`{% block %}` 作用域修复**：块体按 minijinja VM 帧语义视为独立作用域，块内
  `set` 不再外泄。修复"块内 `set` + 块外引用"时校验放行、宽松模式静默输出不完整
  G-code（如 `G1 F`）的漏报
- **未定义变量名恢复防错位**：include/extends 期间子模板报错时，不再用主模板源码
  恢复变量名（子模板字节范围套在主模板上会得到同偏移处的无关标识符）；仅当错误
  确属所传源码对应模板时才恢复
- **`Parse.col` 列口径统一为字符**（与 `Variable.col` 的 minijinja span 口径一致）：
  错误行内含多字节字符（如中文注释）时列号不再虚高
- **`set_path_loader` 安全加固**：模板名自加校验，拒绝空名、Windows 盘符前缀
  （`C:`——`PathBuf::push` 会整体替换 base，可逃出根目录）、绝对路径；安全文档
  改写为与引擎实际行为一致（`..`/`.`/`\` 段本就被 minijinja `safe_join` 拦截，
  原文档示例方向反了）
- `nc_pad` 超范围值（如 `1e300`）不再饱和截断为 `i64::MAX`，直接报错
- `nc_fixed(小数位)` / `nc_pad(宽度)` 设上限（32 / 1024），消除巨量分配导致的进程 abort

### Changed

- **模块拆分**：`lib.rs`（约 2500 行 → 文档 + 再导出 + 测试）拆为
  `error.rs` / `extract.rs` / `filters.rs` / `renderer.rs` 四个模块，公共 API 不变
- `add_template` 同名静默替换语义写入文档
- 文档明确有限性防线范围：仅覆盖本库注册的过滤器，裸 `{{ x }}` 与内建操作不保护
- 新增 `extract_template_refs`：提取 `{% include %}`/`{% extends %}`/
  `{% import %}`/`{% from %}` 的静态（字符串字面量）模板引用名，供上层递归校验
  组合模板

---

## [nctool-core 0.2.1] - 2026-09-03

"阶段 A（需求与设计收口）落地 + 第三轮彻底代码审查修复"：golden 基线 15 组、
MachineConfig 键名 schema、API 冻结清单明确。

### Added（阶段 A）

- **golden 基线扩充到 15 组**（A2）：5 内置模板 × 3 机床预设，每组冻结渲染输出
  （`.nc`）与校验报告（`.report.txt`），取代原有 4 文件弱覆盖；`assert_golden` 支持
  `NCTOOL_UPDATE_GOLDEN=1` 人工刷新（防误改基线掩盖回归）；新增防漏项测试
  （矩阵必须覆盖全部内置模板）
- **MachineConfig 键名 schema**（A4）：`KNOWN_CONFIG_KEYS` 登记 20 个已知键
  （类型/默认值/描述）+ `validate_config_keys`；未知键 / 非整数的
  `program_digits` 类键值 / 非枚举的 `units` 类键值给出结构化告警。
  generic 预设与 schema 默认值一致性测试，防"模板用不到、改配置不报错"的静默漂移

### Fixed（第三轮审查）

- **内置模板 `program_header` 修复（P1）**：`G{{ machine.coordinate_system }}` 与
  配置值 `G54`/`G94` 叠加产出非法双前缀 `GG54`/`GG94`；改为直接输出配置值。
  原三处 `contains("G54")` 弱断言被 `"GG54"` 穿透，现全部升级为字节级 golden
- **内置模板 `tool_change` 修复（P1）**：刀具号裸输出 f64（`M6 T5.0`，T 字址标准
  只接受整数）；改为 `T{{ tool_num | nc_strip }}`，并补 tool_change/safe_move 的
  字节级渲染测试（原零覆盖）
- **内置模板 `drill_cycle` 规格去重（A1 核对随手修复）**：`r_plane` 规格重复链
  调用 `with_min(0.0).with_unit("mm")`，收敛为单份
- **校验穿透 include/extends（P2）**：组合模板引用的已注册模板，其必选参数同样
  参与渲染前校验（环引用防护）；此前主模板 include 子模板时，子模板必选参数
  缺失只能在渲染阶段才暴露，违背"渲染前可发现错误"的设计承诺
- **`registry.render` 与 `validate` 口径对齐（P2）**：render 应用规格默认值兜底；
  新增 `render_with_machine`（注入机床系统变量 + 兜底）。消除文档推荐流程
  "validate 通过 → render 失败"的分歧
- **新增 `GCodeGenerator::generate_lenient`（P1，配合 CLI）**：宽松生成复用严格
  管线的规格默认值兜底、`machine` 注入与后处理，保证宽松输出是严格输出的超集
- **错误链完整化（P2）**：`PipelineError`/`RegistryError` 标记 `#[non_exhaustive]`
  并实现 `source()`；管线不再 `map_err(|_|…)` 吞掉注册表错误（新增
  `PipelineError::Registry` 变体）；`RegistryError::Compile` 结构化携带
  `(name, TplError)`，Display 不再三层重复模板名
- **去重**：`apply_spec_defaults`/`build_render_context`/`param_to_minijinja` 收敛
  到 model 模块单份实现（原先 core 内两份 + CLI 一份共三处拷贝）
- `GenerationOptions` 行号生成修复：`step=0` 视为 1（此前产出全 `N0000`）；加法改
  `checked_add` 防溢出
- 删除死公共 API：自由函数 `has_errors`、类型别名 `ValidationResult`（零调用）
- `ParamSpec` 文档修正：`default` 与 `required` 非互斥（与既有用法/测试一致）
- 校验错误消息携带模板名 + 行列定位（此前只报参数名）
- 参数名与系统注入变量（`machine` 等）同名时输出警告（此前被静默覆盖）
- **机床配置键接线**：`program_prefix`/`program_digits` 接入 program_header，
  `units` 替代硬编码 metric；`line_number_prefix`/`line_number_digits` 接入
  G-code 后处理行号（此前均为装饰性键）
- `registry.render`/`render_with_machine` 文档标注绕过校验层的 NaN 风险；
  `apply_spec_defaults` 文档写明优先级（用户值 > 规格默认 > 模板内联 default）
- 声明 `rust-version = "1.82"`（三个 crate）

---

## [nctool-cli 0.2.1] - 2026-09-03

"阶段 A 接线 + 第三轮彻底代码审查修复"。

### Added（阶段 A）

- `machine show` 输出配置告警（A4 接线）：未知键 / 非法值经
  `validate_config_keys` 检查后以 `⚠` 行展示（text）并附 `warnings` 数组（JSON），
  不阻断命令成功——自定义机床配置携带扩展键仍可正常使用

### Fixed（第三轮审查）

- **`render --lenient` 收编核心管线（P1）**：此前宽松路径绕过规格默认值兜底与
  全部后处理——旗舰模板 `drill_cycle` 省略 `r_plane` + `--lenient` 反而渲染失败
  （宽松比严格更易失败），`--line-numbers/--header/--ascii/--strip-blank` 在该分支
  静默失效。现改走 `GCodeGenerator::generate_lenient`，并删除 CLI 侧
  `build_context`/`render_lenient` 副本（上下文构建收归 core 单份实现）
- **`--out` 写入安全（P2）**：拒绝输出路径与源模板相同（防止渲染结果覆盖并销毁
  模板源码）；父目录缺失时自动创建（对齐 `templates new` 的目录策略）
- **配置解析不再 gate 全部命令（P2）**：`completion`/`ui`/`part` 不读配置文件，
  CWD 存在损坏的 `nctool.toml` 时补全生成不再一并失败
- **脚手架修复**：`templates new` 骨架的坐标系行删除多余 `G` 前缀（与
  program_header 同源的 `GG54` 问题）
- `--lenient` 帮助文案对齐实际语义（经过滤器引用的变量仍需具体值才能求值）

### Changed

- **退出码矩阵**：0 成功 / 1 校验失败 / 2 参数错误（与 clap 一致）/ 3 IO / 4 配置 /
  5 模板·机床未找到 / 6 渲染失败（此前一律 1）
- `validate --format json` 失败输出补齐 `error:{kind,message}`（此前仅 `data`）
- `--param` 类型强制后缀 `k:s=k:n=k:b=`；前导零纯数字（`007`）保持字符串；
  `--help` 写明推断规则
- `--params-file` 数值走 `is_finite` 防护（`1e999` → inf 拒绝），与 `--param` 对齐
- 全局配置路径遵循平台约定（Windows `%APPDATA%\nctool`、Unix `XDG_CONFIG_HOME`），
  兼容回退 `~/.config`；项目配置从 CWD **向上递归**查找
- `config show` 展示全局/项目配置来源路径；配置单次加载并缓存于 `Ctx`
  （`machine list`/`resolve_machine`/`config show` 不再重复读盘）
- `machine list` JSON 增加 `builtin` 标记（text 通道原有"自定义"标注对齐）
- `render` JSON 成功输出附 `warnings` 数组
- `templates show`/`inspect` 模板解析优先级与 `render`/`validate` 统一（注册表名 → 文件路径）
- stdout 断管道（`| head`）不再 panic；`templates new a.j2` 不再生成 `a.j2.j2`；
  `--dir`/`--param` 帮助文案对齐实际行为
- CI 增加 `windows-latest`（此前仅 ubuntu，平台特有路径/目录行为无覆盖）

### 测试

- workspace 总计 **279 项**（2026-09-03 实测，全绿）：nctool-tpl 104 单元 + 18 集成 + 1 文档、
  nctool-core 84 单元 + 10 集成、nctool-cli 23 单元 + 39 集成。
  相对 nctool-cli 0.2.0 基线 211 项 +68（阶段 A 新增：core +4 单元 +1 集成、
  cli +1 集成——合计 +6，余为第三轮审查补测）

---

## [未发布]

### Changed（阶段 A3 — 1.0 API 冻结清单）

**自 `nctool-tpl 0.3.2` / `nctool-core 0.2.1` 起，以下公共 API 面在 1.0.0 前不再
发生破坏性变更**；新增能力走 `additive`：只加不删、不改签名、不改语义。确需
破坏性变更时须先拆出预发布版本并在此节登记。

- **nctool-tpl**：`parse` / `Ast` / `Variable` / `extract_variables` /
  `extract_undeclared` / `extract_template_refs` / `Renderer`
  （`new` / `with_lenient` / `with_strict` / `is_lenient` / `render` / `add_template` /
  `set_path_loader` / `render_template`）/ `TplError`（`#[non_exhaustive]`，变体集冻结）/
  `Value` 再导出 / NC 过滤器（`nc_fixed` / `nc_strip` / `nc_pad`）与数学过滤器
- **nctool-core**：`model`（`ParamValue` / `ParamKind` / `ParamSpec` / `ParameterSet` /
  `MachineConfig`）、`validate`（`spec` / `validate_template` / `validate_with_vars` /
  `ValidationReport` / `ValidationIssue` / `ValidationLevel` / `IssueKind`）、
  `registry`（`TemplateCategory` / `TemplateSource` / `TemplateEntry` /
  `TemplateRegistry` / `RegistryError`）、`machine`（`MachinePreset` / `MachineId` /
  `KNOWN_CONFIG_KEYS` / `validate_config_keys`）、`pipeline`（`GCodeGenerator` /
  `GenerationOptions` / `OutputFormat` / `PipelineError`）

**1.0 前候选破坏性变更（未决，不进 0.3.2/0.2.1）**：

- `safe_move` / `drill_cycle` 的参数规格上界可能按机床动态化（PROCESS_CHECKLIST F1）
- 删除 `AST` 内部结构字段的二次暴露（如有）

### Changed（阶段 A5 — 文档与基线）

- 新增 [docs/PROCESS_CHECKLIST.md](docs/PROCESS_CHECKLIST.md)：工艺核对清单
  （逐行核对结论 + F1–F5 发现项 + 外部评审待办）
- README 顶部新增定位与风险声明（未经工艺验证，投产前必须自行核对）
- `docs/ROADMAP.md` / `docs/DEV_PLAN_CLI_UI.md` 数据校准

---

## [nctool-cli 0.2.0] - 2026-09-01

"阶段 1 CLI 核心能力"：脚本化生成 G-code 全流程可用；同时承载阶段 0 的命令树骨架与工程基线。

### Added

- **完整命令树**：`templates list/show/new`、`inspect`、`validate`、`render/generate`、`machine list/show`、`config init/show`、`completion`（`ui`/`part` 为阶段 2/4 占位）
- **参数输入**：`--param k=v`（类型自动推断：数值/字符串/布尔）+ `--params-file`（JSON 对象，显式 `--param` 覆盖文件）
- **配置层叠**：全局 `~/.config/nctool/config.toml` + 项目 `./nctool.toml`，项目覆盖全局（模板目录/默认机床/自定义机床表）
- **统一错误输出**：`--format text|json` 双通道；JSON 输出结构化错误对象 `{ok, error:{kind, message}}`
- **宽松模式**：`render --lenient` 未定义变量渲染为空（缺失参数不阻断）
- **模板脚手架**：`templates new` 生成带参数规格注释的骨架
- **golden 测试**：CLI 渲染输出与 `nctool-core` 管线逐字节一致

### Changed

- 新增 `cli/` crate 并接入 workspace（`members = [".", "core", "cli"]`），binary 名 `nctool`
- `release.yml` 增加 `nctool-cli-v*` tag 发布通道（对齐 tpl/core 命名约定）

### 测试

- nctool-cli：19 单元 + 29 集成（含 golden / 退出码 / JSON / 脚手架 / 配置层叠），workspace 总计 211 项（本发布基线；后续版本见 [nctool-cli 0.2.1]）

---

## [nctool-cli 0.1.0] - 2026-09-01

"阶段 0 脚手架与命令框架"：clap 命令树骨架 + 全局选项 + 工程基线。

### Added

- 新增 `cli/` crate（package `nctool-cli`，binary `nctool`），依赖 `nctool-core` + `nctool-tpl`
- clap derive 命令树；全局选项 `--machine` / `--template-dir` / `--format` / `--verbose`
- 统一错误类型 `CliError`（text/JSON 双输出通道）
- 参数解析单元测试（类型推断 / params-file / 覆盖优先级）
- CI 五道门（fmt / clippy / test / doc / cargo audit）经 workspace 级命令自动覆盖新 crate

---

## [nctool-core 0.2.0] - 2026-08-31

"第二轮全面代码审查修复"：补齐错误定位与 NC 输出健壮性；模板作用域语义修复见 [nctool-tpl 0.3.1]。

### Added

- **`GenerationOptions::ascii_only`**：开启后 G-code 输出中的非 ASCII 字符替换为 `?`
  （含头部注释与模板名）；仅对 `OutputFormat::Gcode` 生效，`Text` 始终原样

### Fixed

- **machine 元信息注入**：`machine.id` / `vendor` / `model` 现可被模板直接引用
  （原先只注入 `config` 键值表）；config 同名键以元信息优先
- **校验错误类型**：`TemplateRegistry::validate` 错误从 `String` 改为 `RegistryError`
  （新增 `NotFound` 变体）；管线中校验失败不再误映射为 `PipelineError::Render`
- **头部注释 ASCII 化**：管线头部注释改为英文 ASCII 文本（许多 CNC 控制器对非 ASCII 敏感）
- **Memory 模板去重存储**：`TemplateSource::Memory` 不再复制一份源码（源码以
  `TemplateEntry::source_text` 为单一权威来源）

### Changed（破坏性，0.x 阶段）

- `TemplateSource::Memory` 从 `Memory(String)` 改为无载荷变体（源码统一读 `source_text`）
- `MachinePreset::to_config` 移除（与 `config()` 重复）
- `TemplateRegistry::validate` 返回 `Result<ValidationReport, RegistryError>`
- core 依赖 `minijinja` 去除多余的 `unstable_machinery` 等 feature 声明（由 nctool-tpl 启用并合并）
- 依赖升级：`nctool-tpl` 0.3.0 → 0.3.1

### 其他

- 补全发布元数据：`nctool-core` 的 `repository` 指向 GitHub 仓库

### 测试

- nctool-core 52 单元 + 8 集成，workspace 总计 163 项

---

## [nctool-tpl 0.3.1] - 2026-08-31

"第二轮全面代码审查修复"：修正变量提取的作用域语义，补齐错误定位与 NC 输出健壮性。

### Fixed

- **set/with 自引用漏报**：`{% set total = total + price %}` 中右侧引用原被误判为模板局部，
  导致必选参数校验漏报、严格渲染才报错。现 RHS 先在外层作用域求值，再绑定目标
- **作用域泄漏**：`for` / `with` / `macro` 现在创建独立作用域（与 Jinja2 语义一致），
  循环/块内 `set` 的名字不再泄漏到块外；`if` 仍不创建作用域
- **UndefinedVariable 恢复变量名**：debug feature 下错误携带字节范围，尽力从源码恢复缺失变量名
  （`{{ missing }}` → `"missing"`）；属性链场景无法判定缺失位置时留空（宁缺毋错）
- **nc_pad 拒绝负数**：负输入会拼出 `O-001` 这类非法 G-code，现报渲染错误

### 其他

- 新增 `.gitattributes`（统一 LF），消除 Windows 下 autocrlf 幻影改动
- `set_path_loader` 文档补充路径穿越安全性说明（模板视为可信输入）
- 补全发布元数据：`nctool-tpl` 的 `repository` 指向 GitHub 仓库

### 测试

- nctool-tpl 85 单元 + 17 集成 + 1 文档（workspace 总计 163 项，含 nctool-core）

---

## [0.1.0] nctool-core - 2026-08-31

"阶段 1 核心库"：workspace 新增子 crate `core/`（nctool-core v0.1.0），在 nctool-tpl 之上提供面向 G-code 生成的生产级能力，并通过阶段性代码审查。

### Added

- **workspace 化**：根 Cargo.toml 改为 workspace（`members = [".", "core"]`，resolver=2）
- **数据模型**（`model`）：`ParamValue`（Number/String/Bool，serde 带标签）、`ParamKind`、`ParamSpec`、`ParameterSet`（BTreeMap + fluent API）、`MachineConfig`、`Tool`、`Operation`、`Part`
- **参数校验引擎**（`validate`）：`validate_template` / `validate_with_vars` 渲染前校验（必选齐全 / 类型匹配 / 默认值兜底 / 冗余警告），`ValidationReport` 结构化错误，系统变量（默认 `machine`）校验豁免
- **模板注册表**（`registry`）：内存 / 文件 / 内置模板统一管理，内置 5 个 G-code 子程序（program_header / program_footer / tool_change / safe_move / drill_cycle）
- **机床配置**（`machine`）：`MachinePreset`（Generic / WflM65 / IndexMs40）内建配置，模板经 `{{ machine.xxx }}` 引用实现一套模板适配多机床
- **生成管线**（`pipeline`）：`GCodeGenerator.generate`（校验 → 规格默认值兜底 → 上下文合并 → 渲染 → 后处理），`PipelineError` 三变体
- **根导出补全**：`lib.rs` 增加 `validate_template` / `validate_with_vars` / `spec` 根路径导出

### Fixed（阶段性代码审查）

- **非有限数防护**：数值参数为 NaN/Inf 时校验阶段拒绝（原实现会经 JSON 中间层**静默转 0** 污染坐标）
- **上下文构建**：改为 minijinja 原生构造，移除 serde_json 中间层，杜绝非有限数被篡改
- **Text 格式契约**：Text 格式仅渲染、保留原始行（原实现会对所有行 trim，破坏空白）
- **行号逻辑**：`max_line_number` 概念清晰化（原实现将"行数上限"与"行号数值上限"混用）
- **校验去重**：`validate_template` / `validate_with_vars` 提取共享核心 `check_vars`
- **错误映射**：`RegistryError` 新增 `Compile` 变体（原将模板编译失败误映射为 `Io`）；内置模板注册失败不再静默吞错（`expect`）
- **占位测试清理**：移除纯为消告警的 `param_kind_reuse_in_tests`

### 测试

- nctool-core：49 → 55 项（47 单元 + 8 集成），workspace 总计 146 项
- 新增覆盖：NaN/Inf 校验拒绝（含 `validate_with_vars` 路径）、有限数通过、NaN 管线拦截、Text 格式保留空白、行号到上限后停止

---

## [0.3.0] - 2026-08-31

"NC 数值格式化"版本：新增 G-code 专用数值格式化过滤器与严格/宽松渲染模式切换。

### Added

- **NC 数值格式化过滤器**：`nc_fixed(N)`（固定小数位）、`nc_strip`（去尾零）、`nc_pad(N)`（前导零填充），用于 G-code 坐标值、程序号、行号的格式化输出
- **严格/宽松模式切换**：`Renderer::with_lenient()` / `with_strict()` / `is_lenient()`。默认严格（未定义变量报错），宽松模式渲染未定义变量为空字符串；`extract_undeclared` 的必选判定不受模式影响
- **非有限数防护**：所有 NC 过滤器对 NaN/Inf 输入报 [`TplError::Render`]，防止非法坐标写入 G-code
- **测试**：8 项 NC 过滤器测试 + 5 项模式切换测试，总测试 78 → 91 项

### 用法示例

```jinja
O{{ prog | nc_pad(4) }}
N{{ line | nc_pad(4) }} G1 X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }} F{{ feed | nc_strip }}
```

```rust
let r = Renderer::new().with_lenient(); // 宽松模式：未定义变量渲染为空
```

### 测试

- 从 78 项增至 91 项（73 单元 + 17 集成 + 1 文档）
- 新增覆盖：nc_fixed / nc_strip / nc_pad 的常规与异常输入；严格/宽松模式切换、模式与参数提取的独立性

### 性能基线（release，~260 字节 G-code 模板）

| 操作 | 耗时（中位数） | 吞吐 |
| --- | --- | --- |
| `parse` | 2.61 µs | 98 MiB/s |
| `extract_undeclared` | 1.97 µs | — |

"基座打磨"版本：多模板能力、错误细分、API 稳定化、工程化全覆盖。

### Added

- **多模板渲染**：`Renderer::add_template`（内存注册）、`Renderer::set_path_loader`（目录加载）、`Renderer::render_template`（渲染已注册模板），支持 `{% include %}` / `{% extends %}` / `{% import %}`
- **错误类型细分**：`TplError` 新增 `TemplateNotFound` / `UndefinedVariable` / `UnknownFilter` / `UnknownTest` 变体，上层可精准处理
- **可选/必选参数区分**：`Variable.optional` 字段，`default`/`d` 过滤器和 `is defined`/`is undefined` 测试标记的变量为可选
- **列号定位**：`TplError::Parse.col` 从恒为 1 的占位值改为真实列号（基于 minijinja `debug` feature 的字节范围换算）
- **性能基准**：`criterion` benchmark（parse / extract_undeclared / render 三场景），`cargo bench` 可运行
- **CI 流水线**：GitHub Actions（fmt / clippy / test / doc / cargo audit），push 和 PR 触发
- **CHANGELOG**：本文件

### Changed

- **[破坏性] API 收窄**：`Ast` 的 `name` / `source` 从 pub 字段改为私有字段 + 访问方法（`name()` / `source()`），减少未来破坏面
- **[破坏性] 错误枚举扩展**：`TplError` 标注 `#[non_exhaustive]`，未来新增变体不破坏下游；match 请保留通配分支
- **minijinja 版本锁定**：从 `^2.24.0` 收窄为 `~2.24.0`（允许补丁，锁定 minor），因依赖 `unstable_machinery` AST API
- **release 配置**：新增 `codegen-units = 1`（配合已有 LTO + strip）
- **文档**：所有公共 API 补全 doc comment，`cargo doc --no-deps` 零警告；新增 `#![warn(missing_docs)]` lint

### Fixed

- 列号定位：`TplError::Parse.col` 不再恒为 1，现在指向解析器停止位置的最佳近似
- 可选参数误报：有 `default()` 兜底的变量不再被列为必选未声明变量

### 测试

- 从 11 项增至 56 项（38 单元 + 17 集成 + 1 文档）
- 新增覆盖：多模板 include/extends/import、错误细分、边界 case（空模板/纯文本/注释/保留名/循环变量/宏参数/嵌套 default/filter 链/复杂表达式）、列号定位、可选/必选区分

### 性能基线（release，~260 字节 G-code 模板）

| 操作 | 耗时（中位数） | 吞吐 |
| --- | --- | --- |
| `parse` | 2.61 µs | 98 MiB/s |
| `extract_undeclared` | 1.97 µs | — |
| `render` | 5.32 µs | 48 MiB/s |

---

## [0.1.0] - 2026-08-30

初始版本：NCtool 模板解析核心。

### Added

- 核心 API：`parse` / `extract_variables` / `extract_undeclared` / `Renderer`
- 数学过滤器集：sin / cos / tan / asin / acos / atan / sqrt / exp / ln / log10 / pow / floor / ceil（含 NaN/Inf 有限性校验）
- Strict 未定义变量策略：缺失变量直接渲染失败
- 11 项测试
- README、MIT 许可、demo 示例、G-code 模板
