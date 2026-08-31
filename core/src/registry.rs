//! 模板注册表：统一管理内存模板、文件系统模板与内置模板库。
//!
//! 每个模板条目携带分类、描述与参数规格，支持：
//! - 按分类列出/筛选模板
//! - 渲染前参数校验（委托 [`validate_template`]）
//! - 渲染（含 `{% include %}` / `{% extends %}` 等模板间引用）

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nctool_tpl::Renderer;

use crate::model::ParamSpec;
use crate::validate::{validate_template, ValidationReport};

/// 模板分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateCategory {
    /// 通用子程序（程序头/尾、换刀、安全移动、主轴/冷却）
    General,
    /// 铣削
    Milling,
    /// 车削
    Turning,
    /// 钻孔/攻丝/铰孔
    Drilling,
    /// 机床特定
    Machine,
}

impl TemplateCategory {
    /// 分类标签（用于列表展示）。
    pub fn label(&self) -> &'static str {
        match self {
            TemplateCategory::General => "通用",
            TemplateCategory::Milling => "铣削",
            TemplateCategory::Turning => "车削",
            TemplateCategory::Drilling => "钻孔",
            TemplateCategory::Machine => "机床",
        }
    }
}

/// 模板源码来源。
#[derive(Debug, Clone)]
pub enum TemplateSource {
    /// 内存模板（源码直接提供）
    Memory(String),
    /// 文件系统模板（注册时加载内容，记录路径）
    File(PathBuf),
    /// 内置模板库
    Builtin,
}

/// 模板条目：注册表中的一个模板。
#[derive(Debug, Clone)]
pub struct TemplateEntry {
    /// 模板名（唯一，用于引用与渲染）
    pub name: String,
    /// 分类
    pub category: TemplateCategory,
    /// 描述
    pub description: String,
    /// 源码来源
    pub source: TemplateSource,
    /// 参数规格（渲染前校验）
    pub params: Vec<ParamSpec>,
    /// 模板源码（统一为字符串，供渲染）
    pub source_text: String,
}

