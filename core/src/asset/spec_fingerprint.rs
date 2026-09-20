//! 参数规格指纹：对有效 [`ParamSpec`] 集合做规范化串拼接后取 FNV-1a 64。
//!
//! 用途：建立"模板参数规格变化 → 预设失效"的可检测因果链（架构设计 §7.5）。
//! **只用于变更检测，不是安全用途**——同输入结果稳定，规格任一实质字段变化必变。

use crate::model::{DeriveRule, ParamKind, ParamSpec, ParamValue, RequiredIf};

use super::guard::fnv1a64;

/// 参数规格指纹服务（无状态）。
pub struct SpecFingerprint;

impl SpecFingerprint {
    /// 计算规格集合指纹，输出 `fnv1a64:<16 位十六进制>`。
    ///
    /// 对 [`Self::canonical`] 的结果取 FNV-1a 64。
    pub fn of(specs: &[ParamSpec]) -> String {
        let canonical = Self::canonical(specs);
        format!("fnv1a64:{:016x}", fnv1a64(canonical.as_bytes()))
    }

    /// 规范化串：按参数名排序后，对每个规格按**固定字段序**拼接。
    ///
    /// 字段序：`name|kind|required|min|max|integer|unit|options|required_if|derive|default`。
    /// **每个字段值**（含 `name`/`kind`/`required`/`integer`）都经长度前缀编码，
    /// 故分隔符 `|` 与记录符 `\n` 无法被值注入；`None` 记 `-1:`，数值用
    /// `format!("{:?}", f64)`；`options` / `required_if` 取值 / `derive` 表项排序后拼接。
    /// `description` 不参与（文档性文本变化不应使预设失效）。
    pub fn canonical(specs: &[ParamSpec]) -> String {
        let mut sorted: Vec<&ParamSpec> = specs.iter().collect();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));

        let mut out = String::new();
        for s in sorted {
            let fields = [
                enc_str(&s.name),
                enc_str(&kind_tag(s.kind)),
                enc_str(&s.required.to_string()),
                opt_f64(s.min),
                opt_f64(s.max),
                enc_str(&s.integer.to_string()),
                opt_str(&s.unit),
                opt_options(&s.options),
                opt_required_if(&s.required_if),
                opt_derive(&s.derive),
                opt_value(&s.default),
            ];
            out.push_str(&fields.join("|"));
            out.push('\n');
        }
        out
    }
}

/// 类型标签（`format!("{:?}")` 稳定且穷尽；新增变体会自然带出新标签）。
fn kind_tag(kind: ParamKind) -> String {
    format!("{kind:?}")
}

// ---------------------------------------------------------------------------
// 无歧义编码
//
// 规范化串里"字段用 `|` 连接、记录用 `\n` 结尾"，若字段值本身含 `|`/`\n`，
// 或"`None` 记 `-`"与真实值 `"-"` 撞车，都会造成**不同规格得到同一指纹**——
// 于是"规格变了但预设被判为不陈旧"，正是本指纹要防的静默错误。
//
// 解决：对每个字段值做**长度前缀编码** `<字节长度>:<值>`，`None` 记 `-1:`。
// 长度前缀使值自定界——分隔符与哨兵字符都无法被注入，无需任何转义。
// ---------------------------------------------------------------------------

/// 单值编码：`<字节长度>:<值>`（`len()` 为字节数，前后一致）。
fn enc_str(s: &str) -> String {
    format!("{}:{}", s.len(), s)
}

/// 可选值编码：`None` → `-1:`；`Some(s)` → [`enc_str`]。
fn enc_opt(value: Option<String>) -> String {
    match value {
        None => "-1:".to_string(),
        Some(s) => enc_str(&s),
    }
}

/// 值序列编码：逐元素 [`enc_str`] 后**直接拼接**（长度自定界，无需分隔符）。
fn enc_seq(items: &[String]) -> String {
    let mut out = String::new();
    for it in items {
        out.push_str(&enc_str(it));
    }
    out
}

/// `Option<f64>` 的确定性编码：数值用 `format!("{:?}", f64)`，再长度前缀。
fn opt_f64(v: Option<f64>) -> String {
    enc_opt(v.map(|x| format!("{x:?}")))
}

