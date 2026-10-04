//! Local plugin installation and restart-applied profile configuration.
use crate::external::{ExternalManifest, ExternalPlugin};
use nctool_assets::{WriteError, WriteKernel};
use nctool_plugin_sdk::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub profile: String,
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
    pub providers: BTreeMap<String, String>,
    pub plugins: BTreeMap<String, Value>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            profile: "template".into(),
            enabled: vec![],
            disabled: vec![],
            providers: BTreeMap::new(),
            plugins: BTreeMap::new(),
        }
    }
}
fn path(home: &Path) -> PathBuf {
    home.join("config.json")
}
pub fn load(home: &Path) -> PluginResult<(Config, Option<nctool_assets::FileFingerprint>)> {
    let p = path(home);
    let fp = WriteKernel::read_fingerprint(&p).map_err(asset_error)?;
    if fp.is_none() {
        return Ok((Config::default(), None));
    }
    let c: Config = serde_json::from_value(parse_json(&std::fs::read(p)?)?)?;
    validate(&c)?;
    Ok((c, fp))
}
pub fn validate(c: &Config) -> PluginResult<()> {
    if !["template", "nc"].contains(&c.profile.as_str()) {
        return Err(PluginError::new("invalid_profile", &c.profile));
    }
    for id in c
        .enabled
        .iter()
        .chain(&c.disabled)
        .chain(c.plugins.keys())
        .chain(c.providers.values())
    {
        if !crate::valid_id(id) {
            return Err(PluginError::new("invalid_plugin_id", id));
        }
    }
    Ok(())
}
pub fn save(
    home: &Path,
    c: &Config,
    expected: Option<nctool_assets::FileFingerprint>,
) -> PluginResult<()> {
    validate(c)?;
    std::fs::create_dir_all(home)?;
    WriteKernel::write_guarded(&path(home), &serde_json::to_vec_pretty(c)?, expected)
        .map_err(asset_error)?;
    Ok(())
}
pub fn set_enabled(home: &Path, id: &str, enabled: bool) -> PluginResult<()> {
    if !crate::valid_id(id) {
        return Err(PluginError::new("invalid_plugin_id", id));
    }
    let (mut config, fp) = load(home)?;
    config.enabled.retain(|s| s != id);
    config.disabled.retain(|s| s != id);
    if enabled {
        config.enabled.push(id.into());
    } else {
        config.disabled.push(id.into());
    }
    save(home, &config, fp)
}
pub fn installed(home: &Path) -> PluginResult<Vec<(ExternalManifest, PathBuf)>> {
    let root = home.join("plugins");
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && !entry.file_name().to_string_lossy().starts_with('.') {
            let manifest = ExternalManifest::read(&entry.path())?;
            if entry.file_name().to_str() != Some(&manifest.descriptor.id) {
                return Err(PluginError::new(
                    "plugin_directory",
                    "plugin directory must match its ID",
                ));
            }
            out.push((manifest, entry.path()));
        }
    }
    out.sort_by(|a, b| a.0.descriptor.id.cmp(&b.0.descriptor.id));
    Ok(out)
}
pub fn external_plugins(home: &Path, config: &Config) -> PluginResult<Vec<Arc<dyn Plugin>>> {
    let installed = installed(home)?;
    for id in &config.enabled {
        if !["template", "math", "nc", "process"].contains(&id.as_str())
            && !installed.iter().any(|(m, _)| &m.descriptor.id == id)
        {
            return Err(PluginError::new("plugin_missing", id));
        }
    }
    Ok(installed
        .into_iter()
        .filter(|(m, _)| {
            config.enabled.contains(&m.descriptor.id) && !config.disabled.contains(&m.descriptor.id)
        })
        .map(|(manifest, root)| Arc::new(ExternalPlugin::new(manifest, root)) as Arc<dyn Plugin>)
        .collect())
}
pub fn install(home: &Path, source: &Path) -> PluginResult<String> {
    let source = source.canonicalize()?;
    let manifest = ExternalManifest::read(&source)?;
    if ["template", "math", "nc", "process"].contains(&manifest.descriptor.id.as_str()) {
        return Err(PluginError::new(
            "reserved_plugin_id",
            &manifest.descriptor.id,
        ));
    }
    let plugin_root = home.join("plugins");
    std::fs::create_dir_all(&plugin_root)?;
    let plugin_root = plugin_root.canonicalize()?;
    if plugin_root.starts_with(&source) {
        return Err(PluginError::new(
            "recursive_install",
            "source contains installation directory",
        ));
    }
    let target = plugin_root.join(&manifest.descriptor.id);
    if target.exists() {
        return Err(PluginError::new(
            "conflict",
            "plugin already installed; uninstall it before installing a different version",
        ));
    }
    let stage = plugin_root.join(format!(
        ".stage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&stage)?;
    let result = (|| {
        copy_tree(&source, &stage, &mut 0, &mut 0)?;
        ExternalManifest::read(&stage)?;
        std::fs::rename(&stage, &target)?;
        Ok(manifest.descriptor.id.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(stage);
    }
    result
}
fn copy_tree(source: &Path, target: &Path, files: &mut usize, bytes: &mut u64) -> PluginResult<()> {
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            return Err(PluginError::new(
                "plugin_symlink",
                "plugin packages cannot contain symlinks",
            ));
        }
        let destination = target.join(entry.file_name());
        if ty.is_dir() {
            std::fs::create_dir(&destination)?;
            copy_tree(&entry.path(), &destination, files, bytes)?;
        } else if ty.is_file() {
            *files += 1;
            *bytes += entry.metadata()?.len();
            if *files > 2048 || *bytes > 64 * 1024 * 1024 {
                return Err(PluginError::new(
                    "package_limit",
                    "plugin package exceeds limits",
                ));
            }
            std::fs::copy(entry.path(), destination)?;
        } else {
            return Err(PluginError::new(
                "plugin_file_type",
                "plugin packages contain only regular files",
            ));
        }
    }
    Ok(())
}
pub fn uninstall(home: &Path, id: &str) -> PluginResult<()> {
    if !crate::valid_id(id) {
        return Err(PluginError::new("invalid_plugin_id", id));
    }
    set_enabled(home, id, false)?;
    let path = home.join("plugins").join(id);
    if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}
pub fn asset_error(error: WriteError) -> PluginError {
    let code = match error {
        WriteError::Conflict { .. } | WriteError::LockBusy { .. } => "conflict",
        WriteError::PathEscape { .. } => "path_escape",
        WriteError::NotFound(_) => "not_found",
        _ => "asset_error",
    };
    PluginError::new(code, error.to_string())
}

/// Generated output is written atomically only after the action has fully succeeded.
pub fn write_output(path: &Path, bytes: &[u8]) -> PluginResult<()> {
    WriteKernel::write_atomic(path, bytes).map_err(asset_error)
}
