//! 属性测试：`extract_undeclared` 的**可选判定必须可靠**（架构评估 P1-2 的不变量）。
//!
//! 判为"可选"意味着"严格模式下不提供它也能渲染"。一旦判错（把必选当可选），
//! 用户会拿到一个渲染失败的模板、却查不出到底缺哪个参数——`extract.rs` 里
//! 修过的真实缺陷（`{% set x = x | default(v) %}` 惯用法被误判）正是这一类。
//!
//! **反方向不成立、也不该断言**：本项目刻意"宁多勿漏"——
//! `{% if a is defined %}{{ a }}{% endif %}` 中 a 缺省时模板其实能渲染，
//! 但仍被记为必选（保守策略）。故只断言单侧不变量。
//!
//! 用例由确定性伪随机生成器产出：无第三方依赖，失败可复现。

use std::collections::BTreeMap;

use nctool_tpl::{extract_undeclared, parse, Renderer, Value};

/// 线性同余生成器：确定性、零依赖，保证失败用例可复现（失败时打印的模板
/// 重新跑必然复现，不需要"再跑一次看看"）。
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[(self.next() as usize) % xs.len()]
    }
}

/// 片段库：只用 `{a}` / `{b}` 两个变量占位符与 `{m}` 宏名占位符，
/// 且都能在"所有变量都是数值 1.0"的上下文里渲染成功
/// （否则断言失败会指向生成器本身，而不是被测逻辑）。
const FRAGMENTS: &[&str] = &[
    // 无变量片段：真实模板里"零参数模板"是常见形态，样本必须覆盖到
    "G21 G90",
    "{{ 1 + 1 }}",
    "{{ {a} }}",
    "{{ {a} | default(1) }}",
    "{{ {a} + {b} }}",
    "{{ ({a} + {b}) | default(1) }}",
    "{{ {a} | nc_fixed(3) }}",
    "{{ {a} | default({b}) }}",
    "{% if {a} is defined %}{{ {a} }}{% endif %}",
    "{% if {a} | default(0) %}{{ {b} }}{% endif %}",
    "{% set {a} = {a} | default(1) %}{{ {a} }}",
    "{% set {a} = {b} + 1 %}{{ {a} }}",
    "{% with {a} = {b} + 1 %}{{ {a} }}{% endwith %}",
    "{% for i in [1, 2] %}{{ {a} }}{% endfor %}",
    "{% macro {m}() %}{{ {a} }}{% endmacro %}{{ {m}() }}",
    "{# 注释里的 {a} 不算引用 #}{{ {b} }}",
    "{% if {a} %}{{ {b} }}{% else %}{{ {a} }}{% endif %}",
];

const VAR_POOL: &[&str] = &["a", "b", "c"];
const CASES: usize = 300;

/// 生成 `count` 个模板：每个由 1–3 个随机片段拼接而成。
///
/// 宏名带片段序号：同一模板里出现两个宏片段时不能重名（重名是模板自身错误，
/// 与被测逻辑无关）。
fn generate(seed: u64, count: usize) -> Vec<String> {
    let mut rng = Rng::new(seed);
    (0..count)
        .map(|_| {
            let parts = 1 + (rng.next() % 3) as usize;
            (0..parts)
                .map(|part| {
                    let fragment = rng.pick(FRAGMENTS);
                    fragment
                        .replace("{a}", rng.pick(VAR_POOL))
                        .replace("{b}", rng.pick(VAR_POOL))
                        .replace("{m}", &format!("m{part}"))
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect()
}

#[test]
fn variables_marked_optional_can_really_be_omitted() {
    let renderer = Renderer::new();
    let mut no_undeclared = 0usize;
    let mut with_optional = 0usize;
    let mut with_required = 0usize;

    for (i, src) in generate(0x5EED_2026, CASES).into_iter().enumerate() {
        let ast = match parse(&src, "prop.j2") {
            Ok(ast) => ast,
            Err(e) => panic!("case {i}: 生成器产出了非法模板\n{src}\n{e}"),
        };
        let vars = extract_undeclared(&ast);
        if vars.is_empty() {
            no_undeclared += 1;
        }
        if vars.iter().any(|v| v.optional) {
            with_optional += 1;
        }
        if vars.iter().any(|v| !v.optional) {
            with_required += 1;
        }

        // 只提供被判为"必选"的变量；被判为可选的**一个都不给**。
        // 片段都是数值上下文，故一律给 1.0。
        let required: Vec<&str> = vars
            .iter()
            .filter(|v| !v.optional)
            .map(|v| v.name.as_str())
            .collect();
        let ctx: BTreeMap<&str, Value> = required
            .iter()
            .map(|name| (*name, Value::from(1.0)))
            .collect();

        let out = renderer.render(&src, "prop.j2", &Value::from_serialize(&ctx));
        assert!(
            out.is_ok(),
            "case {i}: 被判为可选的变量缺省后，严格模式渲染失败 —— 可选判定不可靠\n\
             模板:\n{src}\n已提供(必选): {required:?}\n错误: {}",
            out.unwrap_err()
        );
    }

    // 生成器退化保护：若片段库/随机源被改坏，让"全必选"或"全无变量"的用例
    // 悄悄占满样本，上面的断言会变得毫无信息量。三种形态都必须真实出现。
    assert!(no_undeclared > 0, "样本里应有不含未声明变量的模板");
    assert!(with_optional > 0, "样本里应有含可选变量的模板");
    assert!(with_required > 0, "样本里应有含必选变量的模板");
}
