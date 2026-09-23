//! 统一错误类型与 text/JSON 输出。

use std::fmt;

use nctool_core::pipeline::PipelineError;
use nctool_core::registry::RegistryError;

use crate::cli::FormatArg;

/// 校验报告的 JSON 值（HTTP API 与 `validate --format json` 共用）。
///
/// 形状定义在 [`nctool_core::validate::ValidationReportJson`]（单一来源），
/// 这里只负责转成 `serde_json::Value` 以便嵌进更大的响应体 / 输出对象。
/// 此前两份实现（`server.rs::validation_json` 与 `commands/validate.rs::report_json`）
/// 逐字段各写一遍，是"改一处漏一处"的典型温床。
pub fn report_json(
    template: &str,
    report: &nctool_core::validate::ValidationReport,
) -> serde_json::Value {
    serde_json::to_value(report.json_view(template))
        .expect("校验报告视图结构固定（&str/bool/usize/Vec），序列化不会失败")
}

/// CLI 错误：所有命令失败的统一出口。
///
/// `kind` 为 JSON 输出使用的错误分类；`message` 为人类可读描述。
/// `silent`：命令已自行输出完整错误（如 validate 已打印报告），
/// 仅抑制 JSON 通道的重复错误对象；text 通道的 stderr 提示仍保留。
#[derive(Debug)]
pub struct CliError {
    /// 错误分类标识（JSON 输出用）
    pub kind: &'static str,
    /// 人类可读错误描述
    pub message: String,
    /// 是否抑制 JSON 通道的重复输出（命令已自行输出完整错误）
    pub silent: bool,
}

