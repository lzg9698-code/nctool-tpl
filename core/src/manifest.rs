//! 模板清单（`templates.yaml`）：模板的**声明式元数据**来源。
//!
//! 引入本模块的原因：在引入清单之前，模板的分类/描述/可见性只能写在
//! Rust 源码里（见 `registry::builtin_templates`），导致
//! 「新增一个模板」必须改代码重编译。清单把这份元数据外部化，
//! 使模板目录可以独立于代码演进。
//!
//! # 元数据解析优先级
//!
//! 同一个字段有多个来源时，按以下优先级取值（高 → 低）：
//!
//! 1. **清单**（`templates.yaml`）—— 显式声明，优先级最高
//! 2. **模板头部注释**（`{# NAME: xx #}` / `{# DESCRIPTION: xx #}`）—— 就近声明
//! 3. **文件名 / 目录默认值** —— 兜底
//!
//! 这套三级回退沿用 [NCTool_V3] 的既有约定，保证模板文件单独复制到别处
//! 也不会丢失可读名称。
//!
//! [NCTool_V3]: https://github.com/lzg9698-code/NCTool_V3

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

use crate::model::{ParamKind, ParamSpec, ParamValue, RequiredIf};
use crate::registry::TemplateCategory;

/// 读取模板头部元数据时扫描的最大行数。
///
/// 头部注释必须出现在文件开头附近；扫描全部内容在大模板上会浪费 IO，
/// 且背离「头部声明」的语义。沿用源项目的 10 行约定。
const HEADER_SCAN_LINES: usize = 10;

/// `{# PARAMS: #}` 块**体**的最大行数。
///
/// 参数表天然比 `NAME` / `DESCRIPTION` 长（机床模板最多十余个参数），
/// 因此不能沿用 [`HEADER_SCAN_LINES`]。这里给一个宽松但有界的上限，
/// 避免在畸形（块永不闭合）的文件上无界扫描。
///
/// 上限只约束块体，**不约束寻找 `{# PARAMS:` 开标记的过程**：找标记是
/// 线性扫描（注册表本来就读了整个文件），限它只会让"块写在文件200 行
/// 之后"变回一种静默丢弃——整块约束无声失效，而加载日志一片正常。
const PARAMS_SCAN_LINES: usize = 200;

/// 清单文件名（位于模板目录根部）。
pub const MANIFEST_FILE: &str = "templates.yaml";

/// 单个模板的元数据。
///
/// 所有字段都是**可选**的：未声明时回退到模板头部注释或文件名，
/// 因此清单只需描述"与默认值不同"的模板。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateMeta {
    /// 显示名。
    ///
    /// 未声明时回退到模板头部 `{# NAME: #}`，再回退到文件名（含扩展名）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// 描述。
    ///
    /// 未声明时回退到模板头部 `{# DESCRIPTION: #}`，再回退到空串。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// 是否在模板列表中可见。
    ///
    /// `false` 用于「功能模块专用模板」：程序可调用，但不展示给用户选择。
    /// 这是**面向用户的视图过滤**，与模板的真实可用性解耦。
    ///
    /// 缺省为 `true`（未声明即可见）。
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub visible: bool,

    /// 默认输出文件名（不含扩展名）。
    ///
    /// 供 CLI / Web UI 生成输出文件时作为默认值，降低用户重复输入。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_filename: Option<String>,

    /// 默认输出扩展名（含点，如 `.NC` / `.MPF` / `.SPF`）。
    ///
    /// 沿用西门子 / INDEX 约定：
    /// - `.NC`：通用数控程序
    /// - `.MPF`：主程序（Main Program File）
    /// - `.SPF`：子程序（Sub Program File）
    ///
    /// 缺省为 `.NC`。
    #[serde(
        default = "default_extension",
        skip_serializing_if = "is_default_extension"
    )]
    pub output_extension: String,

    /// 分类覆盖。
    ///
    /// 未声明时按所在目录推断（见 [`classify_by_path`]），
    /// 仍无法判定则归入 [`TemplateCategory::General`]。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<TemplateCategory>,

    /// 归属的机床方案包 id。
    ///
    /// 机床专用模板（含机床专有 G 代码，如 INDEX 的 `L184`/`AROT`）必须声明此项，
    /// 且其所在目录名为 `machines/<machine_id>/`。
    /// \[Q16\] 未声明时该模板对所有机床可见；声明后仅在选定对应机床时暴露。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,

    /// 工艺评审状态。
    ///
    /// 迁移自外部项目的模板默认标记为 `unreviewed`——
    /// 这些模板的 G/M 代码**未经真实工艺评审与机床空运行验证**，
    /// 投产前必须由工艺人员逐行核对。\[Q16\]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TemplateStatus>,

    /// 参数规格覆盖层（稀疏）：**按参数名**覆盖/补充模板头部 `{# PARAMS: #}`
    /// 声明的规格。
    ///
    /// 头部注释能表达的只有「名字 + 类型 + 必选性 + 描述」；而 `min` / `max` /
    /// `integer` / `options` / `required_if` / `default` 这类**约束**无处安放，
    /// 只能写在清单里。本字段即为此而生：
    ///
    /// ```yaml
    /// templates:
    ///   "turning/undercut_fs.j2":
    ///     params:
    ///       - name: FS_Z_PLUS1
    ///         required_if: { param: side, values: ["Right"] }
    ///       - name: FS_Z_MINUS1
    ///         required_if: { param: side, values: ["Left"] }
    /// ```
    ///
    /// **只写要改的字段**，未写的字段沿用头部声明（见 [`ParamOverride`]）；
    /// 头部未声明的参数也可以在此新增（此时 `kind` 缺省为
    /// [`ParamKind::Any`]）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<ParamOverride>>,
}

/// **必须手写**，不能用 `#[derive(Default)]`。
///
/// 派生出来的 `Default` 给出 `visible = false` / `output_extension = ""`，而 serde
/// 路径（`#[serde(default = "default_true")]` / `default_extension`）给出
/// `true` / `".NC"` —— 同一个「缺省」在两处含义不同。`TemplateMeta::resolve` 对
/// **清单未提及**的模板走 `unwrap_or_default()`，拿到的就是派生那个版本：
/// 往 `templates/` 里放一个新 `.j2` 而不加清单条目，用户列表里看不到它
/// （`list_visible` 过滤掉），输出文件名还丢扩展名。且全程不报错。
impl Default for TemplateMeta {
    fn default() -> Self {
        Self {
            name: None,
            description: None,
            visible: true,
            output_filename: None,
            output_extension: default_extension(),
            category: None,
            machine: None,
            status: None,
            params: None,
        }
    }
}

/// `Option<Option<T>>` 的反序列化：区分「键未写」与「键写成 `null`」。
///
/// 必须显式写出来：serde 对 `Option<Option<T>>` 的默认行为是把**缺失和 `null`
/// 都变成外层 `None`**，那样就分不出「不改」与「清空」—— 正是本类型要解决的问题。
/// 配合 `#[serde(default)]`（缺失 → `None`）后：
///
/// - 键未写 → `None`（沿用被覆盖的既有值）
/// - `键: null` → `Some(None)`（**清空**该字段）
/// - `键: 值` → `Some(Some(值))`（设值）
fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(de).map(Some)
}

/// 参数规格的**稀疏覆盖**声明（清单 `params` 列表项）。
///
/// 与 [`ParamSpec`] 的区别：本类型的字段**全部可选**，因此能区分
/// 「没写」与「写成默认值」——这是稀疏覆盖的前提。若直接复用 `ParamSpec`，
/// 只写 `options` 的一条覆盖会把 `kind` 悄悄降级成 serde 默认值。
///
/// `deny_unknown_fields`：拼错字段名必须报错，否则约束会静默不生效。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamOverride {
    /// 参数名（与模板中变量名一致）
    pub name: String,
    /// 类型（不写则沿用头部声明；头部也没写时为 [`ParamKind::Any`]）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ParamKind>,
    /// 是否必选（文档性声明）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// 默认值。**写 `null` 表示清空**（取消继承来的默认值）
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub default: Option<Option<ParamValue>>,
    /// 数值下界（含）。**写 `null` 表示清空**
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub min: Option<Option<f64>>,
    /// 数值上界（含）。**写 `null` 表示清空**
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub max: Option<Option<f64>>,
    /// 是否要求整数值
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integer: Option<bool>,
    /// 计量单位（仅文档与错误提示用）。**写 `null` 表示清空**
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub unit: Option<Option<String>>,
    /// 候选项白名单。**写 `[]` 或 `null` 表示清空**（取消继承来的白名单）
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub options: Option<Option<Vec<ParamValue>>>,
    /// 条件必选。**写 `null` 表示清空**
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub required_if: Option<Option<RequiredIf>>,
    /// 派生规则（由 Rust 侧查表算好注入，见 [`crate::derive`]）。**写 `null` 表示清空**
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub derive: Option<Option<crate::model::DeriveRule>>,
    /// 用途说明
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ParamOverride {
    /// 覆盖到既有规格上。
    ///
    /// 可清空的字段是 `Option<Option<T>>`：**外层 `Some` = 清单里写了这个键**，
    /// 内层 `Some(v)` 设值、`None`（YAML `null`）清空。不这样分层就分不出
    /// 「没写 → 沿用继承值」和「写了 null → 解除继承」——`Option<T>` 只能设不能清，
    /// 于是变量库里给 `U_Q` 声明的 `min: 0` 能在**所有**模板上生效且无从解除，
    /// 哪怕某模板确实需要负值；继承来的 `derive` 同理，想关掉只能改变量库。
    fn apply_to(self, spec: &mut ParamSpec) {
        if let Some(kind) = self.kind {
            spec.kind = kind;
        }
        if let Some(required) = self.required {
            spec.required = required;
        }
        if let Some(default) = self.default {
            spec.default = default;
        }
        if let Some(min) = self.min {
            spec.min = min;
        }
        if let Some(max) = self.max {
            spec.max = max;
        }
        if let Some(integer) = self.integer {
            spec.integer = integer;
        }
        if let Some(unit) = self.unit {
            spec.unit = unit;
        }
        if let Some(options) = self.options {
            // `[]` 与 `null` 等价：都是「不要白名单」。
            // 空列表**不能**当成"没写" —— 那样就永远无法解除继承来的白名单。
            spec.options = match options {
                Some(v) if !v.is_empty() => Some(v),
                _ => None,
            };
        }
        if let Some(required_if) = self.required_if {
            spec.required_if = required_if;
        }
        if let Some(derive) = self.derive {
            spec.derive = derive;
        }
        if let Some(description) = self.description {
            spec.description = description;
        }
    }
}

