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

/// Builtins supported by the current distribution (availability is feature-dependent).
pub const BUILTIN_PLUGIN_IDS: &[&str] = &["template", "parameters", "math"];

/// Builtin identities removed from this distribution. Their IDs stay reserved so a
/// same-named external plugin cannot take over a retired execution identity.
pub const RETIRED_PLUGIN_IDS: &[&str] = &[
    "nc",
    "nc-parameters",
    "nc-managed",
    "sync-turning",
    "undercut",
    "process",
    "process-parameters",
];

/// Domain namespaces that belonged to the removed builtins. These are the same
/// tokens the frontend uses to recognize retired machining intent, so the rule stays
/// a minimal reserved-word list rather than a classifier for third-party content:
/// only these exact ids and `<token>.` prefixes are reserved.
const RETIRED_DOMAIN_TOKENS: &[&str] = &["nc", "machine", "process", "sync-turning", "undercut"];

/// True when the id names a removed builtin plugin.
pub fn is_retired_plugin_id(id: &str) -> bool {
    RETIRED_PLUGIN_IDS.contains(&id)
}

/// True for a retired action or service identity namespaced by a removed builtin,
/// for example `nc.generate`, `machine.list` or `sync-turning.generate`.
pub fn is_retired_namespace_id(id: &str) -> bool {
    RETIRED_DOMAIN_TOKENS.iter().any(|token| {
        id.strip_prefix(token)
            .is_some_and(|rest| rest.starts_with('.'))
    })
}

/// Exact asset-collection ids the removed builtins actually declared (read off the
/// archived descriptors and business helpers, not guessed): the NC machine collection,
/// the managed NC parameter collection, and the sync-turning / undercut business
/// collections.
const RETIRED_COLLECTION_IDS: &[&str] = &[
    "nc",
    "nc-parameters",
    "sync-turning-assets",
    "undercut-assets",
];

/// True for a retired asset-collection identity.
pub fn is_retired_collection_id(id: &str) -> bool {
    RETIRED_COLLECTION_IDS.contains(&id)
}

