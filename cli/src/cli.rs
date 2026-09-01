//! clap 命令行定义：完整命令树 + 全局选项。

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// 全局输出风格（CLI 自身的结果展示，与生成选项的 Gcode/Text 无关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FormatArg {
    /// 人类可读文本
    Text,
    /// 机器可读 JSON
    Json,
}

impl Default for FormatArg {
    fn default() -> Self {
        FormatArg::Text
    }
}

/// 全局公共选项（可在任意子命令后使用）。
#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    /// 机床标识（内置 generic / wfl_m65 / index_ms40，或配置文件中的自定义机床）
    #[arg(long, global = true)]
    pub machine: Option<String>,

    /// 模板目录（加载其中 *.j2 模板；覆盖配置文件中的 template_dir）
    #[arg(long, global = true)]
    pub template_dir: Option<PathBuf>,

    /// 结果输出格式
    #[arg(long, global = true, value_enum, default_value_t = FormatArg::default())]
    pub format: FormatArg,

    /// 详细输出（打印校验警告等）
    #[arg(long, global = true)]
    pub verbose: bool,
}

/// nctool —— 数控 G-code 模板工具
#[derive(Debug, Parser)]
#[command(
    name = "nctool",
    version,
    about = "NCtool 模板工具：浏览/校验/渲染 G-code 模板",
    long_about = "nctool 在 nctool-tpl + nctool-core 之上提供命令行入口：\n\
                  模板浏览（templates）、变量提取（inspect）、参数校验（validate）、\n\
                  G-code 生成（render/generate）、机床配置（machine）与配置管理（config）。"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

/// 子命令树。
#[derive(Debug, Subcommand)]
pub enum Command {
    /// 模板管理：列表 / 查看 / 新建
    Templates(TemplatesArgs),
    /// 变量提取：必选/可选参数 + 行列定位
    Inspect(InspectArgs),
    /// 渲染前参数校验，输出结构化报告
    Validate(ValidateArgs),
    /// 渲染生成 G-code
    Render(RenderArgs),
    /// 生成 G-code（与 render 相同，管线后处理全开时的规范入口）
    Generate(RenderArgs),
    /// 机床配置：列表 / 查看
    Machine(MachineArgs),
    /// 配置管理：初始化示例配置 / 查看生效配置
    Config(ConfigArgs),
    /// 启动本地 Web UI（规划于阶段 2）
    Ui(UiArgs),
    /// 零件级批量生成（规划于阶段 4）
    Part(PartArgs),
    /// 生成 shell 补全脚本
    Completion(CompletionArgs),
}

// ---------------------------------------------------------------------------
// templates
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct TemplatesArgs {
    #[command(subcommand)]
    pub command: TemplatesCommand,
}

#[derive(Debug, Subcommand)]
pub enum TemplatesCommand {
    /// 列出模板（可按分类筛选）
    List(TemplatesListArgs),
    /// 查看模板源码与参数表
    Show(TemplatesShowArgs),
    /// 新建模板骨架
    New(TemplatesNewArgs),
}

#[derive(Debug, Args)]
pub struct TemplatesListArgs {
    /// 按分类筛选（通用/铣削/车削/钻孔/机床）
    #[arg(long, value_enum)]
    pub category: Option<CategoryArg>,
}

#[derive(Debug, Args)]
pub struct TemplatesShowArgs {
    /// 模板名（内置名或模板文件路径）
    pub template: String,
}

#[derive(Debug, Args)]
pub struct TemplatesNewArgs {
    /// 新模板名
    pub name: String,
    /// 分类
    #[arg(long, value_enum, default_value_t = CategoryArg::General)]
    pub category: CategoryArg,
    /// 输出目录（默认：配置的 template_dir，未配置时 ./templates）
    #[arg(long)]
    pub dir: Option<PathBuf>,
}

/// 模板分类（CLI 侧枚举，映射到 core 的 TemplateCategory）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CategoryArg {
    /// 通用
    #[value(alias = "通用")]
    General,
    /// 铣削
    #[value(alias = "铣削")]
    Milling,
    /// 车削
    #[value(alias = "车削")]
    Turning,
    /// 钻孔
    #[value(alias = "钻孔")]
    Drilling,
    /// 机床
    #[value(alias = "机床")]
    Machine,
}

