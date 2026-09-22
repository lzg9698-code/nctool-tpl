//! 变量库（`templates/variables.yaml`）：**按变量名**定义的全局参数规格。
//!
//! # 与模板头部 `{# PARAMS: #}` 的分工
//!
//! - **头部**是**模板局部**声明：这个模板用哪些参数、各自什么类型；
//! - **变量库**是**跨模板的全局定义**：同一个变量名在多台机床/多个模板上含义一致时
//!   只写一次，避免在 12 个模板里重复声明 `U_Q` 的候选值。
//! - **清单 `templates.yaml` 的 `params`** 是**本模板的覆盖层**，优先级最高。
//!
//! 合并顺序：**头部声明 → 变量库（按名覆盖）→ 清单 `params`**。
//!
//! # 来源与取舍
//!
//! 内容来自源项目 NCTool_V3 的 `configs/variable_repo.json`（62 个变量）。
//! 导入时有三条**刻意的不导入**，都是为了避免"静默产出错误 G-code"：
//!
//! 1. **不导入 `default_value`**：源库的默认值是**针对特定样件的预填值**
//!    （如 `U_A = 141.25` 是那根轴的有效长度）。规格默认值会让缺参**静默通过**
//!    （[`crate::validate`] 把 `spec.default` 视为已提供），用户少填一个零件尺寸
//!    就会拿到另一根轴的加工程序。CNC 零件尺寸没有"合理默认值"。
//! 2. **不导入 `description`**：头部描述是写给**这个模板**的，更贴近上下文；
//!    库里的描述作为 YAML 注释保留，不参与覆盖。
//! 3. **不导入表达式型默认值**：`CIRCLIP_X_DOWN_D = 120-0.54/2` 这类是**表达式**，
//!    按项目原则「模板只做变量替换，计算在 Rust 侧完成」，它们应由调用方预计算后注入。
//!
//! `options` 与类型则**全量导入**——那正是头部 `{# PARAMS: #}` 表达不了的约束，
//! 也是本模块存在的理由。

use std::collections::BTreeSet;
use std::path::Path;

use crate::manifest::{merge_params, ManifestError, ParamOverride};
use crate::model::ParamSpec;

/// 变量库文件名（位于模板目录根部）。
pub const VARIABLES_FILE: &str = "variables.yaml";

/// 变量库：一组**按变量名**生效的参数规格定义。
///
/// 内部用 [`ParamOverride`] 承载——与清单 `params` 同一套稀疏覆盖语义，
/// 因此"只写要改的字段"这一约定对库与清单完全一致。
#[derive(Debug, Clone, Default)]
pub struct VariableLibrary {
    entries: Vec<ParamOverride>,
}

impl VariableLibrary {
    /// 空库（所有变量都只有头部声明可用）。
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// 从 YAML 文本解析变量库。
    ///
    /// 接受的文档结构（两种写法均可）：
    ///
    /// ```yaml
    /// # 写法一：带 variables 键（推荐）
    /// variables:
    ///   - name: U_Q
    ///     kind: choice
    ///     options: [0, 8, 10, 12.5]
    /// ```
    ///
    /// ```yaml
    /// # 写法二：顶层直接是列表
    /// - name: U_Q
    ///   kind: choice
    /// ```
    ///
    /// 与清单解析同理，**不能用「先试 A 失败再试 B」**区分两种写法：带 `variables`
    /// 键的文档也能解析成裸列表吗？不能，但反过来——顶层列表解析成 `VariablesFile`
    /// 会因缺键而得到**空库**（若该字段带 `default`），于是第二种写法静默失效。
    /// 因此先解析成通用 `serde_yaml::Value` 显式判型。
    pub fn from_yaml(text: &str, origin: &Path) -> Result<Self, ManifestError> {
        let value: serde_yaml::Value =
            serde_yaml::from_str(text).map_err(|e| ManifestError::Parse {
                path: origin.to_path_buf(),
                source: e,
            })?;
        let entries: Vec<ParamOverride> = match value {
            serde_yaml::Value::Null => Vec::new(),
            serde_yaml::Value::Sequence(_) => {
                serde_yaml::from_str(text).map_err(|e| ManifestError::Parse {
                    path: origin.to_path_buf(),
                    source: e,
                })?
            }
            serde_yaml::Value::Mapping(map) => {
                if !map.contains_key(serde_yaml::Value::String("variables".to_string())) {
                    return Err(ManifestError::Parse {
                        path: origin.to_path_buf(),
                        source: serde::de::Error::custom(
                            "变量库顶层应为 `variables:` 键或直接的变量列表",
                        ),
                    });
                }
                let file: VariablesFile =
                    serde_yaml::from_str(text).map_err(|e| ManifestError::Parse {
                        path: origin.to_path_buf(),
                        source: e,
                    })?;
                file.variables
            }
            _ => {
                return Err(ManifestError::Parse {
                    path: origin.to_path_buf(),
                    source: serde::de::Error::custom(
                        "变量库顶层应为映射（`variables:` 键）或变量列表",
                    ),
                })
            }
        };
        Self::from_entries(entries, origin)
    }

