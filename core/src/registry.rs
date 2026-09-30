//! 模板注册表：统一管理内存模板、文件系统模板与内置模板库。
//!
//! 每个模板条目携带分类、描述与参数规格，支持：
//! - 按分类列出/筛选模板
//! - 渲染前参数校验（委托 [`crate::validate::validate_template`]）
//! - 渲染（含 `{% include %}` / `{% extends %}` 等模板间引用）

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use nctool_tpl::{Renderer, Value, ValueKind};

use crate::model::ParamSpec;
use crate::validate::{
    validate_template_with_machine, validate_with_vars_with_machine, ValidationReport,
};

/// 模板分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateCategory {
    /// 通用子程序（程序头/尾、换刀、安全移动、主轴/冷却）
    General,
    /// 铣削
    Milling,
    /// 车削
    Turning,
    /// 钻孔/攻丝/铰孔
    Drilling,
    /// 切槽（卡簧槽、越程槽等成形槽）
    Grooving,
    /// 机床特定
    Machine,
}

impl TemplateCategory {
    /// 分类标签（用于列表展示）。
    pub fn label(&self) -> &'static str {
        match self {
            TemplateCategory::General => "通用",
            TemplateCategory::Milling => "铣削",
            TemplateCategory::Turning => "车削",
            TemplateCategory::Drilling => "钻孔",
            TemplateCategory::Grooving => "切槽",
            TemplateCategory::Machine => "机床",
        }
    }

    /// 全部分类（用于遍历式守卫测试与 UI 枚举）。
    ///
    /// **新增变体必须同步登记**：`aliases` / `dir_names` / `label` 是穷尽匹配
    /// （编译器强制），`ALL` 由守卫测试与变体数对拍守住。
    pub const ALL: [TemplateCategory; 6] = [
        TemplateCategory::General,
        TemplateCategory::Milling,
        TemplateCategory::Turning,
        TemplateCategory::Drilling,
        TemplateCategory::Grooving,
        TemplateCategory::Machine,
    ];

    /// 用户可见名（英文 id，大小写不敏感）。穷尽匹配——新增变体必须登记，
    /// 否则 HTTP/CLI 侧会把新分类判为非法值。
    fn aliases(&self) -> &'static [&'static str] {
        match self {
            TemplateCategory::General => &["general"],
            TemplateCategory::Milling => &["milling"],
            TemplateCategory::Turning => &["turning"],
            TemplateCategory::Drilling => &["drilling"],
            TemplateCategory::Grooving => &["grooving"],
            TemplateCategory::Machine => &["machine"],
        }
    }

    /// 模板目录名（大小写不敏感）。穷尽匹配——新增变体必须登记，否则
    /// `templates/<新目录>/` 下的模板会被静默归入"通用"。
    fn dir_names(&self) -> &'static [&'static str] {
        match self {
            TemplateCategory::General => &["general", "common"],
            TemplateCategory::Milling => &["milling", "mill"],
            TemplateCategory::Turning => &["turning", "turn"],
            TemplateCategory::Drilling => &["drilling", "drill"],
            TemplateCategory::Grooving => &["grooving", "groove"],
            TemplateCategory::Machine => &["machines", "machine"],
        }
    }

    /// 目录名 → 分类。目录归类表的**唯一入口**。
    pub fn from_dir_name(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        Self::ALL
            .iter()
            .copied()
            .find(|c| c.dir_names().iter().any(|d| *d == lower))
    }
}

impl std::str::FromStr for TemplateCategory {
    type Err = ();

    /// 分类名解析的**唯一入口**（中英文、大小写不敏感）：英文 id 用
    /// `aliases()`，中文显示名用 [`Self::label`]。
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.trim().to_ascii_lowercase();
        Self::ALL
            .iter()
            .copied()
            .find(|c| c.aliases().iter().any(|a| *a == lower) || c.label() == lower)
            .ok_or(())
    }
}

/// 模板源码来源。
#[derive(Debug, Clone)]
pub enum TemplateSource {
    /// 内存模板（源码可通过 [`TemplateEntry::source_text`] 读取，不重复存储）
    Memory,
    /// 文件系统模板（注册时加载内容，记录路径）
    File(PathBuf),
    /// 内置模板库
    Builtin,
}

/// 模板条目：注册表中的一个模板。
#[derive(Debug, Clone)]
pub struct TemplateEntry {
    /// 模板名（唯一，用于引用与渲染）
    pub name: String,
    /// 分类
    pub category: TemplateCategory,
    /// 描述
    pub description: String,
    /// 源码来源
    pub source: TemplateSource,
    /// 参数规格（渲染前校验）
    pub params: Vec<ParamSpec>,
    /// 模板源码（统一为字符串，供渲染）。请用 [`Self::source_text`] 读取；更新源码
    /// 使用 [`Self::with_source_text`]，保证静态分析缓存同步失效。
    source_text: String,
    /// 是否在模板列表中可见（`false` = 功能模块专用，程序可调用但不展示）。
    ///
    /// 这是**面向用户的视图过滤**，与模板的真实可用性解耦：被隐藏的模板
    /// 依然可以被 `include` 或被 CLI 直接渲染，只是不出现在选择列表里。
    /// 由模板清单 `templates.yaml` 的 `visible` 字段声明，缺省 `true`。
    pub visible: bool,
    /// 默认输出文件名（不含扩展名）。
    pub output_filename: Option<String>,
    /// 默认输出扩展名（含点，如 `.NC` / `.MPF` / `.SPF`）。
    pub output_extension: String,
    /// 归属的机床方案包 id（`None` = 对全部机床可见）。
    ///
    /// 机床专用模板（含机床专有 G 代码）声明此项后，仅在选定对应机床时暴露，
    /// 避免通用场景误用而产出无法执行的程序。
    pub machine: Option<String>,
    /// 工艺评审状态（`None` = 未标注）。
    pub status: Option<crate::manifest::TemplateStatus>,
    /// 静态分析缓存（惰性，见 [`TemplateEntry::analysis`]）。
    ///
    /// 私有：缓存是**实现细节**，外部只通过 `analysis()` 取用；
    /// 若把它暴露出去，调用方就能塞进与 `source_text` 不符的结论。
    analysis: OnceCell<Result<Analysis, nctool_tpl::TplError>>,
}

/// 模板的静态分析产物：解析一次，多处复用。
///
/// [`TemplateEntry`] 缓存的是**解析产物**而非 AST 本身——`nctool_tpl::Ast`
/// 借用源码（`Ast<'a>`），无法自引用地存进条目里。
///
/// 这两个字段正是 [`TemplateRegistry::extract_params`] 与
/// [`TemplateRegistry::validate`] 需要的全部信息：变量表用于参数校验，
/// 模板引用用于穿透 `{% include %}` / `{% extends %}` 闭包。
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    /// 未声明变量（含可选/必选判定与行列定位）
    pub variables: Vec<nctool_tpl::Variable>,
    /// 引用的模板名（`{% include %}` / `{% extends %}` / `{% import %}`）
    pub refs: Vec<String>,
}

impl Analysis {
    /// 解析源码并提取分析产物；解析失败返回带行列定位的原始错误。
    fn of(source: &str, name: &str) -> Result<Self, nctool_tpl::TplError> {
        let ast = nctool_tpl::parse(source, name)?;
        Ok(Self {
            variables: nctool_tpl::extract_undeclared(&ast),
            refs: nctool_tpl::extract_template_refs(&ast),
        })
    }
}

impl TemplateEntry {
    /// 构造最小条目（补齐新增字段的缺省值），供既有调用方平滑迁移。
    ///
    /// 缺省：`visible = true`、`output_extension = ".NC"`、无输出文件名、
    /// 不绑定机床、未标注评审状态。
    pub fn new(
        name: impl Into<String>,
        category: TemplateCategory,
        description: impl Into<String>,
        source: TemplateSource,
        params: Vec<ParamSpec>,
        source_text: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            category,
            description: description.into(),
            source,
            params,
            source_text: source_text.into(),
            visible: true,
            output_filename: None,
            output_extension: ".NC".to_string(),
            machine: None,
            status: None,
            analysis: OnceCell::new(),
        }
    }

    /// 惰性静态分析（首次调用解析，之后直接复用）。
    ///
    /// 解析**失败也会被缓存**（负缓存），避免每次取用都重跑一遍注定失败的解析；
    /// 需要结构化错误（行列定位）的调用方在 `Err` 分支自行 `nctool_tpl::parse`
    /// 重取一次——该分支只在模板本身有语法错误时走到。
    ///
    /// # 与源码更新的关系
    /// 本缓存以私有 `source_text` 为准。源码只可通过 [`Self::with_source_text`] 替换，
    /// 该方法会同步使缓存失效，避免继续使用旧源码的分析结论。
    pub fn analysis(&self) -> Result<&Analysis, &nctool_tpl::TplError> {
        match self
            .analysis
            .get_or_init(|| Analysis::of(&self.source_text, &self.name))
        {
            Ok(a) => Ok(a),
            Err(e) => Err(e),
        }
    }

    /// 读取模板源码。
    pub fn source_text(&self) -> &str {
        &self.source_text
    }

    /// 替换模板源码并失效旧静态分析缓存。
    pub fn with_source_text(mut self, source_text: impl Into<String>) -> Self {
        self.source_text = source_text.into();
        self.invalidate_analysis();
        self
    }

    /// 丢弃静态分析缓存；源码不可直接修改，故仅由安全的更新方法调用。
    fn invalidate_analysis(&mut self) {
        self.analysis = OnceCell::new();
    }

    /// 设置可见性。
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// 设置默认输出文件名与扩展名。
    pub fn with_output(mut self, filename: Option<String>, extension: impl Into<String>) -> Self {
        self.output_filename = filename;
        self.output_extension = extension.into();
        self
    }

    /// 设置归属机床方案包。
    pub fn with_machine(mut self, machine: Option<String>) -> Self {
        self.machine = machine;
        self
    }

    /// 设置工艺评审状态。
    pub fn with_status(mut self, status: Option<crate::manifest::TemplateStatus>) -> Self {
        self.status = status;
        self
    }
}

