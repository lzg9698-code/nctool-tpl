//! NC postprocessing, extracted from the tested 1.x pipeline.
use crate::model::MachineConfig;
/// 输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// G-code 输出（应用行号/注释/空行等后处理）
    Gcode,
    /// 纯文本输出（仅渲染，不做后处理）
    Text,
}

/// 生成选项。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GenerationOptions {
    /// 输出格式
    pub format: OutputFormat,
    /// 是否生成行号（`N0010 N0020 ...`）
    pub line_numbers: bool,
    /// 行号步进（0 视为 1，即每行都编号）
    pub line_number_step: u32,
    /// 行号上限（超过后不再编号）
    pub max_line_number: u32,
    /// **编号起始值**：第一行编号从 `start + step` 开始，而非从 `step` 开始。
    ///
    /// 单模板渲染时保持 `0`（与历史行为逐字节一致）；多工序拼接时由
    /// `crate::part` 传入上一工序的**末行号**，使 `N0010 / N0020` 跨工序
    /// 连续续编而不是每段重来（E5 走查局限 5.1）。
    ///
    /// 取"上一段末行号"而非"上一段行数 × step"：段内可能有程序号行
    /// （`O1001`）与已有行号行不参与编号，按行数推算会错位。
    ///
    /// **与 `Self::max_line_number` 的关系**：起始值本身不参与上限判断，
    /// 只有实际要写入的 `start + n*step` 才会被上限拦下——上限是"程序里
    /// 能出现多大的行号"，不是"这一段能编多少行"。
    pub line_number_start: u32,
    /// 是否生成头部注释（模板名等）
    pub add_header_comment: bool,
    /// 是否删除空行
    pub strip_blank_lines: bool,
    /// 仅输出 ASCII 字符（非 ASCII 字符替换为 `?`）。
    ///
    /// 许多 CNC 控制器对 G-code 中的非 ASCII 字符（如中文注释）处理不稳定，
    /// 直传机床的场景建议开启。仅对 `OutputFormat::Gcode` 生效，
    /// `OutputFormat::Text` 始终原样输出。
    pub ascii_only: bool,
}

impl Default for GenerationOptions {
    fn default() -> Self {
        Self {
            format: OutputFormat::Gcode,
            line_numbers: false,
            line_number_step: 10,
            max_line_number: 9999,
            line_number_start: 0,
            add_header_comment: false,
            strip_blank_lines: false,
            ascii_only: false,
        }
    }
}

/// 生成结果：正文 + 末行号 + **后处理告警**。
///
/// # 为什么告警要单独回传
///
/// `warnings` 表达"程序已经生成，但**与用户预期不符**"的情形（P2-1：行号撞
/// 上限后停止编号）。它**不能**混进正文 —— 正文是 G-code，往里塞提示会直接
/// 产出非法程序；也**不该**是硬错误 —— 程序本身是完整可用的，只是后半段没有
/// 行号，阻断渲染的代价大于收益。
///
/// 调用方（CLI / HTTP / `part generate`）负责把它呈现给用户：**静默丢弃等于
/// 用户永远不知道自己拿到的是一份"半成品编号"的程序**。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GenerationOutcome {
    /// 生成的正文（G-code 或纯文本）
    pub text: String,
    /// 该段最后一个被写入的行号（未开行号时为起始值）
    pub end_line_number: u32,
    /// 后处理告警（当前只有行号上限/溢出一类）
    pub warnings: Vec<String>,
}

fn sanitize_ascii(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii() { c } else { '?' })
        .collect()
}

/// 后处理：头部注释 / 行号 / 空行清理。
///
/// 行号宽度默认值（机床配置缺 `line_number_digits` 时）。
const DEFAULT_LINE_NUMBER_DIGITS: usize = 4;
/// 行号宽度上限。
///
/// 机床配置的 `line_number_digits` 是用户可编辑的字符串，解析后若无上界，
/// 配置一个 `1000000000` 就会让**每一行**都去分配 GB 级缓冲 —— 而 Rust 的
/// 分配失败是进程 abort，**不可捕获**。
///
/// 取值 32 而非对齐 `nc_pad` 的 `MAX_NC_PAD_WIDTH`（1024），是因为两者的
/// 放大倍数不同：`nc_pad` 只作用于**一行**（程序号），而行号前缀作用于
/// **每一行**，总分配量是 `行数 × 位宽`。1024 位在万行程序上就是 10 MB 的
/// 纯前导零，既无意义又拖慢后处理。32 位已远超任何真实控制器（常见 4–5 位）。
///
/// `pub(crate)`：写层（`asset::machine::preflight`）需要据此**提示**用户
/// "该值将被夹紧"，但**不得复制**夹紧逻辑（AC-2.9）。暴露同一常量即单一来源。
pub(crate) const MAX_LINE_NUMBER_DIGITS: usize = 32;

