//! 渲染器：minijinja `Environment` + 数学过滤器集 + NC 数值格式化过滤器。
//!
//! 提供严格/宽松模式的模板渲染、多模板注册（include/extends/import）
//! 与文件系统目录加载（含路径安全校验）。

use std::path::Path;

use minijinja::Environment;

use crate::error::{from_minijinja_error, TplError};
use crate::filters::{
    checked_math, filter_nc_fixed, filter_nc_pad, filter_nc_signed, filter_nc_strip,
};

// 渲染器：minijinja Environment + 数学过滤器集
// ---------------------------------------------------------------------------

/// 渲染器。内部持有 minijinja `Environment`，并注册一组数学过滤器和 NC 数值格式化过滤器。
///
/// 数学过滤器集（全部基于 Rust 标准库 `f64`，零额外依赖）：
/// `sin` `cos` `tan` `asin` `acos` `atan` `sqrt` `exp` `ln` `log10` `pow` `floor` `ceil`
///
/// **角度制三角函数**（工艺图纸按度输入，见下）：
/// - `sin_d(D)` `cos_d(D)` `tan_d(D)`：以**度**为输入
/// - `asin_d(x)` `acos_d(x)` `atan_d(x)`：以**度**为输出
///
/// NC 数值格式化过滤器（G-code 专用）：
/// - `nc_fixed(N)`：固定小数位，`{{ x | nc_fixed(3) }}` → `21.000`
/// - `nc_signed(N)`：强制正号 + 固定小数位，`{{ x | nc_signed(3) }}` → `+21.000`
///   （负数为 `-4.500`）。用于**增量坐标/旋转量**——部分控制器要求显式正号，
///   省略号会被误判为绝对值。对应源项目 Jinja2 的 `fmt_coord`
/// - `nc_strip`：去尾零，`{{ x | nc_strip }}` → `21`（输入 21.0）
/// - `nc_pad(N)`：前导零填充，`{{ n | nc_pad(4) }}` → `0001`（程序号/行号用）
///
/// # 角度制 vs 弧度制（重要）
///
/// `sin`/`cos`/`tan` 等**不带 `_d` 后缀**的过滤器一律按**弧度**计算（与 Rust 标准库一致）。
/// 工艺图纸上的角度是**度**，直接写 `x | sin` 会得到错误的坐标——且是**静默**的错误
/// （`sin(30°) = 0.5`，而 `sin(30 rad) ≈ -0.988`），错误的坐标会写进 G-code 导致撞刀。
///
/// 迁移自 Python/Jinja2 的模板尤其危险：Python 侧常写成 `math.sin(math.radians(x))`
/// 并暴露为 `sin`，也就是**那个 `sin` 是度制**，而本 crate 的 `sin` 是弧度制。
/// 迁移时必须二选一：
/// - 写成 `x | sin_d`（推荐，意图明确）
/// - 或显式换算 `(x * pi / 180) | sin`
///
/// 因此**新模板一律用 `_d` 后缀**；裸 `sin`/`cos`/`tan` 只应在确认输入本就是弧度时使用。
///
/// 所有数学过滤器和 NC 过滤器对结果做**有限性校验**：一旦产生 `NaN`/`Inf`（如 `sqrt(-1)`、
/// `asin(2)`、`ln(0)`），渲染立即失败并报 [`TplError::Render`]，避免非法坐标静默写入 G-code。
///
/// 注意：该防线只覆盖**本 crate 注册的过滤器**。裸 `{{ x }}` 输出、minijinja 内建
/// 过滤器/运算（如 `round`、`x + 1`）产生的 NaN/Inf 不在保护范围内——请先经上层
/// 参数校验逻辑（如 `nctool-core`）保证上下文数值有限。
#[derive(Debug)]
pub struct Renderer {
    env: Environment<'static>,
    /// 宽松模式下未定义变量渲染为空字符串（而非报错）。
    lenient: bool,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    /// 新建渲染器（带数学过滤器集）。
    ///
    /// 默认使用 **Strict** 未定义变量策略：模板引用缺失变量时直接渲染失败并报错，
    /// 避免静默输出不完整 G-code（与 `jinja2.meta` + `StrictUndefined` 的做法一致）。
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        env.add_filter("sin", |v: f64| checked_math(v.sin(), "sin"));
        env.add_filter("cos", |v: f64| checked_math(v.cos(), "cos"));
        env.add_filter("tan", |v: f64| checked_math(v.tan(), "tan"));
        env.add_filter("asin", |v: f64| checked_math(v.asin(), "asin"));
        env.add_filter("acos", |v: f64| checked_math(v.acos(), "acos"));
        env.add_filter("atan", |v: f64| checked_math(v.atan(), "atan"));
        env.add_filter("sqrt", |v: f64| checked_math(v.sqrt(), "sqrt"));
        env.add_filter("exp", |v: f64| checked_math(v.exp(), "exp"));
        env.add_filter("ln", |v: f64| checked_math(v.ln(), "ln"));
        env.add_filter("log10", |v: f64| checked_math(v.log10(), "log10"));
        env.add_filter("pow", |v: f64, e: f64| checked_math(v.powf(e), "pow"));
        env.add_filter("floor", |v: f64| checked_math(v.floor(), "floor"));
        env.add_filter("ceil", |v: f64| checked_math(v.ceil(), "ceil"));
        // 角度制三角函数（工艺图纸按度输入）：`_d` 后缀 = degrees。
        // 见上方「角度制 vs 弧度制」——新模板一律用这组，避免静默错坐标。
        env.add_filter("sin_d", |v: f64| {
            checked_math(v.to_radians().sin(), "sin_d")
        });
        env.add_filter("cos_d", |v: f64| {
            checked_math(v.to_radians().cos(), "cos_d")
        });
        env.add_filter("tan_d", |v: f64| {
            checked_math(v.to_radians().tan(), "tan_d")
        });
        // 反三角以度输出（`asin_d(0.5)` → `30`）
        env.add_filter("asin_d", |v: f64| {
            checked_math(v.asin().to_degrees(), "asin_d")
        });
        env.add_filter("acos_d", |v: f64| {
            checked_math(v.acos().to_degrees(), "acos_d")
        });
        env.add_filter("atan_d", |v: f64| {
            checked_math(v.atan().to_degrees(), "atan_d")
        });
        // NC 数值格式化过滤器（G-code 专用）
        env.add_filter("nc_fixed", filter_nc_fixed);
        env.add_filter("nc_signed", filter_nc_signed);
        env.add_filter("nc_strip", filter_nc_strip);
        env.add_filter("nc_pad", filter_nc_pad);
        Self {
            env,
            lenient: false,
        }
    }

    /// 切换为**宽松模式**：模板中未定义变量渲染为空字符串，而非报错。
    ///
    /// 消费式 builder 方法，便于链式构造：`Renderer::new().with_lenient()`。
    /// 默认构造已是严格模式，此方法用于需要宽松渲染的场景（如先渲染、后由
    /// [`extract_undeclared`](crate::extract_undeclared) 校验必选参数的流程）。
    pub fn with_lenient(mut self) -> Self {
        self.env
            .set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
        self.lenient = true;
        self
    }

    /// 显式切换为**严格模式**（默认）：模板中未定义变量渲染报错。
    ///
    /// 消费式 builder 方法，便于链式构造：`Renderer::new().with_strict()`。
    pub fn with_strict(mut self) -> Self {
        self.env
            .set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        self.lenient = false;
        self
    }

    /// 当前是否为宽松模式（未定义变量渲染为空而非报错）。
    pub fn is_lenient(&self) -> bool {
        self.lenient
    }

    /// 渲染模板。`context` 用 `minijinja::context!` 宏或 `Value::from_serialize` 构造。
    ///
    /// 此方法渲染**单段字符串**模板，不涉及模板间引用。如需 `{% include %}` /
    /// `{% extends %}` / `{% import %}`，请先用 [`add_template`](Self::add_template)
    /// 或 [`set_path_loader`](Self::set_path_loader) 注册模板，再调用
    /// [`render_template`](Self::render_template)。
    pub fn render(
        &self,
        source: &str,
        name: &str,
        context: &minijinja::Value,
    ) -> Result<String, TplError> {
        let tmpl = self
            .env
            .template_from_named_str(name, source)
            .map_err(|err| from_minijinja_error(err, name, Some(source)))?;
        tmpl.render(context)
            .map_err(|err| from_minijinja_error(err, name, Some(source)))
    }

    /// 注册一个内存模板（owned 字符串，无生命周期约束）。
    ///
    /// 注册后可通过 [`render_template`](Self::render_template) 按名称渲染，
    /// 且模板内的 `{% include "name" %}` / `{% extends "name" %}` /
    /// `{% import "name" %}` 能正确解析到已注册的模板。
    ///
    /// **同名语义**：同名重复注册时**静默替换**（后者覆盖前者，无任何提示）；
    /// 与 [`set_path_loader`](Self::set_path_loader) 同名时，内存模板优先，
    /// 目录中的同名文件无法覆盖已注册模板。
    pub fn add_template(
        &mut self,
        name: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<(), TplError> {
        let name = name.into();
        let source = source.into();
        self.env
            .add_template_owned(name.clone(), source.clone())
            .map_err(|err| from_minijinja_error(err, &name, Some(&source)))
    }

    /// 从文件系统目录动态加载模板。
    ///
    /// 目录下的文件按**文件名（含扩展名）**作为模板名引用，例如
    /// `templates/sub.gcode` 可被 `{% include "sub.gcode" %}` 引用。
    /// 模板按需加载并缓存，同一名称只加载一次。
    ///
    /// # 安全性
    ///
    /// 模板内容视为**可信输入**。模板名经过两层校验后才与目录拼接：
    ///
    /// - 本方法自加的校验：拒绝空名、含 `:`（Windows 盘符前缀如 `C:`，
    ///   `PathBuf::push` 会整体替换 base）、以 `/` 或 `\` 开头（绝对路径）的名字；
    /// - minijinja 引擎的 `safe_join`：拒绝以 `.` 开头的路径段（含 `..`/`.`）
    ///   与含 `\` 的段，因此 `{% include "../x" %}`、`{% include "..\x" %}`
    ///   无法逃出目录。
    ///
    /// 目录内相对子路径（`sub/dir/x`）是允许的（特性而非漏洞）。
    /// 请勿将不受信任来源的模板交给本加载器。
    pub fn set_path_loader(&mut self, dir: impl AsRef<Path>) {
        let dir = dir.as_ref().to_path_buf();
        self.env.set_loader(move |name| {
            if name.is_empty()
                || name.contains(':')
                || name.starts_with('/')
                || name.starts_with('\\')
            {
                // 视为模板不存在（loader 约定：Ok(None) = 未找到）
                return Ok(None);
            }
            minijinja::path_loader(&dir)(name)
        });
    }

    /// 渲染已注册或已加载的模板（支持 `include` / `extends` / `import`）。
    ///
    /// 模板需先通过 [`add_template`](Self::add_template) 注册，或通过
    /// [`set_path_loader`](Self::set_path_loader) 配置目录加载。
    pub fn render_template(
        &self,
        name: &str,
        context: &minijinja::Value,
    ) -> Result<String, TplError> {
        let tmpl = self
            .env
            .get_template(name)
            .map_err(|err| from_minijinja_error(err, name, None))?;
        tmpl.render(context)
            .map_err(|err| from_minijinja_error(err, name, Some(tmpl.source())))
    }
}
