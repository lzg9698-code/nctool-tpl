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
/// `Clone` 是刻意的：`nctool-core` 的注册表会**缓存模板的静态分析结果**，
/// 其中失败态需连同 `TplError` 一起留存，避免每次取用都重新解析一遍
/// （`extract_params` 需要返回带行列定位的原始错误，不能只存 Display 文本）。
#[non_exhaustive]
#[derive(Debug, Clone)]
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

/// 沿 `source()` 链收集嵌套错误描述，把被外层包装吞掉的**根因**找回来。
///
/// minijinja 把子模板（`{% include %}` / `{% extends %}` / `{% import %}`）
/// 的错误包成 `BadInclude` / `EvalBlock` 等外层错误，而**外层 `Display` 不含内层原因**：
///
/// ```text
/// could not render include: error in "turning/_undercut_common.j2" (in turning/undercut_fs.j2:24)
/// ```
///
/// 用户据此只知道"某个 include 挂了"，不知道挂在哪一行、为什么挂——实测中这条
/// 消息完全无法定位问题。而 `source()` 链上的内层错误仍带着自己的模板名、行号与
/// 原因（如 `unknown filter: ... (in frag.j2:1)`），因此沿链收集即可恢复可诊断性。
///
/// 返回**从外到内**的每一层描述（不含最外层本身，它已由调用方持有）；最内层
/// 即根本原因。链长度上限 [`MAX_ERROR_CHAIN`]，防御异常实现造成的环。
fn nested_error_chain(err: &minijinja::Error) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(err);
    while let Some(e) = cur {
        if out.len() >= MAX_ERROR_CHAIN {
            break;
        }
        // minijinja 内层错误的 `Display` 自带 `(in <模板>:<行>)` 定位，
        // 直接采用即可，无需再拼模板名与行号（否则重复）。
        out.push(e.to_string());
        cur = e.source();
    }
    out
}

/// 错误链展开的最大层数（防异常 `source()` 实现构成环）。
const MAX_ERROR_CHAIN: usize = 8;

/// 把最外层消息与嵌套链拼成一条可诊断的消息。
///
/// 无嵌套时原样返回；有嵌套时以 ` ← ` 逐层追加，末段即根本原因：
///
/// ```text
/// could not render include: error in "sub.j2" (in main.j2:2) ← unknown filter: ... (in sub.j2:1)
/// ```
fn message_with_root_cause(err: &minijinja::Error, source: Option<&str>, name: &str) -> String {
    let mut message = err.to_string();
    // 类别名式消息（无 detail）补上出错表达式片段
    if let Some(snippet) = opaque_error_snippet(err, source, name) {
        message.push_str(&format!("（出错表达式：{snippet}）"));
    }
    let chain = nested_error_chain(err);
    if !chain.is_empty() {
        message.push_str(&format!(" ← {}", chain.join(" ← ")));
    }
    message
}

/// 对**类别名式**错误（`detail` 为空）补出出错表达式的源码片段。
///
/// minijinja 对部分运行期错误不设置 `detail`，消息退化成
/// `invalid operation (in uz_dj_x.j2:83)`——用户既不知道错在哪一句、也不知道
/// 错的是什么。实测此时 `range()` 仍精确指向出错表达式（如 `-U_A`），
/// 据此可以给出可操作的提示。
///
/// 仅在以下条件同时满足时取值，避免误报：
/// - `detail` 为空（有可读原因时不必再补片段）；
/// - 错误确实发生在所传源码对应的模板（`include`/`extends` 期间子模板报错时，
///   其字节范围不适用于主模板源码，强行取会得到无关片段）；
/// - `range()` 可用且切片非空。
fn opaque_error_snippet(
    err: &minijinja::Error,
    source: Option<&str>,
    fallback_name: &str,
) -> Option<String> {
    if err.detail().is_some_and(|d| !d.is_empty()) {
        return None;
    }
    if err.name() != Some(fallback_name) {
        return None;
    }
    let snippet = source?.get(err.range()?)?.trim();
    if snippet.is_empty() {
        return None;
    }
    Some(truncate_chars(snippet, SNIPPET_MAX_CHARS))
}

/// 出错表达式片段的最大字符数（超出加省略号）。
///
/// 表达式可能很长（整行三元表达式），不截断会让错误消息失去可读性；
/// 按**字符**而非字节截断，避免在多字节字符中间切断。
const SNIPPET_MAX_CHARS: usize = 60;

fn truncate_chars(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
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
    // 消息带上嵌套链的根因与（必要时）出错表达式：外层包装（BadInclude 等）
    // 自身不含原因，只报外层等于让用户无从下手。
    let message = message_with_root_cause(&err, source, fallback_name);
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