impl CliError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            silent: false,
        }
    }

    /// 标记为"已输出完整错误"，抑制 JSON 通道的重复错误对象。
    pub fn silent(mut self) -> Self {
        self.silent = true;
        self
    }

    /// 写内核错误 → CLI 错误（**共享映射**，消除 preset / machine 两份漂移）。
    ///
    /// `not_found_kind` 由调用方指定条目不存在的分类
    /// （preset → `"preset_not_found"`；machine → `"machine_not_found"`），
    /// 二者都归退出码 5。
    ///
    /// 分类口径（`WriteError` 为 `#[non_exhaustive]`，未来变体走 `_` 归 `io`）：
    ///
    /// - `Conflict` → `write_conflict`(6)
    /// - `PathEscape`（reason 含"已存在"）→ `name_conflict`(6)；其余 → `args`(2)
    /// - `ReadOnly` / `Io` / `_` → `io`(3)
    /// - `NotFound` → `not_found_kind`(5)
    /// - `Corrupt` → `config`(4)：**配置文件本身**不可用（如 `nctool.toml` 损坏）。
    ///
    /// 注意：预设的 `map_write_err` **有意**把 `Corrupt` 归 `io`(3)（预设是资产、
    /// 非配置），故它只对 `Corrupt` 覆写、其余委托本函数。
    pub fn from_write_error(
        err: nctool_core::asset::WriteError,
        not_found_kind: &'static str,
    ) -> CliError {
        use nctool_core::asset::WriteError;
        match err {
            WriteError::Conflict { path, .. } => CliError::new(
                "write_conflict",
                format!(
                    "写入冲突：{} 已被外部修改，未覆盖。可选：① 重试以当前内容为基线 \
                     ② 放弃 ③ 另存为其它名称",
                    path.display()
                ),
            ),
            WriteError::PathEscape { rel, reason } => {
                // 重名（`rename` 的新名已存在 / upsert 目标已存在）也走这里 ——
                // 对用户是"名字不可用"，与 preset 同语义。
                if reason.contains("已存在") {
                    CliError::new("name_conflict", format!("{reason}：{rel}"))
                } else {
                    CliError::new("args", format!("名称非法：{rel}（{reason}）"))
                }
            }
            WriteError::ReadOnly { path } => {
                CliError::new("io", format!("目标只读或无写入权限：{}", path.display()))
            }
            WriteError::NotFound(m) => CliError::new(not_found_kind, m),
            // 配置文件损坏是**配置**问题（拒绝覆盖），归 `config`(4) —— 与 preset
            // 把损坏预设文件归 io(3) 有意不同（见本函数文档与设计 D4/D7）。
            WriteError::Corrupt(m) => CliError::new("config", m),
            // ERR-NUM-UNDERFLOW：下溢是**数值正确性**问题，不是"文件损坏"。
            // 归 `args`(2)，与 `--params-file` / `--param` 通道一致（用户在命令行上
            // 处理的是同一类问题），且**不**参与"损坏文件降级"策略。
            WriteError::NumUnderflow { .. } => CliError::new("args", err.to_string()),
            WriteError::Io(e) => CliError::new("io", format!("读写失败：{e}")),
            _ => CliError::new("io", "读写失败"),
        }
    }

    /// 命令失败对应的进程退出码。
    ///
    /// 矩阵：`0` 成功；`1` 参数校验未通过；`2` 参数/用法错误（与 clap 一致）；
    /// `3` IO 失败；`4` 配置错误；`5` 模板/机床/**预设**未找到；`6` 渲染/注册表/写冲突失败
    /// （含 `write_conflict` 乐观锁冲突、`name_conflict` 名称已存在）；
    /// `7` 功能尚未实现；未知分类兜底归 `1`。
    ///
    /// `config`(4) 的判据：**配置文件本身**不可用（如 `nctool.toml` 损坏）。
    /// 机床写路径把 `WriteError::Corrupt` 归此码（见 [`CliError::from_write_error`]），
    /// 与预设把损坏文件归 `io`(3) **有意不同**（D4）。
    pub fn exit_code(&self) -> u8 {
        match self.kind {
            "validation" => 1,
            "args" => 2,
            "io" => 3,
            "config" => 4,
            // `preset_not_found` 归这里（而非 2）：语义是"你点的东西不存在"，
            // 与 `template_not_found` 同类。CLI 侧由 `WriteError::NotFound` 产生。
            "template_not_found" | "machine_not_found" | "preset_not_found" => 5,
            "render" | "pipeline" | "registry" | "template_duplicate" | "template_empty"
            | "template_compile" | "write_conflict" | "name_conflict" => 6,
            "not_implemented" => 7,
            _ => 1,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for CliError {}

/// 检测 JSON 文本中的**下溢字面量**并用 `serde_json` **确认**，命中则返回 `CliError`。
///
/// 两段式（ERR-NUM-UNDERFLOW，设计 §3.1 / 附录 D4）：
/// 1. [`nctool_core::json_num::scan_underflow_candidates`] 在 `core` 做文本层**候选提取**
///    （宽松预筛 `|真值| < 2^-1000`，绝不漏报）；
/// 2. 本函数用 **`serde_json` 实测确认**：把候选字面量包成 `{"x":<字面量>}` 解析，
///    得 `0.0` 而预筛已保证十进制真值非零 ⇒ **确认为下溢**。
///
/// **为什么确认必须在 `cli` 侧**：`serde_json` 在 `core` 只是 dev-dependency，
/// `core` 运行时不可调用它（硬约束）；且 JSON 与 YAML 的十进制→f64 行为**不同**
/// （`serde_json` 非正确舍入、`serde_yaml` 正确舍入），**必须各自用自己的解析器确认**。
///
/// `origin` 用于错误消息中的来源标签（如 `参数文件 <path>`）。错误 kind 复用 `"args"`
/// （退出码 2，语义：用户传入的参数值不合法）。**硬失败**，不受 `--lenient` 影响。
///
/// 返回 `None` 表示未发现（或候选经确认均为合法值）——**不**代表 JSON 一定合法，
/// 语法错误仍由调用方原有的 `serde_json` 解析路径报错。
pub fn json_underflow_error(text: &str, origin: &str) -> Option<CliError> {
    for cand in nctool_core::json_num::scan_underflow_candidates(text) {
        if confirm_json_underflow(&cand.literal) {
            return Some(CliError::new(
                "args",
                underflow_message("json", origin, &cand),
            ));
        }
    }
    None
}

/// 用 `serde_json` 实测某字面量是否被**静默归零**（下溢确认）。
///
/// 包一层同格式的壳 `{"x":<字面量>}` 再解析（与 YAML 侧 `x: <字面量>` 同构）。
/// 解析得 `0.0` ⇒ `true`；解析得非零 / 报语法错 ⇒ `false`（语法错交原解析路径报错）。
pub fn confirm_json_underflow(literal: &str) -> bool {
    let probe = format!("{{\"x\":{literal}}}");
    match serde_json::from_str::<serde_json::Value>(&probe) {
        Ok(v) => v.get("x").and_then(serde_json::Value::as_f64) == Some(0.0),
        Err(_) => false,
    }
}

/// 构造下溢错误消息：`<来源> 第 <行> 行第 <列> 列：数值 <前缀>:<字面量> 低于 f64 …`。
///
/// `prefix` 为**载体前缀**（`json` / `yaml` / `cli`）—— 描述用户手上那坨文本的格式，
/// 与实现库名无关（`serde_json` 是库名、用户不认；且换库即失效）。
pub fn underflow_message(
    prefix: &str,
    origin: &str,
    cand: &nctool_core::json_num::UnderflowCandidate,
) -> String {
    format!(
        "{origin} 第 {} 行第 {} 列：数值 {prefix}:{} 低于 f64 最小可表示正数，\
         会被静默当作 0（G-code 将产出错误坐标）。请改用可表示的数值。",
        cand.line, cand.column, cand.literal
    )
}

/// 检测**单个 CLI 字面量**（`--param x=<v>` 的取值）是否下溢，命中则返回 `CliError`。
///
/// 第三通道：`--param` 的取值由 **`str::parse::<f64>`** 解释（非 `serde_json`、非
/// `serde_yaml`）——三者的十进制→f64 舍入行为互不相同，**必须用本通道自己的解析器确认**。
///
/// 两段式：先 [`nctool_core::json_num::scan_underflow_candidates`] 对字面量做候选预筛
/// （`|真值| < 2^-1000`），再由 [`confirm_cli_underflow`] 用 `str::parse` 实测确认。
/// 单字面量的扫描与 JSON 扫描共用同一状态机（`Normal` 态直接读完整 token）。
///
/// `origin` 用于错误消息中的来源标签（如 `参数 --param x`）。kind 复用 `"args"`（退出码 2）。
/// **硬失败**，不受 `--lenient` 影响。返回 `None` ⇒ 该取值不是下溢字面量。
pub fn cli_underflow_error(value: &str, origin: &str) -> Option<CliError> {
    for cand in nctool_core::json_num::scan_underflow_candidates(value) {
        if confirm_cli_underflow(&cand.literal) {
            return Some(CliError::new(
                "args",
                underflow_message("cli", origin, &cand),
            ));
        }
    }
    None
}

/// 用 **`str::parse::<f64>`** 实测某字面量是否被**静默归零**（`--param` 通道的下溢确认）。
///
/// 解析得 `0.0` ⇒ `true`（配合预筛已保证十进制真值非零 ⇒ 确认下溢）；
/// 解析得非零 / 报错 ⇒ `false`。
///
/// **不可复用 [`confirm_json_underflow`]**：`serde_json` 与 `str::parse` 的十进制→f64
/// 舍入不同（`serde_json` 非正确舍入），对边界字面量（如 `2.4703282292062328e-324`）
/// 会给出不同结论；确认必须用调用方**实际使用**的那条解析器。
pub fn confirm_cli_underflow(literal: &str) -> bool {
    matches!(literal.parse::<f64>(), Ok(v) if v == 0.0)
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        CliError::new("io", err.to_string())
    }
}

/// 写内核错误 → CLI 错误。
///
/// **单一来源**：`templates` / `preset`（后续 `machine`）的写路径共用此实现，
/// 避免各命令族各写一份映射导致同一错误在不同命令下退出码不同。
///
/// 分类口径：乐观锁冲突 → `write_conflict`(6)；目标只读 / 一般 IO → `io`(3)；
/// 路径越界与名称非法 → `args`(2)；"目标不可用" → `io`(3)。
/// `WriteError` 为 `#[non_exhaustive]`，新增变体走 `_` 臂归 `io`，
/// 因此**必须**同步检查本函数的分类是否需要细分（编译器不会报错）。
impl From<nctool_core::asset::WriteError> for CliError {
    fn from(err: nctool_core::asset::WriteError) -> Self {
        use nctool_core::asset::WriteError;
        match err {
            WriteError::Conflict { path, .. } => CliError::new(
                "write_conflict",
                format!(
                    "写入冲突：{} 已被外部修改，未覆盖。可选：① 重试以当前内容为基线 \
                     ② 放弃 ③ 另存为其它名称",
                    path.display()
                ),
            ),
            WriteError::PathEscape { rel, reason } => {
                CliError::new("args", format!("路径越界被拒绝：{rel}（{reason}）"))
            }
            WriteError::ReadOnly { path } => {
                CliError::new("io", format!("目标只读或无写入权限：{}", path.display()))
            }
            // 目标不可用（如同名目录占位）≠ "数据损坏"：payload 已自述。
            WriteError::Corrupt(m) => CliError::new("io", m),
            // ERR-NUM-UNDERFLOW：下溢是数值正确性问题 → `args`(2)，与其它 CLI 通道一致。
            WriteError::NumUnderflow { .. } => CliError::new("args", err.to_string()),
            WriteError::Io(e) => CliError::new("io", format!("写入失败：{e}")),
            _ => CliError::new("io", "写入失败"),
        }
    }
}

impl From<RegistryError> for CliError {
    fn from(err: RegistryError) -> Self {
        // RegistryError 为 non_exhaustive：未来新增变体归入 registry 分类
        let kind = match &err {
            RegistryError::NotFound(_) => "template_not_found",
            RegistryError::Duplicate(_) => "template_duplicate",
            RegistryError::EmptySource(_) => "template_empty",
            RegistryError::Compile { .. } => "template_compile",
            RegistryError::Io(_) => "io",
            _ => "registry",
        };
        CliError::new(kind, err.to_string())
    }
}

impl From<PipelineError> for CliError {
    fn from(err: PipelineError) -> Self {
        // PipelineError 为 non_exhaustive：未来新增变体归入 pipeline 分类
        match err {
            PipelineError::TemplateNotFound(name) => {
                CliError::new("template_not_found", format!("模板不存在: {name}"))
            }
            PipelineError::Validation(report) => CliError::new("validation", report.summary()),
            PipelineError::Render(err) => CliError::new("render", err.to_string()),
            PipelineError::Registry(err) => CliError::new("registry", err.to_string()),
            _ => CliError::new("pipeline", err.to_string()),
        }
    }
}

impl From<nctool_tpl::TplError> for CliError {
    fn from(err: nctool_tpl::TplError) -> Self {
        CliError::new("render", err.to_string())
    }
}

/// 结果输出风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStyle {
    Text,
    Json,
}