pub fn ensure_plugin_allowed(id: &str) -> PluginResult<()> {
    if !crate::valid_id(id) {
        return Err(PluginError::new("invalid_plugin_id", id));
    }
    if is_retired_plugin_id(id) {
        return Err(PluginError::new("retired_plugin", id));
    }
    Ok(())
}

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
    if c.profile != "template" {
        return Err(PluginError::new(
            "invalid_profile",
            "此版本仅支持 template profile；旧 nc profile 请先运行 nctool config check/migrate。",
        ));
    }
    for id in c
        .enabled
        .iter()
        .chain(&c.disabled)
        .chain(c.plugins.keys())
        .chain(c.providers.values())
    {
        ensure_plugin_allowed(id)?;
    }
    // Provider keys are service identities: a binding to a retired service can never
    // resolve and must not survive validation.
    for service in c.providers.keys() {
        if is_retired_namespace_id(service) {
            return Err(PluginError::new("retired_identity", service));
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
    ensure_plugin_allowed(id)?;
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
/// A plugin directory that exists but cannot be loaded. It is kept as a record so the
/// failure stays visible — and can be reported when that plugin is selected — instead
/// of being silently dropped or aborting the whole inventory.
#[derive(Clone, Debug)]
pub struct InstalledError {
    /// The directory name under `plugins/`.
    pub directory: String,
    /// The directory path.
    pub path: PathBuf,
    /// Why the directory could not be loaded.
    pub error: PluginError,
}

/// Result of [`scan_installed`]: the loadable manifests and the damaged directories.
pub type InstalledScan = (Vec<(ExternalManifest, PathBuf)>, Vec<InstalledError>);

/// Scan `plugins/`: every loadable manifest, plus a record of directories that carry a
/// damaged manifest.
///
/// A directory without a `plugin.json` is unrelated to plugins and is skipped; a
/// permission or other IO failure while listing or reading is returned as a hard error
/// so it is never masked as a merely damaged manifest.
pub fn scan_installed(home: &Path) -> PluginResult<InstalledScan> {
    let root = home.join("plugins");
    if !root.exists() {
        return Ok((vec![], vec![]));
    }
    let mut installed = vec![];
    let mut broken = vec![];
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let directory = entry.file_name().to_string_lossy().into_owned();
        match std::fs::symlink_metadata(path.join("plugin.json")) {
            // No manifest at all: not a plugin directory. Skip it instead of failing.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            // A directory we cannot even probe is a real access failure, not a damaged
            // plugin; surface it rather than hiding it.
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        match ExternalManifest::read(&path) {
            Ok(manifest) if directory == manifest.descriptor.id => installed.push((manifest, path)),
            Ok(manifest) => broken.push(InstalledError {
                error: PluginError::new(
                    "plugin_directory",
                    format!("插件目录名与清单 id {} 不一致", manifest.descriptor.id),
                ),
                directory,
                path,
            }),
            // A manifest that exists but cannot be read is an IO/permission failure;
            // only a parse or validation failure is recorded as a damaged manifest.
            Err(error) if error.code == "io" => return Err(error),
            Err(error) => broken.push(InstalledError {
                directory,
                path,
                error,
            }),
        }
    }
    installed.sort_by(|a, b| a.0.descriptor.id.cmp(&b.0.descriptor.id));
    broken.sort_by(|a, b| a.directory.cmp(&b.directory));
    Ok((installed, broken))
}

pub fn installed(home: &Path) -> PluginResult<Vec<(ExternalManifest, PathBuf)>> {
    Ok(scan_installed(home)?.0)
}
pub fn external_plugins(home: &Path, config: &Config) -> PluginResult<Vec<Arc<dyn Plugin>>> {
    let (installed, broken) = scan_installed(home)?;
    for id in &config.enabled {
        ensure_plugin_allowed(id)?;
        if BUILTIN_PLUGIN_IDS.contains(&id.as_str())
            || installed.iter().any(|(m, _)| &m.descriptor.id == id)
        {
            continue;
        }
        // An enabled plugin that is not loadable stays fail-closed: refuse to start
        // without it, and name the concrete manifest failure when the directory is
        // present rather than silently starting without the selected plugin.
        if let Some(record) = broken.iter().find(|record| &record.directory == id) {
            return Err(PluginError::new(
                "plugin_broken",
                format!("已启用插件 {id} 的目录无法加载：{}", record.error),
            ));
        }
        return Err(PluginError::new("plugin_missing", id));
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
    ensure_plugin_allowed(&manifest.descriptor.id)?;
    if BUILTIN_PLUGIN_IDS.contains(&manifest.descriptor.id.as_str()) {
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
/// Map an asset-layer error to a plugin error whose message never contains a filesystem path.
///
/// This is the only form exposed over HTTP; absolute paths stay in CLI/stderr output
/// (see the redaction contract in `nctool_assets::lock`). The code and the distinction
/// between a conflicting write and a busy lock are preserved.
pub fn asset_error(error: WriteError) -> PluginError {
    let code = match &error {
        WriteError::Conflict { .. } | WriteError::LockBusy { .. } => "conflict",
        WriteError::PathEscape { .. } => "path_escape",
        WriteError::NotFound(_) => "not_found",
        _ => "asset_error",
    };
    PluginError::new(code, error.public_message())
}

/// Generated output is written atomically only after the action has fully succeeded.
///
/// This is a CLI-only path, so the error keeps the path-bearing [`WriteError`] text for
/// local diagnosis instead of the redacted [`asset_error`] form.
pub fn write_output(path: &Path, bytes: &[u8]) -> PluginResult<()> {
    WriteKernel::write_atomic(path, bytes).map_err(|error| {
        let code = match &error {
            WriteError::Conflict { .. } | WriteError::LockBusy { .. } => "conflict",
            WriteError::PathEscape { .. } => "path_escape",
            WriteError::NotFound(_) => "not_found",
            _ => "asset_error",
        };
        PluginError::new(code, error.to_string())
    })
}

#[cfg(test)]
mod asset_error_tests {
    use super::*;

    #[test]
    fn http_facing_messages_never_include_absolute_paths() {
        let absolute = std::path::PathBuf::from("/home/someone/workspace/templates/demo.j2");
        // Every variant that can carry a path must be covered: the HTTP mappers in the
        // template/parameter plugins and the runtime share `public_message()`.
        for error in [
            WriteError::Conflict {
                path: absolute.clone(),
                expected: None,
                actual: None,
            },
            WriteError::LockBusy {
                path: absolute.clone(),
            },
            WriteError::ReadOnly {
                path: absolute.clone(),
            },
            WriteError::PathEscape {
                rel: "demo.j2".into(),
                reason: format!("目标是指向别处的符号链接，拒绝写入: {}", absolute.display()),
            },
            WriteError::NotFound(format!("删除目标不存在: {}", absolute.display())),
            WriteError::Corrupt(format!(
                "目标位置已被占用（存在同名目录或非普通文件）: {}",
                absolute.display()
            )),
            WriteError::Io(std::io::Error::other(format!(
                "cannot open {}",
                absolute.display()
            ))),
        ] {
            let mapped = asset_error(error);
            assert!(
                !mapped.message.contains("/home/someone"),
                "path leaked over HTTP: {}",
                mapped.message
            );
        }
        // The two write failures stay distinguishable to clients.
        assert_eq!(
            asset_error(WriteError::Conflict {
                path: absolute.clone(),
                expected: None,
                actual: None,
            })
            .message,
            "写入冲突：资产已被其他进程或窗口修改，请刷新后重试"
        );
        assert_eq!(
            asset_error(WriteError::LockBusy { path: absolute }).code,
            "conflict"
        );
    }
}