impl CategoryArg {
    pub fn to_core(self) -> nctool_core::registry::TemplateCategory {
        use nctool_core::registry::TemplateCategory;
        match self {
            CategoryArg::General => TemplateCategory::General,
            CategoryArg::Milling => TemplateCategory::Milling,
            CategoryArg::Turning => TemplateCategory::Turning,
            CategoryArg::Drilling => TemplateCategory::Drilling,
            CategoryArg::Machine => TemplateCategory::Machine,
        }
    }

    pub fn from_core(c: nctool_core::registry::TemplateCategory) -> &'static str {
        use nctool_core::registry::TemplateCategory;
        match c {
            TemplateCategory::General => "通用",
            TemplateCategory::Milling => "铣削",
            TemplateCategory::Turning => "车削",
            TemplateCategory::Drilling => "钻孔",
            TemplateCategory::Machine => "机床",
        }
    }
}

// ---------------------------------------------------------------------------
// inspect / validate / render
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct InspectArgs {
    /// 模板名或模板文件路径
    pub template: String,
}

/// 校验/渲染共享的参数输入选项。
#[derive(Debug, Args)]
pub struct ParamInputArgs {
    /// 参数 k=v（类型自动推断：数值/字符串/布尔；k:s=v 强制字符串、
    /// k:n=v 强制数值、k:b=v 强制布尔；前导零纯数字按字符串处理）
    #[arg(long = "param", value_name = "K=V")]
    pub param: Vec<String>,

    /// 参数文件（JSON 对象，如 {"x": 21.0, "tool": "D12"}）
    #[arg(long = "params-file", value_name = "FILE")]
    pub params_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// 模板名或模板文件路径
    pub template: String,

    #[command(flatten)]
    pub params: ParamInputArgs,
}

#[derive(Debug, Args)]
pub struct RenderArgs {
    /// 模板名或模板文件路径
    pub template: String,

    #[command(flatten)]
    pub params: ParamInputArgs,

    /// 输出文件（默认写 stdout）
    #[arg(long)]
    pub out: Option<PathBuf>,

    /// 生成行号
    #[arg(long)]
    pub line_numbers: bool,

    /// 头部注释
    #[arg(long)]
    pub header: bool,

    /// 仅输出 ASCII（非 ASCII 替换为 ?）
    #[arg(long)]
    pub ascii: bool,

    /// 清理空行
    #[arg(long)]
    pub strip_blank: bool,

    /// 宽松模式：未定义变量渲染为空字符串（经过滤器引用的变量仍需具体值）
    #[arg(long)]
    pub lenient: bool,
}

// ---------------------------------------------------------------------------
// machine
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct MachineArgs {
    #[command(subcommand)]
    pub command: MachineCommand,
}

#[derive(Debug, Subcommand)]
pub enum MachineCommand {
    /// 列出机床预设
    List,
    /// 查看机床配置
    Show(MachineShowArgs),
}

#[derive(Debug, Args)]
pub struct MachineShowArgs {
    /// 机床标识（内置或自定义）
    pub id: String,
}

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// 生成示例 nctool.toml（当前目录）
    Init,
    /// 展示生效配置（全局 + 项目层叠合并）
    Show,
}

// ---------------------------------------------------------------------------
// ui / part / completion
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct UiArgs {
    /// 监听地址
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    /// 监听端口
    #[arg(long, default_value_t = 8787)]
    pub port: u16,
    /// 启动后自动打开浏览器
    #[arg(long)]
    pub open: bool,
}

#[derive(Debug, Args)]
pub struct PartArgs {
    #[command(subcommand)]
    pub command: PartCommand,
}

#[derive(Debug, Subcommand)]
pub enum PartCommand {
    /// 零件级批量生成（多工序一次生成）
    Generate(PartGenerateArgs),
}

#[derive(Debug, Args)]
pub struct PartGenerateArgs {
    /// 零件定义文件（JSON）
    pub part: PathBuf,
    /// 输出目录
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ShellArg {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Elvish,
}

#[derive(Debug, Args)]
pub struct CompletionArgs {
    /// 目标 shell
    pub shell: ShellArg,
}