impl From<&FormatArg> for OutputStyle {
    fn from(f: &FormatArg) -> Self {
        match f {
            FormatArg::Text => OutputStyle::Text,
            FormatArg::Json => OutputStyle::Json,
        }
    }
}

/// 向 stdout 写入文本。
///
/// 断管道（`BrokenPipe`，如 `nctool ... | head`）静默忽略、进程正常退出；
/// 其余写入错误打印到 stderr。避免 `println!` 在管道下游提前关闭时以 panic 收场。
///
/// `pub(crate)`：命令层需要**自行拼装**完整载荷（如 `validate --format json`
/// 的失败包络）时直接调它 —— 否则会退回到 `println!`，而 `println!` 遇到断管道
/// 是 panic（退出码 101），偏离本模块的错误码契约。
pub(crate) fn write_stdout_quiet(text: &str) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    if let Err(e) = lock.write_all(text.as_bytes()) {
        if e.kind() != std::io::ErrorKind::BrokenPipe {
            eprintln!("error: 输出失败: {e}");
        }
    }
}

/// text 通道的成功输出：保证**恰好一个**结尾换行。
///
/// 多补一个换行会让 `$(nctool ...)` 之类的调用多出空行；少补一个则与
/// `println!` 语义不一致——两种都是"看起来没问题"的输出缺陷，故单独成函数
/// 并加测试钉住。
fn text_ok_buf(text: &str) -> String {
    let mut buf = text.to_string();
    if !text.ends_with('\n') {
        buf.push('\n');
    }
    buf
}

