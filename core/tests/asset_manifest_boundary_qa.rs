//! QA（严过关）**第 2 轮独立复验**：`templates.yaml` 定点文本编辑的
//! 条目边界（P1）与"可解析性闸门"（P2-1）。
//!
//! 立场：**证伪**工程师的第 3 轮修复结论，而非复述。断言一律写"正确行为"，
//! 失败即说明源码仍有缺陷。与工程师用例的区别：本文件独立构造输入，并额外
//! 用**变异判别**证明闸门输入不是空洞的（见文末 `qa_sibling_input_*`）。

use nctool_core::asset::{manifest_append_entry, manifest_entry_body, manifest_is_parseable};

// ===========================================================================
// P1：条目正文边界（`manifest_entry_body`）
// ===========================================================================

/// 同级缩进的**相邻兄弟条目**（无空行分隔）→ 必须停止，不得吞并。
/// 这是 P1 的核心判别输入（`indent <= key_indent` 才停）。
#[test]
fn qa_entry_body_stops_at_same_indent_sibling() {
    let text = "templates:\n  \"a.j2\":\n    name: \"A\"\n  \"b.j2\":\n    name: \"B\"\n";
    let body = manifest_entry_body(text, "a.j2").expect("应找到 a.j2");
    assert_eq!(
        body,
        vec!["  name: \"A\"".to_string()],
        "同级兄弟条目被吞并: {body:?}"
    );
}

/// 更浅缩进 → 停止（不得当作字段吞入）。
#[test]
fn qa_entry_body_stops_at_shallower_indent() {
    let text = "templates:\n    \"a.j2\":\n  name: \"A\"\n";
    let body = manifest_entry_body(text, "a.j2").unwrap();
    assert!(body.is_empty(), "更浅缩进不得被吞入: {body:?}");
}

/// 更深缩进 → 继续（作为字段，保留相对缩进）。
#[test]
fn qa_entry_body_continues_at_deeper_indent() {
    let text = "templates:\n  \"a.j2\":\n    name: \"A\"\n      nested: 1\n";
    let body = manifest_entry_body(text, "a.j2").unwrap();
    assert_eq!(
        body,
        vec!["  name: \"A\"".to_string(), "    nested: 1".to_string()]
    );
}

/// 空行 → 停止。
#[test]
fn qa_entry_body_stops_at_blank_line() {
    let text = "templates:\n  \"a.j2\":\n    name: \"A\"\n\n  \"b.j2\":\n    name: \"B\"\n";
    let body = manifest_entry_body(text, "a.j2").unwrap();
    assert_eq!(body, vec!["  name: \"A\"".to_string()]);
}

/// 顶格的下一个顶层键 → 停止。
#[test]
fn qa_entry_body_stops_at_top_level_key() {
    let text = "templates:\n  \"a.j2\":\n    name: \"A\"\nvariables:\n  foo: 1\n";
    let body = manifest_entry_body(text, "a.j2").unwrap();
    assert_eq!(body, vec!["  name: \"A\"".to_string()]);
}

/// 文件末尾**无换行** → 正常收集到末尾。
#[test]
fn qa_entry_body_at_eof_without_newline() {
    let text = "templates:\n  \"a.j2\":\n    name: \"A\"";
    let body = manifest_entry_body(text, "a.j2").unwrap();
    assert_eq!(body, vec!["  name: \"A\"".to_string()]);
}

/// 键行之后无任何行 → 空正文（不 panic）。
#[test]
fn qa_entry_body_empty_when_key_is_last() {
    let text = "templates:\n  \"a.j2\":\n";
    let body = manifest_entry_body(text, "a.j2").unwrap();
    assert!(body.is_empty(), "{body:?}");
}

/// P1 **端到端**：相邻兄弟条目 → 复制 a 的正文并追加新条目 →
/// 结果仍可解析、无重复键、兄弟 b 的内容未被吞。
#[test]
fn qa_append_after_adjacent_siblings_stays_valid() {
    let text = "templates:\n  \"a.j2\":\n    name: \"A\"\n  \"b.j2\":\n    name: \"B\"\n";
    let body = manifest_entry_body(text, "a.j2").unwrap();
    let out = manifest_append_entry(text, "anew.j2", &body).unwrap();

    assert!(manifest_is_parseable(&out), "追加后应仍是合法 YAML:\n{out}");
    assert_eq!(out.matches("\"a.j2\":").count(), 1, "a 重复:\n{out}");
    assert_eq!(out.matches("\"b.j2\":").count(), 1, "b 重复:\n{out}");
    assert_eq!(out.matches("\"anew.j2\":").count(), 1, "anew 缺失:\n{out}");
    assert!(out.contains("    name: \"B\""), "兄弟 b 的内容被吞:\n{out}");
}

/// 反例：a 的正文若**含** b 的键行（即被吞），追加后必判非法 —— 这是 P1 的
/// 失效形态，用 `manifest_is_parseable` 反向确认"该闸门确实能识别损坏"。
#[test]
fn qa_parseable_gate_rejects_swallowed_sibling_shape() {
    let good = "templates:\n  \"a.j2\":\n    name: \"A\"\n  \"b.j2\":\n    name: \"B\"\n";
    assert!(manifest_is_parseable(good));
    // 模拟"吞并"后追加造成的重复键形态
    let swallowed =
        "templates:\n  \"a.j2\":\n    name: \"A\"\n  \"b.j2\":\n    name: \"B\"\n  \"b.j2\":\n    name: \"B\"\n";
    assert!(
        !manifest_is_parseable(swallowed),
        "重复键形态必须被判为不可解析"
    );
}

// ===========================================================================
// P2-1：可解析性闸门（`manifest_is_parseable`）
// ===========================================================================

