# Changelog

本项目遵循 [Keep a Changelog](https://keepachangelog.com/) 格式和 [语义化版本](https://semver.org/)。

## 版本策略

- **0.x 阶段**：API 仍在演进，`minor` 版本升级可能包含破坏性变更（如本版本的 API 收窄、错误细分）。
- **1.0.0**：公共 API 稳定后发布，此后严格遵循语义化版本（`major` = 破坏性变更，`minor` = 向后兼容新功能，`patch` = 向后兼容修复）。
- 破坏性变更均在 `Changed` 节明确标注，并在 README 中说明迁移方式。

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
