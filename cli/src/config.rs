//! 配置层叠加载：全局（平台约定路径）+ 项目 `./nctool.toml`（向上递归查找）。
//!
//! 项目配置覆盖全局配置（模板目录、默认机床、自定义机床、默认生成选项）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nctool_core::MachineConfig;
use serde::{Deserialize, Serialize};

use crate::output::CliError;

/// nctool 配置文件内容。
///
/// `deny_unknown_fields`：拼错的键（`templte_dir`）必须**响亮失败**（解析错误
/// → `read_config_file_lossy` 记 warning 并提示是哪个键），而不是被 serde
/// 静默忽略后回落到内置模板集 —— 后者正是本仓库零容忍的"配置没生效却看不出来"
/// （与 `PartSpec` / `ManifestFile` 的收紧口径一致）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// 已加载的层叠配置（含来源路径，供 `config show` 排障展示）。
#[derive(Debug, Clone, Default)]
pub struct LoadedConfig {
    /// 实际生效的全局配置文件路径（未发现则为 `None`）
    pub global_path: Option<PathBuf>,
    /// 实际生效的项目配置文件路径（未发现则为 `None`）
    pub project_path: Option<PathBuf>,
    /// 层叠合并后的配置
    pub merged: NctoolConfig,
    /// 配置文件问题（目前仅损坏 TOML）：降级为警告，不阻断命令执行。
    ///
    /// 读文件权限等 IO 错误仍然返回 `CliError`，因为此时无法安全判断
    /// 配置内容；只有确定是用户可修复的 TOML 语法错误才回退为空配置。
    pub warnings: Vec<String>,
}

/// 全局配置候选路径（按优先级）：
/// - Windows：`%APPDATA%\nctool\config.toml`
/// - Unix：`$XDG_CONFIG_HOME/nctool/config.toml`
/// - 兜底：`$HOME/.config/nctool/config.toml`（兼容既有路径；Windows 无 HOME 时取 `USERPROFILE`）
fn global_config_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if cfg!(windows) {
        if let Some(app) = std::env::var_os("APPDATA") {
            paths.push(PathBuf::from(app).join("nctool").join("config.toml"));
        }
    } else if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        paths.push(PathBuf::from(xdg).join("nctool").join("config.toml"));
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        paths.push(
            PathBuf::from(home)
                .join(".config")
                .join("nctool")
                .join("config.toml"),
        );
    }
    paths
}

/// 项目配置路径：从当前目录**向上递归**查找（到文件系统根为止），与 git 式
/// 工具的直觉一致——在项目子目录执行命令也能命中仓库根的 `nctool.toml`。
///
/// `pub(crate)`：`Ctx::project_config_path`（写路径）复用它定位待写文件。
pub(crate) fn find_project_config() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join("nctool.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = dir.parent()?;
    }
}

fn read_config_file(path: &Path) -> Result<Option<NctoolConfig>, CliError> {
    if !path.exists() {
        return Ok(None);
    }
    let text = crate::args::read_text_capped(path, "配置文件")?;
    let mut cfg: NctoolConfig = toml::from_str(&text).map_err(|e| {
        CliError::new(
            "config",
            format!("配置文件解析失败 {}: {e}", path.display()),
        )
    })?;
    // 相对 `template_dir` **相对配置文件所在目录**解析，而不是留给 `Ctx` 按
    // cwd 解析：`find_project_config` 从 cwd 向上找，子目录执行时会命中仓库根
    // 的配置，却把 `template_dir = "templates"` 拼到 `<子目录>/templates` ——
    // 若该处恰好存在同名目录就**静默换了一套模板集**（产出错误 G-code），
    // 不存在则报"模板目录不存在"、位置与文档（"子目录执行也能用"）矛盾。
    // 绝对路径原样通过；解析一次后与 cwd 无关。
    if let Some(dir) = cfg.template_dir.take() {
        cfg.template_dir = Some(resolve_against_config(path, dir));
    }
    Ok(Some(cfg))
}

