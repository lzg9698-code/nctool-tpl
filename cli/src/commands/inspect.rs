//! `inspect` 子命令：变量提取（必选/可选 + 行列定位）+ **已声明的参数规格**。
//!
//! 规格（类型 / 候选值 / 区间 / 整数 / 条件必选 / 派生）来自三层来源：模板头部
//! `{# PARAMS: #}`、`templates/variables.yaml`、`templates.yaml` 的 `params`
//! 覆盖层（见 `nctool_core::manifest`）。这些约束在 `validate` / `render` 时是
//! **真的会拦人**的，所以必须让用户能提前看到——否则只能靠"触发一次报错"来发现。

use nctool_core::{ParamSpec, ParamValue};
use nctool_tpl::Variable;

use crate::cli::InspectArgs;
use crate::context::Ctx;
use crate::output::CliError;

use super::templates::{extract_variables, resolve_source};

/// 一行最多列出多少个候选值。
///
/// 机床模板的候选值最多 12 个（顶尖型号），全列出来会把一行撑到几百字符；
/// 超出部分折叠为 `…（共 N 项）`。
const MAX_SHOWN_OPTIONS: usize = 6;

/// 参数名对齐宽度上限（名字过长时不强行对齐，避免整行被推得很远）。
const MAX_NAME_WIDTH: usize = 24;

/// 参数的展示分组。
///
/// **不再只分"必选 / 可选"两桶**：规格引入后必选性不止两态——派生参数由系统填充
/// （不要求提供）、条件必选参数取决于控制参数取值。混在两桶里会误导用户去填
/// 一个不该填的参数。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bucket {
    /// 由系统按规格规则计算注入
    Derived,
    /// 满足触发条件时才必选
    Conditional,
    /// 无条件必选
    Required,
    /// 模板内有默认值兜底
    Optional,
}

impl Bucket {
    /// 展示顺序（也是分组的标题）。
    fn all() -> [Bucket; 4] {
        [
            Bucket::Required,
            Bucket::Conditional,
            Bucket::Derived,
            Bucket::Optional,
        ]
    }

    fn title(&self) -> &'static str {
        match self {
            Bucket::Derived => "派生参数（系统注入，无需提供）",
            Bucket::Conditional => "条件必选参数（满足条件时必填）",
            Bucket::Required => "必选参数",
            Bucket::Optional => "可选参数",
        }
    }
}

