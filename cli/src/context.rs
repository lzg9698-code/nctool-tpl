//! 命令执行上下文：解析全局选项、构建模板注册表、解析机床配置。

use std::path::PathBuf;

use nctool_core::machine::MachinePreset;
use nctool_core::pipeline::GCodeGenerator;
use nctool_core::registry::TemplateCategory;
use nctool_core::{MachineConfig, ParameterSet};

use crate::args;
use crate::cli::GlobalArgs;
use crate::config;
use crate::output::{CliError, OutputStyle};

/// 命令执行上下文（由全局选项 + 配置文件解析而来）。
#[derive(Debug, Clone)]
pub struct Ctx {
    /// 结果输出风格
    pub style: OutputStyle,
    /// 详细输出
    pub verbose: bool,
    /// 模板目录（CLI --template-dir 优先，否则配置文件）
    pub template_dir: Option<PathBuf>,
    /// 默认机床（CLI --machine 优先，否则配置文件）
    pub default_machine: Option<String>,
}

impl Ctx {
    /// 从全局选项解析上下文（读配置层叠）。
    pub fn from_global(g: &GlobalArgs) -> Result<Self, CliError> {
        let cfg = config::merged_config()?;
        Ok(Ctx {
            style: OutputStyle::from(&g.format),
            verbose: g.verbose,
            template_dir: g.template_dir.clone().or(cfg.template_dir),
            default_machine: g.machine.clone().or(cfg.default_machine),
        })
    }

    /// 构建模板注册表：内置模板 + 模板目录中的 *.j2 文件。
    ///
    /// 目录模板以**完整文件名（含扩展名）**作为模板名（如 `my_op.j2`），
    /// 与 `nctool-tpl` 的目录加载器约定一致，且不与内置模板名冲突。
    pub fn build_registry(&self) -> Result<GCodeGenerator, CliError> {
        let mut gen = GCodeGenerator::new();
        if let Some(dir) = &self.template_dir {
            if !dir.exists() {
                return Err(CliError::new(
                    "io",
                    format!("模板目录不存在: {}", dir.display()),
                ));
            }
            let entries = std::fs::read_dir(dir)?;
            for entry in entries {
                let path = entry?.path();
                if !path.is_file() {
                    continue;
                }
                let is_j2 = path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("j2"));
                if !is_j2 {
                    continue;
                }
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                gen.registry_mut().add_file(
                    name,
                    TemplateCategory::General,
                    format!("文件模板: {}", path.display()),
                    &path,
                    vec![],
                )?;
            }
        }
        Ok(gen)
    }

    /// 解析机床配置：`--machine` / 配置默认值 / 内置 generic。
    ///
    /// 内置预设优先；否则查找配置文件中的自定义机床；都找不到则报错。
    pub fn resolve_machine(&self, explicit: Option<&str>) -> Result<MachineConfig, CliError> {
        let id = match explicit {
            Some(id) => id.to_string(),
            None => self
                .default_machine
                .clone()
                .unwrap_or_else(|| "generic".to_string()),
        };
        if let Some(preset) = MachinePreset::from_id(&id) {
            return Ok(preset.config());
        }
        let cfg = config::merged_config()?;
        if let Some(m) = cfg.machine.get(&id) {
            return Ok(m.clone());
        }
        Err(CliError::new(
            "machine_not_found",
            format!("未知机床 '{id}'（内置: generic/wfl_m65/index_ms40，或配置自定义机床）"),
        ))
    }

    /// 模板目录中是否存在指定文件模板（用于 `templates show` 等命令按路径定位）。
    pub fn find_template_file(&self, name_or_path: &str) -> Option<PathBuf> {
        let p = PathBuf::from(name_or_path);
        if p.is_file() {
            return Some(p);
        }
        if let Some(dir) = &self.template_dir {
            let candidate = dir.join(name_or_path);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }
}