    /// 校验并构造：**同名重复定义直接报错**。
    ///
    /// 静默让后者覆盖前者会让"我改了定义却不生效"变成难查的问题；
    /// 而变量库是全局共享文件，重复定义几乎总是合并时的疏忽。
    fn from_entries(entries: Vec<ParamOverride>, origin: &Path) -> Result<Self, ManifestError> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for e in &entries {
            if !seen.insert(e.name.as_str()) {
                return Err(ManifestError::DuplicateVariable {
                    path: origin.to_path_buf(),
                    name: e.name.clone(),
                });
            }
        }
        Ok(Self { entries })
    }

    /// 从模板目录加载变量库。
    ///
    /// 文件不存在时返回**空库**而非错误——变量库是可选的，
    /// 没有它时参数规格只由头部声明与清单提供，这是合法状态。
    pub fn load(dir: &Path) -> Result<Self, ManifestError> {
        let path = dir.join(VARIABLES_FILE);
        if !path.exists() {
            return Ok(Self::empty());
        }
        let text = std::fs::read_to_string(&path).map_err(|e| ManifestError::Io {
            path: path.clone(),
            source: e,
        })?;
        Self::from_yaml(&text, &path)
    }

    /// 库中定义的变量数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 库是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按变量名查询定义。
    pub fn get(&self, name: &str) -> Option<&ParamOverride> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// 遍历库中全部定义。
    pub fn iter(&self) -> impl Iterator<Item = &ParamOverride> {
        self.entries.iter()
    }

    /// 把变量库合并进模板的参数规格。
    ///
    /// - `base`：头部 `{# PARAMS: #}` 声明（可已含清单覆盖，但本函数应在
    ///   **清单覆盖之前**调用，以保证「清单 > 变量库 > 头部」的优先级）；
    /// - `source` / `template_name`：模板源码与名字，用于找出模板**实际引用**
    ///   的变量名。
    ///
    /// **只对"模板确实引用了"的变量生效**（含头部已声明的与源码里引用但未声明的）。
    /// 理由：库里是全局变量，若不加筛选地注入规格，模板就会多出一批它根本没引用的
    /// 参数规格，进而触发"规格声明了未引用参数"告警——把真实问题淹没在噪声里。
    ///
    /// 源码解析失败时**退回只用头部声明的名字**：模板语法错误会在注册阶段
    /// 由编译报出，这里不重复报错。
    pub fn apply(&self, base: Vec<ParamSpec>, source: &str, template_name: &str) -> Vec<ParamSpec> {
        if self.entries.is_empty() {
            return base;
        }
        let mut names: BTreeSet<String> = base.iter().map(|s| s.name.clone()).collect();
        if let Ok(ast) = nctool_tpl::parse(source, template_name) {
            for var in nctool_tpl::extract_undeclared(&ast) {
                names.insert(var.name);
            }
        }
        let relevant: Vec<ParamOverride> = self
            .entries
            .iter()
            .filter(|e| names.contains(&e.name))
            .cloned()
            .collect();
        merge_params(base, &relevant)
    }
}

