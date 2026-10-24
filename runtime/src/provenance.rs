//! Bounded, deterministic change-detection fingerprints, never source authentication.
use nctool_plugin_sdk::*;
use std::{path::Path, time::UNIX_EPOCH};

pub fn fingerprint(bytes: &[u8]) -> String {
    format!(
        "fnv1a64-v1:{:016x}:{}",
        nctool_assets::FileFingerprint::of_bytes(bytes, UNIX_EPOCH).hash,
        bytes.len()
    )
}

/// Observe installed package files at host boot. Symlinks and oversized trees fail
/// closed; timestamps and machine-specific absolute paths are excluded.
pub fn package(root: &Path) -> PluginResult<String> {
    fn walk(
        root: &Path,
        dir: &Path,
        items: &mut Vec<(String, String)>,
        size: &mut u64,
    ) -> PluginResult<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                return Err(PluginError::new(
                    "plugin_symlink",
                    "package fingerprint refuses symbolic links",
                ));
            }
            if kind.is_dir() {
                // Interpreter caches are generated after startup and are not package sources.
                if entry.file_name() != "__pycache__" {
                    walk(root, &entry.path(), items, size)?;
                }
            } else if kind.is_file() {
                *size += entry.metadata()?.len();
                if *size > 64 * 1024 * 1024 || items.len() >= 4096 {
                    return Err(PluginError::new(
                        "package_limit",
                        "package fingerprint exceeds limits",
                    ));
                }
                let bytes = std::fs::read(entry.path())?;
                let name = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                items.push((name, fingerprint(&bytes)));
            }
        }
        Ok(())
    }
    let mut items = vec![];
    walk(root, root, &mut items, &mut 0)?;
    items.sort();
    Ok(fingerprint(&serde_json::to_vec(&items)?))
}

/// Config values are private unless their exact Schema node opts in with
/// x-nctool-public:true. writeOnly and credential-like names override that opt-in.
/// Omitted/private values share a marker: the digest cannot be used to guess them.
pub fn public_config(schema: &Value, config: &Value) -> String {
    fn project(schema: &Value, value: &Value) -> Value {
        if schema.get("writeOnly").and_then(Value::as_bool) == Some(true) {
            return json!("<redacted>");
        }
        if let Some(object) = value.as_object() {
            return Value::Object(
                object
                    .iter()
                    .map(|(key, value)| {
                        let sensitive = [
                            "password",
                            "secret",
                            "token",
                            "credential",
                            "private",
                            "authorization",
                            "api_key",
                            "apikey",
                            "pin",
                        ]
                        .iter()
                        .any(|needle| key.to_lowercase().contains(needle));
                        (
                            key.clone(),
                            if sensitive {
                                json!("<redacted>")
                            } else {
                                project(&schema["properties"][key], value)
                            },
                        )
                    })
                    .collect(),
            );
        }
        if let Some(array) = value.as_array() {
            return Value::Array(array.iter().map(|v| project(&schema["items"], v)).collect());
        }
        if schema.get("x-nctool-public").and_then(Value::as_bool) == Some(true) {
            value.clone()
        } else {
            json!("<redacted>")
        }
    }
    fingerprint(&serde_json::to_vec(&project(schema, config)).unwrap())
}