/// 合并参数规格：头部注释声明为**基础**，清单覆盖层**按参数名**覆盖/补充。
///
/// - 同名：只覆盖清单里写了的字段，其余沿用头部声明；
/// - 新名：追加到末尾（头部未声明该参数，`kind` 缺省为 [`ParamKind::Any`]）。
///
/// 头部声明的**顺序被保留**（参数表按模板作者的书写顺序展示更符合阅读习惯）。
pub fn merge_params(base: Vec<ParamSpec>, overrides: &[ParamOverride]) -> Vec<ParamSpec> {
    let mut out = base;
    for ov in overrides {
        match out.iter_mut().find(|s| s.name == ov.name) {
            Some(spec) => ov.clone().apply_to(spec),
            None => {
                let mut spec = ParamSpec::new(
                    ov.name.clone(),
                    ParamKind::Any,
                    ov.description.clone().unwrap_or_default(),
                );
                ov.clone().apply_to(&mut spec);
                out.push(spec);
            }
        }
    }
    out
}

/// 模板的工艺评审状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateStatus {
    /// 未经工艺评审（迁移模板的默认状态）
    Unreviewed,
    /// 已通过工艺评审
    Reviewed,
    /// 已在机床上空运行验证
    Verified,
}

impl TemplateStatus {
    /// 状态标签（用于列表展示）。
    pub fn label(&self) -> &'static str {
        match self {
            TemplateStatus::Unreviewed => "未评审",
            TemplateStatus::Reviewed => "已评审",
            TemplateStatus::Verified => "已空运行",
        }
    }
}

fn default_true() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

fn default_extension() -> String {
    ".NC".to_string()
}

fn is_default_extension(s: &str) -> bool {
    s == ".NC"
}

/// 模板清单：`相对路径 → 元数据`。
///
/// 相对路径以模板目录为根，使用 `/` 作为分隔符（跨平台统一）。
#[derive(Debug, Clone, Default)]
pub struct TemplateManifest {
    entries: BTreeMap<String, TemplateMeta>,
    /// 规范化后互相碰撞的键（如 `turning\a.j2` 与 `turning/a.j2`）。
    ///
    /// 归一化会把两者变成同一个键，后读到的**静默覆盖**先读到的 —— 用户以为
    /// 两个条目都在生效，实际只有一个。加载时记下来供调用方告警（对照
    /// [`crate::variables`] 对重复变量是直接报错的，此处不宜更松）。
    duplicates: Vec<String>,
}

impl TemplateManifest {
    /// 空清单（所有字段都走回退逻辑）。
    pub fn empty() -> Self {
        Self {
            entries: BTreeMap::new(),
            duplicates: Vec::new(),
        }
    }

    /// 规范化后互相碰撞的键对（加载时记录，用于告警）。
    pub fn duplicate_keys(&self) -> &[String] {
        &self.duplicates
    }

    /// 从 YAML 文本解析清单。
    ///
    /// 接受的文档结构（两种写法均可）：
    ///
    /// ```yaml
    /// # 写法一：顶层带 templates 键
    /// templates:
    ///   "turning/undercut.j2":
    ///     name: 越程槽加工
    ///     visible: true
    /// ```
    ///
    /// ```yaml
    /// # 写法二：顶层直接是映射
    /// "turning/undercut.j2":
    ///   name: 越程槽加工
    /// ```
    pub fn from_yaml(text: &str, origin: &Path) -> Result<Self, ManifestError> {
        // ERR-NUM-UNDERFLOW：清单是 YAML，其中的数值字面量（如 `1e-400`）会被
        // serde_yaml 静默归零，进而污染参数默认值 / 上下界。**先**在文本层做候选提取，
        // 再逐条用 serde_yaml **实测确认**（`confirm_underflow_yaml`）——那条通道自己的
        // 解析器。命中即硬失败（不静默降级）。
        for cand in crate::json_num::scan_underflow_candidates_yaml(text) {
            if crate::json_num::confirm_underflow_yaml(&cand) {
                return Err(ManifestError::Underflow {
                    path: origin.to_path_buf(),
                    literal: cand.literal,
                    line: cand.line,
                    column: cand.column,
                });
            }
        }
        // 两种手写形式都支持：
        //   1. 带 `templates:` 键（文档推荐）
        //   2. **裸映射**：顶层直接是 `"路径": {…}`
        //
        // **不能用「先试 A，失败再试 B」来区分**：`ManifestFile.templates` 带
        // `#[serde(default)]`，把裸映射解析成 `ManifestFile` 会**成功**（所有
        // 未知键被忽略）但得到一个空清单——B 分支永远轮不到，裸映射写法静默失效。
        //
        // 因此先解析成通用 `serde_yaml::Value`，**显式检查有无 `templates` 键**
        // 来决定走哪条路；有该键却内容不合法时报错，避免静默降级。
        let value: serde_yaml::Value =
            serde_yaml::from_str(text).map_err(|e| ManifestError::Parse {
                path: origin.to_path_buf(),
                source: e,
            })?;
        let map = match value {
            serde_yaml::Value::Mapping(m) => m,
            // 空文件会被解析为 Null；视作「无清单声明」而非错误
            serde_yaml::Value::Null => return Ok(Self::empty()),
            _ => {
                return Err(ManifestError::Parse {
                    path: origin.to_path_buf(),
                    source: serde::de::Error::custom(
                        "清单顶层应为映射（`templates:` 键或直接的模板路径键）",
                    ),
                })
            }
        };
        // 有 `templates` 键 → 推荐形式，复用 ManifestFile 的严格解析
        // （`deny_unknown_fields` 会挡住拼错的顶层键）
        if map.contains_key(serde_yaml::Value::String("templates".to_string())) {
            let file: ManifestFile =
                serde_yaml::from_str(text).map_err(|e| ManifestError::Parse {
                    path: origin.to_path_buf(),
                    source: e,
                })?;
            return Ok(Self::from_entries(file.templates));
        }
        // 无 `templates` 键 → 裸映射形式
        let entries: BTreeMap<String, TemplateMeta> =
            serde_yaml::from_str(text).map_err(|e| ManifestError::Parse {
                path: origin.to_path_buf(),
                source: e,
            })?;
        Ok(Self::from_entries(entries))
    }

    /// 规范化键后构造清单（统一分隔符为 `/`，去掉前导 `./`）。
    ///
    /// 归一化可能让两个不同的原始键落到同一个键上（`turning\a.j2` vs
    /// `turning/a.j2`），此时后者覆盖前者且**不报错**。这里把冲突记进
    /// [`Self::duplicate_keys`]，由调用方决定怎么提示。
    fn from_entries(entries: BTreeMap<String, TemplateMeta>) -> Self {
        let mut normalized: BTreeMap<String, TemplateMeta> = BTreeMap::new();
        let mut originals: BTreeMap<String, String> = BTreeMap::new();
        let mut duplicates: Vec<String> = Vec::new();

        for (raw_key, meta) in entries {
            let key = normalize_key(&raw_key);
            match originals.get(&key) {
                Some(first) => duplicates.push(format!(
                    "{raw_key} 与 {first} 规范化后同为 {key}（只保留其中一个）"
                )),
                None => {
                    originals.insert(key.clone(), raw_key);
                }
            }
            normalized.insert(key, meta);
        }

        Self {
            entries: normalized,
            duplicates,
        }
    }

    /// 从文件加载清单。
    ///
    /// 文件不存在时返回**空清单**而非错误——清单是可选的，
    /// 没有清单时全部字段走回退逻辑，这是合法状态。
    ///
    /// 读取走 [`crate::io_limit::read_text_capped`]（P1-1）：清单随后要交给
    /// `serde_yaml`，而它的别名展开没有预算 —— 一份"锚点套锚点"的清单可以
    /// 展开出 GB 级节点树。上限必须在**解析之前**生效，读完再判已经晚了。
    pub fn load(dir: &Path) -> Result<Self, ManifestError> {
        let path = dir.join(MANIFEST_FILE);
        if !path.exists() {
            return Ok(Self::empty());
        }
        let text = crate::io_limit::read_text_capped(&path, crate::io_limit::MAX_SOURCE_BYTES)
            .map_err(|e| ManifestError::Io {
                path: path.clone(),
                source: e,
            })?;
        Self::from_yaml(&text, &path)
    }

    /// 查询某个模板的元数据（相对路径）。
    ///
    /// 未在清单中声明时返回 `None`，调用方应走回退逻辑。
    pub fn get(&self, rel_path: &str) -> Option<&TemplateMeta> {
        self.entries.get(&normalize_key(rel_path))
    }

    /// 清单中声明的模板数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 清单是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 遍历清单条目（`相对路径 → 元数据`）。
    pub fn iter(&self) -> impl Iterator<Item = (&String, &TemplateMeta)> {
        self.entries.iter()
    }