/// 注册表错误。
#[derive(Debug)]
#[non_exhaustive]
pub enum RegistryError {
    /// 模板不存在
    NotFound(String),
    /// 模板名重复
    Duplicate(String),
    /// 模板源码为空
    EmptySource(String),
    /// 模板源码无法编译（语法错误等）。
    ///
    /// `err` 的 [`Display`](std::fmt::Display) 已携带模板名与行列定位，
    /// 本变体的 Display 不再重复包装模板名。
    Compile {
        /// 模板名
        name: String,
        /// 底层模板错误
        err: nctool_tpl::TplError,
    },
    /// 读取文件失败
    Io(std::io::Error),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::NotFound(name) => write!(f, "模板不存在: {name}"),
            RegistryError::Duplicate(name) => write!(f, "模板名重复: {name}"),
            RegistryError::EmptySource(name) => write!(f, "模板源码为空: {name}"),
            RegistryError::Compile { err, .. } => write!(f, "模板无法编译: {err}"),
            RegistryError::Io(err) => write!(f, "文件读取失败: {err}"),
        }
    }
}

impl std::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RegistryError::Compile { err, .. } => Some(err),
            RegistryError::Io(err) => Some(err),
            _ => None,
        }
    }
}

/// 内置模板安装失败的记录（降级方案的**可见性出口**）。
///
/// 背景（P1-4）：[`TemplateRegistry::new`] 内部曾对内置模板注册用
/// `.expect()`——一次编译失败就让进程在**启动瞬间** panic 且无诊断。现改为
/// 「跳过该模板 + 记录」；但**降级比 panic 更危险**（崩溃立刻可见，某个内置
/// 模板悄悄缺失不会），故每条失败必须可查、必经 stderr / `/health` 暴露。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinWarning {
    /// 未安装的内置模板名。
    pub name: String,
    /// 失败原因（[`RegistryError`] 的 Display；`Compile` 变体已含模板名与行列）。
    pub message: String,
}

/// 模板注册表。
///
/// 内部持有 [`Renderer`]（注册模板并复用其编译缓存），是模板的单一权威来源。
#[derive(Debug)]
pub struct TemplateRegistry {
    entries: BTreeMap<String, TemplateEntry>,
    renderer: Renderer,
    /// 系统注入变量名（如 `machine`）：渲染时由管线注入上下文，
    /// 校验时视为已提供，不要求参数集提供。
    system_vars: Vec<String>,
    /// 宽松模式渲染器缓存（惰性构建；注册新模板时失效）。
    ///
    /// 宽松是建 `Environment` 时的标志，无法在同一个渲染器上切换，因此需要
    /// 第二个环境。早期实现**每次调用**都新建渲染器并把全部模板重新注册、
    /// 重新编译一遍——成本与模板数成正比，且发生在每个宽松渲染请求上。
    /// 改为惰性构建一次；失败原因（某模板编译不过）一并缓存，语义不变。
    lenient_cache: OnceCell<Result<Renderer, nctool_tpl::TplError>>,
    /// 内置模板安装失败列表（P1-4 降级方案的结构化留痕；正常恒为空）。
    builtin_warnings: Vec<BuiltinWarning>,
}

impl TemplateRegistry {
    /// 创建空注册表（含默认内置模板库）。
    pub fn new() -> Self {
        let mut registry = Self {
            entries: BTreeMap::new(),
            renderer: Renderer::new(),
            system_vars: vec!["machine".to_string()],
            lenient_cache: OnceCell::new(),
            builtin_warnings: Vec::new(),
        };
        registry.install_builtins();
        registry
    }

    /// 注册一个模板条目。
    pub fn add_entry(&mut self, entry: TemplateEntry) -> Result<(), RegistryError> {
        if entry.source_text.trim().is_empty() {
            return Err(RegistryError::EmptySource(entry.name.clone()));
        }
        if self.entries.contains_key(&entry.name) {
            return Err(RegistryError::Duplicate(entry.name.clone()));
        }
        // 同步到渲染器（支持模板间 include/extends/import）
        self.renderer
            .add_template(entry.name.clone(), entry.source_text.clone())
            .map_err(|err| RegistryError::Compile {
                name: entry.name.clone(),
                err,
            })?;
        self.entries.insert(entry.name.clone(), entry);
        // 模板集变了：宽松渲染器缓存随之失效（否则新模板在宽松模式下不可见）
        self.lenient_cache = OnceCell::new();
        Ok(())
    }

    /// 从内存模板注册（便捷构造）。
    pub fn add_memory(
        &mut self,
        name: impl Into<String>,
        category: TemplateCategory,
        description: impl Into<String>,
        source: impl Into<String>,
        params: Vec<ParamSpec>,
    ) -> Result<(), RegistryError> {
        let name = name.into();
        let source_text = source.into();
        self.add_entry(TemplateEntry::new(
            name,
            category,
            description,
            TemplateSource::Memory,
            params,
            source_text,
        ))
    }

    /// 从文件系统模板注册（加载文件内容）。
    pub fn add_file(
        &mut self,
        name: impl Into<String>,
        category: TemplateCategory,
        description: impl Into<String>,
        path: impl AsRef<Path>,
        params: Vec<ParamSpec>,
    ) -> Result<(), RegistryError> {
        let name = name.into();
        let path = path.as_ref().to_path_buf();
        // 上限读取（P1-1）：模板源码后续会走 `nctool_tpl::parse`，且清单/变量库
        // 侧还要交给 `serde_yaml`（别名展开无预算）。上限必须在读入时就生效。
        let source_text =
            crate::io_limit::read_text_capped(&path, crate::io_limit::MAX_SOURCE_BYTES).map_err(
                |e| {
                    // 把路径并进 io::Error 的消息里：`RegistryError::Io` 只装 io::Error，
                    // 不补路径时用户看到的是「文件读取失败: 系统找不到指定的文件」——
                    // 完全不知道是哪个文件。改变体形状是破坏性变更（crate 已发布），
                    // 故在构造处补，`source()` 与变体形状都不变。
                    RegistryError::Io(std::io::Error::new(
                        e.kind(),
                        format!("{}: {e}", path.display()),
                    ))
                },
            )?;
        self.add_entry(TemplateEntry::new(
            name,
            category,
            description,
            TemplateSource::File(path),
            params,
            source_text,
        ))
    }

    /// 按名称获取模板条目。
    pub fn get(&self, name: &str) -> Option<&TemplateEntry> {
        self.entries.get(name)
    }

    /// 列出模板（可按分类筛选）。
    ///
    /// **不过滤可见性**——需要面向用户的列表用 [`Self::list_visible`]。
    pub fn list(&self, category: Option<TemplateCategory>) -> Vec<&TemplateEntry> {
        self.entries
            .values()
            .filter(|e| category.is_none_or(|c| e.category == c))
            .collect()
    }

    /// 列出**可见**模板（可按分类筛选），跳过 `visible = false` 的条目。
    ///
    /// 供 `templates list` 等面向用户的场景使用；程序化遍历用 [`Self::list`]。
    pub fn list_visible(&self, category: Option<TemplateCategory>) -> Vec<&TemplateEntry> {
        self.entries
            .values()
            .filter(|e| e.visible)
            .filter(|e| category.is_none_or(|c| e.category == c))
            .collect()
    }

    /// 列出归属指定机床方案包（或对所有机床可见）的模板。
    ///
    /// `machine = None` 时只返回**不绑定机床**的通用模板；指定 id 时
    /// 额外包含该方案包内的模板。
    pub fn list_for_machine(
        &self,
        machine: Option<&str>,
        category: Option<TemplateCategory>,
        include_hidden: bool,
    ) -> Vec<&TemplateEntry> {
        self.entries
            .values()
            .filter(|e| include_hidden || e.visible)
            .filter(|e| category.is_none_or(|c| e.category == c))
            .filter(|e| match (&e.machine, machine) {
                // 通用模板：任何机床场景都可用
                (None, _) => true,
                // 机床专用模板：仅当其方案包与选定机床一致
                (Some(m), Some(id)) => m == id,
                // 机床专用模板 + 未指定机床 → 不暴露
                (Some(_), None) => false,
            })
            .collect()
    }

    /// 模板数量。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 提取模板的**完整参数闭包**（穿透 `{% include %}` / `{% extends %}`）。
    ///
    /// [`validate`](Self::validate) 内部已做同样的事，但只返回"缺没缺"的结论。
    /// 本方法把中间结果直接暴露出来，供 `inspect` 这类需要**列出参数表**的
    /// 调用方使用——否则组合模板（主模板 + `include` 片段）会只列出主模板
    /// 自身的变量，遗漏片段引用的参数，用户按表填参会渲染失败。
    ///
    /// 注入的系统变量（默认 `machine`）已从结果中剔除：它们由管线提供，
    /// 不是用户需要填写的参数。
    ///
    /// 语义与 [`validate`](Self::validate) 保持一致：同名变量的必选性取"或"，
    /// 环引用有防护，未注册的被引用模板静默跳过（其变量无法静态并入）。
    ///
    /// 模板不存在时返回 [`RegistryError::NotFound`]。
    pub fn extract_params(&self, name: &str) -> Result<Vec<nctool_tpl::Variable>, RegistryError> {
        let entry = self
            .entries
            .get(name)
            .ok_or_else(|| RegistryError::NotFound(name.to_string()))?;
        // 复用静态分析缓存；解析失败时返回带行列定位的原始错误
        // （`TplError` 已实现 `Clone`，故可直接从缓存取出）
        let analysis = match entry.analysis() {
            Ok(a) => a,
            Err(err) => {
                return Err(RegistryError::Compile {
                    name: entry.name.clone(),
                    err: err.clone(),
                })
            }
        };
        let mut vars = analysis.variables.clone();
        let mut specs = entry.params.clone();
        let mut visited = std::collections::BTreeSet::from([entry.name.clone()]);
        self.collect_include_closure(entry, &mut vars, &mut specs, &mut visited);
        // 系统注入变量由管线提供，不从用户处索要
        vars.retain(|v| !self.system_vars.iter().any(|s| s == &v.name));
        Ok(vars)
    }

