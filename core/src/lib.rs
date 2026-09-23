//! # nctool-core
//!
//! NCtool 核心库：在 `nctool-tpl`（模板解析 + 变量提取 + 渲染）之上，
//! 提供面向 G-code 生成的生产级能力：
//!
//! - **数据模型**：参数值/参数规格（含取值范围与整数约束）、机床配置、参数集
//! - **参数校验引擎**：渲染前校验必选参数齐全、类型匹配、取值区间、整数性
//! - **模板注册表**：统一管理内存模板、文件系统模板与内置模板库
//! - **G-code 生成管线**：参数校验 → 渲染 → 后处理（行号/注释/数值格式化）
//!
//! 设计原则：**渲染前可发现错误**。通过 [`validate`] 在校验阶段就能定位缺失参数、
//! 类型不匹配等问题，而不是在生成后才暴露。

#![warn(missing_docs)]

pub mod asset;
pub mod derive;
pub mod machine;
pub mod manifest;
pub mod model;
pub mod part;
pub mod pipeline;
pub mod registry;
pub mod validate;
pub mod variables;

// 资产写入内核（原子写 / 乐观锁 / 路径防护 / 规格指纹 / 预设存储 / 机床写策略）
pub use asset::{
    default_preset_path, ensure_outside_template_root, is_builtin_machine, now_iso8601,
    validate_asset_name, CompletenessReport, CrossTemplateReport, FileFingerprint, LoadOutcome,
    MachineSaveReport, MachineWriter, Preset, PresetFile, PresetStore, PresetView, SafePath,
    SpecFingerprint, StaleReport, WriteAction, WriteError, WriteKernel, WriteOutcome, CONFIG_FILE,
    PRESET_FILE, PRESET_SCHEMA_VERSION,
};
// 数据模型根导出
pub use model::{
    DeriveRule, MachineConfig, ParamKind, ParamSpec, ParamValue, ParameterSet, RequiredIf,
};
// 校验
pub use validate::{
    spec, validate_template, validate_with_vars, IssueKind, ValidationIssue, ValidationLevel,
    ValidationReport,
};
// 模板注册表
pub use registry::{Analysis, TemplateCategory, TemplateEntry, TemplateRegistry, TemplateSource};
// 模板清单（templates.yaml）
pub use manifest::{
    extract_header_meta, merge_params, HeaderMeta, ManifestError, ParamOverride, ResolvedMeta,
    TemplateManifest, TemplateMeta, TemplateStatus,
};
// 变量库（variables.yaml）
pub use variables::{VariableLibrary, VARIABLES_FILE};
// 参数派生（Rust 侧查表计算后注入）
pub use derive::DeriveError;
// 生成管线
pub use pipeline::{GCodeGenerator, GenerationOptions, OutputFormat, PipelineError};
// 零件级批量生成（多工序一次生成：参数继承 / 行号续编 / 事务语义）
pub use part::{
    resolve_machine, OpFailure, OpOutcome, PartError, PartOp, PartOpOptions, PartOptions,
    PartOutcome, PartSpec,
};
// 机床配置
pub use machine::{MachineId, MachinePreset};