/// `inspect <template>`：解析模板并提取其引用的外部变量，附上已声明的规格。
pub fn run(ctx: &Ctx, args: &InspectArgs) -> Result<(), CliError> {
    let (name, source, _, system_vars) = resolve_source(ctx, &args.template)?;
    // 已注册模板走**参数闭包**提取（穿透 include/extends）：组合模板若只列
    // 主模板自身的变量，用户按表填参会漏掉片段所需的参数，渲染才报错。
    // 未注册的文件路径无法解析 include，退化为单模板提取。
    let gen = ctx.build_registry()?;
    let specs: Vec<ParamSpec> = gen
        .registry()
        .get(&name)
        .map(|e| e.params.clone())
        .unwrap_or_default();
    let (vars, from_closure) = match gen.registry().extract_params(&name) {
        Ok(vars) => (vars, true),
        Err(_) => (extract_variables(&source, &name, &system_vars)?, false),
    };

    let find = |n: &str| specs.iter().find(|s| s.name == n);
    let bucket = |v: &Variable| -> Bucket {
        match find(&v.name) {
            // 派生优先：它同时可能带 required / required_if，但那都不该由用户填
            Some(s) if s.derive.is_some() => Bucket::Derived,
            // 模板内有兜底（`| default(v)`）时不算"条件必选"——它根本不会缺
            Some(s) if s.required_if.is_some() && !v.optional => Bucket::Conditional,
            _ if v.optional => Bucket::Optional,
            _ => Bucket::Required,
        }
    };
    let pick = |b: Bucket| vars.iter().filter(|v| bucket(v) == b).collect::<Vec<_>>();

    // JSON：规格字段**复用 HTTP API 的 `spec_json`**，避免两处形状各自漂移
    // （Web UI 读的是同一份字段名）。
    let entry_json = |v: &Variable| {
        let mut obj = serde_json::json!({ "name": v.name, "line": v.line, "col": v.col });
        if let (Some(s), Some(map)) = (find(&v.name), obj.as_object_mut()) {
            map.insert("spec".to_string(), crate::server::spec_json(s));
        }
        obj
    };
    let group_json = |b: Bucket| pick(b).iter().map(|v| entry_json(v)).collect::<Vec<_>>();
    let data = serde_json::json!({
        "template": name,
        "required": group_json(Bucket::Required),
        "conditional": group_json(Bucket::Conditional),
        "derived": group_json(Bucket::Derived),
        "optional": group_json(Bucket::Optional),
    });

    // 文本输出
    let width = vars
        .iter()
        .map(|v| v.name.chars().count())
        .max()
        .unwrap_or(0)
        .min(MAX_NAME_WIDTH);
    let mut text = format!("模板: {name}\n");
    for b in Bucket::all() {
        let group = pick(b);
        text.push_str(&format!("\n{}（{}）:\n", b.title(), group.len()));
        for v in group {
            text.push_str(&render_line(v, find(&v.name), width));
        }
    }
    if vars.is_empty() {
        text.push_str("  （无外部变量引用）\n");
    }
    // 规格缺类型提示：这类参数不做类型检查，写错要到渲染期才暴露
    let untyped: Vec<&str> = vars
        .iter()
        .filter(|v| {
            matches!(
                find(&v.name).map(|s| s.kind),
                Some(nctool_core::ParamKind::Any)
            )
        })
        .map(|v| v.name.as_str())
        .collect();
    if !untyped.is_empty() {
        text.push_str(&format!(
            "\n注：{} 未标注类型（不做类型检查）。补齐方式：模板头部 `{{# PARAMS: #}}` 加类型列，\n\
             或在 templates/variables.yaml / 清单 params 里声明 kind。\n",
            untyped.join("、")
        ));
    }

    // 提示 include 穿透：组合模板的参数表包含片段引用的变量，行列号指向
    // 片段自身（行号看起来"越界"是正常的，因为片段是另一个文件）。
    if from_closure {
        let refs = nctool_tpl::parse(&source, &name)
            .map(|ast| nctool_tpl::extract_template_refs(&ast))
            .unwrap_or_default();
        if !refs.is_empty() {
            text.push_str(&format!(
                "\n注：本模板引用了 {}——上表已并入其参数；\n\
                 行列号指向片段文件自身（片段内行号）。\n",
                refs.join("、")
            ));
        }
    }

    ctx.style.print_ok(&text, data);
    Ok(())
}

/// 渲染一行：`  名字  行 L 列 C  类型  约束摘要  描述`。
fn render_line(v: &Variable, spec: Option<&ParamSpec>, width: usize) -> String {
    let pad = " ".repeat(width.saturating_sub(v.name.chars().count()));
    let mut line = format!("  {}{pad}  行 {} 列 {}", v.name, v.line, v.col);
    if let Some(s) = spec {
        line.push_str(&format!("  {}", s.kind.label()));
        let constraints = constraint_summary(s);
        if !constraints.is_empty() {
            line.push_str(&format!("  {constraints}"));
        }
        if !s.description.is_empty() {
            line.push_str(&format!("  {}", s.description));
        }
    }
    line.push('\n');
    line
}

/// 规格约束的摘要（`可选值 0/8/10/12.5；范围 0~5；条件必选 side = "Right"`）。
fn constraint_summary(spec: &ParamSpec) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(opts) = options_summary(spec) {
        parts.push(format!("可选值 {opts}"));
    }
    match (spec.min, spec.max) {
        (Some(min), Some(max)) => parts.push(format!("范围 {min}~{max}")),
        (Some(min), None) => parts.push(format!("≥ {min}")),
        (None, Some(max)) => parts.push(format!("≤ {max}")),
        (None, None) => {}
    }
    if spec.integer {
        parts.push("整数".to_string());
    }
    if let Some(unit) = &spec.unit {
        parts.push(format!("单位 {unit}"));
    }
    if let Some(rif) = &spec.required_if {
        parts.push(format!("条件必选 {}", rif.display()));
    }
    if let Some(derive) = &spec.derive {
        parts.push(format!("派生 {}", derive.display()));
    }
    parts.join("；")
}

