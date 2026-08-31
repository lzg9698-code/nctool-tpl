//! G-code 生成管线：**参数校验 → 上下文合并 → 渲染 → 后处理**。
//!
//! 管线输入：模板名 + 参数集 + 机床配置 + 生成选项；
//! 输出：可直接用于机床的 G-code 字符串。
//!
//! 管线在渲染**前**完成参数校验（发现缺失/类型问题），渲染后做后处理
//! （行号、头部注释、空行清理）。

use crate::model::{MachineConfig, ParamValue, ParameterSet};
use crate::registry::TemplateRegistry;
use crate::validate::ValidationReport;

/// 管线错误。
#[derive(Debug)]
pub enum PipelineError {
    /// 模板不存在
    TemplateNotFound(String),
    /// 参数校验未通过（含错误级问题）
    Validation(ValidationReport),
    /// 渲染失败（模板语法、未知过滤器等）
    Render(nctool_tpl::TplError),
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineError::TemplateNotFound(name) => write!(f, "模板不存在: {name}"),
            PipelineError::Validation(report) => write!(f, "参数校验未通过:\n{}", report.summary()),
            PipelineError::Render(err) => write!(f, "渲染失败: {err}"),
        }
    }
}

impl std::error::Error for PipelineError {}

/// 输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// G-code 输出（应用行号/注释/空行等后处理）
    Gcode,
    /// 纯文本输出（仅渲染，不做后处理）
    Text,
}

/// 生成选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationOptions {
    /// 输出格式
    pub format: OutputFormat,
    /// 是否生成行号（`N0010 N0020 ...`）
    pub line_numbers: bool,
    /// 行号步进
    pub line_number_step: u32,
    /// 行号上限（超过后不再编号）
    pub max_line_number: u32,
    /// 是否生成头部注释（模板名等）
    pub add_header_comment: bool,
    /// 是否删除空行
    pub strip_blank_lines: bool,
    /// 仅输出 ASCII 字符（非 ASCII 字符替换为 `?`）。
    ///
    /// 许多 CNC 控制器对 G-code 中的非 ASCII 字符（如中文注释）处理不稳定，
    /// 直传机床的场景建议开启。仅对 [`OutputFormat::Gcode`] 生效，
    /// [`OutputFormat::Text`] 始终原样输出。
    pub ascii_only: bool,
}

impl Default for GenerationOptions {
    fn default() -> Self {
        Self {
            format: OutputFormat::Gcode,
            line_numbers: false,
            line_number_step: 10,
            max_line_number: 9999,
            add_header_comment: false,
            strip_blank_lines: false,
            ascii_only: false,
        }
    }
}

/// G-code 生成器。
///
/// 持有 [`TemplateRegistry`]，提供端到端的模板 → G-code 生成能力。
#[derive(Debug)]
pub struct GCodeGenerator {
    registry: TemplateRegistry,
}

impl GCodeGenerator {
    /// 创建生成器（含内置模板库与默认机床配置）。
    pub fn new() -> Self {
        Self {
            registry: TemplateRegistry::new(),
        }
    }

    /// 从已有注册表创建生成器。
    pub fn from_registry(registry: TemplateRegistry) -> Self {
        Self { registry }
    }

    /// 访问模板注册表。
    pub fn registry(&self) -> &TemplateRegistry {
        &self.registry
    }

    /// 可变访问模板注册表（注册自定义模板等）。
    pub fn registry_mut(&mut self) -> &mut TemplateRegistry {
        &mut self.registry
    }

    /// 端到端生成 G-code。
    ///
    /// # 流程
    /// 1. 校验模板参数（必选齐全、类型匹配）
    /// 2. 合并上下文：`params` 参数 + `machine` 机床配置
    /// 3. 渲染模板
    /// 4. 后处理（行号/头部注释/空行清理）
    ///
    /// # 错误
    /// - 模板不存在 → [`PipelineError::TemplateNotFound`]
    /// - 参数校验未通过 → [`PipelineError::Validation`]
    /// - 渲染失败 → [`PipelineError::Render`]
    pub fn generate(
        &self,
        template: &str,
        params: &ParameterSet,
        machine: &MachineConfig,
        opts: &GenerationOptions,
    ) -> Result<String, PipelineError> {
        // 1. 模板存在性
        let entry = match self.registry.get(template) {
            Some(e) => e,
            None => return Err(PipelineError::TemplateNotFound(template.to_string())),
        };

        // 2. 参数校验（渲染前）。校验阶段的唯一失败情形是模板不存在，
        //    统一映射为 TemplateNotFound（第 1 步已拦截，此处为兜底）
        let report = self
            .registry
            .validate(template, params)
            .map_err(|_| PipelineError::TemplateNotFound(template.to_string()))?;
        if report.has_errors() {
            return Err(PipelineError::Validation(report));
        }

        // 3. 规格默认值兜底 + 合并上下文 + 渲染
        let effective = apply_spec_defaults(&entry.params, params);
        let context = build_context(&effective, machine);
        let rendered = self
            .registry
            .render_template(template, &context)
            .map_err(PipelineError::Render)?;

        // 4. 后处理
        Ok(postprocess(&rendered, template, opts))
    }

