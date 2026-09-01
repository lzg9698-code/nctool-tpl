//! 模板错误类型与错误定位辅助。
//!
//! [`TplError`] 细分变体、minijinja 错误转换、源码字节范围 → (行, 列) 换算。

use std::fmt;

/// 模板解析/渲染错误。
///
/// `#[non_exhaustive]`：未来可能新增错误变体，外部 match 应保留通配分支。
///
/// 细分变体让上层可精准处理：例如 `UndefinedVariable` 可触发"参数缺失"提示，
/// `TemplateNotFound` 可触发模板路径检查，而不必解析 message 字符串。
#[non_exhaustive]
#[derive(Debug)]
pub enum TplError {
    /// 语法错误，带模板名与行列号。
    ///
    /// `col` 为 minijinja 停止解析位置的最佳近似（来自其错误携带的字节范围），
    /// 在无法取得字节范围时回退为 `1`。
    Parse {
        /// 触发错误的模板名
        name: String,
        /// 完整错误信息（含 minijinja 原始描述）
        message: String,
        /// 错误所在行（1 起始）
        line: usize,
        /// 错误所在列（1 起始，最佳近似）
        col: usize,
    },
    /// 模板未找到（`{% include %}` / `{% extends %}` / `get_template` 引用了不存在的模板）。
    TemplateNotFound {
        /// 触发错误的模板名
        name: String,
        /// 被引用但不存在的模板名（从 minijinja 错误详情中提取，可能为空）。
        template: String,
        /// 完整错误信息
        message: String,
    },
    /// 未定义变量（严格模式下引用了不存在的变量）。
    UndefinedVariable {
        /// 触发错误的模板名
        name: String,
        /// 变量名（尽力从源码错误位置恢复；无法定位时为空）。
        variable: String,
        /// 完整错误信息
        message: String,
    },
    /// 未知过滤器（模板使用了未注册的过滤器）。
    UnknownFilter {
        /// 触发错误的模板名
        name: String,
        /// 过滤器名（从错误详情中提取，可能为空）。
        filter: String,
        /// 完整错误信息
        message: String,
    },
    /// 未知测试（模板使用了未注册的测试）。
    UnknownTest {
        /// 触发错误的模板名
        name: String,
        /// 测试名（从错误详情中提取，可能为空）。
        test: String,
        /// 完整错误信息
        message: String,
    },
    /// 其他渲染错误（无效操作、参数错误、序列化失败等兜底）。
    Render {
        /// 触发错误的模板名
        name: String,
        /// 完整错误信息
        message: String,
    },
}

impl fmt::Display for TplError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TplError::Parse {
                name,
                message,
                line,
                ..
            } => {
                write!(f, "{name}:{line}: 模板语法错误: {message}")
            }
            TplError::TemplateNotFound {
                name,
                template,
                message,
            } => {
                if template.is_empty() {
                    write!(f, "{name}: 模板未找到: {message}")
                } else {
                    write!(f, "{name}: 模板未找到 '{template}': {message}")
                }
            }
            TplError::UndefinedVariable {
                name,
                variable,
                message,
            } => {
                if variable.is_empty() {
                    write!(f, "{name}: 未定义变量: {message}")
                } else {
                    write!(f, "{name}: 未定义变量 '{variable}': {message}")
                }
            }
            TplError::UnknownFilter {
                name,
                filter,
                message,
            } => {
                if filter.is_empty() {
                    write!(f, "{name}: 未知过滤器: {message}")
                } else {
                    write!(f, "{name}: 未知过滤器 '{filter}': {message}")
                }
            }
            TplError::UnknownTest {
                name,
                test,
                message,
            } => {
                if test.is_empty() {
                    write!(f, "{name}: 未知测试: {message}")
                } else {
                    write!(f, "{name}: 未知测试 '{test}': {message}")
                }
            }
            TplError::Render { name, message } => {
                write!(f, "{name}: 渲染错误: {message}")
            }
        }
    }
}

impl std::error::Error for TplError {}

/// 从错误详情字符串中提取第一个引号（单引号或双引号）内的内容。
///
/// minijinja 的错误详情通常形如 `unknown filter 'foo'` 或
/// `variable 'x' is undefined`，此函数提取其中的名称。
pub(crate) fn extract_quoted(s: &str) -> Option<String> {
    let start = s.find('\'').or_else(|| s.find('"'))?;
    let quote = s.as_bytes()[start];
    let rest = &s[start + 1..];
    let end = rest.find(quote as char)?;
    Some(rest[..end].to_string())
}

/// 从 `"prefix name rest"` 格式的详情中提取 `name`（第一个空白分隔的词）。
///
/// 用于 minijinja 的 `"filter badfilter is unknown"` / `"test badtest is unknown"`
/// 这类无引号格式。
fn extract_after_prefix<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.strip_prefix(prefix)?.split_whitespace().next()
}

