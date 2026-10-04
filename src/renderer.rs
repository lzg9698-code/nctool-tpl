//! Domain-neutral Jinja rendering with explicit, checked extension registration.
use std::path::Path;

use minijinja::Environment;

use crate::error::{from_minijinja_error, TplError};

/// A domain-neutral Jinja renderer. Extensions are opt-in per instance.
#[derive(Debug)]
pub struct Renderer {
    env: Environment<'static>,
    /// 宽松模式下未定义变量渲染为空字符串（而非报错）。
    lenient: bool,
    filters: std::collections::HashSet<String>,
    output_limit: usize,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    /// 新建通用渲染器，不注册领域过滤器。
    ///
    /// 默认使用 **Strict** 未定义变量策略：模板引用缺失变量时直接渲染失败并报错，
    /// 避免静默输出不完整文本（与 `jinja2.meta` + `StrictUndefined` 的做法一致）。
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        env.set_fuel(Some(1_000_000));
        Self {
            env,
            lenient: false,
            filters: std::collections::HashSet::new(),
            output_limit: 16 * 1024 * 1024,
        }
    }

    /// Set instruction and output budgets; both must be nonzero.
    pub fn with_limits(mut self, instructions: u64, output_bytes: usize) -> Result<Self, TplError> {
        if instructions == 0 || output_bytes == 0 {
            return Err(TplError::Render {
                name: "limits".into(),
                message: "render budgets must be nonzero".into(),
            });
        }
        self.env.set_fuel(Some(instructions));
        self.output_limit = output_bytes;
        Ok(self)
    }
    /// Configure standard Jinja whitespace options without a domain-specific preset.
    pub fn with_whitespace(mut self, trim_blocks: bool, lstrip_blocks: bool) -> Self {
        self.env.set_trim_blocks(trim_blocks);
        self.env.set_lstrip_blocks(lstrip_blocks);
        self
    }
    /// Register an extension filter. Duplicate and built-in names are rejected.
    pub fn add_filter<F, Rv, Args>(&mut self, name: &str, filter: F) -> Result<(), TplError>
    where
        F: minijinja::functions::Function<Rv, Args>,
        Rv: minijinja::value::FunctionResult,
        Args: for<'a> minijinja::value::FunctionArgs<'a>,
    {
        if self.filters.contains(name)
            || [
                "abs",
                "attr",
                "batch",
                "bool",
                "capitalize",
                "chain",
                "count",
                "d",
                "default",
                "dictsort",
                "e",
                "escape",
                "first",
                "float",
                "format",
                "groupby",
                "indent",
                "int",
                "items",
                "join",
                "last",
                "length",
                "lines",
                "list",
                "lower",
                "map",
                "max",
                "min",
                "pprint",
                "reject",
                "rejectattr",
                "replace",
                "reverse",
                "round",
                "safe",
                "select",
                "selectattr",
                "slice",
                "sort",
                "split",
                "string",
                "sum",
                "title",
                "tojson",
                "trim",
                "unique",
                "upper",
                "urlencode",
                "zip",
            ]
            .contains(&name)
        {
            return Err(TplError::Render {
                name: name.into(),
                message: "duplicate filter registration".into(),
            });
        }
        self.filters.insert(name.to_owned());
        self.env.add_filter(name.to_owned(), filter);
        Ok(())
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
        let mut writer = LimitedOutput::new(self.output_limit);
        tmpl.render_captured_to(context, &mut writer)
            .map_err(|err| from_minijinja_error(err, name, Some(source)))?;
        Ok(writer.into_string())
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
                || name.contains('\\')
                || std::path::Path::new(name)
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                return Ok(None);
            }
            let root = match dir.canonicalize() {
                Ok(r) => r,
                Err(_) => return Ok(None),
            };
            let path = match root.join(name).canonicalize() {
                Ok(p) if p.starts_with(&root) => p,
                _ => return Ok(None),
            };
            let file = std::fs::File::open(path).map_err(|e| {
                minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
            })?;
            use std::io::Read;
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| {
                    minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
                })?;
            if bytes.len() > 1024 * 1024 {
                return Err(minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "template exceeds 1 MiB",
                ));
            }
            String::from_utf8(bytes).map(Some).map_err(|e| {
                minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
            })
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
        let mut writer = LimitedOutput::new(self.output_limit);
        tmpl.render_captured_to(context, &mut writer)
            .map_err(|err| from_minijinja_error(err, name, Some(tmpl.source())))?;
        Ok(writer.into_string())
    }
}

struct LimitedOutput {
    bytes: Vec<u8>,
    limit: usize,
}
impl LimitedOutput {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
    fn into_string(self) -> String {
        String::from_utf8(self.bytes).expect("Jinja emits valid UTF-8")
    }
}
impl std::io::Write for LimitedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("rendered output exceeds byte budget"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