    /// 校验指定模板的参数（渲染前调用）。
    ///
    /// 校验会**穿透 `{% include %}` / `{% extends %}` 等模板间引用**：被引用
    /// 且已在注册表中注册的模板，其必选参数同样参与检查，避免组合模板的
    /// 参数缺失只能在渲染阶段才暴露。被引用模板未注册时（渲染期报
    /// `TplError::TemplateNotFound`），其变量无法静态并入，仍以主模板自身为准。
    ///
    /// 返回 [`ValidationReport`]，调用方据 [`ValidationReport::is_ok`] 决定是否渲染；
    /// 模板不存在时返回 [`RegistryError::NotFound`]。
    /// 系统注入变量（默认 `machine`）视为已提供，不要求参数集提供。
    pub fn validate(
        &self,
        name: &str,
        params: &crate::model::ParameterSet,
    ) -> Result<ValidationReport, RegistryError> {
        self.validate_with_machine(name, params, None)
    }

    /// 机床联动校验（P0-2/Q-01）：`machine` 参与规格 `max_from` 声明的动态
    /// 上界（如 `tool_change` 的 `spindle_speed` 按机床 `max_spindle_rpm` 收紧）。
    ///
    /// `machine = None` 等价于 [`Self::validate`]（纯静态上界）。渲染管线与
    /// render/validate/machine test 等命令应传 `Some`，让"校验通过"与"渲染用的
    /// 机床"是同一台。
    pub fn validate_with_machine(
        &self,
        name: &str,
        params: &crate::model::ParameterSet,
        machine: Option<&crate::model::MachineConfig>,
    ) -> Result<ValidationReport, RegistryError> {
        let entry = self
            .entries
            .get(name)
            .ok_or_else(|| RegistryError::NotFound(name.to_string()))?;
        let system: Vec<&str> = self.system_vars.iter().map(String::as_str).collect();
        // 主模板解析失败：由 validate_template 报告（含行列定位）。
        // 这里刻意走一次完整重解析而非从缓存取错误——`validate_template` 会把
        // 行列定位包装成 `IssueKind::ParseError` 问题项，语义必须与既有一致；
        // 该分支仅在模板本身有语法错误时进入。
        let analysis = match entry.analysis() {
            Ok(a) => a,
            Err(_) => {
                return Ok(validate_template_with_machine(
                    &entry.source_text,
                    &entry.name,
                    &entry.params,
                    params,
                    &system,
                    machine,
                ))
            }
        };
        // 主模板变量 + include/extends 闭包（穿透模板间引用，防环）
        let mut vars = analysis.variables.clone();
        let mut specs = entry.params.clone();
        let mut visited = std::collections::BTreeSet::from([entry.name.clone()]);
        self.collect_include_closure(entry, &mut vars, &mut specs, &mut visited);
        Ok(validate_with_vars_with_machine(
            &vars, &specs, params, &system, machine,
        ))
    }

    /// 递归并入 include/extends 引用的已注册模板的未声明变量与规格。
    ///
    /// 同名变量的必选性取"或"（任一处非兜底引用即必选）；规格先访问者优先
    /// （更接近主模板的声明，同名不覆盖）。环引用由 `visited` 防护。
    ///
    /// 入口是**条目**而非 AST：AST 借用源码、无法跨调用持有，改为读取条目上
    /// 惰性缓存的 [`Analysis`]（解析结果），使 `extract_params` / `validate`
    /// 与全部子模板都只解析一次。
    fn collect_include_closure(
        &self,
        entry: &TemplateEntry,
        vars: &mut Vec<nctool_tpl::Variable>,
        specs: &mut Vec<ParamSpec>,
        visited: &mut std::collections::BTreeSet<String>,
    ) {
        // 解析失败的模板没有可并入的闭包：主模板由调用方报 ParseError，
        // 子模板沿用「无法静态并入、留待渲染期报 TemplateNotFound」的既有语义
        let Ok(analysis) = entry.analysis() else {
            return;
        };
        for ref_name in &analysis.refs {
            if !visited.insert(ref_name.clone()) {
                continue; // 防环：a → b → a
            }
            let Some(sub) = self.entries.get(ref_name) else {
                continue; // 引用未注册模板：渲染期报 TemplateNotFound，此处无法静态并入
            };
            if let Ok(sub_analysis) = sub.analysis() {
                for v in &sub_analysis.variables {
                    merge_var(vars, v.clone());
                }
            }
            // 先并入**本层**被引用模板的规格，再递归进它引用的模板。
            //
            // 合并策略是「先到先得」（见下方 `any` 判断），所以先入表者优先。
            // 顺序写反 —— 先递归再入表 —— 会让**离主模板最远**的那份声明胜出：
            // `main → child → grandchild` 且 child 与 grandchild 对同名参数声明了
            // 不同 `options` 时，采用 grandchild 的，与文档承诺的「更接近主模板的
            // 声明优先」相反。不报错，只是静默按另一套约束校验。
            for spec in &sub.params {
                if !specs.iter().any(|s| s.name == spec.name) {
                    specs.push(spec.clone());
                }
            }
            self.collect_include_closure(sub, vars, specs, visited);
        }
    }

    /// 系统注入变量名列表（默认 `["machine"]`；渲染时由管线注入上下文）。
    pub fn system_vars(&self) -> &[String] {
        &self.system_vars
    }

    /// 设置系统注入变量名列表（默认 `["machine"]`）。
    ///
    /// 渲染时由管线注入上下文的变量应列在这里，避免校验时误报"必选参数缺失"。
    pub fn set_system_vars(&mut self, vars: Vec<String>) {
        self.system_vars = vars;
    }

    /// 渲染模板：应用规格默认值兜底后，仅以参数集作为上下文渲染。
    ///
    /// 与 [`Self::validate`] 的口径一致（规格默认值视为已提供）。
    /// **不注入系统变量**：引用 `{{ machine.xxx }}` 的模板（如内置
    /// program_header）请用 [`Self::render_with_machine`]，或直接走
    /// [`crate::pipeline::GCodeGenerator::generate`] 管线。
    ///
    /// 完整校验（必选 / 类型 / 白名单 / 区间）仍需走 [`Self::validate`] 或管线
    /// [`crate::pipeline::GCodeGenerator::generate`]；但**有限性不必调用方操心**——
    /// 渲染入口统一拦一道 NaN/Inf（见 `ensure_finite_context`），
    /// 不会把 `"NaN"` / `"inf"` 静默写进输出。
    pub fn render(
        &self,
        name: &str,
        params: &crate::model::ParameterSet,
    ) -> Result<String, nctool_tpl::TplError> {
        let context = match self.entries.get(name) {
            Some(entry) => {
                let effective = crate::model::apply_spec_defaults(&entry.params, params);
                effective.to_minijinja_value()
            }
            // 不存在的模板：走 render_template 触发统一的 TemplateNotFound
            None => params.to_minijinja_value(),
        };
        self.render_template(name, &context)
    }

    /// 渲染模板（注入机床系统变量 + 规格默认值兜底）。
    ///
    /// 上下文口径与管线 [`crate::pipeline::GCodeGenerator::generate`] 一致：
    /// 参数裸值 + `machine` 对象（config 键值 + `id`/`vendor`/`model` 元信息）。
    /// 同样带有限性闸门：上下文中出现 NaN/Inf 直接报错，不进入渲染。
    pub fn render_with_machine(
        &self,
        name: &str,
        params: &crate::model::ParameterSet,
        machine: &crate::model::MachineConfig,
    ) -> Result<String, nctool_tpl::TplError> {
        let context = match self.entries.get(name) {
            Some(entry) => {
                let effective = crate::model::apply_spec_defaults(&entry.params, params);
                crate::model::build_render_context(&effective, machine)
            }
            None => crate::model::build_render_context(params, machine),
        };
        self.render_template(name, &context)
    }

    /// 渲染模板（使用自定义上下文，可用于注入 `machine` 等系统变量）。
    ///
    /// 渲染前跑 `ensure_finite_context`：上下文里的 NaN/Inf 一律拒绝渲染。
    /// 自定义上下文绕过了 [`Self::validate`]，这道闸门是它唯一的数值防线。
    pub fn render_template(
        &self,
        name: &str,
        context: &Value,
    ) -> Result<String, nctool_tpl::TplError> {
        ensure_finite_context(name, context)?;
        self.renderer.render_template(name, context)
    }

