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

/// 写内核错误的**单一分类表**（P1-11）。
///
/// 此前 `WriteError` → 分类/状态码的映射散落 6 处（`CliError::from_write_error`、
/// `From<WriteError>`、`preset::map_write_err`、`templates::map_write_err`、
/// `templates::map_create_err` 的委托臂、HTTP `write_error_resp`），彼此口径
/// 冲突且 `#[non_exhaustive]` 新增变体时 6 处都要手工同步——漏一处即静默错分。
/// 现全部查本表。
///
/// - `cli_kind`：CLI 分类（经 [`CliError::exit_code`] 得退出码）。
/// - `http_status` / `http_kind`：HTTP 响应的状态码与 `error.kind`。
/// - `cli_msg_with_path`：**仅** CLI/stderr 通道允许在消息里带 `path.display()`；
///   HTTP 通道一律脱敏（P1-17 / cli-review P1-3），响应体绝不含绝对路径。
pub(crate) struct WriteErrClass {
    /// CLI 侧 [`CliError::kind`]
    pub(crate) cli_kind: &'static str,
    /// HTTP 状态码
    pub(crate) http_status: u16,
    /// HTTP 侧 `error.kind`
    pub(crate) http_kind: &'static str,
    /// CLI/stderr 消息是否可携带 `path.display()`（HTTP 恒不可）
    pub(crate) cli_msg_with_path: bool,
}