#[test]
fn qa_parseable_accepts_wellformed() {
    assert!(manifest_is_parseable(
        "templates:\n  \"a.j2\":\n    name: \"A\"\n"
    ));
    assert!(manifest_is_parseable("templates:\n"));
    assert!(manifest_is_parseable("# 只有注释\n"));
    assert!(manifest_is_parseable("")); // 空文件 = 空文档（非损坏）
    assert!(manifest_is_parseable(
        "templates:\r\n  \"a.j2\":\r\n    name: \"A\"\r\n"
    ));
}

#[test]
fn qa_parseable_rejects_corrupt() {
    // 未闭合 flow 序列
    assert!(!manifest_is_parseable(
        "templates:\n  \"a.j2\":\n    name: \"A\"\n  bad: [\n"
    ));
    // 重复键
    assert!(!manifest_is_parseable(
        "templates:\n  \"a.j2\":\n    name: \"A\"\n  \"a.j2\":\n    name: \"B\"\n"
    ));
    // tab 缩进（YAML 禁止用 tab 缩进）
    assert!(!manifest_is_parseable(
        "templates:\n\t\"a.j2\":\n\t\tname: \"A\"\n"
    ));
}

/// BOM / CRLF / 空文件 / tab 缩进的边界判定（实测后固化为闸门）。
///
/// - BOM 前缀的**合法**清单必须判为可解析（否则 Windows 上带 BOM 的清单会被
///   "过度降级"、条目永远写不进去 —— 功能静默失效）；
/// - CRLF 合法；空文件是空文档（非损坏）；tab 缩进 YAML 规范禁止，判为不可解析。
#[test]
fn qa_parseable_handles_bom_crlf_empty_tab() {
    let bom = "\u{FEFF}templates:\n  \"a.j2\":\n    name: \"A\"\n";
    assert!(
        manifest_is_parseable(bom),
        "带 BOM 的合法清单不得被误判为损坏（否则过度降级）"
    );
    assert!(manifest_is_parseable(
        "templates:\r\n  \"a.j2\":\r\n    name: \"A\"\r\n"
    ));
    assert!(manifest_is_parseable(""));
    assert!(
        !manifest_is_parseable("templates:\n\t\"a.j2\":\n"),
        "tab 缩进违反 YAML 规范，应判为不可解析"
    );
}

// ===========================================================================
// 反空洞（变异判别）：证明工程师的两条新用例**是否真能区分对错实现**
// ===========================================================================

/// 重放 `manifest_entry_body` 的三种停止条件，用于变异判别。
/// `mode`: 0 = 正确（`<=`）；1 = 缺陷（`<`，P1 修复前的边界）；2 = 更早行为（仅空行/顶格停）。
fn body_variant(text: &str, key: &str, mode: u8) -> Vec<String> {
    fn indent_of(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }
    let lines: Vec<&str> = text.lines().collect();
    let key_line = |l: &str| -> bool {
        if l.trim_start().len() == l.len() {
            return false;
        }
        let t = l.trim();
        t == format!("\"{key}\":") || t == format!("{key}:")
    };
    let start = lines.iter().position(|l| key_line(l)).expect("键行");
    let key_indent = indent_of(lines[start]);
    let mut body = Vec::new();
    for l in lines.iter().skip(start + 1) {
        let stop = l.is_empty()
            || match mode {
                0 => indent_of(l) <= key_indent,
                1 => indent_of(l) < key_indent,
                _ => indent_of(l) == 0,
            };
        if stop {
            break;
        }
        body.push(l[key_indent..].to_string());
    }
    body
}

/// 同级兄弟输入**必须**能区分"正确"与"缺陷（`<`）"实现；且真实实现等于正确版。
/// 若此断言失败，说明该输入无法判别对错 → 任何基于它的闸门都是空洞的。
#[test]
fn qa_sibling_input_discriminates_correct_vs_buggy() {
    let sibling = "templates:\n  \"a.j2\":\n    name: \"A\"\n  \"b.j2\":\n    name: \"B\"\n";
    let correct = body_variant(sibling, "a.j2", 0);
    let buggy_lt = body_variant(sibling, "a.j2", 1);
    let buggy_noindent = body_variant(sibling, "a.j2", 2);

    assert_eq!(correct, vec!["  name: \"A\"".to_string()]);
    assert_ne!(
        correct, buggy_lt,
        "同级兄弟输入无法区分 `<=` 与 `<` → 闸门空洞"
    );
    assert_ne!(
        correct, buggy_noindent,
        "同级兄弟输入无法区分 `<=` 与「仅空行/顶格停」 → 闸门空洞"
    );
    // 真实实现必须等于"正确版"
    assert_eq!(manifest_entry_body(sibling, "a.j2").unwrap(), correct);
}

/// 工程师用例 `entry_body_stops_when_indent_not_deeper_than_key` 使用的**更浅缩进**
/// 输入：它能区分"正确"与"仅空行/顶格停"（旧行为），但**不能**区分 `<=` 与 `<`
/// （两种实现都停）。故它**不是** P1 边界（`<`→`<=`）的判别闸门。
#[test]
fn qa_shallower_input_cannot_discriminate_le_vs_lt() {
    let shallower = "templates:\n    \"a.j2\":\n  name: \"A\"\n";
    let correct = body_variant(shallower, "a.j2", 0);
    let buggy_lt = body_variant(shallower, "a.j2", 1);
    let buggy_noindent = body_variant(shallower, "a.j2", 2);

    assert_eq!(
        correct, buggy_lt,
        "更浅缩进输入本就不能区分 `<=` 与 `<`（两者都停）"
    );
    assert_ne!(
        correct, buggy_noindent,
        "更浅缩进输入应能区分「仅空行/顶格停」旧行为"
    );
}