/// `Option<String>` 的编码。
fn opt_str(v: &Option<String>) -> String {
    enc_opt(v.clone())
}

/// 参数值的规范化表示（带类型前缀，避免 `1`（整数）与 `"1"`（字符串）撞车）。
fn canon_value(v: &ParamValue) -> String {
    match v {
        ParamValue::Number(n) => format!("n:{n:?}"),
        ParamValue::Integer(i) => format!("i:{i}"),
        ParamValue::String(s) => format!("s:{s}"),
        ParamValue::Bool(b) => format!("b:{b}"),
        ParamValue::List(items) => {
            let inner: Vec<String> = items.iter().map(canon_value).collect();
            // 元素逐个长度前缀后拼接，外层再标注元素个数，避免 `,` 注入歧义。
            format!("l:{}[{}]", inner.len(), enc_seq(&inner))
        }
    }
}

/// `Option<ParamValue>` 的编码。
fn opt_value(v: &Option<ParamValue>) -> String {
    enc_opt(v.as_ref().map(canon_value))
}

/// `options` 白名单的编码：元素排序后逐个长度前缀拼接；`None` 记 `-1:`。
fn opt_options(v: &Option<Vec<ParamValue>>) -> String {
    enc_opt(v.as_ref().map(|list| {
        let mut items: Vec<String> = list.iter().map(canon_value).collect();
        items.sort();
        enc_seq(&items)
    }))
}

/// 条件必选的编码：`param` 与排序后的取值序列各自长度前缀拼接；`None` 记 `-1:`。
fn opt_required_if(v: &Option<RequiredIf>) -> String {
    enc_opt(v.as_ref().map(|r| {
        let mut values: Vec<String> = r.values.iter().map(canon_value).collect();
        values.sort();
        format!("{}{}", enc_str(&r.param), enc_seq(&values))
    }))
}