/// 构建渲染上下文：`params`（裸值）+ `machine`（机床配置对象）。
///
/// 与 `nctool-core` 管线中的上下文构建保持一致（参数裸值注入，`machine` 含
/// 元信息 id/vendor/model 与 config 键值），保证 CLI 输出与库管线逐字节一致。
pub fn build_context(params: &ParameterSet, machine: &MachineConfig) -> minijinja::Value {
    let mut map: std::collections::BTreeMap<String, minijinja::Value> =
        std::collections::BTreeMap::new();
    for (k, v) in &params.values {
        let value = match v {
            nctool_core::ParamValue::Number(n) => minijinja::Value::from_serialize(n),
            nctool_core::ParamValue::String(s) => minijinja::Value::from_serialize(s),
            nctool_core::ParamValue::Bool(b) => minijinja::Value::from_serialize(b),
        };
        map.insert(k.clone(), value);
    }
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

/// 宽松模式渲染：未定义变量渲染为空字符串（缺失参数不再阻断）。
///
/// 使用 `nctool-tpl` 的 `Renderer::with_lenient()`，注册全部模板（内置 + 目录），
/// 并注入 `machine` 上下文。适用于"参数可缺省、缺失即留空"的柔性模板。
pub fn render_lenient(
    gen: &GCodeGenerator,
    template: &str,
    params: &ParameterSet,
    machine: &MachineConfig,
) -> Result<String, CliError> {
    let mut renderer = nctool_tpl::Renderer::new().with_lenient();
    for entry in gen.registry().list(None) {
        renderer.add_template(&entry.name, &entry.source_text)?;
    }
    let ctx = build_context(params, machine);
    let out = renderer.render_template(template, &ctx)?;
    Ok(out)
}

/// 构造参数集：`--params-file` + `--param`（显式参数优先）。
pub fn build_params(
    params_file: Option<&std::path::Path>,
    params: &[String],
) -> Result<ParameterSet, CliError> {
    args::build_parameter_set(params_file, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nctool_core::MachinePreset;

    #[test]
    fn ctx_default_machine_is_generic() {
        let ctx = Ctx {
            style: OutputStyle::Text,
            verbose: false,
            template_dir: None,
            default_machine: None,
        };
        let m = ctx.resolve_machine(None).unwrap();
        assert_eq!(m.id, "generic");
    }

    #[test]
    fn resolve_builtin_preset() {
        let ctx = Ctx {
            style: OutputStyle::Text,
            verbose: false,
            template_dir: None,
            default_machine: None,
        };
        let wfl = ctx.resolve_machine(Some("wfl_m65")).unwrap();
        assert_eq!(wfl.vendor, "WFL");
        let idx = ctx.resolve_machine(Some("index_ms40")).unwrap();
        assert_eq!(idx.vendor, "INDEX");
    }

    #[test]
    fn unknown_machine_errors() {
        let ctx = Ctx {
            style: OutputStyle::Text,
            verbose: false,
            template_dir: None,
            default_machine: None,
        };
        assert!(ctx.resolve_machine(Some("no_such")).is_err());
    }

    #[test]
    fn build_context_injects_machine_and_params() {
        let mut ps = ParameterSet::new();
        ps.set_number("x", 21.0);
        let machine = MachinePreset::Generic.config();
        let ctx = build_context(&ps, &machine);
        let x = ctx.get_attr("x").unwrap();
        assert!(x.is_number());
        let m = ctx.get_attr("machine").unwrap();
        assert_eq!(m.get_attr("id").unwrap().to_string(), "generic");
        assert_eq!(m.get_attr("linear").unwrap().to_string(), "G1");
    }

    #[test]
    fn render_lenient_missing_variable_blank() {
        // 宽松模式：直接引用的未定义变量渲染为空，而不是报错
        let mut gen = GCodeGenerator::new();
        gen.registry_mut()
            .add_memory(
                "plain",
                nctool_core::registry::TemplateCategory::General,
                "",
                "G1 X{{ x }} ({{ note }})",
                vec![],
            )
            .unwrap();
        let ps = ParameterSet::new();
        let machine = MachinePreset::Generic.config();
        let out = render_lenient(&gen, "plain", &ps, &machine).unwrap();
        assert_eq!(out, "G1 X ()");
    }

    #[test]
    fn render_lenient_filter_chain_still_validates_value() {
        // 宽松模式只放宽"未定义变量"，NC 过滤器对 undefined 仍报错（不能转 f64）
        let gen = GCodeGenerator::new();
        let ps = ParameterSet::new();
        let machine = MachinePreset::Generic.config();
        let err = render_lenient(&gen, "program_header", &ps, &machine).unwrap_err();
        assert!(
            err.message.contains("undefined"),
            "应报 undefined 转换错误: {}",
            err.message
        );
    }
}
