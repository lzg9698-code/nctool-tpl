//! # nctool-core
//!
//! NCtool 核心库：在 `nctool-tpl`（模板解析 + 变量提取 + 渲染）之上，
//! 提供面向 G-code 生成的生产级能力：
//!
//! - **数据模型**：参数值/参数规格、机床配置、参数集、刀具、零件、工序
//! - **参数校验引擎**：渲染前校验必选参数齐全、类型匹配、默认值兜底
//! - **模板注册表**：统一管理内存模板、文件系统模板与内置模板库
//! - **G-code 生成管线**：参数校验 → 渲染 → 后处理（行号/注释/数值格式化）
//!
//! 设计原则：**渲染前可发现错误**。通过 [`validate`] 在校验阶段就能定位缺失参数、
//! 类型不匹配等问题，而不是在生成后才暴露。

#![warn(missing_docs)]

pub mod machine;
pub mod model;
pub mod pipeline;
pub mod registry;
pub mod validate;

// 数据模型根导出
pub use model::{
    MachineConfig, Operation, ParamKind, ParamSpec, ParamValue, ParameterSet, Part, Tool,
};
// 校验
pub use validate::{
    spec, validate_template, validate_with_vars, ValidationIssue, ValidationLevel, ValidationReport,
};
// 模板注册表
pub use registry::{TemplateCategory, TemplateEntry, TemplateRegistry, TemplateSource};
// 生成管线
pub use pipeline::{GCodeGenerator, GenerationOptions, OutputFormat, PipelineError};
// 机床配置
pub use machine::{MachineId, MachinePreset};
