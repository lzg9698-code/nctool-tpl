//! 模板注册表：统一管理内存模板、文件系统模板与内置模板库。
//!
//! 每个模板条目携带分类、描述与参数规格，支持：
//! - 按分类列出/筛选模板
//! - 渲染前参数校验（委托 [`validate_template`]）
//! - 渲染（含 `{% include %}` / `{% extends %}` 等模板间引用）

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nctool_tpl::{Renderer, Value};

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
    /// 内存模板（源码见 [`TemplateEntry::source_text`]，不重复存储）
    Memory,
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
#[non_exhaustive]
pub enum RegistryError {
    /// 模板不存在
    NotFound(String),
    /// 模板名重复
    Duplicate(String),
    /// 模板源码为空
    EmptySource(String),
    /// 模板源码无法编译（语法错误等）。
    ///
    /// `err` 的 [`Display`](std::fmt::Display) 已携带模板名与行列定位，
    /// 本变体的 Display 不再重复包装模板名。
    Compile {
        /// 模板名
        name: String,
        /// 底层模板错误
        err: nctool_tpl::TplError,
    },
    /// 读取文件失败
    Io(std::io::Error),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::NotFound(name) => write!(f, "模板不存在: {name}"),
            RegistryError::Duplicate(name) => write!(f, "模板名重复: {name}"),
            RegistryError::EmptySource(name) => write!(f, "模板源码为空: {name}"),
            RegistryError::Compile { err, .. } => write!(f, "模板无法编译: {err}"),
            RegistryError::Io(err) => write!(f, "文件读取失败: {err}"),
        }
    }
}