    /// 清单里**没有匹配到任何模板文件**的键（孤儿条目），按字典序返回。
    ///
    /// 孤儿条目是纯静默失效：`params`（白名单/区间）、`visible`、`machine`、
    /// `output_extension` 全部不生效 —— 参数失去约束、模板被隐藏，而没有任何提示。
    /// 写成 `undercut.j2`（实际是 `undercut_fs.j2`）这类笔误尤其危险：
    /// 看起来约束齐全，实际一条都没上。项目已为「规格写了个不存在的参数」设了
    /// `SpecInert` 警告，此处是对称的补口。
    ///
    /// `present` 是**实际存在**的模板键集合（调用方遍历模板目录后提供）。
    pub fn orphan_keys<'a>(&'a self, present: &BTreeSet<String>) -> Vec<&'a str> {
        self.entries
            .keys()
            .filter(|k| !present.contains(k.as_str()))
            .map(String::as_str)
            .collect()
    }

    /// 收集清单中声明的全部机床方案包 id（去重、有序）。
    pub fn machine_packs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .entries
            .values()
            .filter_map(|m| m.machine.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }
}

/// `templates.yaml` 的文档结构（带 `templates` 键的推荐写法）。
///
/// `deny_unknown_fields`：带 `templates:` 的形式只有这一个合法顶层键，拼错
/// （`template:` / `templats:`）或写下多余键必须报错。否则那一行被静默忽略，
/// 用户以为改的是清单、实际什么也没改 —— `from_yaml` 的注释一直声称有这条
/// 保护，但结构体上没有，属"文档说有、代码没有"。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    #[serde(default)]
    templates: BTreeMap<String, TemplateMeta>,
}

/// 清单键规范化：统一使用 `/` 分隔、去掉前导 `./`。
///
/// Windows 上手写清单时容易写成 `turning\undercut.j2`，规范化后与
/// [`path_to_rel_key`] 的产出可比对。
pub fn normalize_key(key: &str) -> String {
    let mut k = key.trim().replace('\\', "/");
    while let Some(rest) = k.strip_prefix("./") {
        k = rest.to_string();
    }
    k
}

/// 由模板目录内的相对路径构造清单键（`/` 分隔）。
pub fn path_to_rel_key(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// 按路径推断模板分类。
///
/// 目录名（大小写不敏感）→ 分类的映射：
///
/// | 目录名 | 分类 |
/// | --- | --- |
/// | `general` / `common` | [`TemplateCategory::General`] |
/// | `milling` / `mill` | [`TemplateCategory::Milling`] |
/// | `turning` / `turn` | [`TemplateCategory::Turning`] |
/// | `drilling` / `drill` | [`TemplateCategory::Drilling`] |
/// | `grooving` / `groove` | [`TemplateCategory::Grooving`] |
/// | `machines` / `machine` | [`TemplateCategory::Machine`] |
///
/// 真正生效的表在 `TemplateCategory::dir_names()`（穷尽匹配）——本表只是
/// 说明，改动请以那里为准。
///
/// **取路径中第一个命中的目录**，因此 `turning/machines/xx.j2` 判为车削。
/// 未命中任何目录时归入 [`TemplateCategory::General`]。
pub fn classify_by_path(rel: &Path) -> TemplateCategory {
    for comp in rel.components() {
        let name = comp.as_os_str().to_string_lossy().to_ascii_lowercase();
        // 目录名表在 TemplateCategory::dir_names（穷尽匹配），新增分类时
        // 编译器强制登记，不会像原先那样被 `_ => None` 静默归入"通用"。
        if let Some(c) = TemplateCategory::from_dir_name(&name) {
            return c;
        }
    }
    TemplateCategory::General
}

/// 从模板源码头部提取元数据注释。
///
/// 识别的标记（沿用 [NCTool_V3] 约定）：
///
/// ```jinja
/// {# NAME: 越程槽加工 #}
/// {# DESCRIPTION: 生成 ES/FS 型越程槽 G 代码 #}
/// {# PARAMS:
///      side       string  required  加工侧面 ("Right" 或 "Left")
///      FS_Z_PLUS1 number  required  FS Z+1 坐标（side=Right 时使用）
///   #}
/// ```
///
/// `NAME` / `DESCRIPTION` 只在前 `HEADER_SCAN_LINES` 行内查找；
/// `PARAMS` 是一张表，另用 `PARAMS_SCAN_LINES` 为界（见
/// `extract_params_block`）。
///
/// 标记不完整（缺 `#}`）时跳过该字段，不报错——头部注释是辅助信息，
/// 不应阻断模板加载。但 `PARAMS` 里**单行解析失败**会记入
/// [`HeaderMeta::warnings`]：静默丢弃一行等于静默丢掉一条参数约束。
///
/// [NCTool_V3]: https://github.com/lzg9698-code/NCTool_V3
pub fn extract_header_meta(source: &str) -> HeaderMeta {
    let mut meta = HeaderMeta::default();
    for line in source.lines().take(HEADER_SCAN_LINES) {
        if let Some(v) = extract_marker(line, "{# NAME:") {
            meta.name = Some(v);
        }
        if let Some(v) = extract_marker(line, "{# DESCRIPTION:") {
            meta.description = Some(v);
        }
    }
    let (params, warnings) = extract_params_block(source);
    meta.params = params;
    meta.warnings = warnings;
    meta
}

/// 解析 `{# PARAMS: ... #}` 参数表块。
///
/// 返回 `(参数规格, 解析告警)`。块不存在时返回空。
///
/// 支持两种现存写法（**逐行**判定，可混用）：
///
/// ```jinja
/// {# PARAMS:
///      U_A            必选  键槽有效长度 A (mm)          # 写法一：名字 + 必选性 + 描述
///      side   string  required  加工侧面                # 写法二：名字 + 类型 + 必选性 + 描述
///   #}
/// ```
///
/// 写法一无法表达类型，此时类型记为 [`ParamKind::Any`]（不做类型检查），
/// 但白名单/条件必选仍可通过清单 `params` 覆盖层声明。
///
/// 描述取结构化前缀之后的全部内容（内部空白归一化为单空格）。
/// 无法解析的行**不静默跳过**，而是产出告警文本交由调用方提示——
/// 静默丢一行等于静默少一条参数约束。同理，**同名重复行**保留先声明者
/// 并告警（按名查找/覆盖层都是 first-wins，留两条只会让"哪条生效"成玄学）。
fn extract_params_block(source: &str) -> (Vec<ParamSpec>, Vec<String>) {
    let mut body: Vec<String> = Vec::new();
    let mut in_block = false;
    let mut truncated = false;
    for line in source.lines() {
        if in_block && body.len() >= PARAMS_SCAN_LINES {
            // 块还没闭合就撞上块体上限：后面写的参数声明会被**静默丢弃**，
            // 而调用方看到的是"一切正常、只是少了几条约束"。记下来交给调用方提示
            // ——静默丢一行等于静默少一条参数约束，与本节开头那条约定同源。
            //
            // 上限**只在块已打开时生效**：若把寻找开标记的过程也截断在200 行，
            // 写在文件后部的 PARAMS 块会整块无声消失（`truncated = in_block`
            // 恒为 false，连告警都没有）——那正是本条要堵的静默丢弃。
            truncated = true;
            break;
        }
        if !in_block {
            // 容错：`{# PARAMS:` / `{#- PARAMS:` / 多空格
            let trimmed = line.trim_start();
            let Some(after_open) = trimmed.strip_prefix("{#") else {
                continue;
            };
            let Some((_, after_marker)) = after_open.split_once("PARAMS:") else {
                continue;
            };
            in_block = true;
            match after_marker.split_once("#}") {
                Some((inner, _)) => {
                    body.push(inner.to_string());
                    break;
                }
                None => body.push(after_marker.to_string()),
            }
            continue;
        }
        match line.split_once("#}") {
            Some((inner, _)) => {
                // 收尾行的 `#}` 之前通常只有空白控制符（`-#}`）或空串；
                // 若把它当参数行会解析出名为 `-` 的假参数并刷出告警。
                let content = inner.trim().trim_matches('-').trim();
                if !content.is_empty() {
                    body.push(content.to_string());
                }
                break;
            }
            None => body.push(line.to_string()),
        }
    }
    if !in_block {
        return (Vec::new(), Vec::new());
    }

    let mut specs = Vec::new();
    let mut warnings = Vec::new();
    if truncated {
        warnings.push(format!(
            "{{# PARAMS: #}} 块超过 {PARAMS_SCAN_LINES} 行仍未闭合，其后的参数声明全部被忽略\
             （类型/白名单/条件必选都不会生效；请把参数表前移或拆短）"
        ));
    }
    // 同名重复行：保留先声明者并告警。按名查找与清单覆盖层都是 first-wins，
    // 两条并存只会让"哪条生效"变成玄学；而静默丢后一条等于静默丢一条约束。
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (n, raw) in body.iter().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        match parse_param_line(line) {
            Ok(spec) => {
                if !seen.insert(spec.name.clone()) {
                    warnings.push(format!(
                        "{{# PARAMS: #}} 第 {} 行参数名 '{}' 与前面重复，以后声明无效\
                         （以先声明者为准）：{line}",
                        n + 1,
                        spec.name
                    ));
                    continue;
                }
                specs.push(spec);
            }
            Err(reason) => warnings.push(format!(
                "{{# PARAMS: #}} 第 {} 行无法解析（{reason}）：{line}",
                n + 1
            )),
        }
    }
    (specs, warnings)
}

/// 解析参数表的一行。
///
/// 先看第二个词是不是必选标记（写法一），否则按类型解析（写法二）。
fn parse_param_line(line: &str) -> Result<ParamSpec, String> {
    let mut tokens = line.split_whitespace();
    let name = tokens.next().ok_or_else(|| "缺少参数名".to_string())?;
    let second = tokens
        .next()
        .ok_or_else(|| "缺少「必选/可选」标记".to_string())?;

    // 写法一：`name 必选 描述`（允许 `可选(ES)` 这类带分支限定词的写法）
    if let Some((required, qualifier)) = parse_required_marker(second) {
        let description = join_description(qualifier.as_deref(), tokens);
        return Ok(ParamSpec::new(name, ParamKind::Any, description).required(required));
    }

    // 写法二：`name type required 描述`
    let kind = parse_kind_name(second)
        .ok_or_else(|| format!("'{second}' 既不是已知类型，也不是「必选/可选」标记"))?;
    let third = tokens
        .next()
        .ok_or_else(|| format!("类型 '{second}' 之后缺少「必选/可选」标记"))?;
    let (required, qualifier) =
        parse_required_marker(third).ok_or_else(|| format!("'{third}' 不是「必选/可选」标记"))?;
    let description = join_description(qualifier.as_deref(), tokens);
    Ok(ParamSpec::new(name, kind, description).required(required))
}