/// 从源码 `offset` 处尽力提取一个标识符（变量名）。
///
/// 用于从 minijinja 运行时错误的字节范围中恢复未定义变量名：
/// debug feature 下错误携带字节范围，通常指向出错的表达式起点
/// （如 `{{ missing }}` 的 `missing`）。无法定位或该处不是标识符时返回 `None`。
pub(crate) fn extract_identifier_at(source: &str, offset: usize) -> Option<String> {
    // 对齐到 UTF-8 字符边界（错误范围可能落在多字节字符中间）
    let mut off = offset.min(source.len());
    while off > 0 && !source.is_char_boundary(off) {
        off -= 1;
    }
    let rest = &source[off..];
    let mut chars = rest.chars();
    let first = chars.next()?;
    if !(first == '_' || first.is_alphabetic()) {
        return None;
    }
    let len = first.len_utf8()
        + chars
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .map(char::len_utf8)
            .sum::<usize>();
    Some(rest[..len].to_string())
}

/// 从源码与错误字节范围中尽力恢复未定义变量的名字。
///
/// minijinja 的 `UndefinedError` 不携带变量名，但 debug feature 下错误携带
/// 字节范围。范围通常覆盖整个出错的表达式（如 `x.missing_attr` 整条属性链），
/// 仅当范围起点恰好是一个**裸标识符**（后面不紧跟 `.` / `[` 等访问符）时才
/// 认定其为缺失变量名 —— 属性链中无法确定缺失的是基础名还是某个属性，
/// 此时返回 `None`（宁缺毋错，避免误导）。
pub(crate) fn extract_undefined_var_name(
    source: &str,
    range: std::ops::Range<usize>,
) -> Option<String> {
    let rest = source.get(range.start..)?;
    let id = extract_identifier_at(rest, 0)?;
    let after = rest[id.len()..].trim_start();
    if after.starts_with('.') || after.starts_with('[') {
        return None;
    }
    Some(id)
}

/// 将 minijinja 错误转换为细分的 [`TplError`]。
///
/// `fallback_name`：当 minijinja 错误未携带模板名时使用的名称。
/// `source`：模板源码，用于语法错误的列号换算（可为 None，此时 col 回退为 1）。
pub(crate) fn from_minijinja_error(
    err: minijinja::Error,
    fallback_name: &str,
    source: Option<&str>,
) -> TplError {
    use minijinja::ErrorKind;
    let name = err.name().unwrap_or(fallback_name).to_string();
    let message = err.to_string();
    let detail = err.detail().unwrap_or("");

    match err.kind() {
        ErrorKind::SyntaxError => {
            let (line, col) = err
                .range()
                .and_then(|range| source.map(|s| line_col_at(s, range.start)))
                .unwrap_or_else(|| (err.line().unwrap_or(1), 1));
            TplError::Parse {
                name,
                message,
                line,
                col,
            }
        }
        ErrorKind::TemplateNotFound => TplError::TemplateNotFound {
            name,
            template: extract_quoted(detail).unwrap_or_default(),
            message,
        },
        ErrorKind::UndefinedError => TplError::UndefinedVariable {
            name,
            // minijinja 的 UndefinedError 不直接携带变量名；debug feature 下
            // 错误携带字节范围（指向出错表达式），尽力从源码恢复变量名。
            // 仅当错误确实发生在所传源码对应的模板（err.name() 与所传名一致）
            // 才恢复：include/extends 期间子模板报错时，其字节范围不适用于
            // 主模板源码，强行恢复会得到同偏移处的无关标识符（宁缺毋错）。
            variable: source
                .and_then(|src| {
                    if err.name() != Some(fallback_name) {
                        return None;
                    }
                    err.range()
                        .and_then(|range| extract_undefined_var_name(src, range))
                })
                .unwrap_or_default(),
            message,
        },
        ErrorKind::UnknownFilter => TplError::UnknownFilter {
            name,
            filter: extract_quoted(detail)
                .or_else(|| extract_after_prefix(detail, "filter ").map(str::to_string))
                .unwrap_or_default(),
            message,
        },
        ErrorKind::UnknownTest => TplError::UnknownTest {
            name,
            test: extract_quoted(detail)
                .or_else(|| extract_after_prefix(detail, "test ").map(str::to_string))
                .unwrap_or_default(),
            message,
        },
        _ => TplError::Render { name, message },
    }
}

/// 由源码字节偏移换算 (行, 列)，均 1 起始；列以**字符**计（与 minijinja AST
/// span 的 `start_col` 口径一致：minijinja lexer 按字符推进列号）。
/// `\n` 视为行分隔符，`\r\n` 中 `\r` 归入行尾。
pub(crate) fn line_col_at(source: &str, byte_offset: usize) -> (usize, usize) {
    let off = byte_offset.min(source.len());
    let mut line = 1usize;
    let mut line_start = 0usize;
    for (i, b) in source.bytes().enumerate() {
        if i >= off {
            break;
        }
        if b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    // 列 = 行首到偏移之间的字符数 + 1（字符口径）；偏移落在字符边界外时
    // get 失败，回退为字节差 + 1（极端场景，避免 panic）
    let col = source
        .get(line_start..off)
        .map_or(off - line_start + 1, |s| s.chars().count() + 1);
    (line, col)
}