    /// 宽松模式渲染（自定义上下文）：未定义变量（裸引用）渲染为空字符串。
    ///
    /// 宽松渲染器**惰性构建并缓存**（注册新模板时失效）：它需要独立的
    /// `Environment`，早期实现每次调用都重建渲染器并把全部模板重新编译一遍。
    /// 构建失败（某模板编译不过）的原因一并缓存，返回语义与逐次重建一致。
    ///
    /// 注意：经**过滤器**引用的未定义变量仍会报错——过滤器需要
    /// 具体值求值，无法以空字符串替代。
    ///
    /// 与 `render_template` 一样带有限性闸门：**宽松只放宽"未定义变量"，
    /// 不放宽"非法数值"**——参数可以缺省，但 NaN/Inf 会让机床走到错误位置。
    pub fn render_template_lenient(
        &self,
        name: &str,
        context: &Value,
    ) -> Result<String, nctool_tpl::TplError> {
        ensure_finite_context(name, context)?;
        let cached = self.lenient_cache.get_or_init(|| {
            let mut renderer = nctool_tpl::Renderer::new().with_lenient();
            for entry in self.entries.values() {
                renderer.add_template(&entry.name, &entry.source_text)?;
            }
            Ok(renderer)
        });
        match cached {
            Ok(renderer) => renderer.render_template(name, context),
            Err(err) => Err(err.clone()),
        }
    }

    /// 访问底层渲染器（高级用法：配置过滤器等）。
    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// 安装内置模板库。
    ///
    /// 单个内置模板注册失败**不再 panic**（P1-4 降级方案，不改 `new()`/`Default`
    /// 签名）：跳过该模板、`eprintln!` 告警、记入 [`Self::builtin_warnings`]。
    /// `add_entry` 是「先校验后入库」（编译过才 `entries.insert`），失败不会留下
    /// 半入库状态。
    fn install_builtins(&mut self) {
        for (name, category, description, source, params) in builtin_templates() {
            let entry = TemplateEntry::new(
                name,
                category,
                description,
                TemplateSource::Builtin,
                params,
                source,
            );
            if let Err(e) = self.add_entry(entry) {
                self.note_builtin_failure(name, &e);
            }
        }
    }

    /// 记录一次内置模板安装失败（降级方案的可见性出口）。
    ///
    /// - **stderr 每进程每个模板只打一次**：HTTP 通道按请求构建注册表
    ///   （`Ctx::build_registry` 失效即重建），无去重会把同一告警刷屏；
    ///   去重用**进程级**静态集合——「至少有一次可见输出」不依赖调用方接不接
    ///   [`Self::builtin_warnings`]（SUMMARY §5 约束 C ①）。
    /// - 结构化留痕进 `self.builtin_warnings`，由 `/health`、`doctor` 等出口暴露。
    fn note_builtin_failure(&mut self, name: &str, err: &RegistryError) {
        static PRINTED: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
        let message = err.to_string();
        let first_time = PRINTED
            .get_or_init(|| Mutex::new(BTreeSet::new()))
            .lock()
            .map(|mut s| s.insert(name.to_string()))
            .unwrap_or(true);
        if first_time {
            eprintln!("nctool: 内置模板 {name} 注册失败，已跳过：{message}");
        }
        self.builtin_warnings.push(BuiltinWarning {
            name: name.to_string(),
            message,
        });
    }

    /// 内置模板安装失败列表；正常情况下为空。
    ///
    /// **非空即意味着有内置模板缺失**，必须经 stderr / `/health`
    /// （`builtinWarnings`）暴露，不得静默——某个内置模板悄悄不存在比 panic
    /// 更难察觉（「静默产出错误程序」红线）。
    pub fn builtin_warnings(&self) -> &[BuiltinWarning] {
        &self.builtin_warnings
    }
}

impl Default for TemplateRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 渲染前的**有限性闸门**：上下文里出现 NaN / Inf 一律拒绝渲染。
///
/// 背景：`validate` 与管线 `generate*` 有 `IssueKind::NonFinite` 检查，但直接
/// 调用本模块的渲染入口（[`TemplateRegistry::render`]、
/// [`TemplateRegistry::render_template`] 等）不走校验层——NaN 会以文本
/// `"NaN"` 静默写进 G-code，机床走到非法坐标。这正是本项目零容忍的那类错误：
/// 与其"渲染出来再说"，不如在进渲染器前就失败。
///
/// 闸门放在 `render_template*` 这一层而非各个参数入口，是刻意的：它同时覆盖
/// 参数集、机床系统变量与调用方自定义上下文，不必为每种上下文各写一遍。
fn ensure_finite_context(name: &str, context: &Value) -> Result<(), nctool_tpl::TplError> {
    match find_non_finite(context, "", 0) {
        Ok(None) => Ok(()),
        Ok(Some(path)) => Err(nctool_tpl::TplError::Render {
            name: name.to_string(),
            message: format!(
                "上下文参数 {path} 为 NaN/Inf（非有限数），拒绝渲染：非有限数会写出非法坐标；\
                 本入口不做完整校验，但有限性必须拦截"
            ),
        }),
        // 扫描本身失败时**fail-closed**：宁可拒绝渲染，也不能把"没扫成"当成
        // "没找到非有限数" —— 后者等于闸门在异常路径上静默失效。
        Err(where_) => Err(nctool_tpl::TplError::Render {
            name: name.to_string(),
            message: format!(
                "上下文参数 {where_} 无法完成有限性扫描，拒绝渲染：\
                 扫描失败时无法保证不存在 NaN/Inf，而非法坐标的代价不可接受"
            ),
        }),
    }
}