/// 拼描述：限定词（如 `(ES)`）在前，其余 token 用单空格连接（内部空白归一化）。
fn join_description<'a>(qualifier: Option<&str>, tokens: impl Iterator<Item = &'a str>) -> String {
    let rest: Vec<&str> = tokens.collect();
    match qualifier {
        Some(q) if !rest.is_empty() => format!("{q} {}", rest.join(" ")),
        Some(q) => q.to_string(),
        None => rest.join(" "),
    }
}

/// 必选标记（中英文，大小写不敏感），允许带**限定后缀**。
///
/// 返回 `(是否必选, 限定词)`。限定词形如 `(ES)` / `(FS)`，表示"该分支专用"——
/// 头部注释表达不了触发条件（那是清单 `params.required_if` 的职责），
/// 因此限定词**保留到描述里**作为可读信息，不参与必选判定。
///
/// `条件必选` 与 `必选` 同样映射为"文档上必选"；若清单没补 `required_if`，
/// 该参数就按无条件必选处理——这是失败安全的方向（宁可多要一个参数，
/// 也不能因条件没声明而放过缺失）。
///
/// 顺序敏感：`条件必选` 必须先于 `必选` 匹配。
fn parse_required_marker(token: &str) -> Option<(bool, Option<String>)> {
    /// (标记词, 是否必选)；标记词按长度降序，保证前缀匹配取到最长者
    const WORDS: [(&str, bool); 8] = [
        ("条件必选", true),
        ("required_if", true),
        ("required", true),
        ("optional", false),
        ("必选", true),
        ("可选", false),
        ("req", true),
        ("opt", false),
    ];
    let lower = token.to_ascii_lowercase();
    for (word, required) in WORDS {
        let Some(rest) = lower.strip_prefix(word) else {
            continue;
        };
        let rest = rest.trim();
        // 只接受空后缀或括号限定词（`(ES)`），避免把 `requiredly` 之类误判为标记。
        // `to_ascii_lowercase` 不改变字节长度，故可按 `word.len()` 切回原串。
        if rest.is_empty() {
            return Some((required, None));
        }
        if rest.starts_with('(') {
            return Some((required, Some(token[word.len()..].trim().to_string())));
        }
    }
    None
}

/// 类型名（中英文，大小写不敏感）。
///
/// 解析规则集中在 [`ParamKind::from_str`]（与枚举定义同处一个文件），此处只
/// 做转发——避免"新增类型要改两处字符串表、漏改一处就静默不认识"。
fn parse_kind_name(token: &str) -> Option<ParamKind> {
    token.parse().ok()
}

/// 从单行中提取 `{# MARKER: ... #}` 的值。
///
/// 兼容 minijinja 的空白控制标记：模板常用 `{# NAME: xxx -#}` 让头部注释
/// 不产生空行，此时 `#}` 前有 `-`，需先剥掉再取值（否则会得到 `"xxx -"`）。
///
/// 同理兼容 `{#- MARKER: ... #}`（此时 `find(marker)` 匹配不到，按普通行跳过；
/// 头部注释的空白控制建议只加在结尾 `-#}`）。
fn extract_marker(line: &str, marker: &str) -> Option<String> {
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find("#}")?;
    let value = rest[..end].trim_end().trim_end_matches('-').trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// 从模板头部注释提取到的元数据。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HeaderMeta {
    /// `{# NAME: xx #}`
    pub name: Option<String>,
    /// `{# DESCRIPTION: xx #}`
    pub description: Option<String>,
    /// `{# PARAMS: #}` 声明的参数规格（无类型声明的记为 [`ParamKind::Any`]）
    pub params: Vec<ParamSpec>,
    /// 参数表里无法解析的行（供调用方提示，不阻断加载）
    pub warnings: Vec<String>,
}

/// 解析后的模板元数据（清单 + 头部注释 + 默认值 三级回退的最终结果）。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedMeta {
    /// 显示名
    pub name: String,
    /// 描述
    pub description: String,
    /// 是否可见
    pub visible: bool,
    /// 默认输出文件名
    pub output_filename: Option<String>,
    /// 默认输出扩展名
    pub output_extension: String,
    /// 分类
    pub category: TemplateCategory,
    /// 归属机床方案包
    pub machine: Option<String>,
    /// 工艺评审状态
    pub status: Option<TemplateStatus>,
    /// 参数规格：头部 `{# PARAMS: #}` 为基础，清单 `params` 稀疏覆盖
    pub params: Vec<ParamSpec>,
    /// 参数表解析告警（供调用方提示；不阻断加载）
    pub warnings: Vec<String>,
}

impl ResolvedMeta {
    /// 按「清单 > 变量库 > 头部注释 > 默认值」回退解析元数据。
    ///
    /// - `rel_path`：模板相对模板目录的路径（作为清单键与分类依据）
    /// - `source`：模板源码（用于提取头部注释与找出引用的变量）
    /// - `manifest_meta`：清单中该模板的声明，`None` 表示清单未提及
    /// - `library`：变量库（全局按名定义），可为空库
    ///
    /// 参数规格的合并顺序：**头部 `{# PARAMS: #}` → 变量库 → 清单 `params`**。
    /// 越靠后越具体（清单是"本模板"的显式覆盖），因此优先级最高。
    pub fn resolve(
        rel_path: &Path,
        source: &str,
        manifest_meta: Option<&TemplateMeta>,
        library: &crate::variables::VariableLibrary,
    ) -> Self {
        let header = extract_header_meta(source);
        let key = path_to_rel_key(rel_path);
        // 兜底名：文件名字符串（含扩展名）。用 `to_string_lossy` 保证
        // 非 UTF-8 文件名（Windows 上可能）不 panic
        let file_name = rel_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| key.clone());

        let m = manifest_meta.cloned().unwrap_or_default();
        // 参数规格三级：头部声明 → 变量库（按名覆盖）→ 清单覆盖层
        let params = library.apply(header.params, source, &key);
        let params = match &m.params {
            Some(overrides) => merge_params(params, overrides),
            None => params,
        };
        Self {
            name: m.name.or(header.name).unwrap_or_else(|| file_name.clone()),
            description: m.description.or(header.description).unwrap_or_default(),
            visible: m.visible,
            output_filename: m.output_filename,
            output_extension: m.output_extension,
            category: m.category.unwrap_or_else(|| classify_by_path(rel_path)),
            machine: m.machine,
            status: m.status,
            params,
            warnings: header.warnings,
        }
    }
}

