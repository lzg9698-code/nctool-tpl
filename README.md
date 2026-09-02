# nctool-tpl

NCtool 模板解析核心：基于 [minijinja](https://github.com/mitsuhiko/minijinja) 的 Jinja2 模板解析 + 变量提取 + 渲染，面向数控加工 G-code 模板场景。

- 近乎零依赖（仅 minijinja 一个 crate），Jinja2 作者本人维护的高性能引擎
- 对标 Python `jinja2.meta`：能从模板中提取「引用的全部变量」与「需要外部上下文提供的未声明变量」
- 内置一组数学过滤器（`f64` 标准库实现），供 G-code 计算使用

## 功能

| API | 说明 |
| --- | --- |
| `parse(source, name)` | 语法检查并生成 AST（带行列定位） |
| `extract_variables(&ast)` | 提取模板中**引用过**的全部变量名（含模板内部声明的） |
| `extract_undeclared(&ast)` | 提取引用但**未在模板内声明**的变量 —— 即渲染时必须由外部提供的参数；并区分**可选 / 必选** |
| `Renderer` | 用上下文渲染出最终文本（G-code），内置数学过滤器集；支持多模板（`include`/`extends`/`import`） |

## NC 数值格式化过滤器

G-code 对数值格式敏感，`Renderer` 内置三个专用过滤器：

| 过滤器 | 用法 | 输入 | 输出 | 用途 |
| --- | --- | --- | --- | --- |
| `nc_fixed(N)` | `{{ x \| nc_fixed(3) }}` | `21.0` | `21.000` | 固定小数位的坐标值 |
| `nc_strip` | `{{ x \| nc_strip }}` | `21.0` | `21` | 去尾零，避免 `X21.0` |
| `nc_pad(N)` | `{{ n \| nc_pad(4) }}` | `1` | `0001` | 程序号/行号前导零 |

```jinja
O{{ prog | nc_pad(4) }}
N{{ line | nc_pad(4) }} G1 X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }} F{{ feed | nc_strip }}
```

渲染结果（prog=1, line=10, x=21.0, y=15.5, feed=0.150）：
```
O0001
N0010 G1 X21.000 Y15.500 F0.15
```

所有 NC 过滤器对非有限数（NaN/Inf）报错，防止非法数值写入 G-code。该防线覆盖本库注册的过滤器；裸 `{{ x }}` 输出或 minijinja 内建操作（如 `round`、算术）产生的 NaN/Inf 不经此校验，请先经上层参数校验（`nctool-core`）保证参数值有限。

## 严格 / 宽松模式

`Renderer` 默认**严格模式**：引用未定义变量直接渲染失败，避免静默输出不完整 G-code。需要宽松渲染时用 `with_lenient()` 切换：

```rust
// 严格模式（默认）：未定义变量报错
let r = Renderer::new();
r.render("X{{ x }}", "t.j2", &minijinja::context!{})?;  // Err(UndefinedVariable)

// 宽松模式：未定义变量渲染为空字符串
let r = Renderer::new().with_lenient();
r.render("X{{ x }}", "t.j2", &minijinja::context!{})?;  // Ok("X")
```

典型流程：`extract_undeclared` 先校验必选参数是否齐全，校验通过后再用严格模式渲染（保证输出完整）；宽松模式适合「参数可缺省、缺失即留空」的柔性模板。`is_lenient()` 可查询当前模式。

## 性能基线

基于 criterion benchmark（`cargo bench`），中等复杂度 G-code 模板（~260 字节，含 set/default、数学过滤器、for 循环、if 条件）：

| 操作 | 耗时（中位数） | 吞吐 |
| --- | --- | --- |
| `parse` | 2.61 µs | 98 MiB/s |
| `extract_undeclared` | 1.97 µs | — |
| `render` | 5.32 µs | 48 MiB/s |

测试环境：release 构建（LTO + strip + codegen-units=1）。重复渲染相同模板时，建议用 `add_template` + `render_template`（minijinja 会缓存编译结果），避免每次重新编译。

## 错误类型

`TplError` 已细分，上层可精准处理（`#[non_exhaustive]`，match 请保留通配分支）：

| 变体 | 触发场景 | 关键字段 |
| --- | --- | --- |
| `Parse` | 模板语法错误 | `line`, `col`（真实行列定位） |
| `TemplateNotFound` | `include`/`extends`/`get_template` 引用了不存在的模板 | `template` |
| `UndefinedVariable` | Strict 模式下引用了未定义变量 | —（minijinja 不携带变量名，可结合模板源码定位） |
| `UnknownFilter` | 使用了未注册的过滤器 | `filter` |
| `UnknownTest` | 使用了未注册的测试 | `test` |
| `Render` | 其他渲染错误（无效操作、参数错误等兜底） | — |

## 多模板渲染

`Renderer` 支持模板间引用，两种注册方式：

```rust
use nctool_tpl::Renderer;

let mut r = Renderer::new();

// 方式一：内存注册（owned 字符串，无生命周期约束）
r.add_template("header.j2", "O{{ prog }} ({{ name }})").unwrap();
r.add_template("main.j2", "{% include \"header.j2\" %}\nG1 X{{ diameter / 2 }}").unwrap();

// 方式二：从文件系统目录动态加载（按需加载并缓存）
// r.set_path_loader("templates/");

let ctx = minijinja::context! { prog => 1000, name => "DEMO", diameter => 42.0 };
let out = r.render_template("main.j2", &ctx).unwrap();
// out = "O1000 (DEMO)\nG1 X21.0"
```

`{% include %}` / `{% extends %}` / `{% import %}` 均能正确解析到已注册或目录中的模板。

每个返回的 `Variable` 都带有 `optional: bool` 字段：`true` 表示该变量的**全部引用**都处于「兜底上下文」（作为 `default`/`d` 过滤器或 `is defined`/`is undefined` 测试的**直接裸变量操作数**）——对 `extract_undeclared` 而言即**可选参数**（缺失时模板仍可安全渲染），`false` 为**必选参数**。详见下方[可选 / 必选判定规则](#可选--必选判定规则)。

## 快速开始

```rust
use nctool_tpl::{parse, extract_undeclared, Renderer, Variable};

let source = r#"{% set feed = 0.15 %}G1 X{{ diameter / 2 }} F{{ feed }}"#;
let ast = parse(source, "demo.j2").unwrap();

// 未声明变量 = 需要外部上下文提供的参数
let undeclared: Vec<Variable> = extract_undeclared(&ast);
assert_eq!(undeclared.len(), 1);
assert_eq!(undeclared[0].name, "diameter");
assert!(!undeclared[0].optional); // 必选

// 可选参数：有 default 兜底 / defined 检查
let src = "G1 F{{ feed | default(0.15) }} {% if coolant is defined %}M8{% endif %}";
let vars: Vec<Variable> = extract_undeclared(&parse(src, "o.j2").unwrap());
assert!(vars.iter().all(|v| v.optional)); // feed / coolant 均为可选

// 渲染（Strict 模式：缺失变量直接报错，不静默输出不完整 G-code）
let renderer = Renderer::new();
let ctx = minijinja::context! { diameter => 42.0 };
let out = renderer.render(source, "demo.j2", &ctx).unwrap();
assert_eq!(out, "G1 X21.0 F0.15");
```

## 命令行工具 nctool（nctool-cli）

工作区新增 `cli/` crate，提供二进制 `nctool`，覆盖模板浏览、变量提取、参数校验与 G-code 生成全流程（基于 `nctool-core` 管线，golden 测试保证输出逐字节一致）：

```bash
# 浏览内置模板
nctool templates list

# 提取模板必选/可选参数（含行列定位）
nctool inspect drill_cycle

# 参数校验（缺失必选 → 退出码 1 + 结构化报告）
nctool validate drill_cycle --param x=21 --param y=15 --param depth=-10 --param feed=100

# 生成 G-code：行号 + 头部注释 + 写文件
nctool render drill_cycle --param x=21 --param y=15 --param depth=-10 --param feed=100 \
    --line-numbers --header --out demo.nc

# 参数文件（JSON）批量输入；显式 --param 覆盖文件值
nctool render my_op.j2 --params-file params.json

# 机床配置查看 / 配置初始化 / shell 补全
nctool machine show wfl_m65
nctool config init
nctool completion bash

# 机器可读输出（--format json）
nctool render drill_cycle --param x=21 --param y=15 --param depth=-10 --param feed=100 --format json
```

运行方式：开发 `cargo run -p nctool-cli -- <命令>`；安装 `cargo install --path cli` 后直接使用 `nctool`。

## 可选 / 必选判定规则

- **可选**：变量的**全部**引用都是 `x | default(默认值)`（别名 `d`）或 `x is defined` / `x is undefined` 的**直接裸变量操作数**。
- **必选**：变量在任意非兜底位置被引用（如 `{{ x }}`、`{{ x / 2 }}`、过滤器/函数参数等），或既有兜底引用又有非兜底引用。

**兜底不向下传播**——这是最容易踩的坑。minijinja 会**先求值操作数、再套用过滤器/测试**，
因此只有裸变量能被安全兜底；操作数是运算、属性或下标时，undefined 参与求值即直接报错，
`default` / `defined` 根本来不及生效：

| 模板 | 判定 | 原因 |
| --- | --- | --- |
| `{{ x \| default(1) }}` | `x` **可选** | 操作数即裸变量，undefined 被兜底 |
| `{{ x \| default(1) \| nc_fixed(3) }}` | `x` **可选** | 兜底后串接过滤器仍安全 |
| `{% if x is defined %}` | `x` **可选** | 同上 |
| `{{ (a+b) \| default(1) }}` | `a`、`b` **必选** | 先算 `a+b`，undefined 参与运算即报错 |
| `{{ a.b \| default(1) }}` | `a` **必选** | 先对 undefined 的 `a` 取属性，报错 |
| `{% if a.b is defined %}` | `a` **必选** | 同上 |

若把后三类误判为可选，上层校验会放行、严格模式渲染却失败，产出**不完整的 G-code**。
因此判定策略是**宁多勿漏**：有疑问即记为必选。
- 模板内部 `set`/`for`/`with`/宏参数等声明的局部变量不进未声明集合，不受此规则影响。

## 数学过滤器

全部基于 Rust 标准库 `f64`，零额外依赖：

`sin` `cos` `tan` `asin` `acos` `atan` `sqrt` `exp` `ln` `log10` `pow` `floor` `ceil`

**有限性校验**：所有数学过滤器对结果做 `is_finite()` 检查，一旦产生 `NaN`/`Inf`（如 `sqrt(-1)`、`ln(0)`），渲染立即失败并报错，避免非法坐标静默写入 G-code。同 NC 过滤器一样，该防线仅覆盖本库注册的过滤器；裸输出与内建操作不在保护范围。

## 示例与测试

```bash
# 运行可执行示例（解析 + 变量提取 + 渲染 templates/demo_gcode.j2）
cargo run --example demo

# 运行全部测试（单元 + 集成 + 文档）
cargo test

# 静态检查与格式
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## 定位精度与判定边界

- **解析错误列号**：`TplError::Parse.col` 取自 minijinja 错误携带的字节范围（需启用 `debug` feature）换算而来，指向**解析器停止处**的 token，是对错误位置的最佳近似（多数场景精确，个别场景如"未闭合块"只精确到行）。无法取得字节范围时回退为 `col = 1`。
- **可选 / 必选判定边界**：只把 `default`/`d` 过滤器与 `defined`/`undefined` 测试的**直接裸变量操作数**记为可选，兜底**不向下传播**到子树（`(a+b) | default(1)`、`a.b | default(1)`、`a.b is defined` 中的变量均记为必选）；`defined` 保护块**内部**的引用仍记为必选（保守策略，宁多勿漏）；`default(参数)` 的默认值表达式里的变量仍记为必选（它必须存在才能求值默认值）。

## License

MIT，见 [LICENSE](LICENSE)。
