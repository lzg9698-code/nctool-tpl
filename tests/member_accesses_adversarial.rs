//! 对抗性验证 `extract_member_accesses`（QA 独立构造）。

use nctool_tpl::{extract_member_accesses, parse};

/// 点访问与**常量字符串下标**都必须被收集。
#[test]
fn dot_and_const_subscript_collected() {
    let ast = parse(
        r#"{{ machine.coolant_on }} {{ machine["coolant_on"] }}"#,
        "m.j2",
    )
    .unwrap();
    assert_eq!(extract_member_accesses(&ast, "machine"), vec!["coolant_on"]);
}

/// 动态下标必须被**忽略**：既不误收集，也不 panic。
#[test]
fn dynamic_subscripts_ignored_not_misfired() {
    let cases = [
        r#"{{ machine[k] }}"#,
        r#"{{ machine["a" ~ b] }}"#,
        r#"{{ machine[f()] }}"#,
        r#"{{ machine[1] }}"#,
        r#"{{ machine[true] }}"#,
        r#"{{ machine[1 + 2] }}"#,
        r#"{{ machine[obj.field] }}"#,
    ];
    for src in cases {
        let ast = parse(src, "m.j2").unwrap();
        let keys = extract_member_accesses(&ast, "machine");
        assert!(keys.is_empty(), "{src} 不应收集任何键，实际 {keys:?}");
    }
}

/// 混合：常量下标 + 点访问 + 动态下标，只有前两者被收集。
#[test]
fn mixed_const_and_dynamic() {
    let ast = parse(r#"{{ machine["a"] ~ machine.b ~ machine[c] }}"#, "m.j2").unwrap();
    assert_eq!(extract_member_accesses(&ast, "machine"), vec!["a", "b"]);
}

/// 只认传入的根，不硬编码 `machine`。
#[test]
fn root_is_parameterized() {
    let ast = parse("{{ cfg.x }} {{ cfg[\"y\"] }} {{ machine.z }}", "m.j2").unwrap();
    assert_eq!(extract_member_accesses(&ast, "cfg"), vec!["x", "y"]);
    assert_eq!(extract_member_accesses(&ast, "machine"), vec!["z"]);
    assert!(extract_member_accesses(&ast, "nope").is_empty());
}

/// 穿透 for/if/macro 语句体，且按出现顺序去重。
#[test]
fn traverse_bodies_and_dedup() {
    let src = "{% for i in xs %}{{ machine.a }}{% endfor %}\
               {% if machine.b %}{{ machine.a }}{% endif %}\
               {% macro m() %}{{ machine.c }}{% endmacro %}";
    let ast = parse(src, "m.j2").unwrap();
    assert_eq!(
        extract_member_accesses(&ast, "machine"),
        vec!["a", "b", "c"]
    );
}