/// `variables.yaml` 的文档结构（带 `variables` 键的推荐写法）。
#[derive(Debug, serde::Deserialize)]
struct VariablesFile {
    #[serde(default)]
    variables: Vec<ParamOverride>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ParamKind, ParamValue};

    fn lib(yaml: &str) -> VariableLibrary {
        VariableLibrary::from_yaml(yaml, Path::new("variables.yaml")).unwrap()
    }

    #[test]
    fn parses_keyed_form() {
        let l = lib(r#"
variables:
  - name: U_Q
    kind: choice
    options: [0, 8, 10, 12.5]
  - name: U_FX
    kind: string
    options: ["闭口", "左开口", "右开口"]
"#);
        assert_eq!(l.len(), 2);
        let u_q = l.get("U_Q").unwrap();
        assert_eq!(u_q.kind, Some(ParamKind::Choice));
        assert_eq!(u_q.options.as_ref().unwrap().as_ref().unwrap().len(), 4);
        assert!(
            l.get("U_FX")
                .unwrap()
                .options
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .len()
                == 3
        );
        assert!(l.get("missing").is_none());
    }

    #[test]
    fn parses_bare_list_form() {
        // 顶层直接是列表也必须能读——不能像清单那样"先试带键形式"而静默得到空库
        let l = lib(r#"
- name: U_Q
  kind: number
  options: [0, 8]
"#);
        assert_eq!(l.len(), 1);
        assert_eq!(l.get("U_Q").unwrap().kind, Some(ParamKind::Number));
    }

    #[test]
    fn empty_and_null_documents_are_empty_library() {
        assert!(lib("").is_empty());
        assert!(lib("# 只有注释\n").is_empty());
        assert!(lib("variables:\n").is_empty());
    }

    #[test]
    fn non_mapping_top_level_rejected() {
        assert!(VariableLibrary::from_yaml("just a string", Path::new("v.yaml")).is_err());
        // 映射但既无 variables 键也不是列表
        assert!(VariableLibrary::from_yaml("foo: bar", Path::new("v.yaml")).is_err());
    }

    #[test]
    fn invalid_yaml_syntax_is_reported() {
        // 短路到 serde_yaml 解析失败的早期分支（非法 YAML）
        let err = VariableLibrary::from_yaml("variables: [1, 2", Path::new("v.yaml"))
            .expect_err("非法 YAML 应报错");
        assert!(matches!(err, ManifestError::Parse { .. }), "{err:?}");
    }

    #[test]
    fn bare_list_with_bad_entry_is_reported() {
        // 顶层是序列但元素不合法（缺 name）→ 解析报错，而不是静默得到空库
        let bad = "- kind: number\n";
        let err = VariableLibrary::from_yaml(bad, Path::new("v.yaml"))
            .expect_err("序列元素缺 name 应报错");
        assert!(matches!(err, ManifestError::Parse { .. }), "{err:?}");
    }

    #[test]
    fn load_missing_file_is_empty_library() {
        // 目录里没有 variables.yaml 时应返回空库，而不是报错
        let dir = std::env::temp_dir().join(format!("nctool_vlib_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let l = VariableLibrary::load(&dir).expect("缺文件应得空库");
        assert!(l.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_reads_file_from_directory() {
        let dir = std::env::temp_dir().join(format!("nctool_vlib_ok_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(
            dir.join("variables.yaml"),
            "variables:\n  - name: U_Q\n    kind: number\n",
        )
        .unwrap();
        let l = VariableLibrary::load(&dir).expect("应读到变量库");
        assert_eq!(l.len(), 1);
        // iter() 也要走到
        assert_eq!(l.iter().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_reports_io_error_for_unreadable_path() {
        // 把一个**目录**当作 variables.yaml 去读 → read_to_string 报 IO 错误，
        // 覆盖 `load` 的 Io 分支（区别于「文件不存在返回空库」）。
        let dir = std::env::temp_dir().join(format!("nctool_vlib_io_{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("variables.yaml"));
        let err = VariableLibrary::load(&dir).expect_err("目录不可当文件读");
        assert!(matches!(err, ManifestError::Io { .. }), "{err:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_variable_is_rejected() {
        // 静默让后者覆盖前者会让"我改了定义却不生效"变成难查的问题
        let yaml = r#"
variables:
  - name: U_Q
    kind: number
  - name: U_Q
    kind: integer
"#;
        let err = VariableLibrary::from_yaml(yaml, Path::new("v.yaml")).unwrap_err();
        assert!(err.to_string().contains("U_Q"), "{err}");
        assert!(err.to_string().contains("重复定义"), "{err}");
    }

    #[test]
    fn unknown_field_is_rejected() {
        // 拼错字段名必须报错，否则约束静默不生效
        let yaml = r#"
variables:
  - name: U_Q
    optons: [0, 8]
"#;
        assert!(VariableLibrary::from_yaml(yaml, Path::new("v.yaml")).is_err());
    }

    #[test]
    fn apply_covers_referenced_but_undeclared_param() {
        // 模板头部没声明 U_Q（只声明了 U_A）但源码引用了它：
        // 变量库应补上规格——这是"19 个迁移模板不用逐个改头部就能拿到类型"的关键
        let l = lib(r#"
variables:
  - name: U_Q
    kind: number
    options: [0, 8, 10, 12.5]
  - name: U_A
    kind: number
"#);
        let base = vec![ParamSpec::new("U_A", ParamKind::Any, "键槽长度")];
        let specs = l.apply(base, "X{{ U_A }} Y{{ U_Q }}\n", "t.j2");
        assert_eq!(specs.len(), 2);
        let u_q = specs.iter().find(|s| s.name == "U_Q").unwrap();
        assert_eq!(u_q.kind, ParamKind::Number);
        assert_eq!(u_q.accepts_option(&ParamValue::Number(8.0)), Some(true));
        // 头部声明为 Any，库把它升为 number
        let u_a = specs.iter().find(|s| s.name == "U_A").unwrap();
        assert_eq!(u_a.kind, ParamKind::Number);
        assert_eq!(u_a.description, "键槽长度", "库不应清空头部描述");
    }

    #[test]
    fn apply_ignores_unreferenced_variables() {
        // 关键：库里是全局变量，未引用的**不得**注入规格——否则模板会多出一批
        // 它根本没引用的参数规格，触发"规格声明了未引用参数"告警，把真实问题淹没
        let l = lib(r#"
variables:
  - name: U_Q
    kind: number
  - name: U_NEVER_USED
    kind: string
"#);
        let specs = l.apply(Vec::new(), "X{{ U_Q }}\n", "t.j2");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "U_Q");
    }

    #[test]
    fn apply_falls_back_to_declared_names_when_source_unparsable() {
        // 源码解析失败（语法错误）时退回"只用头部声明的名字"：
        // 语法错误会在注册阶段由编译报出，这里不重复报错、也不能 panic
        let l = lib(r#"
variables:
  - name: U_A
    kind: number
"#);
        let base = vec![ParamSpec::new("U_A", ParamKind::Any, "A")];
        let specs = l.apply(base, "{{ unclosed", "bad.j2");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].kind, ParamKind::Number);
    }

    #[test]
    fn empty_library_leaves_specs_untouched() {
        let base = vec![ParamSpec::new("U_A", ParamKind::Any, "A")];
        let specs = VariableLibrary::empty().apply(base.clone(), "X{{ U_A }}", "t.j2");
        assert_eq!(specs, base);
    }

    #[test]
    fn apply_does_not_inject_defaults_for_missing_names() {
        // 库条目只改字段，不凭空造"必选"或默认值
        let l = lib(r#"
variables:
  - name: U_A
    kind: number
"#);
        let specs = l.apply(Vec::new(), "X{{ U_A }}\n", "t.j2");
        assert_eq!(specs.len(), 1);
        assert!(specs[0].default.is_none(), "库不应注入默认值");
        assert!(!specs[0].required, "库不应把参数标为文档必选");
    }
}