/// 分类查表：`not_found_kind` / `corrupt_kind` 由调用方按上下文传入
/// （preset → `"preset_not_found"` / `"io"`；machine 的 `nctool.toml` →
/// `"machine_not_found"` / `"config"`），不再靠"谁调哪个 From"隐式决定。
///
/// `WriteError` 为 `#[non_exhaustive]`：新增变体**必须**在此显式登记一行。
/// `_` 臂只做**留声**兜底（eprintln 警告 + `io`），绝不静默归类。
/// 已登记的 `LockBusy`（P0-1 跨进程锁）按契约同 `Conflict`
/// （`write_conflict` / 409 / `cli_msg_with_path: true`）—— 两者都可重试。
pub(crate) fn classify_write_error(
    e: &nctool_core::asset::WriteError,
    not_found_kind: &'static str,
    corrupt_kind: &'static str,
) -> WriteErrClass {
    use nctool_core::asset::WriteError;
    match e {
        // 乐观锁冲突：可重试 → 409；退出码 6
        WriteError::Conflict { .. } => WriteErrClass {
            cli_kind: "write_conflict",
            http_status: 409,
            http_kind: "write_conflict",
            cli_msg_with_path: true,
        },
        // 锁争用（P0-1）：另一进程正在写，稍后重试同一操作 → 同 Conflict
        //（409 / write_conflict / 退出码 6），不新增 kind、退出码矩阵零改动
        WriteError::LockBusy { .. } => WriteErrClass {
            cli_kind: "write_conflict",
            http_status: 409,
            http_kind: "write_conflict",
            cli_msg_with_path: true,
        },
        // 重名（upsert/rename 目标已存在）：对用户是"名字不可用" → 409 name_conflict
        WriteError::PathEscape { reason, .. } if reason.contains("已存在") => WriteErrClass {
            cli_kind: "name_conflict",
            http_status: 409,
            http_kind: "name_conflict",
            cli_msg_with_path: false,
        },
        WriteError::PathEscape { .. } => WriteErrClass {
            cli_kind: "args",
            http_status: 400,
            http_kind: "bad_request",
            cli_msg_with_path: false,
        },
        // 目标只读/无权限：本地环境问题，调用方无从修正 → 500；CLI 可带路径
        WriteError::ReadOnly { .. } => WriteErrClass {
            cli_kind: "io",
            http_status: 500,
            http_kind: "internal",
            cli_msg_with_path: true,
        },
        // 条目不存在：调用方问题 → 404，kind 由上下文给出
        WriteError::NotFound(_) => WriteErrClass {
            cli_kind: not_found_kind,
            http_status: 404,
            http_kind: not_found_kind,
            cli_msg_with_path: false,
        },
        // "目标不可用"（如同名目录占位）：preset 归 io(3)、machine 配置归 config(4)；
        // HTTP 一律 500 内部错误（正文不回显细节）
        WriteError::Corrupt(_) => WriteErrClass {
            cli_kind: corrupt_kind,
            http_status: 500,
            http_kind: "internal",
            cli_msg_with_path: false,
        },
        // ERR-NUM-UNDERFLOW：数值正确性问题 → args(2)，HTTP 400 num_underflow
        WriteError::NumUnderflow { .. } => WriteErrClass {
            cli_kind: "args",
            http_status: 400,
            http_kind: "num_underflow",
            cli_msg_with_path: false,
        },
        WriteError::Io(_) => WriteErrClass {
            cli_kind: "io",
            http_status: 500,
            http_kind: "internal",
            cli_msg_with_path: false,
        },
        // 留声兜底：未登记变体不得静默退化（新增变体必须回本表登记）
        #[allow(unreachable_patterns)]
        _ => {
            eprintln!("warning: 未登记的 WriteError 变体，已按 io 兜底：{e}");
            WriteErrClass {
                cli_kind: "io",
                http_status: 500,
                http_kind: "internal",
                cli_msg_with_path: false,
            }
        }
    }
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

    /// 写内核错误 → CLI 错误（**共享映射**，kind/status 查
    /// [`classify_write_error`] 单表，消除 preset / machine / templates 三份漂移）。
    ///
    /// 两个上下文参数由调用方显式传入，不再靠"谁调哪个 From"隐式决定：
    ///
    /// - `not_found_kind`：条目不存在的分类（preset → `"preset_not_found"`；
    ///   machine → `"machine_not_found"`；templates → `"template_not_found"`），
    ///   均归退出码 5。
    /// - `corrupt_kind`："目标不可用"的分类（machine 的 `nctool.toml` 损坏是
    ///   **配置**问题 → `"config"`(4)；preset / templates 是资产 → `"io"`(3)
    ///   ——二者有意不同，见设计 D4）。
    ///
    /// 分类口径（单一来源 = [`classify_write_error`]）：
    ///
    /// - `Conflict` → `write_conflict`(6)
    /// - `LockBusy`（P0-1 锁争用）→ `write_conflict`(6)，与 `Conflict` 同类
    /// - `PathEscape`（reason 含"已存在"）→ `name_conflict`(6)；其余 → `args`(2)
    /// - `ReadOnly` / `Io` / `_` → `io`(3)
    /// - `NotFound` → `not_found_kind`(5)
    /// - `Corrupt` → `corrupt_kind`
    ///
    /// 消息文案按变体逐臂构建；是否携带 `path.display()` 以表的
    /// `cli_msg_with_path` 为准（`NumUnderflow` 为 false：只给行/列/字面量，
    /// 不把文件路径经消息外带）。
    pub fn from_write_error(
        err: nctool_core::asset::WriteError,
        not_found_kind: &'static str,
        corrupt_kind: &'static str,
    ) -> CliError {
        use nctool_core::asset::WriteError;
        let cls = classify_write_error(&err, not_found_kind, corrupt_kind);
        let message = match &err {
            WriteError::Conflict { path, .. } => {
                debug_assert!(cls.cli_msg_with_path, "Conflict 应允许 CLI 消息带路径");
                format!(
                    "写入冲突：{} 已被外部修改，未覆盖。可选：① 重试以当前内容为基线 \
                     ② 放弃 ③ 另存为其它名称",
                    path.display()
                )
            }
            // 锁争用（P0-1）：文案与 Conflict 严格区分 —— 锁争用≠内容被改，
            // 处置提示也不同（稍后重试同一操作，无需以新内容为基线）。
            WriteError::LockBusy { path } => {
                debug_assert!(cls.cli_msg_with_path, "LockBusy 应允许 CLI 消息带路径");
                format!(
                    "文件正被另一个 nctool 进程写入：{}（本次未改动，稍后重试即可）",
                    path.display()
                )
            }
            WriteError::PathEscape { rel, reason } => {
                if reason.contains("已存在") {
                    format!("{reason}：{rel}")
                } else {
                    format!("名称非法：{rel}（{reason}）")
                }
            }
            WriteError::ReadOnly { path } => {
                debug_assert!(cls.cli_msg_with_path, "ReadOnly 应允许 CLI 消息带路径");
                format!("目标只读或无写入权限：{}", path.display())
            }
            WriteError::NotFound(m) | WriteError::Corrupt(m) => m.clone(),
            // ERR-NUM-UNDERFLOW：下溢是**数值正确性**问题，不是"文件损坏"。
            // 归 `args`(2)，与 `--params-file` / `--param` 通道一致（用户在命令行上
            // 处理的是同一类问题），且**不**参与"损坏文件降级"策略。
            // 行/列/字面量足够定位（文件由命令上下文给出），不带路径（表 flag=false）。
            WriteError::NumUnderflow {
                literal,
                line,
                column,
                ..
            } => format!(
                "第 {line} 行第 {column} 列：数值 yaml:{literal} 低于 f64 最小可表示正数\
                 （会被静默变 0，G-code 将产出错误坐标）。请改用可表示的数值。"
            ),
            WriteError::Io(e) => format!("读写失败：{e}"),
            // 未登记变体已由 classify 留声警告；正文保持泛化。
            _ => "读写失败".to_string(),
        };
        CliError::new(cls.cli_kind, message)
    }

    /// 命令失败对应的进程退出码。
    ///
    /// 矩阵：`0` 成功；`1` 参数校验未通过；`2` 参数/用法错误（与 clap 一致）；
    /// `3` IO 失败；`4` 配置错误；`5` 模板/机床/**预设**未找到；`6` 渲染/注册表/写冲突失败
    /// （含 `write_conflict` 乐观锁冲突**与锁争用 `LockBusy`**、`name_conflict` 名称已存在）；
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

// P1-11：`impl From<WriteError> for CliError` 已移出本文件——它无法携带
// `not_found_kind` / `corrupt_kind` 上下文，放在"通用层"只会与
// `from_write_error` 构成两份漂移的口径。现仅存于 `commands::preset`
// （唯一消费者 = `PresetStore::import_presets` 的 `E: From<WriteError>`
// 泛型约束，语义为预设口径）；其它命令族一律显式调用
// `CliError::from_write_error(e, "<上下文>")`，由编译器强制传入上下文。

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
    // P1-11：classify_write_error 单一分类表
    // -----------------------------------------------------------------------

    /// 表把 8 个已登记变体的 CLI kind / HTTP 状态 / HTTP kind / 路径策略全部
    /// 写死；`not_found_kind` / `corrupt_kind` 按上下文传入（同一变体在
    /// preset 与 machine 下允许不同 CLI kind，但 HTTP kind 与状态码一致）。
    /// 新增变体（含后续演进）必须在此同步加一行（见 classify 文档）。
    #[test]
    fn classify_write_error_locks_the_full_table() {
        use nctool_core::asset::WriteError;
        let mk =
            |e: WriteError, nf: &'static str, ck: &'static str| classify_write_error(&e, nf, ck);

        let c = mk(
            WriteError::Conflict {
                path: "p.yaml".into(),
                expected: None,
                actual: None,
            },
            "preset_not_found",
            "io",
        );
        assert_eq!(
            (c.cli_kind, c.http_status, c.http_kind, c.cli_msg_with_path),
            ("write_conflict", 409, "write_conflict", true)
        );

        // 锁争用（P0-1）：与 Conflict 完全同类（409 / write_conflict / 可带路径）
        let c = mk(
            WriteError::LockBusy {
                path: "p.yaml".into(),
            },
            "preset_not_found",
            "io",
        );
        assert_eq!(
            (c.cli_kind, c.http_status, c.http_kind, c.cli_msg_with_path),
            ("write_conflict", 409, "write_conflict", true)
        );

        // 重名：name_conflict / 409；消息（CLI/HTTP）都不带路径
        let c = mk(
            WriteError::PathEscape {
                rel: "x".into(),
                reason: "同名预设已存在".into(),
            },
            "preset_not_found",
            "io",
        );
        assert_eq!(
            (c.cli_kind, c.http_status, c.http_kind, c.cli_msg_with_path),
            ("name_conflict", 409, "name_conflict", false)
        );

        // 其它越界：args / 400 bad_request
        let c = mk(
            WriteError::PathEscape {
                rel: "../x".into(),
                reason: "路径越界".into(),
            },
            "preset_not_found",
            "io",
        );
        assert_eq!(
            (c.cli_kind, c.http_status, c.http_kind),
            ("args", 400, "bad_request")
        );

        let c = mk(
            WriteError::ReadOnly {
                path: "p.yaml".into(),
            },
            "preset_not_found",
            "io",
        );
        assert_eq!(
            (c.cli_kind, c.http_status, c.http_kind, c.cli_msg_with_path),
            ("io", 500, "internal", true)
        );

        // NotFound：CLI kind 由上下文决定，HTTP 跟随且为 404
        let c = mk(
            WriteError::NotFound("没有这个".into()),
            "preset_not_found",
            "io",
        );
        assert_eq!(
            (c.cli_kind, c.http_status, c.http_kind),
            ("preset_not_found", 404, "preset_not_found")
        );
        let c = mk(
            WriteError::NotFound("没有这个".into()),
            "machine_not_found",
            "config",
        );
        assert_eq!(c.cli_kind, "machine_not_found");

        // Corrupt：CLI kind 由 corrupt_kind 决定（preset=io、machine 配置=config）；
        // HTTP 一律 500 internal 且不回显细节
        let c = mk(WriteError::Corrupt("m".into()), "preset_not_found", "io");
        assert_eq!(
            (c.cli_kind, c.http_status, c.http_kind),
            ("io", 500, "internal")
        );
        let c = mk(
            WriteError::Corrupt("m".into()),
            "machine_not_found",
            "config",
        );
        assert_eq!((c.cli_kind, c.http_kind), ("config", "internal"));

        let c = mk(
            WriteError::NumUnderflow {
                path: "p.yaml".into(),
                literal: "1e-400".into(),
                line: 3,
                column: 9,
            },
            "preset_not_found",
            "io",
        );
        assert_eq!(
            (c.cli_kind, c.http_status, c.http_kind, c.cli_msg_with_path),
            ("args", 400, "num_underflow", false)
        );

        let c = mk(
            WriteError::Io(std::io::Error::other("磁盘")),
            "preset_not_found",
            "io",
        );
        assert_eq!(
            (c.cli_kind, c.http_status, c.http_kind),
            ("io", 500, "internal")
        );
    }

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

    /// 阈值分裂对照（SUMMARY §8-9）：`2.4703282292062328e-324` 在 **JSON 侧必须
    /// 归零确认**（`serde_json` 非正确舍入），与 **YAML 侧放过**（`serde_yaml`
    /// 正确舍入，见 `json_num::tests::yaml_confirmation_matches_serde_yaml`）
    /// 形成对照——钉住「两个解析器分别确认」的设计，防止将来合并确认器。
    #[test]
    fn confirm_json_underflow_flags_2470e_324() {
        assert!(
            confirm_json_underflow("2.4703282292062328e-324"),
            "serde_json 把该字面量解析为 0.0 → 必须确认"
        );
        assert!(
            nctool_core::json_num::scan_underflow_candidates(r#"{"x":2.4703282292062328e-324}"#)
                .len()
                == 1,
            "扫描层也应把它提为候选"
        );
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
            ("preset_not_found", 5),
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
        // 未来新增分类必须显式更新退出码表，否则会落到这个 1（"校验未通过"）而不是
        // 默认值 —— 这里锁死行为：新分类必须可测，不能"没有就"的静默通过
        assert_eq!(CliError::new("brand_new_kind", "x").exit_code(), 1);
    }

    /// 锁争用（P0-1）→ CLI 错误：kind 归 `write_conflict`（退出码 6，与乐观锁
    /// 同类），消息带路径供终端用户定位，且与 Conflict 文案严格区分 ——
    /// 锁争用=稍后重试同一操作，冲突=以新内容为基线重试。
    #[test]
    fn lock_busy_maps_to_write_conflict_with_distinct_message() {
        use nctool_core::asset::WriteError;
        let e = CliError::from_write_error(
            WriteError::LockBusy {
                path: std::path::PathBuf::from("C:/data/presets.yaml"),
            },
            "preset_not_found",
            "io",
        );
        assert_eq!(e.kind, "write_conflict");
        assert_eq!(e.exit_code(), 6);
        assert!(
            e.message.contains("另一个 nctool 进程"),
            "应说明锁争用: {}",
            e.message
        );
        assert!(
            e.message.contains("presets.yaml"),
            "CLI/stderr 通道应带路径: {}",
            e.message
        );
        assert!(
            !e.message.contains("外部修改"),
            "锁争用不得复用 Conflict 文案: {}",
            e.message
        );
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