/// JSON 通道的失败包络文本（含结尾换行）；`silent` 错误返回 `None`。
///
/// `silent` 表示命令已自行输出完整错误（如 `validate` 已打印报告），
/// JSON 通道不再重复——重复输出会让 `--format json` 的消费方收到两条错误。
fn json_error_text(err: &CliError) -> Option<String> {
    if err.silent {
        return None;
    }
    let obj = serde_json::json!({
        "ok": false,
        "error": { "kind": err.kind, "message": err.message },
    });
    Some(format!(
        "{}\n",
        serde_json::to_string_pretty(&obj).unwrap_or_default()
    ))
}

/// JSON 通道的成功包络文本（含结尾换行）：`{"ok":true,"data":...}`。
fn json_ok_text<T: serde::Serialize>(data: T) -> String {
    let obj = serde_json::json!({ "ok": true, "data": data });
    format!(
        "{}\n",
        serde_json::to_string_pretty(&obj).unwrap_or_default()
    )
}

impl OutputStyle {
    /// 输出错误：text → stderr 单行；json → 结构化错误对象（stdout）。
    ///
    /// `silent` 错误在 JSON 通道不重复输出（命令已自行输出完整错误）。
    pub fn print_error(&self, err: &CliError) {
        match self {
            OutputStyle::Text => {
                eprintln!("error: {}", err.message);
            }
            OutputStyle::Json => {
                if let Some(text) = json_error_text(err) {
                    write_stdout_quiet(&text);
                }
            }
        }
    }

