# nctool-tpl

NCtool 模板解析核心：基于 [minijinja](https://github.com/mitsuhiko/minijinja) 的 Jinja2 模板解析 + 变量提取 + 渲染，面向数控加工 G-code 模板场景。

- 近乎零依赖（仅 minijinja 一个 crate），Jinja2 作者本人维护的高性能引擎
- 对标 Python `jinja2.meta`：能从模板中提取「引用的全部变量」与「需要外部上下文提供的未声明变量」
- 内置一组数学过滤器（`f64` 标准库实现），供 G-code 计算使用

## 功能

| API | 说明 |
| --- | --- |
| `parse(source, name)` | 语法检查并生成 AST（带行号定位） |
| `extract_variables(&ast)` | 提取模板中**引用过**的全部变量名（含模板内部声明的） |
| `extract_undeclared(&ast)` | 提取引用但**未在模板内声明**的变量 —— 即渲染时必须由外部提供的参数 |
| `Renderer` | 用上下文渲染出最终文本（G-code），内置数学过滤器集 |

## 快速开始

```rust
use nctool_tpl::{parse, extract_undeclared, Renderer};

let source = r#"{% set feed = 0.15 %}G1 X{{ diameter / 2 }} F{{ feed }}"#;
let ast = parse(source, "demo.j2").unwrap();

// 未声明变量 = 需要外部上下文提供的参数
let undeclared: Vec<String> = extract_undeclared(&ast)
    .into_iter().map(|v| v.name).collect();
assert_eq!(undeclared, vec!["diameter".to_string()]);

// 渲染（Strict 模式：缺失变量直接报错，不静默输出不完整 G-code）
let renderer = Renderer::new();
let ctx = minijinja::context! { diameter => 42.0 };
let out = renderer.render(source, "demo.j2", &ctx).unwrap();
assert_eq!(out, "G1 X21.0 F0.15");
```

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

## 已知限制

- **错误定位**：`TplError::Parse.col` 为占位值（恒为 1），因为 minijinja 的错误对象仅暴露行号、未暴露列号；列号细节通常包含在 `message` 文本中。
- **可选参数不区分**：`{% set feed = default_feed | default(0.15) %}` 中的 `default_feed` 虽然有默认值兜底，仍会被 `extract_undeclared` 列为「未声明变量」。工具无法区分「可选」与「必选」参数，该行为与 `jinja2.meta` 一致。

## License

MIT，见 [LICENSE](LICENSE)。