/// 清单加载 / 解析错误。
#[derive(Debug)]
#[non_exhaustive]
pub enum ManifestError {
    /// 读取清单文件失败
    Io {
        /// 文件路径
        path: std::path::PathBuf,
        /// 底层 IO 错误
        source: std::io::Error,
    },
    /// YAML 语法或结构错误
    Parse {
        /// 文件路径
        path: std::path::PathBuf,
        /// 底层解析错误
        source: serde_yaml::Error,
    },
    /// 变量库中同名变量重复定义（后者静默覆盖前者会让"改了定义却不生效"难查）
    DuplicateVariable {
        /// 文件路径
        path: std::path::PathBuf,
        /// 重复的变量名
        name: String,
    },
    /// 清单中的数值字面量下溢（`|真值| < 2^-1000` 且被 YAML 解析器静默归零）。
    ///
    /// 下溢会静默产出错误数值（如默认值变成 `0`），进而生成错误 G-code，故**硬失败**
    /// 而非警告。定位信息（行/列 + 字面量）用于让用户直接改到那一行。
    Underflow {
        /// 文件路径
        path: std::path::PathBuf,
        /// 触发下溢的原始字面量文本
        literal: String,
        /// 字面量所在行（1 起）
        line: usize,
        /// 字面量所在列（1 起，字节计）
        column: usize,
    },
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Io { path, source } => {
                write!(f, "读取模板清单失败 {}: {source}", path.display())
            }
            ManifestError::Parse { path, source } => {
                write!(f, "模板清单格式错误 {}: {source}", path.display())
            }
            ManifestError::DuplicateVariable { path, name } => {
                write!(f, "变量库 {} 中变量 '{name}' 重复定义", path.display())
            }
            ManifestError::Underflow {
                path,
                literal,
                line,
                column,
            } => write!(
                f,
                "模板清单 {} 第 {line} 行第 {column} 列：数值 yaml:{literal} 低于 f64 最小可表示正数，\
                 会被静默当作 0（将产出错误默认值/坐标）。请改用可表示的数值。",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ManifestError::Io { source, .. } => Some(source),
            ManifestError::Parse { source, .. } => Some(source),
            ManifestError::DuplicateVariable { .. } => None,
            // 下溢由本模块的文本层判定，无底层错误源
            ManifestError::Underflow { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 空变量库：元数据解析测试不关心变量库（变量库有自己的测试模块）。
    fn no_lib() -> crate::variables::VariableLibrary {
        crate::variables::VariableLibrary::empty()
    }

    #[test]
    fn parses_keyed_form() {
        let yaml = r#"
templates:
  "turning/undercut.j2":
    name: 越程槽加工
    visible: true
    output_extension: ".NC"
"#;
        let m = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).unwrap();
        let meta = m.get("turning/undercut.j2").unwrap();
        assert_eq!(meta.name.as_deref(), Some("越程槽加工"));
        assert!(meta.visible);
        assert_eq!(meta.output_extension, ".NC");
    }

    /// **P1-1**：清单读取有字节上限，且上限在**解析之前**生效。
    ///
    /// 无界读的后果不是"解析慢"，而是整份文件先进内存；更糟的是清单随后要交给
    /// `serde_yaml`，而它的别名展开没有预算 —— 一份"锚点套锚点"的清单
    /// （billion laughs 变体）能展开出 GB 级节点树。
    ///
    /// 这里用**合法 YAML**（全注释填充）构造超限文件，断言：① 硬失败；
    /// ② 错误说"过大"而非"解析失败"——文件没坏，是拿错了，报"解析失败"
    /// 会把用户引向错误的排障方向。
    #[test]
    fn oversized_manifest_is_rejected_before_parsing() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "nctool_manifest_cap_{}_{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut text = String::from("templates: {}\n#");
        while (text.len() as u64) <= crate::io_limit::MAX_SOURCE_BYTES {
            text.push('x');
        }
        std::fs::write(dir.join(MANIFEST_FILE), &text).unwrap();

        let err = TemplateManifest::load(&dir).expect_err("超限必须硬失败");
        let msg = err.to_string();
        assert!(msg.contains("过大"), "应报「过大」而非解析问题: {msg}");

        // 对照：同一目录换成小清单即可正常加载（证明失败来自大小而非路径/内容）
        std::fs::write(dir.join(MANIFEST_FILE), "templates: {}\n").unwrap();
        let ok = TemplateManifest::load(&dir).expect("小清单应正常加载");
        assert!(ok.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 回归（P1-12）：清单里写了却不存在的键此前**零检测** —— 该条目的
    /// `params`（白名单/区间）、`visible`、`machine`、`output_extension` 全部静默
    /// 失效，看起来约束齐全、实际一条都没上。写成 `undercut.j2`（实际是
    /// `undercut_fs.j2`）这类笔误最容易发生。
    #[test]
    fn orphan_keys_reports_unmatched_entries() {
        let yaml = r#"
templates:
  "turning/undercut_fs.j2":
    visible: false
  "turning/undercut.j2":
    params:
      - name: X
  "milling/gone.j2": {}
"#;
        let m = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).unwrap();
        let present = BTreeSet::from([
            "turning/undercut_fs.j2".to_string(),
            "milling/facing.j2".to_string(),
        ]);
        assert_eq!(
            m.orphan_keys(&present),
            vec!["milling/gone.j2", "turning/undercut.j2"],
            "应报出全部未命中键，且按字典序"
        );

        // 全部命中时无告警（别把正常清单也刷成噪声）
        let all_present = BTreeSet::from([
            "turning/undercut_fs.j2".to_string(),
            "turning/undercut.j2".to_string(),
            "milling/gone.j2".to_string(),
        ]);
        assert!(m.orphan_keys(&all_present).is_empty());
    }

    /// 回归（第四轮 P1-3）：键规范化可能让两个不同的原始键落到同一个键上，
    /// 后读到的**静默覆盖**先读到的 —— 用户以为两个条目都在生效，实际只有一个。
    /// 对照变量库对重复变量是直接报错的，此处至少要有告警。
    #[test]
    fn normalized_key_collision_is_reported() {
        let yaml = r#"
templates:
  "./turning/a.j2":
    visible: false
  "turning/a.j2":
    visible: true
"#;
        let m = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).unwrap();
        assert_eq!(
            m.duplicate_keys().len(),
            1,
            "同义键冲突应被记录：{:?}",
            m.duplicate_keys()
        );
        let msg = &m.duplicate_keys()[0];
        assert!(
            msg.contains("./turning/a.j2") && msg.contains("turning/a.j2"),
            "告警要点出两个原始键，便于定位：{msg}"
        );
        // 覆盖发生了（只留一个），但键本身仍可用
        assert!(m.get("turning/a.j2").is_some());

        // 无冲突时不应刷噪声
        let clean = r#"
templates:
  "turning/a.j2": {}
  "milling/b.j2": {}
"#;
        assert!(
            TemplateManifest::from_yaml(clean, Path::new("templates.yaml"))
                .unwrap()
                .duplicate_keys()
                .is_empty()
        );
    }

    /// 回归（P2-19）：带 `templates:` 的形式此前**没有** `deny_unknown_fields`，
    /// 多写或拼错一个顶层键会被静默忽略 —— 用户以为改的是清单、实际什么也没改。
    /// `from_yaml` 的注释一直声称有这条保护，但结构体上没有。
    #[test]
    fn manifest_rejects_unknown_top_level_key() {
        let yaml = "templates:\n  \"a.j2\": {}\nextra: 1\n";
        let err = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).unwrap_err();
        assert!(
            err.to_string().contains("extra"),
            "多余的顶层键必须报错而不是忽略: {err}"
        );

