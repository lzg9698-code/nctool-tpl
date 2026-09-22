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
    use nctool_core::ParamKind;

    fn var(name: &str) -> Variable {
        Variable {
            name: name.to_string(),
            line: 1,
            col: 1,
            start: 0,
            end: 0,
            optional: false,
        }
    }

    #[test]
    fn constraint_summary_covers_every_shape() {
        // 覆盖 `constraint_summary` 的全部分支：候选值 / 范围（双端/仅下/仅上）/
        // 整数 / 单位 / 条件必选 / 派生。这些文本是用户看得到的约束提示。
        let base = || ParamSpec::new("p", ParamKind::Number, "");

        // 空规格 → 无约束
        assert_eq!(constraint_summary(&base()), "");

        // 范围三态
        assert_eq!(
            constraint_summary(&base().with_range(-5.0, 5.0)),
            "范围 -5~5"
        );
        assert_eq!(constraint_summary(&base().with_min(0.0)), "≥ 0");
        assert_eq!(constraint_summary(&base().with_max(10.0)), "≤ 10");

        // 单位 + 整数
        assert_eq!(
            constraint_summary(&base().with_unit("mm").with_range(0.0, 1.0)),
            "范围 0~1；单位 mm"
        );

        // 候选值
        let choice = base().with_options(vec![ParamValue::Number(0.0), ParamValue::Number(8.0)]);
        assert_eq!(constraint_summary(&choice), "可选值 0 / 8");
    }

    #[test]
    fn options_summary_folds_when_over_limit() {
        let base = || ParamSpec::new("p", ParamKind::Choice, "");
        // 无候选值 → None
        assert_eq!(options_summary(&base()), None);
        // 空候选值 → None
        assert_eq!(options_summary(&base().with_options(vec![])), None);

        // 恰好上限：全列出
        let at_limit: Vec<ParamValue> = (0..MAX_SHOWN_OPTIONS)
            .map(|i| ParamValue::Number(i as f64))
            .collect();
        let s = options_summary(&base().with_options(at_limit)).unwrap();
        assert!(!s.contains("共"), "未超限不应折叠: {s}");

        // 超限：折叠并标注总数
        let over: Vec<ParamValue> = (0..MAX_SHOWN_OPTIONS + 3)
            .map(|i| ParamValue::Number(i as f64))
            .collect();
        let total = over.len();
        let s = options_summary(&base().with_options(over)).unwrap();
        assert!(s.contains("…"), "超限应折叠: {s}");
        assert!(s.contains(&format!("共 {total} 项")), "应标注总数: {s}");
    }

    #[test]
    fn render_line_pads_alignment_and_optionally_carries_spec() {
        // 无名元（spec=None）：只有名字与行列
        let bare = render_line(&var("x"), None, 6);
        assert!(bare.starts_with("  x"), "{bare}");
        assert!(bare.contains("行 1 列 1"), "{bare}");
        assert!(bare.ends_with('\n'), "每行应以换行结束: {bare:?}");

        // 带规格：类型 + 约束 + 描述
        let spec = ParamSpec::new("feed", ParamKind::Number, "进给速度")
            .with_range(0.0, 100.0)
            .with_unit("mm/min");
        let line = render_line(&var("feed"), Some(&spec), 8);
        assert!(line.contains("数值"), "应带类型标签: {line}");
        assert!(line.contains("范围 0~100"), "{line}");
        assert!(line.contains("进给速度"), "{line}");
    }

    #[test]
    fn bucket_all_has_stable_display_order() {
        // 分组顺序是用户看到的顺序，锁定以防意外重排
        let titles: Vec<&str> = Bucket::all().iter().map(|b| b.title()).collect();
        assert_eq!(titles.len(), 4);
        assert!(titles[0].contains("必选参数"));
        assert!(titles[1].contains("条件必选"));
        assert!(titles[2].contains("派生"));
        assert!(titles[3].contains("可选参数"));
    }
}
