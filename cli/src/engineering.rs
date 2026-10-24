//! Host-owned project persistence for declarative plugin workbenches.
use nctool_assets::{JsonStore, SafePath};
use nctool_plugin_sdk::*;
use nctool_runtime::{
    config,
    package::{Package, Workbench},
    Runtime,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

pub struct Engineering {
    pub views: Vec<Value>,
    root: PathBuf,
    registrations: BTreeMap<(String, String), Workbench>,
    preview_slots: AtomicUsize,
}
pub struct PreviewSlot<'a>(&'a AtomicUsize);
impl Drop for PreviewSlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Engineering {
    pub fn preview_slot(&self) -> PluginResult<PreviewSlot<'_>> {
        self.preview_slots
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                if n < 2 {
                    Some(n + 1)
                } else {
                    None
                }
            })
            .map(|_| PreviewSlot(&self.preview_slots))
            .map_err(|_| PluginError::new("plugin_busy", "预览繁忙，请稍后重试"))
    }
    pub fn prepare(
        &self,
        runtime: &Arc<Runtime>,
        action: &str,
        input: Value,
    ) -> PluginResult<Value> {
        if let Some(prepare) = self
            .registrations
            .values()
            .find(|view| view.generate_action == action)
            .and_then(|view| view.prepare_action.as_ref())
        {
            let prepared = runtime.invoke(prepare, input)?;
            prepared
                .data
                .get("input")
                .filter(|v| v.is_object())
                .cloned()
                .ok_or_else(|| PluginError::new("invalid_output", "生成准备必须返回 input 对象"))
        } else {
            Ok(input)
        }
    }
    pub fn collections(&self) -> Vec<AssetCollection> {
        self.registrations
            .iter()
            .map(|((owner, view), spec)| AssetCollection {
                id: format!("documents.{owner}.{}", spec.kind),
                export_action: format!("$documents/{owner}/{view}/export"),
                import_action: format!("$documents/{owner}/{view}/import"),
            })
            .collect()
    }
    pub fn collection_call(&self, action: &str, input: &Value) -> PluginResult<ActionResult> {
        let parts: Vec<_> = action.split('/').collect();
        let [_, owner, view, op] = parts.as_slice() else {
            return Err(PluginError::new("not_found", "未知文档集合"));
        };
        let store = self.store(owner, view)?;
        let stores = [("projects", &store)];
        let data = match *op {
            "export" => nctool_assets::export_collection(&stores).map_err(crate::asset_error)?,
            "import" => {
                if let Some(receipt) = input.get("rollback") {
                    nctool_assets::rollback_collection(&stores, receipt)
                        .map_err(crate::asset_error)?;
                    json!({"rolled_back":true})
                } else {
                    nctool_assets::import_collection(
                        &stores,
                        &input["collection"],
                        input["validate_only"].as_bool().unwrap_or(false),
                        |_, _, document| {
                            Self::validate_document(document)
                                .map_err(|e| nctool_assets::WriteError::Corrupt(e.message))
                        },
                    )
                    .map_err(crate::asset_error)?
                }
            }
            _ => return Err(PluginError::new("not_found", "未知文档集合动作")),
        };
        Ok(ActionResult::data(data))
    }
    fn validate_document(document: &Value) -> PluginResult<()> {
        if !document.is_object()
            || document["format_version"] != 1
            || !document["raw"].is_string()
            || !document["invalid"].is_object()
            || !document["formats"].is_array()
            || !document["template"].is_string()
            || document["invalid"]
                .as_object()
                .is_some_and(|map| map.values().any(|v| !v.is_string()))
            || document["formats"]
                .as_array()
                .is_some_and(|list| list.iter().any(|v| !v.is_string()))
        {
            return Err(PluginError::new("invalid_input", "项目封装无效"));
        }
        Ok(())
    }
    pub fn new(home: &Path, workspace: &Path, runtime: &Runtime) -> PluginResult<Self> {
        let active = runtime.descriptors();
        let mut views = vec![];
        let mut registrations = BTreeMap::new();
        for (manifest, path) in config::installed(home)? {
            if !active.iter().any(|d| d.id == manifest.descriptor.id) {
                continue;
            }
            if let Some(package) = Package::read(&path, &manifest)? {
                for view in package.workbenches {
                    let mut value = serde_json::to_value(&view)?;
                    value["plugin"] = json!(package.id);
                    views.push(value);
                    registrations.insert((package.id.clone(), view.id.clone()), view);
                }
            }
        }
        Ok(Self {
            views,
            registrations,
            root: workspace.to_path_buf(),
            preview_slots: AtomicUsize::new(0),
        })
    }
    pub fn view(&self, owner: &str, id: &str) -> PluginResult<&Workbench> {
        self.registrations
            .get(&(owner.into(), id.into()))
            .ok_or_else(|| PluginError::new("not_found", "工作台未启用"))
    }
    fn store(&self, owner: &str, view: &str) -> PluginResult<JsonStore> {
        let kind = &self.view(owner, view)?.kind;
        let mut path = self.root.clone();
        for component in [".nctool", "plugin-data", owner, kind] {
            path = SafePath::from_root(&path)
                .map_err(crate::asset_error)?
                .resolve(component)
                .map_err(crate::asset_error)?;
            match std::fs::create_dir(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        JsonStore::new(&path).map_err(crate::asset_error)
    }
    pub fn documents(
        &self,
        owner: &str,
        view: &str,
        op: &str,
        input: &Value,
    ) -> PluginResult<Value> {
        let store = self.store(owner, view)?;
        let name = input["id"].as_str().unwrap_or("");
        match op {
            "list" => Ok(json!({"items":store.list().map_err(crate::asset_error)?})),
            "read" => {
                let (document, fingerprint) = store.read(name).map_err(crate::asset_error)?;
                Ok(json!({"document":document,"fingerprint":fingerprint}))
            }
            "save" => {
                if input
                    .get("expected")
                    .is_some_and(|v| !v.is_null() && !v.is_string())
                {
                    return Err(PluginError::new(
                        "invalid_input",
                        "expected 必须为 null 或旧指纹字符串",
                    ));
                }
                let document = input
                    .get("document")
                    .filter(|v| v.is_object())
                    .ok_or_else(|| PluginError::new("invalid_input", "项目必须为对象"))?;
                Self::validate_document(document)?;
                let fingerprint = store
                    .save(name, document, input["expected"].as_str())
                    .map_err(crate::asset_error)?;
                Ok(json!({"fingerprint":fingerprint}))
            }
            "remove" => {
                let expected = input["expected"]
                    .as_str()
                    .ok_or_else(|| PluginError::new("invalid_input", "删除需要读取时的指纹"))?;
                store.remove(name, expected).map_err(crate::asset_error)?;
                Ok(json!({"removed":name}))
            }
            _ => Err(PluginError::new("not_found", op)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projects_preserve_invalid_drafts_conflicts_and_collection_rollback() {
        let root = std::env::temp_dir().join(format!(
            "engineering-documents-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let view:Workbench=serde_json::from_value(json!({"id":"editor","title":"Editor","kind":"project","analyze_action":"demo.analyze","generate_action":"demo.generate","schema":{"type":"object"},"example":{},"formats":["json"]})).unwrap();
        let host = Engineering {
            views: vec![],
            root: root.clone(),
            registrations: BTreeMap::from([(("demo".into(), "editor".into()), view)]),
            preview_slots: AtomicUsize::new(0),
        };
        let doc = json!({"format_version":1,"raw":"{unfinished","invalid":{"/x":"-"},"formats":["json"],"template":""});
        let fp = host
            .documents(
                "demo",
                "editor",
                "save",
                &json!({"id":"one","document":doc,"expected":null}),
            )
            .unwrap()["fingerprint"]
            .clone();
        assert_eq!(
            host.documents("demo", "editor", "read", &json!({"id":"one"}))
                .unwrap()["document"],
            doc
        );
        assert_eq!(
            host.documents(
                "demo",
                "editor",
                "save",
                &json!({"id":"one","document":doc})
            )
            .unwrap_err()
            .code,
            "conflict"
        );
        assert!(host
            .documents("other", "editor", "list", &json!({}))
            .is_err());
        assert!(host
            .documents("demo", "editor", "read", &json!({"id":"../../escape"}))
            .is_err());
        let exported = host
            .collection_call("$documents/demo/editor/export", &json!({}))
            .unwrap();
        host.documents(
            "demo",
            "editor",
            "remove",
            &json!({"id":"one","expected":fp}),
        )
        .unwrap();
        let preflight = host
            .collection_call(
                "$documents/demo/editor/import",
                &json!({"collection":exported.data,"validate_only":true}),
            )
            .unwrap();
        assert_eq!(preflight.data["count"], 1);
        assert!(host
            .documents("demo", "editor", "read", &json!({"id":"one"}))
            .is_err());
        let imported = host
            .collection_call(
                "$documents/demo/editor/import",
                &json!({"collection":exported.data,"validate_only":false}),
            )
            .unwrap();
        host.collection_call(
            "$documents/demo/editor/import",
            &json!({"rollback":imported.data["receipt"]}),
        )
        .unwrap();
        assert!(host
            .documents("demo", "editor", "read", &json!({"id":"one"}))
            .is_err());
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