    /// 便捷：使用通用机床配置生成 G-code。
    pub fn generate_generic(
        &self,
        template: &str,
        params: &ParameterSet,
        opts: &GenerationOptions,
    ) -> Result<String, PipelineError> {
        let machine = crate::machine::MachinePreset::Generic.config();
        self.generate(template, params, &machine, opts)
    }
}

impl Default for GCodeGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// 应用参数规格的默认值兜底：规格声明了 `default`、且参数集未提供的参数，
/// 渲染前自动填入默认值（用户提供的值优先，不被覆盖）。
fn apply_spec_defaults(specs: &[crate::model::ParamSpec], params: &ParameterSet) -> ParameterSet {
    let mut effective = params.clone();
    for spec in specs {
        if !effective.contains(&spec.name) {
            if let Some(default) = &spec.default {
                effective.values.insert(spec.name.clone(), default.clone());
            }
        }
    }
    effective
}

/// 构建渲染上下文：`params`（裸值）+ `machine`（机床配置对象）。
///
/// [`ParamValue`] 以**裸值**注入（数值→数字、字符串→字符串、布尔→布尔），
/// 使模板能直接以 `{{ x }}` 引用参数。数值直接经 minijinja 序列化，**不经过
/// JSON 中间层**，因此 NaN/Inf 不会被静默篡改（校验层已拒绝它们进入管线）。
///
/// `machine` 对象包含 `config` 的全部键值（**字符串**，如 `{{ machine.rapid }}`）
/// 以及元信息 `id` / `vendor` / `model`。若 `config` 中存在同名键，元信息优先。
/// 注意 `config` 值均为字符串，模板中做数值比较需先转换（如 `| int`）。
fn build_context(params: &ParameterSet, machine: &MachineConfig) -> minijinja::Value {
    let mut map: std::collections::BTreeMap<String, minijinja::Value> =
        std::collections::BTreeMap::new();
    for (k, v) in &params.values {
        map.insert(k.clone(), param_to_minijinja(v));
    }
    // 注入 machine 对象（config 键值 + 元信息，模板通过 {{ machine.xxx }} 引用）
    let mut machine_obj: std::collections::BTreeMap<&str, minijinja::Value> =
        std::collections::BTreeMap::new();
    for (k, v) in &machine.config {
        machine_obj.insert(k.as_str(), minijinja::Value::from(v.as_str()));
    }
    machine_obj.insert("id", minijinja::Value::from(machine.id.as_str()));
    machine_obj.insert("vendor", minijinja::Value::from(machine.vendor.as_str()));
    machine_obj.insert("model", minijinja::Value::from(machine.model.as_str()));
    map.insert(
        "machine".to_string(),
        minijinja::Value::from_serialize(&machine_obj),
    );
    minijinja::Value::from_serialize(&map)
}

/// 将 [`ParamValue`] 转为 minijinja 裸值（数值/字符串/布尔）。
fn param_to_minijinja(v: &ParamValue) -> minijinja::Value {
    match v {
        ParamValue::Number(n) => minijinja::Value::from_serialize(n),
        ParamValue::String(s) => minijinja::Value::from_serialize(s),
        ParamValue::Bool(b) => minijinja::Value::from_serialize(b),
    }
}

/// 非 ASCII 字符替换为 `?`（`GenerationOptions::ascii_only` 后处理）。
fn sanitize_ascii(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii() { c } else { '?' })
        .collect()
}