/// 派生规则的编码：`from` + 排序后的 `(键,值)` 表 + 回退值，各段长度前缀；`None` 记 `-1:`。
fn opt_derive(v: &Option<DeriveRule>) -> String {
    enc_opt(v.as_ref().map(|d| {
        let mut table: Vec<String> = d
            .table
            .iter()
            .map(|(k, val)| format!("{}{}", enc_str(&canon_value(k)), enc_str(&canon_value(val))))
            .collect();
        table.sort();
        format!(
            "{}{}{}",
            enc_str(&d.from),
            enc_seq(&table),
            opt_value(&d.fallback)
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DeriveRule, ParamKind, ParamSpec, ParamValue, RequiredIf};

    fn spec(name: &str, kind: ParamKind) -> ParamSpec {
        ParamSpec::new(name, kind, "文档说明")
    }

    #[test]
    fn fingerprint_is_stable_across_calls() {
        let specs = vec![spec("x", ParamKind::Number), spec("y", ParamKind::String)];
        assert_eq!(SpecFingerprint::of(&specs), SpecFingerprint::of(&specs));
        // 格式：fnv1a64: + 16 hex
        let fp = SpecFingerprint::of(&specs);
        assert!(fp.starts_with("fnv1a64:"), "{fp}");
        assert_eq!(fp.len(), "fnv1a64:".len() + 16);
    }

    #[test]
    fn fingerprint_ignores_order_and_description() {
        let mut a = spec("x", ParamKind::Number);
        a.description = "说明一".into();
        let mut b = spec("y", ParamKind::Integer);
        b.description = "说明二".into();
        let s1 = vec![a.clone(), b.clone()];
        let s2 = vec![b, a];
        assert_eq!(
            SpecFingerprint::of(&s1),
            SpecFingerprint::of(&s2),
            "排序 + 忽略 description：仅顺序/文档不同不得改变指纹"
        );
    }

    #[test]
    fn fingerprint_changes_on_name_kind_and_every_constraint() {
        let base = vec![spec("x", ParamKind::Number)];
        let base_fp = SpecFingerprint::of(&base);

        // 参数名变化
        assert_ne!(
            base_fp,
            SpecFingerprint::of(&[spec("z", ParamKind::Number)])
        );
        // 类型变化
        assert_ne!(
            base_fp,
            SpecFingerprint::of(&[spec("x", ParamKind::Integer)])
        );

        // 约束逐项变化 → 指纹必变
        let mut min = spec("x", ParamKind::Number);
        min.min = Some(0.0);
        assert_ne!(base_fp, SpecFingerprint::of(&[min]), "min 变化应使指纹变");

        let mut max = spec("x", ParamKind::Number);
        max.max = Some(100.0);
        assert_ne!(base_fp, SpecFingerprint::of(&[max]), "max 变化应使指纹变");

        let mut integer = spec("x", ParamKind::Number);
        integer.integer = true;
        assert_ne!(
            base_fp,
            SpecFingerprint::of(&[integer]),
            "integer 变化应使指纹变"
        );

        let mut unit = spec("x", ParamKind::Number);
        unit.unit = Some("mm".into());
        assert_ne!(base_fp, SpecFingerprint::of(&[unit]), "unit 变化应使指纹变");

        let mut required = spec("x", ParamKind::Number);
        required.required = true;
        assert_ne!(
            base_fp,
            SpecFingerprint::of(&[required]),
            "required 变化应使指纹变"
        );

        let mut options = spec("x", ParamKind::Choice);
        options.options = Some(vec![ParamValue::String("闭口".into())]);
        assert_ne!(
            base_fp,
            SpecFingerprint::of(&[options]),
            "options 变化应使指纹变"
        );

        let mut default = spec("x", ParamKind::Number);
        default.default = Some(ParamValue::Number(0.5));
        assert_ne!(
            base_fp,
            SpecFingerprint::of(&[default]),
            "default 变化应使指纹变"
        );

        let mut rif = spec("x", ParamKind::Number);
        rif.required_if = Some(RequiredIf::new(
            "side",
            [ParamValue::String("Right".into())],
        ));
        assert_ne!(
            base_fp,
            SpecFingerprint::of(&[rif]),
            "required_if 变化应使指纹变"
        );

        let mut derive = spec("x", ParamKind::Number);
        derive.derive = Some(DeriveRule {
            from: "src".into(),
            table: vec![(ParamValue::String("A".into()), ParamValue::Number(1.0))],
            fallback: None,
        });
        assert_ne!(
            base_fp,
            SpecFingerprint::of(&[derive]),
            "derive 变化应使指纹变"
        );
    }

    #[test]
    fn options_and_required_if_order_is_normalized() {
        let mut a = spec("x", ParamKind::Choice);
        a.options = Some(vec![
            ParamValue::String("b".into()),
            ParamValue::String("a".into()),
        ]);
        let mut b = spec("x", ParamKind::Choice);
        b.options = Some(vec![
            ParamValue::String("a".into()),
            ParamValue::String("b".into()),
        ]);
        assert_eq!(
            SpecFingerprint::of(&[a]),
            SpecFingerprint::of(&[b]),
            "options 顺序不同、集合相同 → 指纹应一致"
        );

        let mut r1 = spec("x", ParamKind::Number);
        r1.required_if = Some(RequiredIf::new(
            "side",
            [
                ParamValue::String("L".into()),
                ParamValue::String("R".into()),
            ],
        ));
        let mut r2 = spec("x", ParamKind::Number);
        r2.required_if = Some(RequiredIf::new(
            "side",
            [
                ParamValue::String("R".into()),
                ParamValue::String("L".into()),
            ],
        ));
        assert_eq!(
            SpecFingerprint::of(&[r1]),
            SpecFingerprint::of(&[r2]),
            "required_if 取值顺序不同、集合相同 → 指纹应一致"
        );
    }

    #[test]
    fn string_and_number_options_are_distinct() {
        // "8"（文本）与 8（数值）是不同候选项，规范化串必须区分
        let mut text = spec("x", ParamKind::Choice);
        text.options = Some(vec![ParamValue::String("8".into())]);
        let mut num = spec("x", ParamKind::Choice);
        num.options = Some(vec![ParamValue::Integer(8)]);
        assert_ne!(SpecFingerprint::of(&[text]), SpecFingerprint::of(&[num]));
    }

    #[test]
    fn empty_specs_has_stable_fingerprint() {
        let fp = SpecFingerprint::of(&[]);
        assert_eq!(fp, format!("fnv1a64:{:016x}", fnv1a64(b"")));
    }

    /// 覆盖 `canon_value` 的 Bool / List（含嵌套）分支：能稳定产出且彼此可区分。
    #[test]
    fn canon_covers_bool_and_list_values() {
        let mut b = spec("x", ParamKind::Bool);
        b.default = Some(ParamValue::Bool(true));
        let mut l = spec("x", ParamKind::List);
        l.default = Some(ParamValue::List(vec![
            ParamValue::Integer(1),
            ParamValue::String("a".into()),
            ParamValue::List(vec![ParamValue::Bool(false)]),
        ]));

        let fb = SpecFingerprint::of(&[b]);
        let fl = SpecFingerprint::of(&[l.clone()]);
        assert_ne!(fb, fl, "Bool 与 List 默认值应产生不同指纹");
        assert_eq!(fl, SpecFingerprint::of(&[l]), "List 默认值指纹应稳定");
    }

    /// P2 回归：`None` 与真实值 `"-"` 必须可区分（长度前缀编码消除哨兵歧义）。
    #[test]
    fn none_does_not_collide_with_literal_dash() {
        let mut none = spec("x", ParamKind::Number);
        none.unit = None;
        let mut dash = spec("x", ParamKind::Number);
        dash.unit = Some("-".into());
        assert_ne!(
            SpecFingerprint::of(&[none]),
            SpecFingerprint::of(&[dash]),
            "unit=None 与 unit=Some(\"-\") 撞车"
        );

        // 其它可选字段同样：default None vs Some(String("-"))
        let mut d_none = spec("x", ParamKind::Number);
        d_none.default = None;
        let mut d_dash = spec("x", ParamKind::Number);
        d_dash.default = Some(ParamValue::String("-".into()));
        assert_ne!(
            SpecFingerprint::of(&[d_none]),
            SpecFingerprint::of(&[d_dash])
        );
    }

    /// P2 回归：字段值里注入分隔符（`|` / `\n`）不得与"另一组不同规格"撞车。
    ///
    /// 用旧编码（字段 `|` 连接、`None` 记 `-`）时，下面 A/B 会**得到同一指纹**：
    /// A: unit="x", options=["y"]            → `…|x|s:y|-|…`
    /// B: unit="x|s:y", options=None         → `…|x|s:y|-|…`
    /// 长度前缀编码后二者必然不同。
    #[test]
    fn separator_injection_does_not_collide() {
        let mut a = spec("x", ParamKind::Number);
        a.unit = Some("x".into());
        a.options = Some(vec![ParamValue::String("y".into())]);

        let mut b = spec("x", ParamKind::Number);
        b.unit = Some("x|s:y".into());
        b.options = None;

        assert_ne!(
            SpecFingerprint::of(&[a.clone()]),
            SpecFingerprint::of(&[b]),
            "含 `|` 的值与另一组不同规格撞车"
        );
        assert_eq!(
            SpecFingerprint::of(&[a.clone()]),
            SpecFingerprint::of(&[a]),
            "含 `|` 的值本身应稳定"
        );

        // 换行注入：名称含 `\n` 不得与"两组规格"的串接撞车
        let mut nl = spec("a\nb", ParamKind::Number);
        nl.unit = None;
        let mut two = vec![spec("a", ParamKind::Number)];
        two.push(spec("b", ParamKind::Number));
        assert_ne!(
            SpecFingerprint::of(&[nl]),
            SpecFingerprint::of(&two),
            "名称含 `\\n` 与两条规格撞车"
        );

        // 名称含 `|`：与"名称 + 字段边界错位"的另一组不撞车
        let mut pipe = spec("a|b", ParamKind::Number);
        pipe.unit = None;
        let mut split = spec("a", ParamKind::Number);
        split.unit = Some("b".into());
        assert_ne!(SpecFingerprint::of(&[pipe]), SpecFingerprint::of(&[split]));
    }
}