/// 注册表错误。
#[derive(Debug)]
pub enum RegistryError {
    /// 模板名重复
    Duplicate(String),
    /// 模板源码为空
    EmptySource(String),
    /// 模板源码无法编译（语法错误等）
    Compile(String),
    /// 读取文件失败
    Io(std::io::Error),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::Duplicate(name) => write!(f, "模板名重复: {name}"),
            RegistryError::EmptySource(name) => write!(f, "模板源码为空: {name}"),
            RegistryError::Compile(name) => write!(f, "模板无法编译: {name}"),
            RegistryError::Io(err) => write!(f, "文件读取失败: {err}"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// 模板注册表。
///
/// 内部持有 [`Renderer`]（注册模板并复用其编译缓存），是模板的单一权威来源。
#[derive(Debug)]
pub struct TemplateRegistry {
    entries: BTreeMap<String, TemplateEntry>,
    renderer: Renderer,
    /// 系统注入变量名（如 `machine`）：渲染时由管线注入上下文，
    /// 校验时视为已提供，不要求参数集提供。
    system_vars: Vec<String>,
}

impl TemplateRegistry {
    /// 创建空注册表（含默认内置模板库）。
    pub fn new() -> Self {
        let mut registry = Self {
            entries: BTreeMap::new(),
            renderer: Renderer::new(),
            system_vars: vec!["machine".to_string()],
        };
        registry.install_builtins();
        registry
    }

    /// 注册一个模板条目。
    pub fn add_entry(&mut self, entry: TemplateEntry) -> Result<(), RegistryError> {
        if entry.source_text.trim().is_empty() {
            return Err(RegistryError::EmptySource(entry.name.clone()));
        }
        if self.entries.contains_key(&entry.name) {
            return Err(RegistryError::Duplicate(entry.name.clone()));
        }
        // 同步到渲染器（支持模板间 include/extends/import）
        self.renderer
            .add_template(entry.name.clone(), entry.source_text.clone())
            .map_err(|err| RegistryError::Compile(format!("{}: {err}", entry.name)))?;
        self.entries.insert(entry.name.clone(), entry);
        Ok(())
    }

    /// 从内存模板注册（便捷构造）。
    pub fn add_memory(
        &mut self,
        name: impl Into<String>,
        category: TemplateCategory,
        description: impl Into<String>,
        source: impl Into<String>,
        params: Vec<ParamSpec>,
    ) -> Result<(), RegistryError> {
        let name = name.into();
        let source_text = source.into();
        self.add_entry(TemplateEntry {
            name,
            category,
            description: description.into(),
            source: TemplateSource::Memory(source_text.clone()),
            params,
            source_text,
        })
    }

    /// 从文件系统模板注册（加载文件内容）。
    pub fn add_file(
        &mut self,
        name: impl Into<String>,
        category: TemplateCategory,
        description: impl Into<String>,
        path: impl AsRef<Path>,
        params: Vec<ParamSpec>,
    ) -> Result<(), RegistryError> {
        let name = name.into();
        let path = path.as_ref().to_path_buf();
        let source_text = std::fs::read_to_string(&path).map_err(RegistryError::Io)?;
        self.add_entry(TemplateEntry {
            name,
            category,
            description: description.into(),
            source: TemplateSource::File(path),
            params,
            source_text,
        })
    }

    /// 按名称获取模板条目。
    pub fn get(&self, name: &str) -> Option<&TemplateEntry> {
        self.entries.get(name)
    }

    /// 列出模板（可按分类筛选）。
    pub fn list(&self, category: Option<TemplateCategory>) -> Vec<&TemplateEntry> {
        self.entries
            .values()
            .filter(|e| category.is_none_or(|c| e.category == c))
            .collect()
    }

    /// 模板数量。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 校验指定模板的参数（渲染前调用）。
    ///
    /// 返回 [`ValidationReport`]，调用方据 [`ValidationReport::is_ok`] 决定是否渲染。
    /// 系统注入变量（默认 `machine`）视为已提供，不要求参数集提供。
    pub fn validate(
        &self,
        name: &str,
        params: &crate::model::ParameterSet,
    ) -> Result<ValidationReport, String> {
        let entry = self
            .entries
            .get(name)
            .ok_or_else(|| format!("模板不存在: {name}"))?;
        let system: Vec<&str> = self.system_vars.iter().map(String::as_str).collect();
        Ok(validate_template(
            &entry.source_text,
            &entry.name,
            &entry.params,
            params,
            &system,
        ))
    }

    /// 设置系统注入变量名列表（默认 `["machine"]`）。
    ///
    /// 渲染时由管线注入上下文的变量应列在这里，避免校验时误报"必选参数缺失"。
    pub fn set_system_vars(&mut self, vars: Vec<String>) {
        self.system_vars = vars;
    }

    /// 渲染模板（仅用参数集作为上下文）。
    pub fn render(
        &self,
        name: &str,
        params: &crate::model::ParameterSet,
    ) -> Result<String, nctool_tpl::TplError> {
        let context = params.to_minijinja_value();
        self.render_template(name, &context)
    }

    /// 渲染模板（使用自定义上下文，可用于注入 `machine` 等系统变量）。
    pub fn render_template(
        &self,
        name: &str,
        context: &minijinja::Value,
    ) -> Result<String, nctool_tpl::TplError> {
        self.renderer.render_template(name, context)
    }

    /// 访问底层渲染器（高级用法：配置过滤器等）。
    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// 安装内置模板库。
    fn install_builtins(&mut self) {
        for (name, category, description, source, params) in builtin_templates() {
            // 内置模板注册失败视为编程错误（源码应为合法模板）
            let entry = TemplateEntry {
                name: name.to_string(),
                category,
                description: description.to_string(),
                source: TemplateSource::Builtin,
                params,
                source_text: source.to_string(),
            };
            // 内置模板注册失败视为编程错误（源码应为合法模板）
            self.add_entry(entry).expect("内置模板注册失败");
        }
    }
}

impl Default for TemplateRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 内置模板定义：`(名称, 分类, 描述, 源码, 参数规格)`。
///
/// 内置模板是 G-code 开发的基础子程序，可被用户模板 `{% include %}` 复用。
fn builtin_templates() -> Vec<(
    &'static str,
    TemplateCategory,
    &'static str,
    &'static str,
    Vec<ParamSpec>,
)> {
    use crate::model::{ParamKind, ParamValue};

    vec![
        (
            "program_header",
            TemplateCategory::General,
            "程序头：程序号 + 注释头 + 单位/坐标系初始化",
            concat!(
                "O{{ prog | nc_pad(4) }}\n",
                "( {{ part_name | default('') }} )\n",
                "( {{ op_name | default('') }} )\n",
                "G21 (metric)\n",
                "G{{ machine.coordinate_system }}\n",
                "G{{ machine.feed_mode }}\n",
                "M5\nM9\n",
            ),
            vec![
                crate::validate::spec(
                    "prog",
                    ParamKind::Number,
                    true,
                    None,
                    "程序号（前导零填充 4 位）",
                ),
                crate::validate::spec(
                    "part_name",
                    ParamKind::String,
                    false,
                    Some(ParamValue::String("".into())),
                    "零件名称（注释）",
                ),
                crate::validate::spec(
                    "op_name",
                    ParamKind::String,
                    false,
                    Some(ParamValue::String("".into())),
                    "工序名称（注释）",
                ),
            ],
        ),
        (
            "program_footer",
            TemplateCategory::General,
            "程序尾：主轴/冷却关闭 + 程序结束",
            concat!("M5\nM9\n", "{{ machine.program_end }}\n"),
            vec![],
        ),
        (
            "tool_change",
            TemplateCategory::General,
            "换刀：主轴停止 + 换刀 + 取消刀具补偿",
            concat!(
                "M5\n",
                "{{ machine.tool_change }} T{{ tool_num }}\n",
                "G40 (取消刀具补偿)\n",
            ),
            vec![crate::validate::spec(
                "tool_num",
                ParamKind::Number,
                true,
                None,
                "刀具号",
            )],
        ),
        (
            "safe_move",
            TemplateCategory::General,
            "安全移动：抬刀到安全高度 + 定位",
            concat!(
                "{{ machine.rapid }} G90 Z{{ safe_z | default(100) }}\n",
                "{{ machine.rapid }} X{{ x }} Y{{ y }}\n",
            ),
            vec![
                crate::validate::spec(
                    "x",
                    ParamKind::Number,
                    true,
                    None,
                    "目标 X 坐标",
                ),
                crate::validate::spec(
                    "y",
                    ParamKind::Number,
                    true,
                    None,
                    "目标 Y 坐标",
                ),
                crate::validate::spec(
                    "safe_z",
                    ParamKind::Number,
                    false,
                    Some(ParamValue::Number(100.0)),
                    "安全高度 Z",
                ),
            ],
        ),
        (
            "drill_cycle",
            TemplateCategory::Drilling,
            "钻孔循环：G81 标准钻孔",
            concat!(
                "{{ machine.rapid }} X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }}\n",
                "{{ machine.linear }} G98 G81 R{{ r_plane | nc_fixed(3) }} Z{{ depth | nc_fixed(3) }} F{{ feed | nc_fixed(3) }}\n",
                "G80 (取消循环)\n",
            ),
            vec![
                crate::validate::spec("x", ParamKind::Number, true, None, "孔 X 坐标"),
                crate::validate::spec("y", ParamKind::Number, true, None, "孔 Y 坐标"),
                crate::validate::spec(
                    "r_plane",
                    ParamKind::Number,
                    false,
                    Some(ParamValue::Number(5.0)),
                    "R 平面（安全高度）",
                ),
                crate::validate::spec("depth", ParamKind::Number, true, None, "钻孔深度"),
                crate::validate::spec("feed", ParamKind::Number, true, None, "进给速度"),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ParameterSet;

    #[test]
    fn registry_installs_builtins() {
        let r = TemplateRegistry::new();
        assert!(r.len() >= 5);
        for name in [
            "program_header",
            "program_footer",
            "tool_change",
            "safe_move",
            "drill_cycle",
        ] {
            assert!(r.get(name).is_some(), "缺少内置模板 {name}");
        }
    }

    #[test]
    fn add_memory_template() {
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "my_op",
            TemplateCategory::Milling,
            "测试工序",
            "X{{ x }}",
            vec![],
        )
        .unwrap();
        assert!(r.get("my_op").is_some());
        assert_eq!(r.list(None).len(), r.len());
    }

    #[test]
    fn duplicate_name_rejected() {
        let mut r = TemplateRegistry::new();
        r.add_memory("dup", TemplateCategory::General, "", "X", vec![])
            .unwrap();
        let err = r
            .add_memory("dup", TemplateCategory::General, "", "Y", vec![])
            .unwrap_err();
        assert!(matches!(err, RegistryError::Duplicate(_)));
    }

    #[test]
    fn empty_source_rejected() {
        let mut r = TemplateRegistry::new();
        let err = r
            .add_memory("empty", TemplateCategory::General, "", "   ", vec![])
            .unwrap_err();
        assert!(matches!(err, RegistryError::EmptySource(_)));
    }

    #[test]
    fn list_by_category() {
        let mut r = TemplateRegistry::new();
        r.add_memory("mill1", TemplateCategory::Milling, "", "X{{ x }}", vec![])
            .unwrap();
        let milling = r.list(Some(TemplateCategory::Milling));
        assert!(milling.iter().any(|e| e.name == "mill1"));
        let general = r.list(Some(TemplateCategory::General));
        assert!(general.iter().any(|e| e.name == "program_header"));
        assert!(!general.iter().any(|e| e.name == "mill1"));
    }

    #[test]
    fn validate_missing_required() {
        let r = TemplateRegistry::new();
        let ps = ParameterSet::new(); // drill_cycle 缺 x/y/depth/feed
        let report = r.validate("drill_cycle", &ps).unwrap();
        assert!(report.has_errors());
        assert!(report.errors().any(|e| e.param.as_deref() == Some("x")));
    }

    #[test]
    fn validate_accepts_system_vars() {
        // machine 是系统注入变量，校验时不应误报缺失
        let r = TemplateRegistry::new();
        let mut ps = ParameterSet::new();
        ps.set_number("prog", 1.0); // program_header 引用了 machine.xxx
        let report = r.validate("program_header", &ps).unwrap();
        assert!(
            report.is_ok(),
            "machine 不应被当作缺失参数: {}",
            report.summary()
        );
    }

    #[test]
    fn render_memory_template_with_params() {
        // registry.render 用纯参数上下文渲染（模板不含系统变量时可用）
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "simple",
            TemplateCategory::General,
            "",
            "X{{ x | nc_fixed(3) }}",
            vec![],
        )
        .unwrap();
        let mut ps = ParameterSet::new();
        ps.set_number("x", 21.0);
        let out = r.render("simple", &ps).unwrap();
        assert_eq!(out.trim(), "X21.000");
    }

    #[test]
    fn render_template_with_machine_context() {
        // render_template 支持自定义上下文（注入 machine 系统变量）
        let r = TemplateRegistry::new();
        let machine = crate::machine::MachinePreset::Generic.config();
        let mut ctx_map = std::collections::BTreeMap::new();
        ctx_map.insert("machine", minijinja::Value::from_serialize(&machine.config));
        ctx_map.insert("prog", minijinja::Value::from_serialize(1.0));
        let ctx = minijinja::Value::from_serialize(&ctx_map);
        let out = r.render_template("program_header", &ctx).unwrap();
        assert!(out.starts_with("O0001"));
        assert!(out.contains("G54"));
    }

    #[test]
    fn render_drill_cycle_via_pipeline() {
        // drill_cycle 引用 machine 系统变量，端到端渲染应通过 GCodeGenerator
        // （pipeline 负责注入 machine 上下文 + 规格默认值兜底）
        let g = crate::pipeline::GCodeGenerator::new();
        let mut ps = crate::model::ParameterSet::new();
        ps.set_number("x", 21.0)
            .set_number("y", 15.0)
            .set_number("depth", -10.0)
            .set_number("feed", 100.0);
        // 未提供 r_plane，应被规格默认值（5.0）兜底
        let machine = crate::machine::MachinePreset::Generic.config();
        let out = g
            .generate(
                "drill_cycle",
                &ps,
                &machine,
                &crate::pipeline::GenerationOptions::default(),
            )
            .unwrap();
        assert!(out.contains("G1 G98 G81 R5.000 Z-10.000 F100.000"));
    }

    #[test]
    fn render_unknown_template_errors() {
        let r = TemplateRegistry::new();
        let ps = ParameterSet::new();
        let err = r.render("no_such_template", &ps).unwrap_err();
        assert!(matches!(err, nctool_tpl::TplError::TemplateNotFound { .. }));
    }
}