/// 后处理：头部注释 / 行号 / 空行清理。
///
/// - **Text 格式**：仅渲染，保留原始行内容（不 trim、不编号、不清理空行、不做 ASCII 清洗）
/// - **Gcode 格式**：可生成行号、清理空行；每行 trim 首尾空白；`ascii_only` 开启时
///   非 ASCII 字符替换为 `?`（头部注释与模板名同样清洗）
///
/// 行号规则：程序号行（`O` 开头）与已有 `N` 前缀的行不重复编号；
/// 行号达到 `max_line_number` 后不再递增。
fn postprocess(rendered: &str, template: &str, opts: &GenerationOptions) -> String {
    let mut out = String::new();

    // 头部注释（两种格式均生效，由用户显式开启）；文本为 ASCII，
    // 模板名若含非 ASCII 字符且开启 ascii_only 时同样被清洗
    if opts.add_header_comment {
        let header = format!(
            "( ================================== )\n( nctool generated G-code )\n( template: {} )\n( ================================== )\n",
            template
        );
        if opts.ascii_only {
            out.push_str(&sanitize_ascii(&header));
        } else {
            out.push_str(&header);
        }
    }

    // Text 格式：仅渲染，不做任何后处理
    if opts.format == OutputFormat::Text {
        out.push_str(rendered);
        return out;
    }

    // Gcode 格式：行号 + 空行清理 + trim + 可选 ASCII 清洗
    let mut line_no: u32 = 0;
    for line in rendered.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if opts.strip_blank_lines {
                continue;
            }
            out.push('\n');
            continue;
        }
        let line_buf;
        let content: &str = if opts.ascii_only {
            line_buf = sanitize_ascii(trimmed);
            &line_buf
        } else {
            trimmed
        };
        if opts.line_numbers {
            let is_program = trimmed.starts_with('O') || trimmed.starts_with('o');
            let already_numbered = trimmed.starts_with('N') || trimmed.starts_with('n');
            if !is_program
                && !already_numbered
                && line_no + opts.line_number_step <= opts.max_line_number
            {
                line_no += opts.line_number_step;
                out.push_str(&format!("N{:04} ", line_no));
            }
        }
        out.push_str(content);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine() -> MachineConfig {
        crate::machine::MachinePreset::Generic.config()
    }

    #[test]
    fn generate_drill_cycle_end_to_end() {
        let g = GCodeGenerator::new();
        let mut ps = ParameterSet::new();
        ps.set_number("x", 21.0)
            .set_number("y", 15.0)
            .set_number("depth", -10.0)
            .set_number("feed", 100.0);
        let out = g
            .generate(
                "drill_cycle",
                &ps,
                &machine(),
                &GenerationOptions::default(),
            )
            .unwrap();
        assert!(out.contains("G1 G98 G81 R5.000 Z-10.000 F100.000"));
        assert!(out.contains("G80"));
    }

    #[test]
    fn generate_with_line_numbers() {
        let g = GCodeGenerator::new();
        let mut ps = ParameterSet::new();
        ps.set_number("x", 21.0)
            .set_number("y", 15.0)
            .set_number("depth", -10.0)
            .set_number("feed", 100.0);
        let opts = GenerationOptions {
            format: OutputFormat::Gcode,
            line_numbers: true,
            ..Default::default()
        };
        let out = g.generate("drill_cycle", &ps, &machine(), &opts).unwrap();
        assert!(out.contains("N0010"), "应生成行号: {out}");
        assert!(out.contains("N0020"));
        assert!(!out.contains("O"), "drill_cycle 无程序号行");
    }

    #[test]
    fn generate_program_header_with_line_numbers_skips_o_line() {
        let g = GCodeGenerator::new();
        let mut ps = ParameterSet::new();
        ps.set_number("prog", 1.0).set_string("part_name", "SHAFT");
        let opts = GenerationOptions {
            format: OutputFormat::Gcode,
            line_numbers: true,
            add_header_comment: true,
            strip_blank_lines: true,
            ..Default::default()
        };
        let out = g
            .generate("program_header", &ps, &machine(), &opts)
            .unwrap();
        // 头部注释
        assert!(out.starts_with("( ======"));
        // O 程序号行不加 N
        assert!(out.contains("\nO0001\n"));
        // 后续行加 N
        assert!(out.contains("\nN0010 "));
        // 无空行（strip）
        assert!(!out.contains("\n\n"));
    }

    #[test]
    fn validation_error_stops_pipeline() {
        let g = GCodeGenerator::new();
        // drill_cycle 缺 x/y/depth/feed
        let err = g
            .generate(
                "drill_cycle",
                &ParameterSet::new(),
                &machine(),
                &GenerationOptions::default(),
            )
            .unwrap_err();
        match err {
            PipelineError::Validation(report) => assert!(report.has_errors()),
            other => panic!("应为校验错误, 实际: {other}"),
        }
    }

    #[test]
    fn template_not_found() {
        let g = GCodeGenerator::new();
        let err = g
            .generate(
                "missing",
                &ParameterSet::new(),
                &machine(),
                &GenerationOptions::default(),
            )
            .unwrap_err();
        assert!(matches!(err, PipelineError::TemplateNotFound(_)));
    }

    #[test]
    fn text_format_skips_line_numbers() {
        let g = GCodeGenerator::new();
        let mut ps = ParameterSet::new();
        ps.set_number("prog", 1.0);
        let opts = GenerationOptions {
            format: OutputFormat::Text,
            line_numbers: true, // 即使要求行号，Text 格式也不加
            ..Default::default()
        };
        let out = g
            .generate("program_header", &ps, &machine(), &opts)
            .unwrap();
        assert!(!out.contains("N00"), "Text 格式不应加行号: {out}");
    }

    #[test]
    fn existing_n_lines_not_double_numbered() {
        // 模板输出已含 N 前缀的行不应被重复编号
        let mut g = GCodeGenerator::new();
        g.registry_mut()
            .add_memory(
                "has_n",
                crate::registry::TemplateCategory::General,
                "",
                "N1000 G0 X{{ x }}\nG1 Z-5",
                vec![],
            )
            .unwrap();
        let mut ps = ParameterSet::new();
        ps.set_number("x", 1.0);
        let opts = GenerationOptions {
            line_numbers: true,
            ..Default::default()
        };
        let out = g.generate("has_n", &ps, &machine(), &opts).unwrap();
        assert!(out.starts_with("N1000 G0"), "已有行号不应重复编号: {out}");
        assert!(!out.contains("N1000 N"), "不应重复编号: {out}");
    }

    #[test]
    fn custom_machine_injected_into_context() {
        // 自定义机床配置应能被模板通过 {{ machine.xxx }} 引用
        let mut g = GCodeGenerator::new();
        g.registry_mut()
            .add_memory(
                "use_machine",
                crate::registry::TemplateCategory::General,
                "",
                "{{ machine.coordinate_system }} {{ machine.max_spindle_rpm }}",
                vec![],
            )
            .unwrap();
        let mut m = machine();
        m.config.insert("coordinate_system".into(), "G55".into());
        m.config.insert("max_spindle_rpm".into(), "4200".into());
        let out = g
            .generate(
                "use_machine",
                &ParameterSet::new(),
                &m,
                &GenerationOptions::default(),
            )
            .unwrap();
        assert_eq!(out.trim(), "G55 4200");
    }

    #[test]
    fn machine_meta_injected_into_context() {
        // machine.id / vendor / model 应一并注入，模板可直接引用
        let mut g = GCodeGenerator::new();
        g.registry_mut()
            .add_memory(
                "machine_meta",
                crate::registry::TemplateCategory::General,
                "",
                "{{ machine.id }} {{ machine.vendor }} {{ machine.model }}",
                vec![],
            )
            .unwrap();
        let out = g
            .generate(
                "machine_meta",
                &ParameterSet::new(),
                &machine(),
                &GenerationOptions::default(),
            )
            .unwrap();
        assert_eq!(out.trim(), "generic Generic CNC");
    }

    #[test]
    fn param_naked_values_in_context() {
        // 参数应以裸值注入：数值/字符串/布尔可直接引用
        let mut g = GCodeGenerator::new();
        g.registry_mut()
            .add_memory(
                "naked",
                crate::registry::TemplateCategory::General,
                "",
                "{{ n }} {{ s }} {{ b }}",
                vec![],
            )
            .unwrap();
        let mut ps = ParameterSet::new();
        ps.set_number("n", 21.0)
            .set_string("s", "hello")
            .set_bool("b", true);
        let out = g
            .generate("naked", &ps, &machine(), &GenerationOptions::default())
            .unwrap();
        // minijinja 默认显示：f64 21.0 → "21.0"，bool true → "True"
        assert_eq!(out.trim(), "21.0 hello True");
    }

    #[test]
    fn nan_param_stops_pipeline() {
        // NaN 参数应被校验拦截，返回 Validation 错误而非渲染出非法坐标
        let g = GCodeGenerator::new();
        let mut ps = ParameterSet::new();
        ps.set_number("x", f64::NAN)
            .set_number("y", 15.0)
            .set_number("depth", -10.0)
            .set_number("feed", 100.0);
        let err = g
            .generate(
                "drill_cycle",
                &ps,
                &machine(),
                &GenerationOptions::default(),
            )
            .unwrap_err();
        match err {
            PipelineError::Validation(report) => {
                assert!(report.has_errors(), "NaN 应产生错误: {}", report.summary())
            }
            other => panic!("应为校验错误, 实际: {other}"),
        }
    }

    #[test]
    fn text_format_preserves_original_lines() {
        // Text 格式仅渲染，不做 trim/空行清理/行号
        let mut g = GCodeGenerator::new();
        g.registry_mut()
            .add_memory(
                "keep_ws",
                crate::registry::TemplateCategory::General,
                "",
                "  G0 X1  \n\n  G1 Z-5  ",
                vec![],
            )
            .unwrap();
        let opts = GenerationOptions {
            format: OutputFormat::Text,
            strip_blank_lines: true,
            line_numbers: true,
            ..Default::default()
        };
        let out = g
            .generate("keep_ws", &ParameterSet::new(), &machine(), &opts)
            .unwrap();
        // 保留前导空格、空行与行号不应出现
        assert!(out.contains("  G0 X1  "), "Text 不应 trim: {:?}", out);
        assert!(out.contains("\n\n"), "Text 不应清理空行: {:?}", out);
        assert!(!out.contains("N00"), "Text 不应加行号: {:?}", out);
    }

    #[test]
    fn ascii_only_replaces_non_ascii_in_gcode() {
        // 中文注释（零件名）在 ascii_only 开启时替换为 ?，输出全 ASCII
        let mut g = GCodeGenerator::new();
        g.registry_mut()
            .add_memory(
                "cn_note",
                crate::registry::TemplateCategory::General,
                "",
                "( {{ note }} )\nG1 X1",
                vec![],
            )
            .unwrap();
        let mut ps = ParameterSet::new();
        ps.set_string("note", "轴类零件");
        let opts = GenerationOptions {
            ascii_only: true,
            ..Default::default()
        };
        let out = g.generate("cn_note", &ps, &machine(), &opts).unwrap();
        assert!(out.is_ascii(), "输出应全 ASCII: {out}");
        assert!(out.contains("( ???? )"), "4 个汉字替换为 4 个 ?: {out}");
        assert!(out.contains("G1 X1"), "ASCII 内容不受影响: {out}");
    }

    #[test]
    fn ascii_only_default_off_keeps_non_ascii() {
        // 默认关闭：非 ASCII 原样保留
        let mut g = GCodeGenerator::new();
        g.registry_mut()
            .add_memory(
                "cn_note2",
                crate::registry::TemplateCategory::General,
                "",
                "( {{ note }} )",
                vec![],
            )
            .unwrap();
        let mut ps = ParameterSet::new();
        ps.set_string("note", "轴类零件");
        let out = g
            .generate("cn_note2", &ps, &machine(), &GenerationOptions::default())
            .unwrap();
        assert!(out.contains("轴类零件"), "默认应保留非 ASCII: {out}");
    }

    #[test]
    fn ascii_only_sanitizes_header_comment() {
        // 开启 ascii_only 后，头部注释（含模板名）同样被清洗
        let g = GCodeGenerator::new();
        let mut ps = ParameterSet::new();
        ps.set_number("prog", 1.0);
        let opts = GenerationOptions {
            add_header_comment: true,
            ascii_only: true,
            ..Default::default()
        };
        let out = g
            .generate("program_header", &ps, &machine(), &opts)
            .unwrap();
        assert!(out.is_ascii(), "头部注释应全 ASCII: {out}");
        assert!(out.contains("nctool generated G-code"));
    }

    #[test]
    fn line_numbers_stop_at_max() {
        // 行号超过 max_line_number 后不再编号，且行内容仍保留
        let mut g = GCodeGenerator::new();
        let tpl: String = (0..3)
            .map(|i| format!("G{} X{}", i, i))
            .collect::<Vec<_>>()
            .join("\n");
        g.registry_mut()
            .add_memory(
                "num_test",
                crate::registry::TemplateCategory::General,
                "",
                &tpl,
                vec![],
            )
            .unwrap();
        let opts = GenerationOptions {
            format: OutputFormat::Gcode,
            line_numbers: true,
            max_line_number: 10,
            ..Default::default()
        };
        let out = g
            .generate("num_test", &ParameterSet::new(), &machine(), &opts)
            .unwrap();
        // 只有一行编号 (N0010)，其余不编号但内容保留
        assert!(out.contains("N0010 G0 X0"));
        assert!(out.contains("G1 X1"), "未编号行内容应保留: {out}");
        assert!(out.contains("G2 X2"), "未编号行内容应保留: {out}");
    }
}