        // 正确的形式仍可解析（别把保护做成误伤）
        let ok = "templates:\n  \"a.j2\": {}\n";
        assert!(TemplateManifest::from_yaml(ok, Path::new("templates.yaml")).is_ok());
    }

    /// 回归（P2-18）：`{# PARAMS: #}` 块超出扫描上限（[`PARAMS_SCAN_LINES`]）时，
    /// 其后的参数声明此前被**静默丢弃** —— 类型 / 白名单 / 条件必选全部不生效，
    /// 而用户看不到任何提示，只当模板本来就没写这些约束。
    #[test]
    fn params_block_beyond_scan_limit_warns() {
        let mut src = String::from("{# PARAMS:\n");
        for i in 0..(PARAMS_SCAN_LINES + 10) {
            src.push_str(&format!("     P{i}  number  必选  参数 {i}\n"));
        }
        src.push_str("#}\nG1 X1\n");

        let meta = extract_header_meta(&src);
        assert!(
            meta.warnings.iter().any(|w| w.contains("仍未闭合")),
            "超限截断必须告警，否则静默少一批参数约束: {:?}",
            meta.warnings
        );

        // 界内正常闭合的块不刷这条告警（别把正常模板也变成噪声）
        let ok = "{# PARAMS:\n     X  number  必选  坐标\n#}\nG1 X1\n";
        assert!(
            !extract_header_meta(ok)
                .warnings
                .iter()
                .any(|w| w.contains("仍未闭合")),
            "界内闭合的块不该报截断"
        );
    }

    /// 回归：开标记落在旧扫描上限（第200 行）之后时，旧实现在
    /// `in_block == false` 上 `break`，整块被**静默丢弃**（连告警都没有）——
    /// 类型/白名单/条件必选全部无声失效。寻找开标记的过程不得截断，
    /// `PARAMS_SCAN_LINES` 只约束块体。
    #[test]
    fn params_block_opening_after_old_search_limit_still_parses() {
        let mut src = String::new();
        for i in 0..(PARAMS_SCAN_LINES + 30) {
            src.push_str(&format!("G1 X{}\n", i));
        }
        src.push_str("{# PARAMS:\n     LATE  number  必选  晚开的参数\n#}\nG1 X1\n");

        let meta = extract_header_meta(&src);
        let names: Vec<&str> = meta.params.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            ["LATE"],
            "晚于旧上限的块必须解析而不是静默丢弃: {names:?}"
        );
        assert!(
            !meta.warnings.iter().any(|w| w.contains("仍未闭合")),
            "块正常闭合不该报截断: {:?}",
            meta.warnings
        );
    }

    /// 回归：同名重复行不得静默并存——按名查找/覆盖层都是 first-wins，
    /// 留两条会让"哪条生效"变成玄学；静默丢后一条又等于静默少一条约束。
    #[test]
    fn params_block_duplicate_names_warn_and_keep_first() {
        let src = "{# PARAMS:\
             \n     A number 必选 第一份\
             \n     A number 可选 第二份\
             \n     B number 必选 另一个\
             \n#}\n";
        let meta = extract_header_meta(src);
        let names: Vec<&str> = meta.params.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["A", "B"], "重复名只保留先声明者: {names:?}");
        assert!(
            meta.params[0].description.contains("第一份"),
            "以先声明者为准: {}",
            meta.params[0].description
        );
        let dup = meta
            .warnings
            .iter()
            .find(|w| w.contains("重复"))
            .expect("同名重复必须告警而不是静默");
        assert!(dup.contains("'A'"), "{dup}");
    }

    #[test]
    fn template_absent_from_manifest_is_visible_with_default_extension() {
        // 回归（P1-4）：`TemplateMeta` 此前 `#[derive(Default)]`，给出
        // visible=false + output_extension=""，而 serde 路径
        // （`#[serde(default = "default_true")]` / `default_extension`）给出
        // true/".NC"。`resolve` 对**清单未提及**的模板走 `unwrap_or_default()`，
        // 拿到的正是派生那个版本 —— 于是「往 templates/ 放一个新 .j2 而不加清单
        // 条目」= 用户列表里看不到它、输出文件名丢扩展名，且全程不报错。
        let r = ResolvedMeta::resolve(
            Path::new("milling/new_part.j2"),
            "; 新模板，清单里没有\n",
            None,
            &no_lib(),
        );
        assert!(r.visible, "清单未提及的模板缺省必须可见");
        assert_eq!(r.output_extension, ".NC", "缺省扩展名必须与 serde 路径一致");

        // 两条路径必须给出同一套缺省；不等就是「同一个缺省有两个含义」，
        // 正是本缺陷的成因，别只修一半。
        let m = TemplateManifest::from_yaml(
            "templates:\n  \"milling/new_part.j2\": {}\n",
            Path::new("templates.yaml"),
        )
        .unwrap();
        let from_serde = m.get("milling/new_part.j2").unwrap();
        assert_eq!(
            from_serde.visible,
            TemplateMeta::default().visible,
            "serde 缺省与 Default::default() 必须一致（visible）"
        );
        assert_eq!(
            from_serde.output_extension,
            TemplateMeta::default().output_extension,
            "serde 缺省与 Default::default() 必须一致（output_extension）"
        );
    }

    #[test]
    fn header_meta_tolerates_whitespace_control_marker() {
        // 模板常用 `{# NAME: xxx -#}` 让头部注释不产生空行；
        // 取值时必须剥掉 `#}` 前的 `-`，否则会得到 "xxx -"。
        let src = "{# NAME: 倒角子程序 -#}\n{# DESCRIPTION: 生成键槽倒角 -#}\n; body\n";
        let meta = extract_header_meta(src);
        assert_eq!(meta.name.as_deref(), Some("倒角子程序"));
        assert_eq!(meta.description.as_deref(), Some("生成键槽倒角"));
    }

    #[test]
    fn header_meta_plain_marker_unchanged() {
        let src = "{# NAME: 越程槽加工 #}\n{# DESCRIPTION: 生成 ES/FS 型越程槽 #}\n";
        let meta = extract_header_meta(src);
        assert_eq!(meta.name.as_deref(), Some("越程槽加工"));
        assert_eq!(meta.description.as_deref(), Some("生成 ES/FS 型越程槽"));
    }

    #[test]
    fn parses_bare_form() {
        let yaml = r#"
"machines/index_g420/1_0.MPF_A.j2":
  name: "INDEX G420 1_0_A"
  output_filename: "1_0"
  output_extension: ".MPF"
  machine: index_g420
"#;
        let m = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).unwrap();
        let meta = m.get("machines/index_g420/1_0.MPF_A.j2").unwrap();
        assert_eq!(meta.output_filename.as_deref(), Some("1_0"));
        assert_eq!(meta.output_extension, ".MPF");
        assert_eq!(meta.machine.as_deref(), Some("index_g420"));
        assert_eq!(m.machine_packs(), vec!["index_g420".to_string()]);
    }

    /// 回归：裸映射形式**不能**静默解析成空清单。
    ///
    /// 曾经的实现是「先试带键形式，失败再试裸映射」。但 `ManifestFile.templates`
    /// 带 `#[serde(default)]`，把裸映射解析成 `ManifestFile` 会**成功**并丢弃
    /// 所有未知键 → 得到空清单，回退分支永远轮不到。结果是：清单文件里写了一堆
    /// 条目，程序却当作没清单（字段全部走回退值），**不报任何错**。
    #[test]
    fn bare_form_is_not_silently_empty() {
        let yaml = r#"
"turning/a.j2":
  name: "A"
  visible: false
"turning/b.j2":
  name: "B"
"#;
        let m = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).unwrap();
        assert_eq!(m.len(), 2, "裸映射的两个条目都应被读到，而不是空清单");
        assert!(!m.get("turning/a.j2").unwrap().visible);
        assert!(m.get("turning/b.j2").unwrap().visible);
    }

    /// 空文件视作「无清单声明」，不报错（模板仍可用）。
    #[test]
    fn empty_file_is_empty_manifest() {
        let m = TemplateManifest::from_yaml("", Path::new("templates.yaml")).unwrap();
        assert!(m.is_empty());
        let m2 =
            TemplateManifest::from_yaml("\n# 只有注释\n", Path::new("templates.yaml")).unwrap();
        assert!(m2.is_empty());
    }

    /// 顶层是标量/序列时明确报错，而不是当成空清单静默通过。
    #[test]
    fn non_mapping_top_level_rejected() {
        assert!(TemplateManifest::from_yaml("- a\n- b\n", Path::new("t.yaml")).is_err());
        assert!(TemplateManifest::from_yaml("just a string", Path::new("t.yaml")).is_err());
    }

    /// 带 `templates:` 键但内容不合法时必须报错，不能静默降级为空清单。
    #[test]
    fn keyed_form_with_invalid_body_errors() {
        let yaml = r#"
templates:
  "a.j2":
    vizible: false
"#;
        assert!(TemplateManifest::from_yaml(yaml, Path::new("t.yaml")).is_err());
    }

    #[test]
    fn visible_defaults_true_and_extension_defaults_nc() {
        let yaml = r#"
templates:
  "a.j2": {}
"#;
        let m = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).unwrap();
        let meta = m.get("a.j2").unwrap();
        assert!(meta.visible, "未声明 visible 应缺省为 true");
        assert_eq!(meta.output_extension, ".NC", "未声明后缀应缺省为 .NC");
    }

    #[test]
    fn hidden_template() {
        let yaml = r#"
templates:
  "grooving/circlip.j2":
    visible: false
"#;
        let m = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).unwrap();
        assert!(!m.get("grooving/circlip.j2").unwrap().visible);
    }

    #[test]
    fn rejects_unknown_field() {
        // 拼错字段名必须报错，而不是静默忽略——静默忽略会让用户以为配置生效了
        let yaml = r#"
templates:
  "a.j2":
    vizible: false
"#;
        assert!(TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).is_err());
    }

    #[test]
    fn normalizes_backslash_keys() {
        // 用**单引号**（YAML 单引号内反斜杠是字面量）。双引号里 `\u` 会被当作
        // Unicode 转义（`\uXXXX`），`"turning\undercut.j2"` 直接解析失败
        // ——这是 YAML 规范行为，不是本模块的问题。
        let yaml = r#"
templates:
  'turning\undercut.j2':
    name: x
"#;
        let m = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).unwrap();
        assert!(m.get("turning/undercut.j2").is_some(), "反斜杠键应被规范化");
    }

    #[test]
    fn extracts_header_meta() {
        let src = "{# NAME: 21-闭口-非圆头键槽-精铣 #}\n{# DESCRIPTION: 闭口非圆头精铣 #}\nG0 X1\n";
        let h = extract_header_meta(src);
        assert_eq!(h.name.as_deref(), Some("21-闭口-非圆头键槽-精铣"));
        assert_eq!(h.description.as_deref(), Some("闭口非圆头精铣"));
    }

    // -------------------------------------------------------------------
    // {# PARAMS: #} 参数表解析
    // -------------------------------------------------------------------

    #[test]
    fn parses_params_block_four_column() {
        // 写法二：`name type required 描述`（turning/ 与 grooving/ 模板在用）
        let src = "\
{# NAME: 越程槽加工（FS 型） #}
{# PARAMS:
     side            string  required  加工侧面 (\"Right\" 或 \"Left\")
     FS_Z_PLUS1      number  required  FS Z+1 坐标 (mm)（side=Right 时使用）
     tip_model       string  可选      尾座中心孔型号
   #}
G1 Z1
";
        let h = extract_header_meta(src);
        assert!(h.warnings.is_empty(), "不应有解析告警: {:?}", h.warnings);
        assert_eq!(h.params.len(), 3);

        let side = &h.params[0];
        assert_eq!(side.name, "side");
        assert_eq!(side.kind, ParamKind::String);
        assert!(side.required);
        assert_eq!(side.description, "加工侧面 (\"Right\" 或 \"Left\")");

        assert_eq!(h.params[1].kind, ParamKind::Number);
        assert_eq!(h.params[2].name, "tip_model");
        assert!(!h.params[2].required, "「可选」应解析为 required=false");
    }

    #[test]
    fn parses_params_block_three_column() {
        // 写法一：`name 必选 描述`（machines/index_g420 的 19 个模板在用）。
        // 无类型声明 → ParamKind::Any（不做类型检查），但参数名/必选性/描述可用。
        let src = "\
{# NAME: 1_0_A #}
{# PARAMS:
     U_PRET_X               必选  车黑皮切深
     U_PRET_Z               必选  车黑皮Z位置
     left_T_START3_END3     必选  左侧夹口/台阶粗车轨迹
-#}
G1 X1
";
        let h = extract_header_meta(src);
        assert!(h.warnings.is_empty(), "不应有解析告警: {:?}", h.warnings);
        assert_eq!(h.params.len(), 3);
        assert_eq!(h.params[0].name, "U_PRET_X");
        assert_eq!(h.params[0].kind, ParamKind::Any);
        assert!(h.params[0].required);
        assert_eq!(h.params[0].description, "车黑皮切深");
    }

    #[test]
    fn params_block_reads_beyond_header_scan_limit() {
        // 参数表天然比 NAME/DESCRIPTION 长：机床模板最多十余个参数，
        // 收尾 `#}` 会落到第 10 行之后，不能沿用 10 行上限。
        let mut src = String::from("{# NAME: 长表 #}\n{# PARAMS:\n");
        for i in 0..18 {
            src.push_str(&format!("     P{i}   number  必选  参数{i}\n"));
        }
        src.push_str("-#}\nG1 X1\n");
        let h = extract_header_meta(&src);
        assert_eq!(h.params.len(), 18, "第 10 行之后的参数也必须被读到");
        assert!(h.warnings.is_empty(), "{:?}", h.warnings);
        assert_eq!(h.params[17].name, "P17");
        assert_eq!(h.params[17].kind, ParamKind::Number);
    }

    #[test]
    fn params_block_warns_on_malformed_line() {
        // 静默丢一行等于静默少一条参数约束（类型/白名单不再校验），
        // 因此无法解析的行必须产出告警交由调用方提示。
        let src = "\
{# PARAMS:
     ok_param     number  必选  正常一行
     bad_param    随意   必选  第二个词既不是类型也不是必选标记
     no_marker    number  描述漏了必选标记
   #}
G1 X1
";
        let h = extract_header_meta(src);
        assert_eq!(h.params.len(), 1, "只有合法行进入规格");
        assert_eq!(h.params[0].name, "ok_param");
        assert_eq!(h.warnings.len(), 2, "两行非法应各报一条: {:?}", h.warnings);
        assert!(h.warnings.iter().any(|w| w.contains("bad_param")));
        assert!(h.warnings.iter().any(|w| w.contains("no_marker")));
    }

    #[test]
    fn params_block_absent_or_single_line() {
        // 无 PARAMS 块
        let h = extract_header_meta("{# NAME: x #}\nG1 X1\n");
        assert!(h.params.is_empty());
        assert!(h.warnings.is_empty());

        // 单行形式
        let h2 = extract_header_meta("{# PARAMS: U_A 必选 键槽有效长度 #}\nG1 X1\n");
        assert_eq!(h2.params.len(), 1);
        assert_eq!(h2.params[0].name, "U_A");
        assert_eq!(h2.params[0].description, "键槽有效长度");

        // 未闭合的块：读到文件尾，不 panic
        let h3 = extract_header_meta("{# PARAMS:\n  A number 必选 描述\n");
        assert_eq!(h3.params.len(), 1);
    }

    #[test]
    fn params_block_accepts_ascii_markers_and_aliases() {
        let src = "\
{# PARAMS:
     a   int      required  整数别名
     b   boolean  optional  布尔别名
     c   enum     req       枚举别名
     d   text     opt       文本别名
     e   number   条件必选  条件必选（触发条件写在清单 required_if 里）
   #}
G1 X1
";
        let h = extract_header_meta(src);
        assert!(h.warnings.is_empty(), "{:?}", h.warnings);
        assert_eq!(h.params[0].kind, ParamKind::Integer);
        assert_eq!(h.params[1].kind, ParamKind::Bool);
        assert_eq!(h.params[2].kind, ParamKind::Choice);
        assert_eq!(h.params[3].kind, ParamKind::String);
        assert!(h.params[2].required);
        assert!(!h.params[3].required);
        // 「条件必选」在头部只表达必选性；触发条件由清单 required_if 补充
        assert!(h.params[4].required);
        assert!(h.params[4].required_if.is_none());
    }

    #[test]
    fn params_block_keeps_branch_qualifier_in_description() {
        // undercut.j2 用 `可选(ES)` / `可选(FS)` 标注"某分支专用"。
        // 头部表达不了触发条件，限定词保留到描述里作为可读信息，不参与必选判定。
        let src = "\
{# PARAMS:
     ES_Z         number  可选(ES) ES 型专用：ES Z 坐标 (mm)
     FS_Z_PLUS1   number  可选(FS) FS 型专用：FS Z+1 坐标 (mm)
     side         string  必选     加工侧面
   #}
G1 X1
";
        let h = extract_header_meta(src);
        assert!(h.warnings.is_empty(), "不应有解析告警: {:?}", h.warnings);
        assert_eq!(h.params.len(), 3);
        assert!(!h.params[0].required);
        assert_eq!(h.params[0].kind, ParamKind::Number);
        assert_eq!(h.params[0].description, "(ES) ES 型专用：ES Z 坐标 (mm)");
        assert_eq!(h.params[1].description, "(FS) FS 型专用：FS Z+1 坐标 (mm)");
        assert!(h.params[2].required);
    }

    #[test]
    fn required_marker_rejects_lookalikes() {
        // `requiredly` 之类不是标记，不能被前缀匹配误判
        assert_eq!(parse_required_marker("requiredly"), None);
        assert_eq!(parse_required_marker("随意"), None);
        assert_eq!(parse_required_marker("optimum"), None);
        // 纯标记
        assert_eq!(parse_required_marker("必选"), Some((true, None)));
        assert_eq!(parse_required_marker("可选"), Some((false, None)));
        assert_eq!(parse_required_marker("REQUIRED"), Some((true, None)));
        // 带限定词
        assert_eq!(
            parse_required_marker("可选(ES)"),
            Some((false, Some("(ES)".to_string())))
        );
    }

    // -------------------------------------------------------------------
    // 清单 params 稀疏覆盖层
    // -------------------------------------------------------------------

    #[test]
    fn manifest_params_override_merges_sparsely() {
        // 头部只声明了名字/类型/必选性；约束（这里用 options）写在清单里。
        // 覆盖**只改写了的字段**，未写的沿用头部声明。
        let src = "\
{# PARAMS:
     U_FX    string  必选  开口方向
     U_A     number  必选  键槽有效长度
   #}
G1 X1
";
        let meta = TemplateMeta {
            params: Some(vec![ParamOverride {
                name: "U_FX".into(),
                options: Some(Some(vec![
                    ParamValue::String("闭口".into()),
                    ParamValue::String("左开口".into()),
                ])),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let r = ResolvedMeta::resolve(Path::new("a.j2"), src, Some(&meta), &no_lib());
        assert_eq!(r.params.len(), 2);
        let u_fx = &r.params[0];
        assert_eq!(u_fx.kind, ParamKind::String, "未覆盖的 kind 应保留头部声明");
        assert!(u_fx.required, "未覆盖的 required 应保留头部声明");
        assert_eq!(u_fx.description, "开口方向", "未覆盖的 description 应保留");
        assert_eq!(
            u_fx.accepts_option(&ParamValue::String("左开口".into())),
            Some(true)
        );
        assert_eq!(
            u_fx.accepts_option(&ParamValue::String("上开口".into())),
            Some(false)
        );
    }

    #[test]
    fn manifest_params_can_add_undeclared_param_and_override_kind() {
        // 头部没写类型（写法一）→ Any；清单覆盖成 Choice 并给出白名单
        let src = "{# PARAMS:\n     U_ID   必选  键槽位置\n   #}\nG1 X1\n";
        let meta = TemplateMeta {
            params: Some(vec![ParamOverride {
                name: "U_ID".into(),
                kind: Some(ParamKind::Choice),
                options: Some(Some(vec![ParamValue::Integer(41), ParamValue::Integer(42)])),
                unit: Some(Some("号".into())),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let r = ResolvedMeta::resolve(Path::new("a.j2"), src, Some(&meta), &no_lib());
        assert_eq!(r.params[0].kind, ParamKind::Choice);
        assert_eq!(r.params[0].unit.as_deref(), Some("号"));
        assert_eq!(
            r.params[0].accepts_option(&ParamValue::Number(41.0)),
            Some(true)
        );

        // 头部完全没提到的参数也能在清单里新增
        let meta2 = TemplateMeta {
            params: Some(vec![ParamOverride {
                name: "extra".into(),
                description: Some("新增参数".into()),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let r2 = ResolvedMeta::resolve(Path::new("a.j2"), src, Some(&meta2), &no_lib());
        assert_eq!(r2.params.len(), 2);
        let extra = r2.params.iter().find(|p| p.name == "extra").unwrap();
        assert_eq!(extra.kind, ParamKind::Any);
        assert_eq!(extra.description, "新增参数");
    }

    #[test]
    fn manifest_params_override_rejects_unknown_field() {
        // 拼错字段名必须报错，否则约束会静默不生效
        let yaml = r#"
templates:
  "a.j2":
    params:
      - name: U_FX
        optons: ["闭口"]
"#;
        assert!(TemplateManifest::from_yaml(yaml, Path::new("t.yaml")).is_err());
    }

    #[test]
    fn manifest_params_override_round_trips() {
        // 手写 YAML 的自然写法：候选值/触发值直接写裸标量，不必写 {type, value}
        let yaml = r#"
templates:
  "turning/undercut_fs.j2":
    params:
      - name: FS_Z_PLUS1
        required_if:
          param: side
          values: ["Right"]
      - name: U_Q
        options: [0, 8, 12.5]
        unit: mm
"#;
        let m = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).unwrap();
        let params = m
            .get("turning/undercut_fs.j2")
            .unwrap()
            .params
            .as_ref()
            .unwrap();
        assert_eq!(params.len(), 2);
        // 外层 `Some` = 键写了；内层 `Some` = 写的是值（写 null 则内层为 None）
        let rif = params[0].required_if.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(rif.param, "side");
        assert!(rif.triggered_by(&ParamValue::String("Right".into())));
        assert!(!rif.triggered_by(&ParamValue::String("Left".into())));
        // `[0, 8, 12.5]` → 前两个是整数、第三个是浮点，与 YAML 标量类型一致
        let opts = params[1].options.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(opts[0], ParamValue::Integer(0));
        assert_eq!(opts[2], ParamValue::Number(12.5));
        assert_eq!(params[1].unit.as_ref().unwrap().as_deref(), Some("mm"));
        // 序列化仍走带标签形式（无歧义）
        let back = serde_yaml::to_string(&params[1]).unwrap();
        assert!(back.contains("type: integer"), "{back}");
    }

    #[test]
    fn params_block_wins_over_nothing_and_manifest_wins_over_header() {
        // 覆盖优先级：清单 > 头部注释
        let src = "{# PARAMS:\n     X   number  必选  头部描述\n   #}\nG1 X1\n";
        let meta = TemplateMeta {
            params: Some(vec![ParamOverride {
                name: "X".into(),
                description: Some("清单描述".into()),
                required: Some(false),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let r = ResolvedMeta::resolve(Path::new("a.j2"), src, Some(&meta), &no_lib());
        assert_eq!(r.params[0].description, "清单描述");
        assert!(!r.params[0].required);
        assert_eq!(r.params[0].kind, ParamKind::Number, "类型沿用头部");
    }

    #[test]
    fn header_meta_ignores_malformed_and_late_lines() {
        // 缺 `#}`：跳过不报错
        let bad = "{# NAME: 没有闭合\n";
        assert_eq!(extract_header_meta(bad).name, None);

        // 第 11 行：超出扫描范围
        let late = format!("{}\n{{# NAME: 太晚 #}}\n", "\n".repeat(10));
        assert_eq!(extract_header_meta(&late).name, None);
    }

    #[test]
    fn manifest_wins_over_header() {
        let src = "{# NAME: 头部名 #}\n";
        let meta = TemplateMeta {
            name: Some("清单名".into()),
            ..Default::default()
        };
        let r = ResolvedMeta::resolve(Path::new("a.j2"), src, Some(&meta), &no_lib());
        assert_eq!(r.name, "清单名");
    }

    #[test]
    fn header_wins_over_filename() {
        let src = "{# NAME: 头部名 #}\n";
        let r = ResolvedMeta::resolve(Path::new("a.j2"), src, None, &no_lib());
        assert_eq!(r.name, "头部名");
    }

    #[test]
    fn filename_is_last_resort() {
        let r = ResolvedMeta::resolve(Path::new("sub/a.j2"), "G0 X1", None, &no_lib());
        assert_eq!(r.name, "a.j2");
    }

    #[test]
    fn classifies_by_directory() {
        use TemplateCategory::*;
        assert_eq!(classify_by_path(Path::new("turning/a.j2")), Turning);
        assert_eq!(classify_by_path(Path::new("milling/a.j2")), Milling);
        assert_eq!(classify_by_path(Path::new("drilling/a.j2")), Drilling);
        assert_eq!(classify_by_path(Path::new("machines/x/a.j2")), Machine);
        assert_eq!(classify_by_path(Path::new("general/a.j2")), General);
        // 未知目录 → 通用
        assert_eq!(classify_by_path(Path::new("whatever/a.j2")), General);
        // 大小写不敏感
        assert_eq!(classify_by_path(Path::new("Turning/a.j2")), Turning);
        // 取第一个命中的目录
        assert_eq!(
            classify_by_path(Path::new("turning/machines/a.j2")),
            Turning
        );
    }

    #[test]
    fn manifest_category_overrides_path() {
        let meta = TemplateMeta {
            category: Some(TemplateCategory::Drilling),
            ..Default::default()
        };
        let r = ResolvedMeta::resolve(Path::new("turning/a.j2"), "", Some(&meta), &no_lib());
        assert_eq!(r.category, TemplateCategory::Drilling);
    }

    #[test]
    fn path_to_rel_key_uses_forward_slash() {
        let key = path_to_rel_key(
            Path::new("machines")
                .join("index_g420")
                .join("a.j2")
                .as_path(),
        );
        assert_eq!(key, "machines/index_g420/a.j2");
    }

    // -------------------------------------------------------------------
    // 参数规格的三级优先级：清单 params > 变量库 > 头部 PARAMS
    // -------------------------------------------------------------------

    /// 变量库（全局按名定义）：`U_Q` 限 0/8。
    fn u_q_library() -> crate::variables::VariableLibrary {
        crate::variables::VariableLibrary::from_yaml(
            "variables:\n  - name: U_Q\n    kind: choice\n    options: [0, 8]\n",
            Path::new("variables.yaml"),
        )
        .unwrap()
    }

    /// 回归（P1-13）：清单覆盖此前只能**设**继承来的字段、不能**清** ——
    /// `Option<T>` 一旦有值就回不到 `None`。后果：变量库给 `U_Q` 声明了
    /// `min` / 白名单 / `unit` 之后，**所有**模板都被套上，某个确实需要负值
    /// （或不想带白名单）的模板在清单里写什么都解不掉。继承来的 `derive` 同理，
    /// 想关掉只能改变量库 —— 那会波及所有模板。
    ///
    /// 现在 `键: null` 表示清空；白名单上 `[]` 与 `null` 等价。
    #[test]
    fn manifest_null_clears_inherited_field() {
        let lib = crate::variables::VariableLibrary::from_yaml(
            "variables:\n  - name: U_Q\n    kind: number\n    min: 0\n    unit: mm\n    options: [0, 8]\n",
            Path::new("variables.yaml"),
        )
        .unwrap();
        let src = "{# PARAMS:\n     U_Q   number  必选  槽宽\n   #}\nX{{ U_Q }}\n";

        // 基线：清单没提及 → 三样都从变量库继承
        let r = ResolvedMeta::resolve(Path::new("a.j2"), src, None, &lib);
        assert_eq!(r.params[0].min, Some(0.0));
        assert_eq!(r.params[0].unit.as_deref(), Some("mm"));
        assert!(r.params[0].options.is_some());

        // 写 null → 逐项清空
        let yaml = r#"
templates:
  "a.j2":
    params:
      - name: U_Q
        min: null
        unit: null
        options: null
"#;
        let m = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).unwrap();
        let r = ResolvedMeta::resolve(Path::new("a.j2"), src, m.get("a.j2"), &lib);
        assert_eq!(r.params[0].min, None, "min: null 必须解除继承的下界");
        assert_eq!(r.params[0].unit, None, "unit: null 必须解除继承的单位");
        assert_eq!(
            r.params[0].options, None,
            "options: null 必须解除继承的白名单"
        );
        assert_eq!(
            r.params[0].kind,
            ParamKind::Number,
            "没写的字段不该被牵连（类型仍来自变量库）"
        );

        // `options: []` 与 `null` 等价；未写的字段保持继承
        let yaml = "templates:\n  \"a.j2\":\n    params:\n      - name: U_Q\n        options: []\n";
        let m = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).unwrap();
        let r = ResolvedMeta::resolve(Path::new("a.j2"), src, m.get("a.j2"), &lib);
        assert_eq!(r.params[0].options, None, "options: [] 也必须解除白名单");
        assert_eq!(r.params[0].min, Some(0.0), "未写的 min 应保持继承值");
    }

    #[test]
    fn variable_library_beats_header_declaration() {
        // 头部只写了 `U_Q number 必选 槽宽`（迁移模板的常见形态），
        // 变量库把类型升为 choice 并补上候选值；描述仍来自头部。
        let src = "{# PARAMS:\n     U_Q   number  必选  槽宽\n   #}\nX{{ U_Q }}\n";
        let r = ResolvedMeta::resolve(Path::new("a.j2"), src, None, &u_q_library());
        assert_eq!(r.params.len(), 1);
        assert_eq!(r.params[0].kind, ParamKind::Choice);
        assert_eq!(
            r.params[0].accepts_option(&ParamValue::Number(8.0)),
            Some(true)
        );
        assert_eq!(
            r.params[0].accepts_option(&ParamValue::Number(9.0)),
            Some(false)
        );
        assert_eq!(r.params[0].description, "槽宽", "库不覆盖描述");
        assert!(r.params[0].required, "库不覆盖头部声明的必选性");
    }

    #[test]
    fn manifest_params_override_beats_variable_library() {
        // 本模板需要库之外的取值时，清单覆盖胜出（覆盖是本模板局部的）
        let src = "{# PARAMS:\n     U_Q   number  必选  槽宽\n   #}\nX{{ U_Q }}\n";
        let meta = TemplateMeta {
            params: Some(vec![ParamOverride {
                name: "U_Q".into(),
                options: Some(Some(vec![ParamValue::Number(0.0), ParamValue::Number(6.0)])),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let r = ResolvedMeta::resolve(Path::new("a.j2"), src, Some(&meta), &u_q_library());
        assert_eq!(
            r.params[0].accepts_option(&ParamValue::Number(6.0)),
            Some(true)
        );
        assert_eq!(
            r.params[0].accepts_option(&ParamValue::Number(8.0)),
            Some(false),
            "清单覆盖应胜出变量库"
        );
        // 清单未覆盖的字段仍沿用变量库
        assert_eq!(r.params[0].kind, ParamKind::Choice);
    }

    #[test]
    fn variable_library_adds_spec_for_referenced_undeclared_param() {
        // 头部没声明、但源码引用了 → 库补上规格（19 个迁移模板不必逐个改头部）
        let src = "{# PARAMS:\n     U_A   number  必选  长度\n   #}\nX{{ U_A }} Y{{ U_Q }}\n";
        let r = ResolvedMeta::resolve(Path::new("a.j2"), src, None, &u_q_library());
        assert_eq!(r.params.len(), 2);
        let u_q = r.params.iter().find(|s| s.name == "U_Q").unwrap();
        assert_eq!(u_q.kind, ParamKind::Choice);
        // 库未定义的 U_A 保持头部声明
        let u_a = r.params.iter().find(|s| s.name == "U_A").unwrap();
        assert_eq!(u_a.kind, ParamKind::Number);
    }

    // -----------------------------------------------------------------------
    // ERR-NUM-UNDERFLOW：清单 YAML 通道
    // -----------------------------------------------------------------------

    /// 清单里的下溢字面量必须硬失败（否则默认值/上下界被静默归零）。
    #[test]
    fn manifest_rejects_underflow_literal() {
        let yaml = r#"
templates:
  "turning/a.j2":
    params:
      - name: X
        default: 1e-400
"#;
        let err = TemplateManifest::from_yaml(yaml, Path::new("templates.yaml"))
            .expect_err("下溢字面量应被拒绝");
        match err {
            ManifestError::Underflow {
                literal,
                line,
                column,
                ..
            } => {
                assert_eq!(literal, "1e-400");
                assert!(line >= 1 && column >= 1, "应带行列定位: {line}:{column}");
            }
            other => panic!("应为 Underflow，得到: {other:?}"),
        }
    }

    /// 注释里的 `# note: 1e-400` 不是数值 → 0 候选，不得误报。
    #[test]
    fn manifest_comment_is_not_underflow() {
        let yaml = r#"
# note: 1e-400
templates:
  "turning/a.j2":
    name: 测试
"#;
        assert!(
            crate::json_num::scan_underflow_candidates_yaml(yaml).is_empty(),
            "注释内容不应产生候选"
        );
        assert!(TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).is_ok());
    }

    /// serde_yaml 正确舍入：`2.4703282292062328e-324` 解析得 `5e-324`（非零），
    /// 确认结果为 **false** —— 它是候选但**不是**下溢，必须放行。
    #[test]
    fn manifest_yaml_confirm_false_for_rounding_safe_literal() {
        let yaml = r#"
templates:
  "turning/a.j2":
    params:
      - name: X
        default: 2.4703282292062328e-324
"#;
        let cands = crate::json_num::scan_underflow_candidates_yaml(yaml);
        assert_eq!(cands.len(), 1, "应为候选（真值 < 2^-1000）");
        assert!(
            !crate::json_num::confirm_underflow_yaml(&cands[0]),
            "serde_yaml 正确舍入得非零 → 确认应为 false"
        );
        assert!(
            TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).is_ok(),
            "确认 false ⇒ 放行"
        );
    }

    /// 合法次正规数 `5e-324`：候选但确认非下溢 → 放行。
    #[test]
    fn manifest_accepts_legal_subnormal() {
        let yaml = r#"
templates:
  "turning/a.j2":
    params:
      - name: X
        default: 5e-324
"#;
        assert!(TemplateManifest::from_yaml(yaml, Path::new("templates.yaml")).is_ok());
        let yaml2 = r#"
templates:
  "turning/a.j2":
    params:
      - name: X
        default: 1e-323
"#;
        assert!(TemplateManifest::from_yaml(yaml2, Path::new("templates.yaml")).is_ok());
    }
}