/// 把配置文件里的相对目录解析为绝对路径（相对**配置文件所在目录**）。
fn resolve_against_config(cfg_path: &Path, dir: PathBuf) -> PathBuf {
    if dir.is_absolute() {
        return dir;
    }
    let base = cfg_path.parent().unwrap_or_else(|| Path::new("."));
    base.join(dir)
}

/// 读取配置的容错入口：损坏 TOML 降级为空配置并记录警告。
///
/// 配置是辅助输入，不应让 `templates list` / `machine show` 等只读命令
/// 因一个拼写错误全部 exit 4；但读文件权限等 IO 错误仍必须阻断，避免
/// 用户误以为配置已生效。
fn read_config_file_lossy(
    path: &Path,
    warnings: &mut Vec<String>,
) -> Result<Option<NctoolConfig>, CliError> {
    match read_config_file(path) {
        Ok(cfg) => Ok(cfg),
        Err(err) if err.kind == "config" => {
            warnings.push(format!("{}；已忽略该配置并使用默认值", err.message));
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

/// 加载层叠配置（全局候选取第一个存在的；项目配置向上递归查找）。
pub fn load() -> Result<LoadedConfig, CliError> {
    let mut warnings = Vec::new();
    let mut global_path = None;
    let mut global = None;
    for path in global_config_candidates() {
        if let Some(cfg) = read_config_file_lossy(&path, &mut warnings)? {
            global_path = Some(path);
            global = Some(cfg);
            break;
        }
    }
    let (project_path, project) = match find_project_config() {
        Some(path) => (
            Some(path.clone()),
            read_config_file_lossy(&path, &mut warnings)?,
        ),
        None => (None, None),
    };
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
    Ok(LoadedConfig {
        global_path,
        project_path,
        merged,
        warnings,
    })
}

/// 示例配置文件内容（`nctool config init` 生成）。
pub const EXAMPLE_CONFIG: &str = r#"# nctool 配置示例
# 配置层级：项目 ./nctool.toml 覆盖全局 ~/.config/nctool/config.toml

# 默认模板目录（加载其中 *.j2 模板）
# 相对路径**相对本配置文件所在目录**解析（与执行命令时的 cwd 无关）
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

    /// 相对 `template_dir` 相对**配置文件目录**解析：子目录执行时不会再把
    /// 仓库根的 `template_dir = "templates"` 拼到 `<cwd>/templates`
    /// （静默换模板集 → 错 G-code）。
    #[test]
    fn relative_template_dir_resolves_against_config_dir_not_cwd() {
        let root = std::env::temp_dir().join(format!("nctool_cfgdir_{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let cfg = root.join("nctool.toml");
        std::fs::write(&cfg, "template_dir = \"tpl\"\n").unwrap();

        let loaded = read_config_file(&cfg).unwrap().expect("应读到配置");
        let got = loaded.template_dir.expect("template_dir 应在");
        assert!(got.is_absolute(), "应解析为绝对路径: {got:?}");
        assert_eq!(got, root.join("tpl"), "应相对配置文件目录，而非 cwd");

        // 绝对路径原样通过
        std::fs::write(&cfg, format!("template_dir = {}\n", toml_path_lit(&root))).unwrap();
        let loaded = read_config_file(&cfg).unwrap().unwrap();
        assert_eq!(loaded.template_dir.unwrap(), root);

        std::fs::remove_dir_all(&root).ok();
    }

    /// Windows 路径写进 TOML 需转义反斜杠；非 Windows 用正斜杠字面量。
    fn toml_path_lit(p: &Path) -> String {
        let s = p.to_string_lossy().replace('\\', "/");
        format!("\"{s}\"")
    }

    /// 拼错的键必须响亮失败（deny_unknown_fields），而不是被静默忽略后
    /// 回落内置模板集。
    #[test]
    fn unknown_config_key_is_rejected_not_silently_ignored() {
        let err = toml::from_str::<NctoolConfig>("templte_dir = \"templates\"\n")
            .expect_err("拼写错误的键应解析失败");
        assert!(
            err.to_string().contains("templte_dir"),
            "错误信息应点名未知键: {err}"
        );
    }
}
