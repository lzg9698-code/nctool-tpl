//! Explicit history retention and byte-preserving backup, independent of plugin boot.
use crate::workbench::{summary_of, Run};
use nctool_assets::{SafePath, WriteKernel};
use nctool_plugin_sdk::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};
const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
fn portable_id(id: &str) -> bool {
    let base = id
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    nctool_runtime::valid_id(id)
        && !["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
        && !(base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    fingerprint: String,
    raw: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    format_version: u32,
    records: BTreeMap<String, Record>,
}
fn read(path: &Path, limit: u64) -> PluginResult<Vec<u8>> {
    let mut bytes = vec![];
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(PluginError::new(
            "history_limit",
            "history data exceeds size limit",
        ));
    }
    Ok(bytes)
}
fn guard(root: &SafePath, id: &str) -> PluginResult<File> {
    if !portable_id(id) {
        return Err(PluginError::new(
            "invalid_run_id",
            "invalid history identifier",
        ));
    }
    let guard = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(
            root.resolve(&format!("{id}.nctool.lock"))
                .map_err(crate::asset_error)?,
        )?;
    guard
        .try_lock()
        .map_err(|_| PluginError::new("conflict", "history record is active or locked"))?;
    Ok(guard)
}
fn terminal(run: &Run) -> bool {
    ["succeeded", "failed", "cancelled", "interrupted"].contains(&run.status.as_str())
}
fn root(workspace: &Path) -> PluginResult<SafePath> {
    SafePath::from_root(&workspace.join(".nctool/runs")).map_err(crate::asset_error)
}
fn record(root: &SafePath, id: &str) -> PluginResult<(Record, Run)> {
    let bytes = read(
        &root
            .resolve(&format!("{id}.json"))
            .map_err(crate::asset_error)?,
        64 * 1024 * 1024,
    )?;
    let run: Run = serde_json::from_value(parse_json(&bytes)?)?;
    if run.id != id || !terminal(&run) {
        return Err(PluginError::new(
            "conflict",
            "only terminal history records may be archived",
        ));
    }
    Ok((
        Record {
            fingerprint: nctool_runtime::provenance::fingerprint(&bytes),
            raw: String::from_utf8(bytes)
                .map_err(|_| PluginError::new("invalid_history", "history must be UTF-8"))?,
        },
        run,
    ))
}
fn inventory(root: &SafePath) -> PluginResult<(Archive, Vec<File>, Vec<String>)> {
    let mut archive = Archive {
        format_version: 1,
        records: BTreeMap::new(),
    };
    let mut guards = vec![];
    let mut skipped = vec![];
    let mut bytes = 0usize;
    for entry in std::fs::read_dir(root.root())? {
        let entry = entry?;
        if !entry.file_type()?.is_file() || entry.path().extension().is_none_or(|e| e != "json") {
            continue;
        }
        let id = entry
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let lock = match guard(root, &id) {
            Ok(lock) => lock,
            Err(e) if e.code == "conflict" => {
                skipped.push(id);
                continue;
            }
            Err(e) => return Err(e),
        };
        let (value, _) = match record(root, &id) {
            Ok(value) => value,
            Err(e) if e.code == "conflict" => {
                skipped.push(id);
                continue;
            }
            Err(e) => return Err(e),
        };
        bytes += value.raw.len();
        if bytes as u64 > MAX_ARCHIVE || archive.records.len() >= 10000 {
            return Err(PluginError::new(
                "history_limit",
                "archive exceeds 10000 records or 256 MiB",
            ));
        }
        archive.records.insert(id, value);
        guards.push(lock);
    }
    skipped.sort();
    Ok((archive, guards, skipped))
}
fn write_archive(destination: &Path, archive: &Archive) -> PluginResult<()> {
    let bytes = serde_json::to_vec(archive)?;
    if bytes.len() as u64 > MAX_ARCHIVE {
        return Err(PluginError::new(
            "history_limit",
            "serialized archive exceeds 256 MiB",
        ));
    }
    WriteKernel::write_guarded(destination, &bytes, None).map_err(crate::asset_error)?;
    Ok(())
}
fn archive_location(root: &SafePath, destination: &Path) -> PluginResult<()> {
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()?;
    if parent.starts_with(root.root()) {
        return Err(PluginError::new(
            "invalid_path",
            "archives must be stored outside the live runs directory",
        ));
    }
    Ok(())
}
pub fn backup(workspace: &Path, destination: &Path) -> PluginResult<Value> {
    let root = root(workspace)?;
    archive_location(&root, destination)?;
    let (archive, _guards, skipped) = inventory(&root)?;
    write_archive(destination, &archive)?;
    Ok(json!({"records":archive.records.len(),"skipped":skipped,"archive":destination}))
}
/// Read-only by default. Applying requires the exact preview plus a new backup path.
pub fn prune(
    workspace: &Path,
    before_ms: u64,
    keep_latest: usize,
    expected: Option<&str>,
    backup: Option<&Path>,
) -> PluginResult<Value> {
    let root = root(workspace)?;
    let (mut archive, _guards, skipped) = inventory(&root)?;
    let mut ordered: Vec<_> = archive
        .records
        .iter()
        .map(|(id, r)| {
            let run: Run = serde_json::from_value(parse_json(r.raw.as_bytes())?)?;
            Ok((
                id.clone(),
                run.started_at,
                run.finished_at.unwrap_or(run.started_at),
            ))
        })
        .collect::<PluginResult<_>>()?;
    ordered.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let protected: std::collections::BTreeSet<_> = ordered
        .iter()
        .take(keep_latest)
        .map(|r| r.0.clone())
        .collect();
    let candidates: Vec<_> = ordered
        .into_iter()
        .filter(|(id, _, end)| *end < before_ms && !protected.contains(id))
        .map(|r| r.0)
        .collect();
    let selected: std::collections::BTreeSet<_> = candidates.iter().collect();
    archive.records.retain(|id, _| selected.contains(id));
    let token = nctool_runtime::provenance::fingerprint(&serde_json::to_vec(
        &json!({"before_ms":before_ms,"keep_latest":keep_latest,"records":archive.records,"skipped":skipped}),
    )?);
    if let Some(expected) = expected {
        if token != expected {
            return Err(PluginError::new(
                "conflict",
                "history retention preview changed; preview again",
            ));
        }
        let backup = backup.ok_or_else(|| {
            PluginError::new(
                "backup_required",
                "applying retention requires a new backup path",
            )
        })?;
        archive_location(&root, backup)?;
        write_archive(backup, &archive)?;
        // All record locks remain held from inventory through backup and deletion.
        // Deletion is per record; a partial filesystem failure is recoverable from backup.
        for id in &candidates {
            let path = root
                .resolve(&format!("{id}.json"))
                .map_err(crate::asset_error)?;
            if nctool_runtime::provenance::fingerprint(&read(&path, 64 * 1024 * 1024)?)
                != archive.records[id].fingerprint
            {
                return Err(PluginError::new(
                    "conflict",
                    "history changed after backup; deletion stopped",
                ));
            }
            std::fs::remove_file(path)?;
            let index = root.root().join("_index").join(format!("{id}.json"));
            if index.exists() {
                std::fs::remove_file(index)?;
            }
        }
    }
    Ok(
        json!({"applied":expected.is_some(),"fingerprint":token,"candidates":candidates,"skipped":skipped,"before_ms":before_ms,"keep_latest":keep_latest}),
    )
}
/// Restore into a new workspace only. Validate the entire archive before writing.
pub fn restore(destination: &Path, source: &Path) -> PluginResult<Value> {
    if destination.exists() {
        return Err(PluginError::new(
            "conflict",
            "restore requires a new workspace directory",
        ));
    }
    let archive: Archive = serde_json::from_value(parse_json(&read(source, MAX_ARCHIVE)?)?)?;
    if archive.format_version != 1 || archive.records.len() > 10000 {
        return Err(PluginError::new(
            "invalid_archive",
            "unsupported history archive",
        ));
    }
    let mut summaries = BTreeMap::new();
    let mut names = std::collections::BTreeSet::new();
    for (id, record) in &archive.records {
        if record.raw.len() > 64 * 1024 * 1024 {
            return Err(PluginError::new("history_limit", "record exceeds 64 MiB"));
        }
        if !portable_id(id)
            || !names.insert(id.to_ascii_lowercase())
            || nctool_runtime::provenance::fingerprint(record.raw.as_bytes()) != record.fingerprint
        {
            return Err(PluginError::new(
                "invalid_archive",
                "invalid record identifier or fingerprint",
            ));
        }
        let run: Run = serde_json::from_value(parse_json(record.raw.as_bytes())?)?;
        if run.id != *id || !terminal(&run) {
            return Err(PluginError::new(
                "invalid_archive",
                "archive contains nonterminal or mismatched record",
            ));
        }
        summaries.insert(id, summary_of(&run));
    }
    // Reserve a new destination with create_dir (never rename over a racing path).
    // Each file is then guarded create-only. On failure leave an explicit incomplete
    // marker and the recovery files; do not remove a directory the user may inspect.
    std::fs::create_dir(destination)?;
    let runs = destination.join(".nctool/runs");
    std::fs::create_dir_all(runs.join("_index"))?;
    let marker = destination.join(".nctool/restore-incomplete.json");
    let mark = WriteKernel::write_guarded(&marker, b"{\"incomplete\":true}", None)
        .map_err(crate::asset_error)?;
    for (id, record) in &archive.records {
        WriteKernel::write_guarded(
            &runs.join(format!("{id}.json")),
            record.raw.as_bytes(),
            None,
        )
        .map_err(crate::asset_error)?;
        WriteKernel::write_guarded(
            &runs.join("_index").join(format!("{id}.json")),
            &serde_json::to_vec(&summaries[id])?,
            None,
        )
        .map_err(crate::asset_error)?;
    }
    WriteKernel::remove_guarded(&marker, mark.revision).map_err(crate::asset_error)?;
    Ok(json!({"restored":archive.records.len(),"workspace":destination}))
}
