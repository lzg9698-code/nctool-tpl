//! 配置层叠加载：全局 `~/.config/nctool/config.toml` + 项目 `./nctool.toml`。
//!
//! 项目配置覆盖全局配置（模板目录、默认机床、自定义机床、默认生成选项）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nctool_core::MachineConfig;
use serde::{Deserialize, Serialize};

use crate::output::CliError;

/// nctool 配置文件内容。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NctoolConfig {
    /// 默认模板目录（加载其中 *.j2）
    #[serde(default)]
    pub template_dir: Option<PathBuf>,
    /// 默认机床标识
    #[serde(default)]
    pub default_machine: Option<String>,
    /// 自定义机床配置（按 id 索引，供 `machine show <id>` 与渲染使用）
    #[serde(default)]
    pub machine: BTreeMap<String, MachineConfig>,
}

/// 全局配置路径：`$HOME/.config/nctool/config.toml`。
///
/// Windows 无 `HOME` 时回退 `USERPROFILE`。
fn global_config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("nctool")
            .join("config.toml"),
    )
}

fn read_config_file(path: &Path) -> Result<Option<NctoolConfig>, CliError> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| CliError::new("io", format!("读取配置文件失败 {}: {e}", path.display())))?;
    let cfg: NctoolConfig = toml::from_str(&text).map_err(|e| {
        CliError::new(
            "config",
            format!("配置文件解析失败 {}: {e}", path.display()),
        )
    })?;
    Ok(Some(cfg))
}

/// 加载层叠配置：(全局, 项目)。
pub fn load_config() -> Result<(Option<NctoolConfig>, Option<NctoolConfig>), CliError> {
    let global = match global_config_path() {
        Some(path) => read_config_file(&path)?,
        None => None,
    };
    let project_path = PathBuf::from("nctool.toml");
    let project = read_config_file(&project_path)?;
    Ok((global, project))
}

/// 合并全局与项目配置：项目覆盖全局的同名标量；自定义机床表按 id 合并（项目覆盖同名）。
pub fn merged_config() -> Result<NctoolConfig, CliError> {
    let (global, project) = load_config()?;
    let mut merged = global.unwrap_or_default();
    if let Some(proj) = project {
        if proj.template_dir.is_some() {
            merged.template_dir = proj.template_dir;
        }
        if proj.default_machine.is_some() {
            merged.default_machine = proj.default_machine;
        }
        for (id, cfg) in proj.machine {
            merged.machine.insert(id, cfg);
        }
    }
    Ok(merged)
}

/// 示例配置文件内容（`nctool config init` 生成）。
pub const EXAMPLE_CONFIG: &str = r#"# nctool 配置示例
# 配置层级：项目 ./nctool.toml 覆盖全局 ~/.config/nctool/config.toml

# 默认模板目录（加载其中 *.j2 模板）
# template_dir = "templates"

# 默认机床标识（内置 generic / wfl_m65 / index_ms40，或下方自定义机床）
# default_machine = "generic"

# 自定义机床：可在渲染时用 --machine <id> 引用
# [machine.hero_custom]
# id = "hero_custom"
# vendor = "HERO"
# model = "X9"
# [machine.hero_custom.config]
# program_prefix = "O"
# linear = "G1"
# max_spindle_rpm = "8000"
"#;

/// 生成示例配置到指定路径（已存在则报错，避免覆盖用户配置）。
pub fn init_config(path: &Path) -> Result<(), CliError> {
    if path.exists() {
        return Err(CliError::new(
            "config",
            format!("配置文件已存在，不覆盖: {}", path.display()),
        ));
    }
    std::fs::write(path, EXAMPLE_CONFIG)
        .map_err(|e| CliError::new("io", format!("写入配置文件失败 {}: {e}", path.display())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_example_config() {
        let cfg: NctoolConfig = toml::from_str(EXAMPLE_CONFIG).unwrap();
        assert!(cfg.template_dir.is_none());
        assert!(cfg.machine.is_empty());
    }

    #[test]
    fn parse_config_with_machine() {
        let toml = r#"
template_dir = "tpl"
default_machine = "wfl_m65"
[machine.hero]
id = "hero"
vendor = "H"
model = "X"
[machine.hero.config]
linear = "G1"
"#;
        let cfg: NctoolConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.template_dir.as_deref(), Some(Path::new("tpl")));
        assert_eq!(cfg.default_machine.as_deref(), Some("wfl_m65"));
        let m = cfg.machine.get("hero").unwrap();
        assert_eq!(m.vendor, "H");
        assert_eq!(m.get("linear"), Some("G1"));
    }

    #[test]
    fn invalid_toml_errors() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("nctool_test_badcfg_{}.toml", std::process::id()));
        std::fs::write(&path, "this is [ not toml").unwrap();
        let result = read_config_file(&path);
        std::fs::remove_file(&path).ok();
        assert!(result.is_err());
    }

    #[test]
    fn init_writes_and_rejects_existing() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("nctool_test_init_{}.toml", std::process::id()));
        init_config(&path).unwrap();
        assert!(path.exists());
        // 已存在 → 拒绝覆盖
        let err = init_config(&path).unwrap_err();
        assert!(err.message.contains("已存在"));
        std::fs::remove_file(&path).ok();
    }
}