    /// 输出成功结果：text → 原样打印；json → 包一层 `{"ok":true,"data":...}`。
    pub fn print_ok<T: serde::Serialize>(&self, text: &str, data: T) {
        match self {
            OutputStyle::Text => write_stdout_quiet(&text_ok_buf(text)),
            OutputStyle::Json => write_stdout_quiet(&json_ok_text(data)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nctool_core::derive::DeriveError;
    use nctool_core::validate::ValidationReport;
    use nctool_tpl::TplError;

    // -----------------------------------------------------------------------
    // ERR-NUM-UNDERFLOW：各通道确认器
    // -----------------------------------------------------------------------

    #[test]
    fn confirm_json_underflow_true_for_zeroed_literal() {
        // `1e-400` 经 serde_json 静默归零
        assert!(confirm_json_underflow("1e-400"));
    }

    #[test]
    fn confirm_json_underflow_false_for_legal_subnormal() {
        // 最小次正规数不受影响
        assert!(!confirm_json_underflow("5e-324"));
        assert!(!confirm_json_underflow("1e-323"));
    }

    #[test]
    fn confirm_cli_and_json_agree_on_clear_cases() {
        // 明确的零与非零，两条通道结论一致
        assert!(confirm_cli_underflow("1e-400"));
        assert!(!confirm_cli_underflow("5e-324"));
        assert!(!confirm_cli_underflow("1e-323"));
    }

    #[test]
    fn json_underflow_error_reports_prefix_and_location() {
        let err = json_underflow_error(r#"{"x": 1e-400}"#, "请求体").expect("应命中下溢");
        assert_eq!(err.kind, "args");
        assert!(
            err.message.contains("json:1e-400"),
            "应带 json: 前缀与字面量: {}",
            err.message
        );
        assert!(
            err.message.contains("第 1 行"),
            "应带行定位: {}",
            err.message
        );
        assert!(
            err.message.contains("列：数值 json:1e-400"),
            "列定位应紧邻字面量: {}",
            err.message
        );
    }

    #[test]
    fn json_underflow_error_none_for_string_content() {
        // 字符串内容里的 `1e-400` 不是数值 → 0 候选 → 不报错
        assert!(json_underflow_error(r#"{"note": "1e-400"}"#, "请求体").is_none());
    }

    #[test]
    fn cli_underflow_error_reports_cli_prefix() {
        let err = cli_underflow_error("1e-400", "参数 --param x").expect("应命中下溢");
        assert_eq!(err.kind, "args");
        assert!(
            err.message.contains("cli:1e-400"),
            "应带 cli: 前缀: {}",
            err.message
        );
    }

    #[test]
    fn cli_underflow_error_none_for_legal_value() {
        assert!(cli_underflow_error("5e-324", "参数 --param x").is_none());
        assert!(cli_underflow_error("21.5", "参数 --param x").is_none());
    }

    /// 退出码是对外契约（README 有完整矩阵，clap 的用法错误也按 2 走），
    /// 逐个钉住，改动即红。
    #[test]
    fn exit_code_matrix() {
        for (kind, want) in [
            ("validation", 1),
            ("args", 2),
            ("io", 3),
            ("config", 4),
            ("template_not_found", 5),
            ("machine_not_found", 5),
            ("render", 6),
            ("pipeline", 6),
            ("registry", 6),
            ("template_duplicate", 6),
            ("template_empty", 6),
            ("template_compile", 6),
            ("write_conflict", 6),
            ("name_conflict", 6),
            ("not_implemented", 7),
        ] {
            assert_eq!(CliError::new(kind, "x").exit_code(), want, "kind={kind}");
        }
    }

    #[test]
    fn unknown_kind_falls_back_to_1() {
        // 新增分类若忘了进矩阵，退出码会悄悄变成 1（与"校验未通过"撞车）——
        // 这个兜底是有意的，但不能是"没想过"的结果
        assert_eq!(CliError::new("brand_new_kind", "x").exit_code(), 1);
    }

    #[test]
    fn display_is_kind_colon_message() {
        let err = CliError::new("args", "参数格式应为 k=v");
        assert_eq!(err.to_string(), "args: 参数格式应为 k=v");
    }

    #[test]
    fn silent_sets_flag_and_keeps_other_fields() {
        let err = CliError::new("validation", "缺参数").silent();
        assert!(err.silent);
        assert_eq!(err.kind, "validation");
        assert_eq!(err.message, "缺参数");
        // 默认不静默
        assert!(!CliError::new("validation", "缺参数").silent);
    }

    #[test]
    fn io_error_maps_to_io() {
        let err: CliError =
            std::io::Error::new(std::io::ErrorKind::NotFound, "没有这个文件").into();
        assert_eq!(err.kind, "io");
        assert_eq!(err.exit_code(), 3);
        assert!(err.message.contains("没有这个文件"));
    }

    #[test]
    fn registry_errors_map_to_kinds() {
        let cases: Vec<(RegistryError, &str, u8)> = vec![
            (RegistryError::NotFound("t".into()), "template_not_found", 5),
            (
                RegistryError::Duplicate("t".into()),
                "template_duplicate",
                6,
            ),
            (RegistryError::EmptySource("t".into()), "template_empty", 6),
            (
                RegistryError::Compile {
                    name: "t".into(),
                    err: TplError::Render {
                        name: "t".into(),
                        message: "boom".into(),
                    },
                },
                "template_compile",
                6,
            ),
            (RegistryError::Io(std::io::Error::other("读失败")), "io", 3),
        ];
        for (err, kind, code) in cases {
            let mapped: CliError = err.into();
            assert_eq!(mapped.kind, kind);
            assert_eq!(mapped.exit_code(), code, "kind={kind}");
        }
    }

    #[test]
    fn pipeline_errors_map_to_kinds() {
        let cases: Vec<(PipelineError, &str, u8)> = vec![
            (
                PipelineError::TemplateNotFound("t".into()),
                "template_not_found",
                5,
            ),
            (
                PipelineError::Validation(ValidationReport::default()),
                "validation",
                1,
            ),
            (
                PipelineError::Render(TplError::Render {
                    name: "t".into(),
                    message: "boom".into(),
                }),
                "render",
                6,
            ),
            (
                PipelineError::Registry(RegistryError::NotFound("t".into())),
                "registry",
                6,
            ),
            // 兜底分支：新增变体时不会被静默归到别的分类
            (
                PipelineError::Derive(DeriveError::MissingSource {
                    target: "tip".into(),
                    from: "dia".into(),
                }),
                "pipeline",
                6,
            ),
        ];
        for (err, kind, code) in cases {
            let mapped: CliError = err.into();
            assert_eq!(mapped.kind, kind);
            assert_eq!(mapped.exit_code(), code, "kind={kind}");
        }
    }

    #[test]
    fn tpl_error_maps_to_render() {
        let err: CliError = TplError::Render {
            name: "t".into(),
            message: "boom".into(),
        }
        .into();
        assert_eq!(err.kind, "render");
        assert_eq!(err.exit_code(), 6);
    }

    #[test]
    fn output_style_follows_format_arg() {
        assert_eq!(OutputStyle::from(&FormatArg::Text), OutputStyle::Text);
        assert_eq!(OutputStyle::from(&FormatArg::Json), OutputStyle::Json);
    }

    #[test]
    fn text_ok_has_exactly_one_trailing_newline() {
        assert_eq!(text_ok_buf("G0 X0"), "G0 X0\n");
        // 已带换行时不得再补一个（否则多出空行）
        assert_eq!(text_ok_buf("G0 X0\n"), "G0 X0\n");
        assert_eq!(text_ok_buf(""), "\n");
    }

    #[test]
    fn json_error_envelope_shape() {
        let text = json_error_text(&CliError::new("args", "格式错误")).expect("非 silent 应有输出");
        let v: serde_json::Value = serde_json::from_str(&text).expect("应为合法 JSON");
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"]["kind"], "args");
        assert_eq!(v["error"]["message"], "格式错误");
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn silent_error_writes_nothing_in_json() {
        // 命令已自行输出完整错误 → JSON 通道必须**一条都不发**，
        // 否则消费方会收到两条错误对象
        let err = CliError::new("validation", "缺参数").silent();
        assert!(json_error_text(&err).is_none());
    }

    #[test]
    fn json_ok_envelope_shape() {
        let text = json_ok_text(serde_json::json!({ "count": 2 }));
        let v: serde_json::Value = serde_json::from_str(&text).expect("应为合法 JSON");
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["count"], 2);
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn print_paths_do_not_panic() {
        // 这是 CLI 唯一的输出出口，写失败/静默分支都不该 panic。
        // 内容正确性由 cli_e2e 的端到端断言覆盖，这里只守"不炸"。
        // 注意：本测试会把两个 JSON 包络打到**测试进程的 stdout**（`cargo test`
        // 输出里能看到），这是 in-process 测试无法避免的，不是输出串了。
        let err = CliError::new("args", "x");
        OutputStyle::Json.print_error(&err);
        OutputStyle::Json.print_error(&CliError::new("args", "x").silent());
        OutputStyle::Text.print_error(&err);
        OutputStyle::Json.print_ok("", serde_json::json!({}));
        OutputStyle::Text.print_ok("G0 X0", serde_json::json!({}));
    }
}
