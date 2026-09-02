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
    /// 一次性加载的层叠配置（含来源路径，供各命令复用，避免重复读盘）
    pub loaded: config::LoadedConfig,
}

impl Ctx {
    /// 从全局选项解析上下文（读配置层叠）。
    pub fn from_global(g: &GlobalArgs) -> Result<Self, CliError> {
        let loaded = config::load()?;
        // 损坏的 TOML 已在配置层降级为空配置；主动提示用户，但不阻断
        // 当前命令。这样只读命令仍可使用内置模板/机床，用户也不会误以为
        // 配置已生效。
        for warning in &loaded.warnings {
            eprintln!("warning: {warning}");
        }
        Ok(Ctx {
            style: OutputStyle::from(&g.format),
            verbose: g.verbose,
            template_dir: g
                .template_dir
                .clone()
                .or_else(|| loaded.merged.template_dir.clone()),
            default_machine: g
                .machine
                .clone()
                .or_else(|| loaded.merged.default_machine.clone()),
            loaded,
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
        // 自定义机床：复用启动时缓存的一次性配置加载（不再重复读盘）
        if let Some(m) = self.loaded.merged.machine.get(&id) {
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

/// 构造参数集：`--params-file` + `--param`（显式参数优先）。
///
/// 渲染上下文的构建（参数裸值 + `machine` 注入）与宽松渲染均已收编到
/// `nctool-core` 管线（`GCodeGenerator::generate` / `generate_lenient`），
/// CLI 不再持有副本，保证两条路径输出逐字节一致。
pub fn build_params(
    params_file: Option<&std::path::Path>,
    params: &[String],
) -> Result<ParameterSet, CliError> {
    args::build_parameter_set(params_file, params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctx_default_machine_is_generic() {
        let ctx = Ctx {
            style: OutputStyle::Text,
            verbose: false,
            template_dir: None,
            default_machine: None,
            loaded: Default::default(),
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
            loaded: Default::default(),
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
            loaded: Default::default(),
        };
        assert!(ctx.resolve_machine(Some("no_such")).is_err());
    }
}