/// 深度优先找出上下文里第一个非有限数，返回它的路径（如 `machine.x`、`passes[2].z`）。
///
/// `depth` 是兜底：自定义对象理论上可无限嵌套，超过上限就停止下探——
/// 宁可放过极端情况，也不能让扫描本身把渲染挂死。
///
/// 返回值：`Ok(None)` 未发现非有限数；`Ok(Some(path))` 发现，`path` 为定位；
/// `Err(where_)` **扫描失败**（遍历器不可用），调用方须按"拒绝渲染"处理。
/// 区分后两者是刻意的：`ok()?` 会把遍历失败折叠成"没找到"，让闸门在异常路径上
/// 静默放行 —— 一个防非法坐标的闸门不该有这种失败模式。
fn find_non_finite(value: &Value, path: &str, depth: usize) -> Result<Option<String>, String> {
    const MAX_DEPTH: usize = 32;
    if depth > MAX_DEPTH {
        return Ok(None);
    }
    let label = || {
        if path.is_empty() {
            "<根值>".to_string()
        } else {
            path.to_string()
        }
    };
    match value.kind() {
        // minijinja 的 `Value` 没有 `as_f64`，数值取值走 `TryFrom<Value> for f64`
        // （它会把 I64/U64/F64 统一转成 f64，整数必然有限，不会误报）
        ValueKind::Number => Ok(match f64::try_from(value.clone()) {
            Ok(n) if !n.is_finite() => Some(label()),
            _ => None,
        }),
        ValueKind::Seq => {
            let items = value
                .try_iter()
                .map_err(|_| format!("{}（序列遍历失败）", label()))?;
            for (i, item) in items.enumerate() {
                let child = format!("{path}[{i}]");
                if let Some(found) = find_non_finite(&item, &child, depth + 1)? {
                    return Ok(Some(found));
                }
            }
            Ok(None)
        }
        ValueKind::Map => {
            let keys = value
                .try_iter()
                .map_err(|_| format!("{}（映射遍历失败）", label()))?;
            for key in keys {
                // 取不到值的键（如自定义对象的特殊成员）跳过，不因此拒绝渲染
                let Ok(item) = value.get_item(&key) else {
                    continue;
                };
                let child = if path.is_empty() {
                    key.to_string()
                } else {
                    format!("{path}.{key}")
                };
                if let Some(found) = find_non_finite(&item, &child, depth + 1)? {
                    return Ok(Some(found));
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// 同名变量合并（include 闭包用）：必选性取"或"（任一处非兜底引用即必选），
/// 位置与定位信息取首次出现。
fn merge_var(vars: &mut Vec<nctool_tpl::Variable>, v: nctool_tpl::Variable) {
    if let Some(existing) = vars.iter_mut().find(|x| x.name == v.name) {
        existing.optional = existing.optional && v.optional;
    } else {
        vars.push(v);
    }
}

/// 内置模板定义：`(名称, 分类, 描述, 源码, 参数规格)`。
///
/// 内置模板是 G-code 开发的基础子程序，可被用户模板 `{% include %}` 复用。
fn builtin_templates() -> Vec<(
    &'static str,
    TemplateCategory,
    &'static str,
    &'static str,
    Vec<ParamSpec>,
)> {
    use crate::model::{ParamKind, ParamValue};

    vec![
        (
            "program_header",
            TemplateCategory::General,
            "程序头：纸带起始符 + 程序号 + 注释头 + 单位/坐标系/取消态初始化",
            concat!(
                "%\n",
                "{{ machine.program_prefix | default('O') }}{{ prog | nc_pad(machine.program_digits | default(4) | int) }}\n",
                "( {{ part_name | default('') }} )\n",
                "( {{ op_name | default('') }} )\n",
                // 单位与 G 码联动：imperial 出 G20，其余出 G21。
                // 用 `machine.units | default(...)` 裸变量形式，保证该变量仍判为可选，
                // 不会因模板引用而把 `machine` 变成必选参数。
                "{{ 'G20' if machine.units | default('metric') == 'imperial' else 'G21' }} ({{ machine.units | default('metric') }})\n",
                "G90 G17 (绝对坐标 / XY 平面)\n",
                "G40 G49 G80 (取消刀补 / 刀长补偿 / 固定循环)\n",
                "{{ machine.coordinate_system }}\n",
                "{{ machine.feed_mode }}\n",
                "M5\nM9\n",
            ),
            vec![
                crate::validate::spec(
                    "prog",
                    ParamKind::Integer,
                    true,
                    None,
                    "程序号（前导零填充 4 位）",
                )
                .with_range(1.0, 9999.0)
                .with_unit("号"),
                crate::validate::spec(
                    "part_name",
                    ParamKind::String,
                    false,
                    Some(ParamValue::String("".into())),
                    "零件名称（注释）",
                ),
                crate::validate::spec(
                    "op_name",
                    ParamKind::String,
                    false,
                    Some(ParamValue::String("".into())),
                    "工序名称（注释）",
                ),
            ],
        ),
        (
            "program_footer",
            TemplateCategory::General,
            "程序尾：主轴/冷却关闭 + 取消循环 + 程序结束 + 纸带结束符",
            concat!(
                "M5\nM9\n",
                "G80 (取消固定循环)\n",
                "{{ machine.program_end }}\n",
                "%\n",
            ),
            vec![],
        ),
        (
            "tool_change",
            TemplateCategory::General,
            "换刀：主轴停止 + 换刀 + 刀长补偿 + 启动主轴与冷却",
            concat!(
                "M5\nM9\n",
                "{{ machine.tool_change }} T{{ tool_num | nc_strip }}\n",
                "G40 (取消刀补)\n",
                "G43 H{{ tool_num | nc_strip }} (刀长补偿)\n",
                // 主轴启动：此前内置模板从不发射 M3，导致钻孔循环在主轴停止状态下执行。
                // spindle_speed 设为必选——主轴转速不可静默取默认值。
                "{{ machine.spindle_on }} S{{ spindle_speed | nc_strip }} (主轴正转)\n",
                "{{ machine.coolant_on }} (冷却开)\n",
            ),
            vec![
                crate::validate::spec(
                    "tool_num",
                    ParamKind::Integer,
                    true,
                    None,
                    "刀具号",
                )
                .with_range(1.0, 999.0)
                .with_unit("号"),
                crate::validate::spec(
                    "spindle_speed",
                    ParamKind::Integer,
                    true,
                    None,
                    "主轴转速（S 值，正整数）",
                )
                // 静态 6000 是**无机床上下文时的兜底**；有机床上下文时以
                // `machine.max_spindle_rpm` 为准（WFL=3500 / INDEX=5000，
                // 见 `core::machine` 预设）。两者缺一不可：只留静态值 =
                // 按机床校验的承诺失灵（P0-2）；只留动态值 = 无机床上下文
                // 的调用方（旧签名/预设检查）失去上界。
                .with_range(1.0, 6000.0)
                .with_max_from("machine.max_spindle_rpm")
                .with_unit("r/min"),
            ],
        ),
        (
            "safe_move",
            TemplateCategory::General,
            "安全移动：抬刀到安全高度 + 定位",
            concat!(
                // 统一用 nc_fixed(3) 格式化，避免此处输出 Z100.0 而钻孔循环输出 Z5.000
                "{{ machine.rapid }} G90 Z{{ safe_z | default(100) | nc_fixed(3) }}\n",
                "{{ machine.rapid }} X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }}\n",
            ),
            vec![
                crate::validate::spec(
                    "x",
                    ParamKind::Number,
                    true,
                    None,
                    "目标 X 坐标",
                ),
                crate::validate::spec(
                    "y",
                    ParamKind::Number,
                    true,
                    None,
                    "目标 Y 坐标",
                ),
                crate::validate::spec(
                    "safe_z",
                    ParamKind::Number,
                    false,
                    Some(ParamValue::Number(100.0)),
                    "安全高度 Z",
                )
                .with_min(0.0)
                .with_unit("mm"),
            ],
        ),
        (
            "drill_cycle",
            TemplateCategory::Drilling,
            "钻孔循环：G81 标准钻孔",
            concat!(
                "{{ machine.rapid }} X{{ x | nc_fixed(3) }} Y{{ y | nc_fixed(3) }}\n",
                // 固定循环块不带 G1：G1 与 G81 同属 01 组模态，前缀 G1 冗余，
                // 且部分控制器对同组重复 G 码敏感，可移植性差。
                "G98 G81 R{{ r_plane | nc_fixed(3) }} Z{{ depth | nc_fixed(3) }} F{{ feed | nc_fixed(3) }}\n",
                "G80 (取消循环)\n",
            ),
            vec![
                crate::validate::spec("x", ParamKind::Number, true, None, "孔 X 坐标")
                    .with_unit("mm"),
                crate::validate::spec("y", ParamKind::Number, true, None, "孔 Y 坐标")
                    .with_unit("mm"),
                crate::validate::spec(
                    "r_plane",
                    ParamKind::Number,
                    false,
                    Some(ParamValue::Number(5.0)),
                    "R 平面（安全高度，应高于工件表面）",
                )
                .with_min(0.0)
                .with_unit("mm"),
                crate::validate::spec("depth", ParamKind::Number, true, None, "钻孔深度")
                    .with_max(0.0)
                    .with_unit("mm"),
                crate::validate::spec("feed", ParamKind::Number, true, None, "进给速度")
                    .with_min(0.001)
                    .with_unit("mm/min"),
            ],
        ),
        (
            "facing",
            TemplateCategory::Milling,
            "面铣：矩形区域往复行切（zigzag）",
            concat!(
                "{{ machine.rapid }} G90 Z{{ safe_z | default(100) | nc_fixed(3) }}\n",
                "{{ machine.rapid }} X{{ x0 | nc_fixed(3) }} Y{{ y0 | nc_fixed(3) }}\n",
                // 下刀到切削深度：下刀进给缺省取切削进给（模态继承）
                "{{ machine.linear }} G90 Z{{ depth | nc_fixed(3) }} F{{ plunge_feed | default(feed) | nc_fixed(3) }}\n",
                // 行数 = ceil(width / stepover)（minijinja 的 round 不支持 method 参数，
                // 用 int(x + 1 - 1e-6) 浮点技巧实现向上取整）；逐行往复：首行就位后直接
                // X 切削，后续每行先 Y 步进（X 保持），再反向 X 切削（方向奇偶交替）。
                // 空白控制使循环体恰好每行输出一个 G-code 块、无多余空行。
                "{% for i in range(0, ((width / stepover + 0.999999) | int)) -%}\n",
                "{% if i > 0 %}{{ machine.linear }} Y{{ (y0 + i * stepover) | nc_fixed(3) }}\n",
                "{% endif %}{{ machine.linear }} X{{ ((x0 + length) if (i % 2 == 0) else x0) | nc_fixed(3) }}{% if i == 0 %} F{{ feed | nc_fixed(3) }}{% endif %}\n",
                "{% endfor -%}\n",
                "{{ machine.rapid }} Z{{ safe_z | default(100) | nc_fixed(3) }}\n",
            ),
            vec![
                crate::validate::spec("x0", ParamKind::Number, true, None, "面铣起点 X 坐标")
                    .with_unit("mm"),
                crate::validate::spec("y0", ParamKind::Number, true, None, "面铣起点 Y 坐标")
                    .with_unit("mm"),
                crate::validate::spec("length", ParamKind::Number, true, None, "单行铣削长度（X 方向）")
                    .with_range(0.001, 500.0)
                    .with_unit("mm"),
                crate::validate::spec("width", ParamKind::Number, true, None, "铣削宽度（Y 方向）")
                    .with_range(0.001, 500.0)
                    .with_unit("mm"),
                crate::validate::spec("depth", ParamKind::Number, true, None, "切削深度（Z，负值向下）")
                    .with_max(0.0)
                    .with_unit("mm"),
                crate::validate::spec("feed", ParamKind::Number, true, None, "切削进给（XY）")
                    .with_min(0.001)
                    .with_unit("mm/min"),
                crate::validate::spec(
                    "stepover",
                    ParamKind::Number,
                    false,
                    Some(ParamValue::Number(10.0)),
                    "行距（Y 方向每行间距）",
                )
                .with_min(0.5)
                .with_unit("mm"),
                crate::validate::spec(
                    "safe_z",
                    ParamKind::Number,
                    false,
                    Some(ParamValue::Number(100.0)),
                    "安全高度 Z",
                )
                .with_min(0.0)
                .with_unit("mm"),
                crate::validate::spec(
                    "plunge_feed",
                    ParamKind::Number,
                    false,
                    None,
                    "下刀进给（Z，缺省取切削进给）",
                )
                .with_min(0.001)
                .with_unit("mm/min"),
            ],
        ),
        (
            "slot_milling",
            TemplateCategory::Milling,
            "键槽铣：X 方向直槽一刀成型（下刀→切削→抬刀→返回）",
            concat!(
                "{{ machine.rapid }} G90 Z{{ safe_z | default(100) | nc_fixed(3) }}\n",
                "{{ machine.rapid }} X{{ x0 | nc_fixed(3) }} Y{{ y0 | nc_fixed(3) }}\n",
                "{{ machine.linear }} G90 Z{{ depth | nc_fixed(3) }} F{{ plunge_feed | default(feed) | nc_fixed(3) }}\n",
                "{{ machine.linear }} X{{ (x0 + length) | nc_fixed(3) }} F{{ feed | nc_fixed(3) }}\n",
                "{{ machine.rapid }} Z{{ safe_z | default(100) | nc_fixed(3) }}\n",
                "{{ machine.rapid }} X{{ x0 | nc_fixed(3) }} Y{{ y0 | nc_fixed(3) }}\n",
            ),
            vec![
                crate::validate::spec("x0", ParamKind::Number, true, None, "槽起点 X 坐标")
                    .with_unit("mm"),
                crate::validate::spec("y0", ParamKind::Number, true, None, "槽起点 Y 坐标")
                    .with_unit("mm"),
                crate::validate::spec("length", ParamKind::Number, true, None, "槽长（X 方向）")
                    .with_range(0.001, 500.0)
                    .with_unit("mm"),
                crate::validate::spec("depth", ParamKind::Number, true, None, "槽深（Z，负值向下）")
                    .with_max(0.0)
                    .with_unit("mm"),
                crate::validate::spec("feed", ParamKind::Number, true, None, "切削进给（X）")
                    .with_min(0.001)
                    .with_unit("mm/min"),
                crate::validate::spec(
                    "safe_z",
                    ParamKind::Number,
                    false,
                    Some(ParamValue::Number(100.0)),
                    "安全高度 Z",
                )
                .with_min(0.0)
                .with_unit("mm"),
                crate::validate::spec(
                    "plunge_feed",
                    ParamKind::Number,
                    false,
                    None,
                    "下刀进给（Z，缺省取切削进给）",
                )
                .with_min(0.001)
                .with_unit("mm/min"),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ParameterSet;

    /// 回归（第四轮 P1-11）：`RegistryError::Io` 只装 `io::Error`，不带路径时
    /// 用户看到的是「文件读取失败: 系统找不到指定的文件」—— 完全不知道是哪个文件。
    /// 变体形状不能改（crate 已发布），故在构造处把路径并进 io::Error 的消息。
    #[test]
    fn add_file_io_error_includes_the_path() {
        let mut r = TemplateRegistry::new();
        let missing = Path::new("/no/such/dir/definitely_missing.j2");
        let err = r
            .add_file("x", TemplateCategory::General, "", missing, vec![])
            .expect_err("不存在的文件应报错");
        assert!(matches!(err, RegistryError::Io(_)), "应是 Io 变体: {err:?}");
        let msg = err.to_string();
        assert!(
            msg.contains("definitely_missing.j2"),
            "错误消息必须带上路径，否则用户无从定位: {msg}"
        );
    }

    #[test]
    fn analysis_is_computed_once_and_matches_direct_extraction() {
        // 缓存不能改变结论：与直接 parse + extract 的结果逐项一致
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "an_probe",
            TemplateCategory::General,
            "分析缓存探针",
            "{% if b is defined %}A{% endif %}G0 X{{ x | default(1) }}\n",
            vec![],
        )
        .unwrap();
        let entry = r.get("an_probe").unwrap();

        let cached = entry.analysis().expect("解析应成功");
        let ast = nctool_tpl::parse(&entry.source_text, &entry.name).unwrap();
        let direct = nctool_tpl::extract_undeclared(&ast);
        assert_eq!(cached.variables, direct, "缓存结果必须与直接提取一致");

        // 二次取用返回同一份（指针相同 = 未重新计算）
        let again = entry.analysis().expect("二次解析应成功");
        assert!(std::ptr::eq(cached, again), "重复取用必须命中缓存");

        // `x` 有 default 兜底 → 可选；`b` 处于 is defined → 可选
        assert!(cached.variables.iter().all(|v| v.optional));
    }

    #[test]
    fn replacing_source_invalidates_cached_analysis() {
        let entry = TemplateEntry::new(
            "cache_update",
            TemplateCategory::General,
            "",
            TemplateSource::Memory,
            vec![],
            "G0 X{{ old_value }}",
        );
        let before = entry.analysis().unwrap();
        assert!(before.variables.iter().any(|v| v.name == "old_value"));

        let updated = entry.with_source_text("G0 Y{{ new_value }}");
        let after = updated.analysis().unwrap();
        assert!(after.variables.iter().any(|v| v.name == "new_value"));
        assert!(!after.variables.iter().any(|v| v.name == "old_value"));
    }

    #[test]
    fn lenient_renderer_cache_sees_templates_registered_later() {
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "lz_first",
            TemplateCategory::General,
            "宽松缓存探针一",
            "A{{ x }}\n",
            vec![],
        )
        .unwrap();

        // 触发宽松渲染器惰性构建并缓存
        let out = r
            .render_template_lenient("lz_first", &Value::UNDEFINED)
            .expect("宽松渲染应成功");
        assert_eq!(out.trim(), "A", "宽松模式下未定义变量渲染为空");

        // 之后注册的模板必须能被宽松渲染看到——缓存若未随注册失效，
        // 这里会报 TemplateNotFound
        r.add_memory(
            "lz_second",
            TemplateCategory::General,
            "宽松缓存探针二",
            "B{{ y }}\n",
            vec![],
        )
        .unwrap();
        let out2 = r
            .render_template_lenient("lz_second", &Value::UNDEFINED)
            .expect("新注册模板应可见");
        assert_eq!(out2.trim(), "B");
    }

    #[test]
    fn registry_installs_builtins() {
        let r = TemplateRegistry::new();
        assert!(r.len() >= 7);
        for name in [
            "program_header",
            "program_footer",
            "tool_change",
            "safe_move",
            "drill_cycle",
        ] {
            assert!(r.get(name).is_some(), "缺少内置模板 {name}");
        }
    }

    #[test]
    fn add_memory_template() {
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "my_op",
            TemplateCategory::Milling,
            "测试工序",
            "X{{ x }}",
            vec![],
        )
        .unwrap();
        assert!(r.get("my_op").is_some());
        assert_eq!(r.list(None).len(), r.len());
    }

    #[test]
    fn duplicate_name_rejected() {
        let mut r = TemplateRegistry::new();
        r.add_memory("dup", TemplateCategory::General, "", "X", vec![])
            .unwrap();
        let err = r
            .add_memory("dup", TemplateCategory::General, "", "Y", vec![])
            .unwrap_err();
        assert!(matches!(err, RegistryError::Duplicate(_)));
    }

    #[test]
    fn empty_source_rejected() {
        let mut r = TemplateRegistry::new();
        let err = r
            .add_memory("empty", TemplateCategory::General, "", "   ", vec![])
            .unwrap_err();
        assert!(matches!(err, RegistryError::EmptySource(_)));
    }

    #[test]
    fn list_by_category() {
        let mut r = TemplateRegistry::new();
        r.add_memory("mill1", TemplateCategory::Milling, "", "X{{ x }}", vec![])
            .unwrap();
        let milling = r.list(Some(TemplateCategory::Milling));
        assert!(milling.iter().any(|e| e.name == "mill1"));
        let general = r.list(Some(TemplateCategory::General));
        assert!(general.iter().any(|e| e.name == "program_header"));
        assert!(!general.iter().any(|e| e.name == "mill1"));
    }

    #[test]
    fn validate_missing_required() {
        let r = TemplateRegistry::new();
        let ps = ParameterSet::new(); // drill_cycle 缺 x/y/depth/feed
        let report = r.validate("drill_cycle", &ps).unwrap();
        assert!(report.has_errors());
        assert!(report.errors().any(|e| e.param.as_deref() == Some("x")));
    }

    #[test]
    fn validate_unknown_template_is_not_found() {
        // 校验不存在的模板 → RegistryError::NotFound（不再是 String 错误）
        let r = TemplateRegistry::new();
        let err = r.validate("no_such", &ParameterSet::new()).unwrap_err();
        assert!(matches!(err, RegistryError::NotFound(_)));
        assert!(err.to_string().contains("no_such"));
    }

    #[test]
    fn validate_accepts_system_vars() {
        // machine 是系统注入变量，校验时不应误报缺失参数
        let r = TemplateRegistry::new();
        let mut ps = ParameterSet::new();
        ps.set_number("prog", 1.0); // program_header 引用了 machine.xxx
        let report = r.validate("program_header", &ps).unwrap();
        assert!(
            report.is_ok(),
            "machine 不应被当作缺失参数: {}",
            report.summary()
        );
    }

    /// P0-2/Q-01（SUMMARY §8 行1）：`tool_change` 的 `spindle_speed` 上界必须
    /// 随机床 `max_spindle_rpm` 联动——同一取值在不同机床上结论不同。
    /// 修复前是静态 `.with_range(1.0, 6000.0)`：`wfl_m65 + 3501`（上限 3500）
    /// 会**通过**，本测试必须红。
    #[test]
    fn spindle_speed_bound_follows_machine_max_spindle_rpm() {
        let r = TemplateRegistry::new();
        // (机床 id, 转速 r/min, 应通过)
        let cases = [
            ("wfl_m65", 3499, true),
            ("wfl_m65", 3500, true),
            ("wfl_m65", 3501, false),
            ("wfl_m65", 5000, false),
            ("index_ms40", 5000, true),
            ("index_ms40", 5001, false),
            ("generic", 6000, true),
            ("generic", 6001, false),
        ];
        for (id, rpm, should_pass) in cases {
            let machine = crate::machine::MachinePreset::from_id(id)
                .unwrap_or_else(|| panic!("未知内置机床 {id}"))
                .config();
            let bound = machine
                .get("max_spindle_rpm")
                .expect("内置机床必有 max_spindle_rpm")
                .to_string();
            let mut ps = ParameterSet::new();
            ps.set_integer("tool_num", 1)
                .set_integer("spindle_speed", rpm);
            let report = r
                .validate_with_machine("tool_change", &ps, Some(&machine))
                .unwrap();
            assert_eq!(
                report.has_errors(),
                !should_pass,
                "{id} + S{rpm} 判定错误（应{}）：{}",
                if should_pass { "通过" } else { "拒绝" },
                report.summary()
            );
            if !should_pass {
                assert!(
                    report.errors().any(|e| {
                        e.kind == crate::validate::IssueKind::OutOfRange
                            && e.message.contains(&bound)
                            && e.message.contains(&format!("机床 {id}"))
                    }),
                    "{id} + S{rpm} 应报 OutOfRange 且标注来自该机床的 {bound}：{}",
                    report.summary()
                );
            }
        }
    }

    /// P0-2/Q-12 配套：机床上限**非法或缺失**时回退静态上界（6000）并发警告，
    /// 绝不以 0/负数为上界（全量误杀 S 值），也绝不静默回退（红线）。
    #[test]
    fn invalid_machine_max_spindle_rpm_falls_back_with_warning() {
        let r = TemplateRegistry::new();
        let base = crate::machine::MachinePreset::from_id("wfl_m65")
            .expect("wfl_m65 是内置预设")
            .config();

        let mut m_zero = base.clone();
        m_zero.config.insert("max_spindle_rpm".into(), "0".into());
        let mut m_neg = base.clone();
        m_neg.config.insert("max_spindle_rpm".into(), "-100".into());
        let mut m_txt = base.clone();
        m_txt.config.insert("max_spindle_rpm".into(), "abc".into());
        let mut m_missing = base.clone();
        m_missing.config.remove("max_spindle_rpm");

        let mut ps = ParameterSet::new();
        ps.set_integer("tool_num", 1)
            .set_integer("spindle_speed", 5000);
        let mut ps_over = ParameterSet::new();
        ps_over
            .set_integer("tool_num", 1)
            .set_integer("spindle_speed", 6001);

        for (label, broken) in [
            ("0", &m_zero),
            ("-100", &m_neg),
            ("abc", &m_txt),
            ("缺失", &m_missing),
        ] {
            let report = r
                .validate_with_machine("tool_change", &ps, Some(broken))
                .unwrap();
            assert!(
                !report.has_errors(),
                "机床上限 {label} 时 5000 必须按静态上界 6000 放行（禁止全量误杀）：{}",
                report.summary()
            );
            assert!(
                report
                    .warnings()
                    .any(|w| w.message.contains("max_spindle_rpm")
                        && w.message.contains("回退")
                        && w.message.contains(&broken.id)),
                "机床上限 {label} 必须产生回退警告（含机床 id）：{}",
                report.summary()
            );
            // 静态兜底本身仍然生效：6001 > 6000 照样拒
            let over = r
                .validate_with_machine("tool_change", &ps_over, Some(broken))
                .unwrap();
            assert!(
                over.has_errors(),
                "机床上限 {label} 时静态上界 6000 仍应拦截 6001：{}",
                over.summary()
            );
        }

        // 合法机床上限：动态值生效（wfl 3500），无回退警告、无任何问题
        let mut ps_ok = ParameterSet::new();
        ps_ok
            .set_integer("tool_num", 1)
            .set_integer("spindle_speed", 3499);
        let ok = r
            .validate_with_machine("tool_change", &ps_ok, Some(&base))
            .unwrap();
        assert!(ok.is_ok(), "wfl_m65 + S3499 应通过：{}", ok.summary());
        assert!(
            !ok.warnings().any(|w| w.message.contains("回退")),
            "机床上限合法时不应有回退警告：{}",
            ok.summary()
        );
    }

    #[test]
    fn validate_rejects_value_outside_spec_options() {
        // 端到端：规格声明候选项白名单后，registry.validate 必须拦住非法工艺选项，
        // 而不是让它一路渲染成与图纸不符的 G-code。
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "groove_form",
            TemplateCategory::Grooving,
            "越程槽形式",
            "U_FX={{ u_fx }}\n",
            vec![
                ParamSpec::new("u_fx", crate::model::ParamKind::Choice, "越程槽形式").with_options(
                    [
                        crate::model::ParamValue::String("闭口".into()),
                        crate::model::ParamValue::String("左开口".into()),
                        crate::model::ParamValue::String("右开口".into()),
                    ],
                ),
            ],
        )
        .unwrap();

        let mut ok = ParameterSet::new();
        ok.set_string("u_fx", "左开口");
        let report = r.validate("groove_form", &ok).unwrap();
        assert!(report.is_ok(), "合法候选应通过: {}", report.summary());

        let mut bad = ParameterSet::new();
        bad.set_string("u_fx", "上开口");
        let report = r.validate("groove_form", &bad).unwrap();
        assert!(report.has_errors(), "{}", report.summary());
        assert!(report.has_kind(crate::validate::IssueKind::NotInOptions));
        // 报错必须列出候选值，否则用户无从修正
        assert!(report.summary().contains("闭口"), "{}", report.summary());
    }

    #[test]
    fn render_memory_template_with_params() {
        // registry.render 用纯参数上下文渲染（模板不含系统变量时可用）
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "simple",
            TemplateCategory::General,
            "",
            "X{{ x | nc_fixed(3) }}",
            vec![],
        )
        .unwrap();
        let mut ps = ParameterSet::new();
        ps.set_number("x", 21.0);
        let out = r.render("simple", &ps).unwrap();
        assert_eq!(out.trim(), "X21.000");
    }

    #[test]
    fn render_rejects_non_finite_param() {
        // 渲染入口不走完整校验，但有限性必须拦：此前 NaN 会以文本 "NaN" 写进 G-code
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "simple",
            TemplateCategory::General,
            "",
            "X{{ x | nc_fixed(3) }}",
            vec![],
        )
        .unwrap();
        for (label, v) in [
            ("NaN", f64::NAN),
            ("Inf", f64::INFINITY),
            ("-Inf", f64::NEG_INFINITY),
        ] {
            let mut ps = ParameterSet::new();
            ps.set_number("x", v);
            let err = r.render("simple", &ps).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("非有限数"), "{label} 应被拒绝，实际: {msg}");
            assert!(msg.contains('x'), "{label} 的报错要指名参数: {msg}");
        }
    }

    #[test]
    fn render_rejects_non_finite_in_nested_context() {
        // 自定义上下文（嵌套列表）里的非有限数同样要拦，且报错要带路径
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "loop_tpl",
            TemplateCategory::General,
            "",
            "{% for p in passes %}X{{ p.z }} {% endfor %}",
            vec![],
        )
        .unwrap();
        let passes = vec![
            std::collections::BTreeMap::from([("z", 1.0f64)]),
            std::collections::BTreeMap::from([("z", f64::INFINITY)]),
        ];
        let ctx = Value::from_serialize(std::collections::BTreeMap::from([("passes", passes)]));
        let err = r.render_template("loop_tpl", &ctx).unwrap_err();
        assert!(
            err.to_string().contains("passes[1].z"),
            "应定位到具体元素，实际: {err}"
        );
    }

    #[test]
    fn lenient_render_rejects_non_finite_too() {
        // 宽松只放宽"未定义变量"，不放宽"非法数值"——参数可以缺省，NaN/Inf 不行
        let mut r = TemplateRegistry::new();
        r.add_memory("s", TemplateCategory::General, "", "X{{ x }}", vec![])
            .unwrap();
        let mut map = std::collections::BTreeMap::new();
        map.insert("x", f64::NAN);
        let ctx = Value::from_serialize(&map);
        let err = r.render_template_lenient("s", &ctx).unwrap_err();
        assert!(err.to_string().contains("非有限数"), "实际: {err}");
    }

    #[test]
    fn finite_context_renders_unchanged() {
        // 回归：闸门不能误伤合法上下文（整数 / 字符串 / 布尔 / 嵌套结构）
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "loop_tpl",
            TemplateCategory::General,
            "",
            "{% for p in passes %}{{ p.tag }}X{{ p.z }} {% endfor %}",
            vec![],
        )
        .unwrap();
        let passes = vec![
            std::collections::BTreeMap::from([
                ("z", Value::from_serialize(1i64)),
                ("tag", Value::from_serialize("A")),
            ]),
            std::collections::BTreeMap::from([
                ("z", Value::from_serialize(2i64)),
                ("tag", Value::from_serialize("B")),
            ]),
        ];
        let ctx = Value::from_serialize(std::collections::BTreeMap::from([("passes", passes)]));
        let out = r.render_template("loop_tpl", &ctx).unwrap();
        assert_eq!(out.trim(), "AX1 BX2");
    }

    #[test]
    fn render_template_with_machine_context() {
        // render_template 支持自定义上下文（注入 machine 系统变量）
        let r = TemplateRegistry::new();
        let machine = crate::machine::MachinePreset::Generic.config();
        let mut ctx_map = std::collections::BTreeMap::new();
        ctx_map.insert("machine", Value::from_serialize(&machine.config));
        ctx_map.insert("prog", Value::from_serialize(1.0));
        let ctx = Value::from_serialize(&ctx_map);
        let out = r.render_template("program_header", &ctx).unwrap();
        // 字节级 golden：坐标系/进给模式直接输出配置值（不重复 G 前缀）。
        // 裸渲染不经过管线后处理，minijinja 默认剥离模板尾换行（无结尾 \n）
        assert_eq!(
            out,
            "%\nO0001\n(  )\n(  )\nG21 (metric)\nG90 G17 (绝对坐标 / XY 平面)\nG40 G49 G80 (取消刀补 / 刀长补偿 / 固定循环)\nG54\nG94\nM5\nM9"
        );
    }

    #[test]
    fn render_applies_spec_defaults_like_validate() {
        // validate 与 render 口径一致：规格默认值在校验与渲染两端都生效
        // （修复前：validate 通过后 render 仍因 r_plane 未定义而失败）
        let r = TemplateRegistry::new();
        let mut ps = ParameterSet::new();
        ps.set_number("x", 21.0)
            .set_number("y", 15.0)
            .set_number("depth", -10.0)
            .set_number("feed", 100.0);
        let report = r.validate("drill_cycle", &ps).unwrap();
        assert!(
            report.is_ok(),
            "r_plane 规格默认应兜底: {}",
            report.summary()
        );
        let machine = crate::machine::MachinePreset::Generic.config();
        let out = r.render_with_machine("drill_cycle", &ps, &machine).unwrap();
        assert!(out.contains("R5.000"), "渲染应同样兜底: {out}");
    }

    #[test]
    fn validate_follows_include_required_params() {
        // 校验穿透 include：组合模板的必选参数不漏检（渲染前可发现错误）
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "my_program",
            TemplateCategory::General,
            "",
            "{% include \"program_header\" %}",
            vec![],
        )
        .unwrap();
        let report = r.validate("my_program", &ParameterSet::new()).unwrap();
        assert!(
            report.has_errors(),
            "子模板 program_header 的 prog 应报必选缺失"
        );
        assert!(
            report.errors().any(|e| e.param.as_deref() == Some("prog")),
            "{}",
            report.summary()
        );
        // 提供后通过；且 prog 被子模板引用——不产生"未引用"冗余警告
        let mut ps = ParameterSet::new();
        ps.set_number("prog", 1.0);
        let report = r.validate("my_program", &ps).unwrap();
        assert!(report.is_ok(), "{}", report.summary());
    }

    #[test]
    fn validate_include_cycle_is_safe() {
        // 环引用（a → b → a）不死循环、不 panic，变量照常检查
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "cyc_a",
            TemplateCategory::General,
            "",
            "{% include \"cyc_b\" %}{{ va }}",
            vec![],
        )
        .unwrap();
        r.add_memory(
            "cyc_b",
            TemplateCategory::General,
            "",
            "{% include \"cyc_a\" %}{{ vb }}",
            vec![],
        )
        .unwrap();
        let report = r.validate("cyc_a", &ParameterSet::new()).unwrap();
        assert!(report.has_errors());
        assert!(report.errors().any(|e| e.param.as_deref() == Some("va")));
        assert!(report.errors().any(|e| e.param.as_deref() == Some("vb")));
    }

    #[test]
    fn render_drill_cycle_via_pipeline() {
        // drill_cycle 引用 machine 系统变量，端到端渲染应通过 GCodeGenerator
        // （pipeline 负责注入 machine 上下文 + 规格默认值兜底）
        let g = crate::pipeline::GCodeGenerator::new();
        let mut ps = crate::model::ParameterSet::new();
        ps.set_number("x", 21.0)
            .set_number("y", 15.0)
            .set_number("depth", -10.0)
            .set_number("feed", 100.0);
        // 未提供 r_plane，应被规格默认值（5.0）兜底
        let machine = crate::machine::MachinePreset::Generic.config();
        let out = g
            .generate(
                "drill_cycle",
                &ps,
                &machine,
                &crate::pipeline::GenerationOptions::default(),
            )
            .unwrap();
        assert!(out.contains("G98 G81 R5.000 Z-10.000 F100.000"));
    }

    #[test]
    fn render_unknown_template_errors() {
        let r = TemplateRegistry::new();
        let ps = ParameterSet::new();
        let err = r.render("no_such_template", &ps).unwrap_err();
        assert!(matches!(err, nctool_tpl::TplError::TemplateNotFound { .. }));
    }

    #[test]
    fn extract_params_missing_template_errors() {
        let r = TemplateRegistry::new();
        let err = r.extract_params("no_such").unwrap_err();
        assert!(matches!(err, RegistryError::NotFound(_)));
    }

    /// `extract_params` 必须**穿透 `{% include %}`**：组合模板若只列出主模板
    /// 自身的变量，用户按顺序填参会漏掉片段所需参数，直到渲染才报错。
    ///
    /// 回归背景：拆分 `undercut.j2` 时把公共起始段抽成
    /// `turning/_undercut_common.j2`，`inspect` 当时只显示 6 个参数，
    /// 而实际需要 11 个（片段的 `STD_KEY`/`Z_START`/`D1_CUT` 等全部缺失）。
    #[test]
    fn extract_params_traverses_include() {
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "frag",
            TemplateCategory::General,
            "片段",
            "G0 Z{{ FRAG_Z | nc_fixed(3) }}\n",
            vec![],
        )
        .unwrap();
        r.add_memory(
            "main",
            TemplateCategory::General,
            "主模板",
            "{% include \"frag\" %}\nG1 X{{ MAIN_X | nc_fixed(3) }}\n",
            vec![],
        )
        .unwrap();

        let vars = r.extract_params("main").unwrap();
        let names: Vec<&str> = vars.iter().map(|v| v.name.as_str()).collect();
        assert!(names.contains(&"MAIN_X"), "应含主模板变量: {names:?}");
        assert!(
            names.contains(&"FRAG_Z"),
            "应并入 include 片段的变量: {names:?}"
        );
    }

    /// 回归（P1-11）：include 闭包里同名参数的规格是「先访问者优先」，
    /// 而遍历顺序此前是「先递归进被引用模板、再并入它自己的规格」——
    /// 于是**离主模板最远**的声明胜出，与文档承诺的「更接近主模板的声明优先」相反。
    ///
    /// 后果不是报错而是静默按另一套约束校验：白名单/区间被换成孙模板那份，
    /// 用户填的值可能被拒或被放行，看的是他没见过的声明。
    #[test]
    fn include_closure_spec_precedence_is_nearest_to_main() {
        use crate::model::{ParamKind, ParamValue};

        let spec = |val: &str| {
            ParamSpec::new("P", ParamKind::String, "同名参数")
                .with_options(vec![ParamValue::String(val.to_string())])
        };
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "grandchild",
            TemplateCategory::General,
            "孙",
            "G0 Z{{ P }}\n",
            vec![spec("grandchild")],
        )
        .unwrap();
        r.add_memory(
            "child",
            TemplateCategory::General,
            "子",
            "{% include \"grandchild\" %}\nG0 Y{{ P }}\n",
            vec![spec("child")],
        )
        .unwrap();
        r.add_memory(
            "main",
            TemplateCategory::General,
            "主",
            "{% include \"child\" %}\n",
            vec![],
        )
        .unwrap();

        let entry = r.get("main").unwrap();
        let mut vars = Vec::new();
        let mut specs = entry.params.clone();
        let mut visited = std::collections::BTreeSet::from(["main".to_string()]);
        r.collect_include_closure(entry, &mut vars, &mut specs, &mut visited);

        let p = specs
            .iter()
            .find(|s| s.name == "P")
            .expect("应并入 P 的规格");
        assert_eq!(
            p.options,
            Some(vec![ParamValue::String("child".into())]),
            "child 比 grandchild 更接近 main，其声明必须优先"
        );
    }

    /// 系统注入变量（`machine`）不出现在参数表中——由管线提供，不问用户要。
    #[test]
    fn extract_params_excludes_system_vars() {
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "sysvar",
            TemplateCategory::General,
            "引用系统变量",
            "{{ machine.rapid }} X{{ x | nc_fixed(3) }}\n",
            vec![],
        )
        .unwrap();
        let vars = r.extract_params("sysvar").unwrap();
        let names: Vec<&str> = vars.iter().map(|v| v.name.as_str()).collect();
        assert!(names.contains(&"x"));
        assert!(!names.contains(&"machine"), "machine 应被剔除: {names:?}");
    }

    /// include 环引用必须能终止（a → b → a），否则 `extract_params` 会栈溢出。
    #[test]
    fn extract_params_survives_include_cycle() {
        let mut r = TemplateRegistry::new();
        r.add_memory(
            "cyc_a",
            TemplateCategory::General,
            "环 A",
            "{% include \"cyc_b\" %}{{ A_VAR }}\n",
            vec![],
        )
        .unwrap();
        r.add_memory(
            "cyc_b",
            TemplateCategory::General,
            "环 B",
            "{% include \"cyc_a\" %}{{ B_VAR }}\n",
            vec![],
        )
        .unwrap();
        let vars = r.extract_params("cyc_a").unwrap();
        let names: Vec<&str> = vars.iter().map(|v| v.name.as_str()).collect();
        assert!(names.contains(&"A_VAR"));
        assert!(names.contains(&"B_VAR"));
    }

    /// **P1-4 守卫**：真实内置模板库必须全部安装成功——「minijinja/nctool-tpl
    /// 升级踩雷」应在 CI 变红，而不是用户运行时 panic（旧 `.expect` 即崩）。
    #[test]
    fn builtin_registry_has_no_warnings() {
        let r = TemplateRegistry::new();
        assert!(
            r.builtin_warnings().is_empty(),
            "内置模板应全部安装成功: {:?}",
            r.builtin_warnings()
        );
        assert!(!r.list(None).is_empty(), "注册表不应为空");
    }

    /// 每个内置模板**单独**过一遍解析器：把"升级后某个模板编译不过"精确到模板名，
    /// 不依赖 registry 构建路径（SUMMARY §5 契约2 配套③）。
    #[test]
    fn every_builtin_template_parses_standalone() {
        for (name, _category, _description, source, _params) in builtin_templates() {
            nctool_tpl::parse(source, name)
                .unwrap_or_else(|e| panic!("内置模板 {name} 解析失败: {e}"));
        }
    }

    /// 降级路径本身可测：失败**结构化记录**、经 `builtin_warnings()` 可查，
    /// 进程不 panic（P1-4）。stderr 告警有进程级去重，此处不断言输出。
    #[test]
    fn builtin_failure_is_recorded_not_panicking() {
        let mut r = TemplateRegistry::new();
        r.note_builtin_failure("bad_tpl", &RegistryError::EmptySource("bad_tpl".into()));
        let w = r.builtin_warnings();
        assert_eq!(w.len(), 1, "失败应被记录: {w:?}");
        assert_eq!(w[0].name, "bad_tpl");
        assert!(
            w[0].message.contains("bad_tpl"),
            "消息应含模板名（可诊断）: {}",
            w[0].message
        );
    }
}
