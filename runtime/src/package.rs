//! Declarative host views, independent from the process protocol descriptor.
use crate::external::ExternalManifest;
use nctool_plugin_sdk::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub format_version: u32,
    pub id: String,
    pub workbenches: Vec<Workbench>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workbench {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub analyze_action: String,
    pub generate_action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepare_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_action: Option<String>,
    pub schema: Value,
    pub example: Value,
    #[serde(default)]
    pub tables: Vec<RecordTable>,
    pub formats: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordTable {
    pub field: String,
    pub title: String,
    pub row: Value,
}
impl Package {
    pub fn read(root: &Path, manifest: &ExternalManifest) -> PluginResult<Option<Self>> {
        let path = root.join("nctool.package.json");
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !meta.is_file() || meta.len() > 1024 * 1024 {
            return Err(PluginError::new(
                "invalid_package",
                "工作台清单必须为不超过 1 MiB 的普通文件",
            ));
        }
        let package: Self = serde_json::from_value(parse_json(&std::fs::read(path)?)?)?;
        if package.format_version != 1
            || package.id != manifest.descriptor.id
            || package.workbenches.is_empty()
            || package.workbenches.len() > 16
        {
            return Err(PluginError::new(
                "invalid_package",
                "工作台清单版本或归属不匹配",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut kinds = BTreeSet::new();
        let mut generators = BTreeSet::new();
        for view in &package.workbenches {
            if !crate::valid_id(&view.id)
                || !crate::valid_id(&view.kind)
                || !ids.insert(&view.id)
                || !kinds.insert(&view.kind)
                || !generators.insert(&view.generate_action)
                || view.title.is_empty()
                || view.title.len() > 256
            {
                return Err(PluginError::new("invalid_package", "工作台标识或标题无效"));
            }
            for action in [&view.analyze_action, &view.generate_action] {
                if !manifest.actions.iter().any(|a| &a.id == action) {
                    return Err(PluginError::new(
                        "invalid_package",
                        "工作台动作必须由本插件提供",
                    ));
                }
            }
            if view
                .prepare_action
                .as_ref()
                .is_some_and(|id| !manifest.actions.iter().any(|a| &a.id == id))
            {
                return Err(PluginError::new(
                    "invalid_package",
                    "生成准备动作必须由本插件提供",
                ));
            }
            if view
                .template_action
                .as_ref()
                .is_some_and(|id| !manifest.actions.iter().any(|a| &a.id == id))
            {
                return Err(PluginError::new(
                    "invalid_package",
                    "模板读取动作必须由本插件提供",
                ));
            }
            if view.schema["type"] != "object" || !view.example.is_object() {
                return Err(PluginError::new(
                    "invalid_package",
                    "工程项目必须声明对象 Schema 和对象示例",
                ));
            }
            compile_schema(&view.schema)?;
            validate_schema(&view.schema, &view.example, "invalid_package")?;
            if view.formats.is_empty()
                || view.formats.len() > 16
                || view.formats.iter().any(|f| !crate::valid_id(f))
            {
                return Err(PluginError::new("invalid_package", "导出格式声明无效"));
            }
            let mut fields = BTreeSet::new();
            for table in &view.tables {
                if !crate::valid_id(&table.field)
                    || !table.row.is_object()
                    || view.schema["properties"][&table.field]["items"]["type"] != "object"
                    || !fields.insert(&table.field)
                    || view.schema["properties"][&table.field]["type"] != "array"
                {
                    return Err(PluginError::new(
                        "invalid_package",
                        "记录表必须绑定顶层数组字段",
                    ));
                }
                validate_schema(
                    &view.schema["properties"][&table.field]["items"],
                    &table.row,
                    "invalid_package",
                )?;
            }
        }
        Ok(Some(package))
    }
}
