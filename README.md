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

所有 NC 过滤器对非有限数（NaN/Inf）报错，防止非法数值写入 G-code。

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

每个返回的 `Variable` 都带有 `optional: bool` 字段：`true` 表示该变量的**全部引用**都处于「兜底上下文」（`default`/`d` 过滤器或 `is defined`/`is undefined` 测试）——对 `extract_undeclared` 而言即**可选参数**（缺失时模板仍可安全渲染），`false` 为**必选参数**。

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

## 可选 / 必选判定规则

- **可选**：变量只出现在 `x | default(默认值)`（别名 `d`）或 `x is defined` / `x is undefined` 的**操作数**位置。
- **必选**：变量在任意非兜底位置被引用（如 `{{ x }}`、`{{ x / 2 }}`、过滤器/函数参数等），或既有兜底引用又有非兜底引用。
- 模板内部 `set`/`for`/`with`/宏参数等声明的局部变量不进未声明集合，不受此规则影响。

## 数学过滤器

全部基于 Rust 标准库 `f64`，零额外依赖：

`sin` `cos` `tan` `asin` `acos` `atan` `sqrt` `exp` `ln` `log10` `pow` `floor` `ceil`

**有限性校验**：所有数学过滤器对结果做 `is_finite()` 检查，一旦产生 `NaN`/`Inf`（如 `sqrt(-1)`、`ln(0)`），渲染立即失败并报错，避免非法坐标静默写入 G-code。

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
- **可选 / 必选判定边界**：只把 `default`/`d` 过滤器与 `defined`/`undefined` 测试的**直接操作数**记为可选；`defined` 保护块**内部**的引用仍记为必选（保守策略，宁多勿漏）；`default(参数)` 的默认值表达式里的变量仍记为必选（它必须存在才能求值默认值）。

## License

MIT，见 [LICENSE](LICENSE)。