/// 候选值的紧凑摘要（超出 [`MAX_SHOWN_OPTIONS`] 折叠为 `…（共 N 项）`）。
fn options_summary(spec: &ParamSpec) -> Option<String> {
    let opts = spec.options.as_ref()?;
    if opts.is_empty() {
        return None;
    }
    let shown: Vec<String> = opts
        .iter()
        .take(MAX_SHOWN_OPTIONS)
        .map(ParamValue::display)
        .collect();
    let mut text = shown.join(" / ");
    if opts.len() > MAX_SHOWN_OPTIONS {
        text.push_str(&format!(" …（共 {} 项）", opts.len()));
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nctool_core::{ParamKind, ParamSpec};

    /// 一个"裸"规格：只给名字，其余字段取最小默认值。
    ///
    /// `ParamSpec` 没有实现 `Default`（字段语义各异，给不出合理默认），
    /// 故在此显式构造 —— 也让用例对"哪些字段属于约束"保持敏感。
    fn spec(name: &str) -> ParamSpec {
        ParamSpec {
            name: name.to_string(),
            kind: ParamKind::Any,
            required: false,
            default: None,
            min: None,
            max: None,
            integer: false,
            unit: None,
            options: None,
            description: String::new(),
            required_if: None,
            derive: None,
        }
    }

    // ---- constraint_summary：每个分支单独钉住 ----

    /// 无任何约束时摘要为空字符串（`render_line` 据此省略该段）。
    #[test]
    fn constraint_summary_empty_when_unconstrained() {
        assert_eq!(constraint_summary(&spec("x")), "");
    }

    /// 只有 `min` / 只有 `max` / 两端都有 —— 三种区间形态各自的文案。
    #[test]
    fn constraint_summary_covers_every_range_shape() {
        let mut only_min = spec("x");
        only_min.min = Some(1.0);
        assert_eq!(constraint_summary(&only_min), "≥ 1");

        let mut only_max = spec("x");
        only_max.max = Some(9.0);
        assert_eq!(constraint_summary(&only_max), "≤ 9");

        let mut both = spec("x");
        both.min = Some(1.0);
        both.max = Some(9.0);
        assert_eq!(constraint_summary(&both), "范围 1~9");
    }

    /// `integer` / `unit` 两个标记位各自贡献一段。
    #[test]
    fn constraint_summary_includes_integer_and_unit() {
        let mut s = spec("n");
        s.integer = true;
        assert_eq!(constraint_summary(&s), "整数");

        let mut s = spec("f");
        s.unit = Some("mm/min".to_string());
        assert_eq!(constraint_summary(&s), "单位 mm/min");
    }

    /// 候选值摘要与其它约束用 `；` 连接，顺序为 可选值 → 范围 → 整数 → 单位。
    ///
    /// 字符串候选**带双引号**是 `ParamValue::display` 的有意行为：本模型里
    /// `"8"`（文本候选）与 `8`（数值候选）是不同候选项，加引号才能区分。
    #[test]
    fn constraint_summary_joins_with_semicolon_in_fixed_order() {
        let mut s = spec("side");
        s.options = Some(vec![
            ParamValue::String("Left".to_string()),
            ParamValue::String("Right".to_string()),
        ]);
        s.min = Some(0.0);
        s.max = Some(5.0);
        s.integer = true;
        s.description = "说明文字不应出现在约束摘要里".to_string();
        assert_eq!(
            constraint_summary(&s),
            "可选值 \"Left\" / \"Right\"；范围 0~5；整数"
        );
        assert!(
            !constraint_summary(&s).contains("说明文字"),
            "描述不属于约束摘要"
        );
    }

    /// 文本候选与数值候选即使字面相同也保持区分（引号是唯一的区分手段）。
    #[test]
    fn string_and_number_options_are_visually_distinct() {
        let mut s = spec("n");
        s.options = Some(vec![
            ParamValue::String("8".to_string()),
            ParamValue::Number(8.0),
        ]);
        assert_eq!(constraint_summary(&s), "可选值 \"8\" / 8");
    }

    /// 布尔与列表候选的展示形态（列表降级为 `<列表>`）。
    #[test]
    fn boolean_and_list_options_display_forms() {
        let mut s = spec("b");
        s.options = Some(vec![ParamValue::Bool(true), ParamValue::Bool(false)]);
        assert_eq!(constraint_summary(&s), "可选值 true / false");

        let mut s = spec("l");
        s.options = Some(vec![ParamValue::List(vec![ParamValue::Number(1.0)])]);
        assert_eq!(constraint_summary(&s), "可选值 <列表>");
    }

    // ---- options_summary ----

    /// 无 `options` 字段、或候选值为空列表 → 不产出摘要（而不是产出空串）。
    #[test]
    fn options_summary_none_for_absent_or_empty() {
        assert_eq!(options_summary(&spec("x")), None);

        let mut empty = spec("x");
        empty.options = Some(Vec::new());
        assert_eq!(options_summary(&empty), None);
    }

    /// 候选值数量恰好等于上限时不折叠；超出 1 个即折叠并给出总数。
    #[test]
    fn options_summary_folds_only_beyond_limit() {
        let many: Vec<ParamValue> = (0..=MAX_SHOWN_OPTIONS)
            .map(|i| ParamValue::Number(i as f64))
            .collect();

        let mut at_limit = spec("x");
        at_limit.options = Some(many[..MAX_SHOWN_OPTIONS].to_vec());
        let text = options_summary(&at_limit).unwrap();
        assert_eq!(text, "0 / 1 / 2 / 3 / 4 / 5");
        assert!(!text.contains("共"), "恰好等于上限不应折叠：{text}");

        let mut over = spec("x");
        over.options = Some(many);
        let text = options_summary(&over).unwrap();
        assert_eq!(text, "0 / 1 / 2 / 3 / 4 / 5 …（共 7 项）");
    }

    // ---- render_line ----

    /// 无规格时只到「行 L 列 C」为止，不留尾随空格。
    #[test]
    fn render_line_without_spec_stops_after_position() {
        let v = Variable {
            name: "x".to_string(),
            line: 3,
            col: 7,
            start: 0,
            end: 1,
            optional: false,
        };
        let line = render_line(&v, None, 1);
        assert_eq!(line, "  x  行 3 列 7\n");
    }

    /// 名字短于 `width` 时右侧补空格对齐；长于 `width` 时不补（`saturating_sub`）。
    #[test]
    fn render_line_pads_to_width_without_overflow() {
        let mk = |name: &str| Variable {
            name: name.to_string(),
            line: 1,
            col: 1,
            start: 0,
            end: 1,
            optional: false,
        };
        // width=5，名字 2 字符 → 补 3 个空格
        assert_eq!(render_line(&mk("ab"), None, 5), "  ab     行 1 列 1\n");
        // 名字比 width 长 → 不补、不 panic
        assert_eq!(
            render_line(&mk("abcdefgh"), None, 3),
            "  abcdefgh  行 1 列 1\n"
        );
    }

    /// 有规格时按 类型 → 约束 → 描述 的顺序附加，且空描述不产生多余分隔。
    #[test]
    fn render_line_appends_kind_constraints_description() {
        let v = Variable {
            name: "feed".to_string(),
            line: 2,
            col: 4,
            start: 0,
            end: 1,
            optional: false,
        };
        let mut s = spec("feed");
        s.kind = ParamKind::Number;
        s.unit = Some("mm/min".to_string());

        let with_desc = {
            let mut s2 = s.clone();
            s2.description = "进给速度".to_string();
            render_line(&v, Some(&s2), 4)
        };
        assert!(with_desc.contains("mm/min"), "{with_desc}");
        assert!(with_desc.contains("进给速度"), "{with_desc}");
        // 描述在单位之后
        let unit_pos = with_desc.find("mm/min").unwrap();
        let desc_pos = with_desc.find("进给速度").unwrap();
        assert!(unit_pos < desc_pos);

        // 描述为空时不追加多余内容
        let no_desc = render_line(&v, Some(&s), 4);
        assert_eq!(
            no_desc,
            format!(
                "  feed  行 2 列 4  {}  单位 mm/min\n",
                ParamKind::Number.label()
            )
        );
    }

    // ---- MAX_NAME_WIDTH 的语义 ----

    /// `run` 里对宽度取 `min(MAX_NAME_WIDTH)`：超长名字不应把整行推得很远。
    #[test]
    fn max_name_width_is_a_bounded_cap() {
        assert_eq!(MAX_NAME_WIDTH, 24);
        let long = "a".repeat(60);
        let width = long.chars().count().min(MAX_NAME_WIDTH);
        assert_eq!(width, MAX_NAME_WIDTH);
    }

    // ---- 分组展示顺序（自远程线合并保留）----

    /// 分组顺序是用户看到的顺序，锁定以防意外重排。
    #[test]
    fn bucket_all_has_stable_display_order() {
        let titles: Vec<&str> = Bucket::all().iter().map(|b| b.title()).collect();
        assert_eq!(titles.len(), 4);
        assert!(titles[0].contains("必选参数"));
        assert!(titles[1].contains("条件必选"));
        assert!(titles[2].contains("派生"));
        assert!(titles[3].contains("可选参数"));
    }
}