impl std::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RegistryError::Compile { err, .. } => Some(err),
            RegistryError::Io(err) => Some(err),
            _ => None,
        }
    }
}

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
            .map_err(|err| RegistryError::Compile {
                name: entry.name.clone(),
                err,
            })?;
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
            source: TemplateSource::Memory,
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
    /// 校验会**穿透 `{% include %}` / `{% extends %}` 等模板间引用**：被引用
    /// 且已在注册表中注册的模板，其必选参数同样参与检查，避免组合模板的
    /// 参数缺失只能在渲染阶段才暴露。被引用模板未注册时（渲染期报
    /// `TplError::TemplateNotFound`），其变量无法静态并入，仍以主模板自身为准。
    ///
    /// 返回 [`ValidationReport`]，调用方据 [`ValidationReport::is_ok`] 决定是否渲染；
    /// 模板不存在时返回 [`RegistryError::NotFound`]。
    /// 系统注入变量（默认 `machine`）视为已提供，不要求参数集提供。
    pub fn validate(
        &self,
        name: &str,
        params: &crate::model::ParameterSet,
    ) -> Result<ValidationReport, RegistryError> {
        let entry = self
            .entries
            .get(name)
            .ok_or_else(|| RegistryError::NotFound(name.to_string()))?;
        let system: Vec<&str> = self.system_vars.iter().map(String::as_str).collect();
        // 主模板解析失败：由 validate_template 报告（含行列定位）
        let ast = match nctool_tpl::parse(&entry.source_text, &entry.name) {
            Ok(ast) => ast,
            Err(_) => {
                return Ok(validate_template(
                    &entry.source_text,
                    &entry.name,
                    &entry.params,
                    params,
                    &system,
                ))
            }
        };
        // 主模板变量 + include/extends 闭包（穿透模板间引用，防环）
        let mut vars = nctool_tpl::extract_undeclared(&ast);
        let mut specs = entry.params.clone();
        let mut visited = std::collections::BTreeSet::from([entry.name.clone()]);
        self.collect_include_closure(&ast, &mut vars, &mut specs, &mut visited);
        Ok(crate::validate::validate_with_vars(
            &vars, &specs, params, &system,
        ))
    }

    /// 递归并入 include/extends 引用的已注册模板的未声明变量与规格。
    ///
    /// 同名变量的必选性取"或"（任一处非兜底引用即必选）；规格先访问者优先
    /// （更接近主模板的声明，同名不覆盖）。环引用由 `visited` 防护。
    fn collect_include_closure(
        &self,
        ast: &nctool_tpl::Ast,
        vars: &mut Vec<nctool_tpl::Variable>,
        specs: &mut Vec<ParamSpec>,
        visited: &mut std::collections::BTreeSet<String>,
    ) {
        for ref_name in nctool_tpl::extract_template_refs(ast) {
            if !visited.insert(ref_name.clone()) {
                continue; // 防环：a → b → a
            }
            let Some(sub) = self.entries.get(&ref_name) else {
                continue; // 引用未注册模板：渲染期报 TemplateNotFound，此处无法静态并入
            };
            if let Ok(sub_ast) = nctool_tpl::parse(&sub.source_text, &sub.name) {
                for v in nctool_tpl::extract_undeclared(&sub_ast) {
                    merge_var(vars, v);
                }
                self.collect_include_closure(&sub_ast, vars, specs, visited);
            }
            for spec in &sub.params {
                if !specs.iter().any(|s| s.name == spec.name) {
                    specs.push(spec.clone());
                }
            }
        }
    }

    /// 系统注入变量名列表（默认 `["machine"]`；渲染时由管线注入上下文）。
    pub fn system_vars(&self) -> &[String] {
        &self.system_vars
    }

    /// 设置系统注入变量名列表（默认 `["machine"]`）。
    ///
    /// 渲染时由管线注入上下文的变量应列在这里，避免校验时误报"必选参数缺失"。
    pub fn set_system_vars(&mut self, vars: Vec<String>) {
        self.system_vars = vars;
    }

    /// 渲染模板：应用规格默认值兜底后，仅以参数集作为上下文渲染。
    ///
    /// 与 [`Self::validate`] 的口径一致（规格默认值视为已提供）。
    /// **不注入系统变量**：引用 `{{ machine.xxx }}` 的模板（如内置
    /// program_header）请用 [`Self::render_with_machine`]，或直接走
    /// [`crate::pipeline::GCodeGenerator::generate`] 管线。
    ///
    /// **绕过校验层**：非有限数（NaN/Inf）的拦截位于校验层（`validate` 与
    /// 管线 `generate`），直接调用本方法时 NaN/Inf 会以文本 `"NaN"`/`"inf"`
    /// 写入输出，请自行保证参数有限。
    pub fn render(
        &self,
        name: &str,
        params: &crate::model::ParameterSet,
    ) -> Result<String, nctool_tpl::TplError> {
        let context = match self.entries.get(name) {
            Some(entry) => {
                let effective = crate::model::apply_spec_defaults(&entry.params, params);
                effective.to_minijinja_value()
            }
            // 不存在的模板：走 render_template 触发统一的 TemplateNotFound
            None => params.to_minijinja_value(),
        };
        self.render_template(name, &context)
    }

    /// 渲染模板（注入机床系统变量 + 规格默认值兜底）。
    ///
    /// 上下文口径与管线 [`crate::pipeline::GCodeGenerator::generate`] 一致：
    /// 参数裸值 + `machine` 对象（config 键值 + `id`/`vendor`/`model` 元信息）。
    pub fn render_with_machine(
        &self,
        name: &str,
        params: &crate::model::ParameterSet,
        machine: &crate::model::MachineConfig,
    ) -> Result<String, nctool_tpl::TplError> {
        let context = match self.entries.get(name) {
            Some(entry) => {
                let effective = crate::model::apply_spec_defaults(&entry.params, params);
                crate::model::build_render_context(&effective, machine)
            }
            None => crate::model::build_render_context(params, machine),
        };
        self.render_template(name, &context)
    }

    /// 渲染模板（使用自定义上下文，可用于注入 `machine` 等系统变量）。
    pub fn render_template(
        &self,
        name: &str,
        context: &Value,
    ) -> Result<String, nctool_tpl::TplError> {
        self.renderer.render_template(name, context)
    }

    /// 宽松模式渲染（自定义上下文）：未定义变量（裸引用）渲染为空字符串。
    ///
    /// 每次调用用独立的宽松渲染器注册全部模板（一次性生成场景，编译缓存
    /// 不复用）。注意：经**过滤器**引用的未定义变量仍会报错——过滤器需要
    /// 具体值求值，无法以空字符串替代。
    pub fn render_template_lenient(
        &self,
        name: &str,
        context: &Value,
    ) -> Result<String, nctool_tpl::TplError> {
        let mut renderer = nctool_tpl::Renderer::new().with_lenient();
        for entry in self.entries.values() {
            renderer.add_template(&entry.name, &entry.source_text)?;
        }
        renderer.render_template(name, &context)
    }

    /// 访问底层渲染器（高级用法：配置过滤器等）。
    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// 安装内置模板库。
    fn install_builtins(&mut self) {
        for (name, category, description, source, params) in builtin_templates() {
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

/// 同名变量合并（include 闭包用）：必选性取"或"（任一处非兜底引用即必选），
/// 位置与定位信息取首次出现。
fn merge_var(vars: &mut Vec<nctool_tpl::Variable>, v: nctool_tpl::Variable) {
    if let Some(existing) = vars.iter_mut().find(|x| x.name == v.name) {
        existing.optional = existing.optional && v.optional;
    } else {
        vars.push(v);
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
            "程序头：纸带起始符 + 程序号 + 注释头 + 单位/坐标系/取消态初始化",
            concat!(
                "%\n",
                "{{ machine.program_prefix | default('O') }}{{ prog | nc_pad(machine.program_digits | default(4) | int) }}\n",
                "( {{ part_name | default('') }} )\n",
                "( {{ op_name | default('') }} )\n",
                // 单位与 G 码联动：imperial 出 G20，其余出 G21。
                // 用 `machine.units | default(...)` 裸变量形式，保证该变量仍判为可选，
                // 不会因模板引用而把 `machine` 变成必选参数。
                "{{ 'G20' if machine.units | default('metric') == 'imperial' else 'G21' }} ({{ machine.units | default('metric') }})\n",
                "G90 G17 (绝对坐标 / XY 平面)\n",
                "G40 G49 G80 (取消刀补 / 刀长补偿 / 固定循环)\n",
                "{{ machine.coordinate_system }}\n",
                "{{ machine.feed_mode }}\n",
                "M5\nM9\n",
            ),
            vec![
                crate::validate::spec(
                    "prog",
                    ParamKind::Integer,
                    true,
                    None,
                    "程序号（前导零填充 4 位）",
                )
                .with_range(1.0, 9999.0)
                .with_unit("号"),
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
            "程序尾：主轴/冷却关闭 + 取消循环 + 程序结束 + 纸带结束符",
            concat!(
                "M5\nM9\n",
                "G80 (取消固定循环)\n",
                "{{ machine.program_end }}\n",
                "%\n",
            ),
            vec![],
        ),
        (
            "tool_change",
            TemplateCategory::General,
            "换刀：主轴停止 + 换刀 + 刀长补偿 + 启动主轴与冷却",
            concat!(
                "M5\nM9\n",
                "{{ machine.tool_change }} T{{ tool_num | nc_strip }}\n",
                "G40 (取消刀补)\n",
                "G43 H{{ tool_num | nc_strip }} (刀长补偿)\n",
                // 主轴启动：此前内置模板从不发射 M3，导致钻孔循环在主轴停止状态下执行。
                // spindle_speed 设为必选——主轴转速不可静默取默认值。
                "{{ machine.spindle_on }} S{{ spindle_speed | nc_strip }} (主轴正转)\n",
                "{{ machine.coolant_on }} (冷却开)\n",
            ),
            vec![
                crate::validate::spec(
                    "tool_num",
                    ParamKind::Integer,
                    true,
                    None,
                    "刀具号",
                )
                .with_range(1.0, 999.0)
                .with_unit("号"),
                crate::validate::spec(
                    "spindle_speed",
                    ParamKind::Integer,
                    true,
                    None,
                    "主轴转速（S 值，正整数）",
                )
                .with_range(1.0, 6000.0)
                .with_unit("r/min"),
            ],
        ),
        (
            "safe_move",
            TemplateCategory::General,
            "安全移动：抬刀到安全高度 + 定位",
            concat!(
                // 统一用 nc_fixed(3) 格式化，避免此处输出 Z100.0 而钻孔循环输出 Z5.000
                "{{ machine.rapid }} G90 Z{{ safe_z | default(100) | nc_fixed(3) }}\n",
                "{{ machine.rapid }} X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }}\n",
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
                )
                .with_min(0.0)
                .with_unit("mm"),
            ],
        ),
        (
            "drill_cycle",
            TemplateCategory::Drilling,
            "钻孔循环：G81 标准钻孔",
            concat!(
                "{{ machine.rapid }} X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }}\n",
                // 固定循环块不带 G1：G1 与 G81 同属 01 组模态，前缀 G1 冗余，
                // 且部分控制器对同组重复 G 码敏感，可移植性差。
                "G98 G81 R{{ r_plane | nc_fixed(3) }} Z{{ depth | nc_fixed(3) }} F{{ feed | nc_fixed(3) }}\n",
                "G80 (取消循环)\n",
            ),
            vec![
                crate::validate::spec("x", ParamKind::Number, true, None, "孔 X 坐标")
                    .with_unit("mm"),
                crate::validate::spec("y", ParamKind::Number, true, None, "孔 Y 坐标")
                    .with_unit("mm"),
                crate::validate::spec(
                    "r_plane",
                    ParamKind::Number,
                    false,
                    Some(ParamValue::Number(5.0)),
                    "R 平面（安全高度）",
                )
                .with_min(0.0)
                .with_unit("mm")
                .with_min(0.0)
                .with_unit("mm"),
                crate::validate::spec("depth", ParamKind::Number, true, None, "钻孔深度")
                    .with_max(0.0)
                    .with_unit("mm"),
                crate::validate::spec("feed", ParamKind::Number, true, None, "进给速度")
                    .with_min(0.001)
                    .with_unit("mm/min"),
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
    fn validate_unknown_template_is_not_found() {
        // 校验不存在的模板 → RegistryError::NotFound（不再是 String 错误）
        let r = TemplateRegistry::new();
        let err = r.validate("no_such", &ParameterSet::new()).unwrap_err();
        assert!(matches!(err, RegistryError::NotFound(_)));
        assert!(err.to_string().contains("no_such"));
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
        ctx_map.insert("machine", Value::from_serialize(&machine.config));
        ctx_map.insert("prog", Value::from_serialize(1.0));
        let ctx = Value::from_serialize(&ctx_map);
        let out = r.render_template("program_header", &ctx).unwrap();
        // 字节级 golden：坐标系/进给模式直接输出配置值（不重复 G 前缀）。
        // 裸渲染不经过管线后处理，minijinja 默认剥离模板尾换行（无结尾 \n）
        assert_eq!(
            out,
            "%\nO0001\n(  )\n(  )\nG21 (metric)\nG90 G17 (绝对坐标 / XY 平面)\nG40 G49 G80 (取消刀补 / 刀长补偿 / 固定循环)\nG54\nG94\nM5\nM9"
        );
    }

    #[test]
    fn render_applies_spec_defaults_like_validate() {
        // validate 与 render 口径一致：规格默认值在校验与渲染两端都生效
        // （修复前：validate 通过后 render 仍因 r_plane 未定义而失败）
        let r = TemplateRegistry::new();
        let mut ps = ParameterSet::new();
        ps.set_number("x", 21.0)
            .set_number("y", 15.0)
            .set_number("depth", -10.0)
            .set_number("feed", 100.0);
        let report = r.validate("drill_cycle", &ps).unwrap();
        assert!(
            report.is_ok(),
            "r_plane 规格默认应兜底: {}",
            report.summary()
        );
        let machine = crate::machine::MachinePreset::Generic.config();
        let out = r.render_with_machine("drill_cycle", &ps, &machine).unwrap();
        assert!(out.contains("R5.000"), "渲染应同样兜底: {out}");
    }

    #[test]
    fn validate_follows_include_required_params() {
        // 校验穿透 include：组合模板的必选参数不漏检（渲染前可发现错误）
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "my_program",
            TemplateCategory::General,
            "",
            "{% include \"program_header\" %}",
            vec![],
        )
        .unwrap();
        let report = r.validate("my_program", &ParameterSet::new()).unwrap();
        assert!(
            report.has_errors(),
            "子模板 program_header 的 prog 应报必选缺失"
        );
        assert!(
            report.errors().any(|e| e.param.as_deref() == Some("prog")),
            "{}",
            report.summary()
        );
        // 提供后通过；且 prog 被子模板引用——不产生"未引用"冗余警告
        let mut ps = ParameterSet::new();
        ps.set_number("prog", 1.0);
        let report = r.validate("my_program", &ps).unwrap();
        assert!(report.is_ok(), "{}", report.summary());
    }

    #[test]
    fn validate_include_cycle_is_safe() {
        // 环引用（a → b → a）不死循环、不 panic，变量照常检查
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "cyc_a",
            TemplateCategory::General,
            "",
            "{% include \"cyc_b\" %}{{ va }}",
            vec![],
        )
        .unwrap();
        r.add_memory(
            "cyc_b",
            TemplateCategory::General,
            "",
            "{% include \"cyc_a\" %}{{ vb }}",
            vec![],
        )
        .unwrap();
        let report = r.validate("cyc_a", &ParameterSet::new()).unwrap();
        assert!(report.has_errors());
        assert!(report.errors().any(|e| e.param.as_deref() == Some("va")));
        assert!(report.errors().any(|e| e.param.as_deref() == Some("vb")));
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
        assert!(out.contains("G98 G81 R5.000 Z-10.000 F100.000"));
    }

    #[test]
    fn render_unknown_template_errors() {
        let r = TemplateRegistry::new();
        let ps = ParameterSet::new();
        let err = r.render("no_such_template", &ps).unwrap_err();
        assert!(matches!(err, nctool_tpl::TplError::TemplateNotFound { .. }));
    }
}