/// 后处理：行号 / 头部注释 / 空行清理 / ASCII 清洗。
///
/// - **Text 格式**：仅渲染，保留原始行内容（不 trim、不编号、不清理空行、不做 ASCII 清洗）
/// - **Gcode 格式**：可生成行号、清理空行；每行 trim 首尾空白；`ascii_only` 开启时
///   非 ASCII 字符替换为 `?`（头部注释与模板名同样清洗）
///
/// 行号规则：**程序号行**与**已有行号的行**不重复编号；行号达到 `max_line_number`
/// 后不再递增。
///
/// 两个前缀都取自机床配置（`program_prefix` 默认 `O`、`line_number_prefix`
/// 默认 `N`），并且**同时识别小写**首字母（`o` / `n`）：控制器输出常用小写，
/// 只比大写会把 `o1000` 当成普通行而重复编号。配置为空串时回退默认值（空串会让
/// `starts_with("")` 恒真，导致整份程序一行都不编号，见 [`non_empty_config`]）。
///
/// 前缀类配置走 [`non_empty_config`]：空串会让 `starts_with("")` 恒真，
/// 后果与键缺失一样是"整份程序不编号"，必须同等回退默认值。
/// 读取机床配置里的**非空**字符串键：键缺失或值为空串都回退 `fallback`。
///
/// 空串不是"合法的空约定"：`line_number_prefix` / `program_prefix` 都参与
/// `starts_with` 判定，空串会让 `starts_with("")` 恒真 —— 每一行都被判为
/// "程序号行"或"已有行号"，于是 `line_numbers: true` 下**一行都不编号**，
/// 且没有任何告警。这与键缺失的后果完全一致，因此两者必须同等对待。
/// 机床配置是用户可编辑的字符串，这一层防御不能省。
fn non_empty_config<'a>(machine: &'a MachineConfig, key: &str, fallback: &'a str) -> &'a str {
    machine
        .get(key)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
}

/// 后处理：行号 / 头部注释 / 空行清理 / ASCII 清洗。
///
/// 返回 `(G-code 文本, 末行号)`。第二个值是**跨工序续编的接口**
/// （见 [`GenerationOptions::line_number_start`]）：调用方把上一段的末行号
/// 传给下一段即可实现连续编号。未开行号或一行都未编号时返回传入的
/// 起始值本身，语义是"编号游标未前进"。
///
/// 之所以由本函数回传末行号而不是让调用方自己算：**只有遍历过每一行的
/// 本函数知道哪些行真的被编了号**（程序号行 `O1001`、已有行号行、空行都
/// 跳过），按"段内行数 × step"推算必然错位。把游标交还调用方，是唯一
/// 不会与编号逻辑脱节的做法。
pub fn postprocess(
    rendered: &str,
    template: &str,
    opts: &GenerationOptions,
    machine: &MachineConfig,
) -> GenerationOutcome {
    let mut out = String::new();

    // 头部注释（两种格式均生效，由用户显式开启）；文本为 ASCII，
    // 模板名若含非 ASCII 字符且开启 ascii_only 时同样被清洗
    if opts.add_header_comment {
        let header = format!(
            "( ================================== )\n( nctool generated G-code )\n( template: {} )\n( ================================== )\n",
            template
        );
        if opts.ascii_only {
            out.push_str(&sanitize_ascii(&header));
        } else {
            out.push_str(&header);
        }
    }

    // 用 match 而非 `if == Text`：新增输出格式时编译器强制在此表态，
    // 不会静默落到 G-code 后处理（加行号 / 清 ASCII）里产出错误程序。
    match opts.format {
        // Text 格式：仅渲染，不做任何后处理（也不推进编号游标）
        OutputFormat::Text => {
            out.push_str(rendered);
            return GenerationOutcome {
                text: out,
                end_line_number: opts.line_number_start,
                warnings: Vec::new(),
            };
        }
        OutputFormat::Gcode => {}
    }

    // Gcode 格式：行号 + 空行清理 + trim + 可选 ASCII 清洗。
    // 行号前缀/宽度与程序号前缀来自机床配置（generic 默认 N / 4 / O），
    // 实现"换机床即换编程约定"；键缺失时回退默认值。
    let line_prefix = non_empty_config(machine, "line_number_prefix", "N");
    // 宽度夹在 [1, MAX_LINE_NUMBER_DIGITS]：机床配置是用户可编辑的字符串，
    // 缺失下界会产出无内容的行号，缺失上界则一行就能触发 GB 级分配
    // （分配失败是进程 abort，不是可捕获错误）。
    let line_digits = machine
        .get("line_number_digits")
        .and_then(|d| d.parse::<usize>().ok())
        .unwrap_or(DEFAULT_LINE_NUMBER_DIGITS)
        .clamp(1, MAX_LINE_NUMBER_DIGITS);
    let program_prefix = non_empty_config(machine, "program_prefix", "O");
    // step=0 视为 1：否则行号原地不动，产出重复的 N0000 行
    let step = opts.line_number_step.max(1);
    // 起始值：多工序续编用。单模板渲染时为 0，行为与历史一致。
    let mut line_no: u32 = opts.line_number_start;
    // 【P2-1】撞上限 / 溢出后**静默停止编号**是最坏的一类失败：产物看起来完全
    // 正常（每行都还在），只是后半段没有行号 —— 用户从产物里看不出来。这里
    // 记下**第一次**发生的位置，循环结束后转成一条告警交给调用方呈现。
    // 只记第一次：第 2 行撞上限后第 3、4…行都会撞，逐行报会把报告刷爆。
    let mut capped_after: Option<u32> = None;
    let mut overflowed = false;
    for line in rendered.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if opts.strip_blank_lines {
                continue;
            }
            out.push('\n');
            continue;
        }
        let line_buf;
        let content: &str = if opts.ascii_only {
            line_buf = sanitize_ascii(trimmed);
            &line_buf
        } else {
            trimmed
        };
        if opts.line_numbers {
            let is_program = trimmed.starts_with(program_prefix)
                || (program_prefix == "O" && trimmed.starts_with('o'));
            let already_numbered = trimmed.starts_with(line_prefix)
                || (line_prefix == "N" && trimmed.starts_with('n'));
            if !is_program && !already_numbered {
                // checked_add：防止 line_no + step 溢出（debug 构建 panic / release 回绕）
                match line_no.checked_add(step) {
                    Some(next) if next <= opts.max_line_number => {
                        line_no = next;
                        out.push_str(&format!(
                            "{line_prefix}{:0width$} ",
                            line_no,
                            width = line_digits
                        ));
                    }
                    // 撞上限：该行及之后都不再编号，但**内容照常写出**
                    Some(_) => {
                        if capped_after.is_none() {
                            capped_after = Some(line_no);
                        }
                    }
                    None => overflowed = true,
                }
            }
        }
        out.push_str(content);
        out.push('\n');
    }

    let mut warnings = Vec::new();
    if let Some(at) = capped_after {
        warnings.push(format!(
            "行号在 N{at} 之后达到上限 {}，后续行不再编号：程序已生成，但后半段没有行号（提高 --max-line 或减小 --line-step 可避免）",
            opts.max_line_number
        ));
    }
    if overflowed {
        warnings.push(format!(
            "行号累加溢出 u32 上限（当前 N{line_no} + step {step}），后续行不再编号：请减小 --line-step 或降低起始行号"
        ));
    }
    GenerationOutcome {
        text: out,
        end_line_number: line_no,
        warnings,
    }
}

#[cfg(test)]
mod regressions {
    use super::*;
    fn machine() -> MachineConfig {
        MachineConfig {
            id: "example".into(),
            vendor: "Example".into(),
            model: "Virtual".into(),
            config: Default::default(),
        }
    }
    #[test]
    fn skips_existing_and_program_numbers_and_warns_once_at_cap() {
        let options = GenerationOptions {
            line_numbers: true,
            max_line_number: 20,
            ..Default::default()
        };
        let output = postprocess(
            "o0001\nN0005 G0 X0\nG1 X1\nG1 X2\nG1 X3\nG1 X4",
            "example",
            &options,
            &machine(),
        );
        assert_eq!(
            output.text,
            "o0001\nN0005 G0 X0\nN0010 G1 X1\nN0020 G1 X2\nG1 X3\nG1 X4\n"
        );
        assert_eq!(output.end_line_number, 20);
        assert_eq!(output.warnings.len(), 1);
    }
    #[test]
    fn custom_prefixes_and_width_are_respected_and_bounded() {
        let mut machine = machine();
        machine
            .config
            .insert("line_number_prefix".into(), "L".into());
        machine.config.insert("program_prefix".into(), "P".into());
        machine
            .config
            .insert("line_number_digits".into(), "999999999".into());
        let output = postprocess(
            "P1\nG0 X0",
            "example",
            &GenerationOptions {
                line_numbers: true,
                ..Default::default()
            },
            &machine,
        );
        assert!(output.text.starts_with("P1\nL"));
        assert!(output.text.len() < 80);
    }
    #[test]
    fn ascii_cleaning_blank_lines_and_headers_are_separate_from_diagnostics() {
        let output = postprocess(
            "\nG0 X1 (测试)\n\n",
            "例子",
            &GenerationOptions {
                ascii_only: true,
                strip_blank_lines: true,
                add_header_comment: true,
                ..Default::default()
            },
            &machine(),
        );
        assert!(output.text.is_ascii());
        assert!(output.text.contains("G0 X1 (??)"));
        assert!(!output.text.contains("\n\n"));
        assert!(output.warnings.is_empty());
    }
    #[test]
    fn cursor_overflow_warns_and_zero_step_advances() {
        let output = postprocess(
            "G0 X0\nG1 X1",
            "example",
            &GenerationOptions {
                line_numbers: true,
                line_number_start: u32::MAX,
                line_number_step: 1,
                max_line_number: u32::MAX,
                ..Default::default()
            },
            &machine(),
        );
        assert_eq!(output.warnings.len(), 1);
        assert_eq!(output.end_line_number, u32::MAX);
        let output = postprocess(
            "G0 X0",
            "example",
            &GenerationOptions {
                line_numbers: true,
                line_number_step: 0,
                ..Default::default()
            },
            &machine(),
        );
        assert_eq!(output.end_line_number, 1);
    }
}
