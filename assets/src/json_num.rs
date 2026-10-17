//! Decimal precision checks before parsing JSON floating-point values.
//! Candidates are screened using decimal arithmetic, then confirmed with the actual
//! parser. Quoted text is skipped and legal subnormal values are retained.

use std::cmp::Ordering;
use std::sync::OnceLock;

/// 单个十进制字面量，拆为 `(有效数字, 十进制指数)`。
///
/// 语义：`value = ± sig_as_integer × 10^exp`。
/// `sig` 为**大端**数字串（每元素是 `0..=9` 的十进制位），不含前导零、不含末尾零；
/// `sig` 为空表示**真值 0**（与 `exp` 无关）。
///
/// 例：`100000e-405` → `sig = [1]`, `exp = -400`；`0.001` → `sig = [1]`, `exp = -3`。
///
/// 私有类型，不导出。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Decimal {
    /// 有效数字（大端，无前导零/末尾零）。空表示真值 0。
    sig: Vec<u8>,
    /// 十进制指数（`sig` 整体乘以 `10^exp`）。
    exp: i64,
}

impl Decimal {
    /// 真值 0 的规范化表示。
    fn zero() -> Decimal {
        Decimal {
            sig: Vec::new(),
            exp: 0,
        }
    }

    /// 是否为真值 0（`sig` 空）。
    fn is_zero(&self) -> bool {
        self.sig.is_empty()
    }
}

/// 把一个十进制字面量解析为 [`Decimal`]，并做规范化。
///
/// 接受的语法（**比 JSON 宽松**，因为本函数只服务于真值预筛）：
/// 可选前导 `+`/`-`、整数部分、可选小数部分、可选 `e`/`E` 指数（指数可带符号与前导零）。
///
/// 返回 `None` 的情形（**任何尾随垃圾都拒绝**）：
/// `1e-400e`、`1.`、`1e`、`1e+`、空串、纯符号等 —— 这些多为非法数字，本模块**不越权**
/// 报语法错，交给调用方的解析器兜底。
///
/// 关于**前导零整数** `01`：JSON 数字语法**不允许**前导零，解析器会以语法错误拒绝。
/// 本函数对 `01` 返回 `None`（不是候选），从而**放给解析器报错**，符合「扫描器只在语法
/// 合法时才需要正确性」的设计原则（§2.3）。
///
/// 指数部分用 `i64` + **saturating** 运算累加，超长指数（如 `1e-99999999999999999999`）
/// 饱和到极值；配合 `sig` 判据仍正确（真值为 0 时 `sig` 空直接排除；真值非零且指数
/// 饱和为极负 → 数量级极小 → 进候选，确认步骤再判）。
fn parse_dec(s: &str) -> Option<Decimal> {
    let bytes = s.as_bytes();
    let mut i: usize = 0;

    // 1. 消费可选前导 '+'/'-'。符号不影响绝对值预筛，直接跳过。
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }

    // 2. 整数部分 + 小数部分。
    let mut digits: Vec<u8> = Vec::new();
    let mut frac_len: i64 = 0;
    let mut saw_digit: bool = false;
    let mut saw_dot: bool = false;
    let mut int_len: usize = 0; // 小数点前的数字个数（判前导零用）
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_digit() {
            digits.push(b - b'0');
            if saw_dot {
                frac_len += 1;
            } else {
                int_len += 1;
            }
            saw_digit = true;
            i += 1;
        } else if b == b'.' && !saw_dot {
            saw_dot = true;
            i += 1;
        } else {
            break;
        }
    }
    // 一个数字都没见（空串、纯符号、以 '.' 开头）→ 拒绝。
    if !saw_digit {
        return None;
    }
    // 数字语法不允许前导零（`01` / `012`）：**不越权**，返回 `None` 交给解析器报错。
    if int_len > 1 && digits[0] == 0 {
        return None;
    }

    // 3. 可选指数部分。
    let mut exp_part: i64 = 0;
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        let mut exp_neg: bool = false;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            exp_neg = bytes[i] == b'-';
            i += 1;
        }
        let mut saw_exp_digit: bool = false;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            let d = (bytes[i] - b'0') as i64;
            // saturating：超长指数饱和到 i64 极值，不失正确性（见函数注释）。
            exp_part = exp_part.saturating_mul(10).saturating_add(d);
            saw_exp_digit = true;
            i += 1;
        }
        // `1e` / `1e+` 等：指数标记后无数字 → 拒绝。
        if !saw_exp_digit {
            return None;
        }
        if exp_neg {
            // `i64::MIN` 取负会溢出，特殊处理为极正（真值极小，仍进候选）。
            exp_part = if exp_part == i64::MIN {
                i64::MAX
            } else {
                exp_part.saturating_neg()
            };
        }
    }

    // 4. 尾随垃圾 → 拒绝（例如 `1e-400e`、`1.2.3`、`12abc`）。
    if i != bytes.len() {
        return None;
    }
    // 数字语法不允许「小数点后无数字」（`1.`）：`saw_dot` 且末字符是 `.` → 非法。
    if saw_dot && bytes.last() == Some(&b'.') {
        return None;
    }

    // 5. 去掉前导零；若全为 0 → 真值 0（空 sig）。
    let first_nonzero = digits.iter().position(|&d| d != 0);
    let mut sig: Vec<u8> = match first_nonzero {
        Some(pos) => digits[pos..].to_vec(),
        None => return Some(Decimal::zero()),
    };

    // 6. 去掉末尾零，每个累进 trailing（末尾零可提为指数）。
    let mut trailing: i64 = 0;
    while let Some(&last) = sig.last() {
        if last == 0 {
            sig.pop();
            trailing += 1;
        } else {
            break;
        }
    }

    // 7. 组合指数：`exp = 指数部分 - 小数位数 + 末尾零数`。
    //    `1.23` → sig=[1,2,3], exp=0-2+0=-2 ✓
    //    `100000e-405` → sig=[1], exp=-405-0+5=-400 ✓
    let exp = exp_part.saturating_sub(frac_len).saturating_add(trailing);
    Some(Decimal { sig, exp })
}

/// 比较两个**非负** [`Decimal`] 的绝对值大小。
///
/// 算法：先比**数量级** `sig.len() + exp`；数量级不同则大者大（`sig` 均已规范化）；
/// 数量级相同再逐位比较 `sig`（缺位补 0），首个不同位决定结果。
///
/// 前置条件：两个 `Decimal` 均为**非负值表示**（`parse_dec` 已丢弃符号），且 `sig`
/// 已规范化（无前导零/末尾零），保证数量级比较的正确性。
fn cmp_abs(a: &Decimal, b: &Decimal) -> Ordering {
    // 真值 0（空 sig）比任何非零值小；两个都空则相等。
    match (a.is_zero(), b.is_zero()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        (false, false) => {}
    }

    let oa = a.sig.len() as i128 + a.exp as i128;
    let ob = b.sig.len() as i128 + b.exp as i128;
    if oa != ob {
        return oa.cmp(&ob);
    }

    // 数量级相同：逐位比较，缺位（超出长度）补 0。
    let n = a.sig.len().max(b.sig.len());
    for k in 0..n {
        let da = a.sig.get(k).copied().unwrap_or(0);
        let db = b.sig.get(k).copied().unwrap_or(0);
        match da.cmp(&db) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// 预筛阈值 `2^-1000`（约 `9.33e-302`）的十进制表示（[`OnceLock`] 缓存）。
///
/// `2^-1000 = 5^1000 / 10^1000`。用运行时 [`pow5`] 生成十进制数字串（决策点 D3），
/// 再去尾零折进指数。**此常量只用于预筛**（`|真值| < 2^-1000` 即列为候选），
/// **不是裁决阈值**——裁决由调用方用实际解析器完成（附录 D4）。
///
/// 保留 `pow5` 而非硬编码，是为了「少一份手抄常量、无漂移风险」；单测断言其规范化
/// 结果的数量级与手工校验值，防算法写错。
fn prescreen_threshold() -> &'static Decimal {
    static THRESHOLD: OnceLock<Decimal> = OnceLock::new();
    THRESHOLD.get_or_init(|| {
        // 分子 5^1000：大端数字串，无前导零。
        let mut digits = pow5(1000);
        // 分母 10^1000 → 初始指数 -1000；再去尾零折进指数。
        let mut exp: i64 = -1000;
        while let Some(&last) = digits.last() {
            if last == 0 {
                digits.pop();
                exp += 1;
            } else {
                break;
            }
        }
        Decimal { sig: digits, exp }
    })
}

/// 计算 `5^e` 的十进制数字串（`e >= 0`），返回**大端**数字（无前导零）。
///
/// 用 `Vec<u8>` 低位在前做「× 5 带进位」的大数乘法，最后翻转为大端。
fn pow5(e: u32) -> Vec<u8> {
    let mut le: Vec<u8> = vec![1]; // 低位在前；`5^0 = 1`。
    for _ in 0..e {
        let mut carry: u8 = 0;
        for d in le.iter_mut() {
            let v = *d * 5 + carry;
            *d = v % 10;
            carry = v / 10;
        }
        while carry > 0 {
            le.push(carry % 10);
            carry /= 10;
        }
    }
    le.reverse();
    le
}

/// 下溢**候选**字面量及其定位信息。
///
/// **名字用 `Candidate` 而非 `Literal`**：明确传递「这**不是**定论」的语义 —— 调用方
/// **必须**再用该格式的实际解析器确认（见模块文档）。确认得 `0.0` 才是真下溢。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnderflowCandidate {
    /// 原始字面量文本（如 `1e-400`），调用方回灌给解析器确认时使用；未做任何改写。
    pub literal: String,
    /// 字面量在文本中的**字节**起始偏移。
    pub byte_offset: usize,
    /// 展示用的 **1-based** 行号（从原文现算）。
    pub line: usize,
    /// 展示用的 **1-based** 列号（按**字符**计，非字节）。
    pub column: usize,
}

/// JSON integer literals outside the supported i64/u64 range, excluding quoted text.
pub fn scan_json_integer_overflows(json: &str) -> Vec<UnderflowCandidate> {
    scan_number_tokens(json, false)
}

/// 扫描 JSON 文本，返回**全部**下溢候选字面量（按出现顺序）。
///
/// 候选标准（**宽松预筛，非裁决**）：字面量语法上是数字、十进制真值非零、且
/// `|真值| < 2^-1000`（约 `9.33e-302`）。
///
/// **候选 ≠ 下溢**：预筛只排除「十进制真值恰为 0」的数字（`0` / `0.0` / `0e-400`）。
/// 合法次正规数如 `5e-324`（真值 `2^-1074 ≈ 4.94e-324`）、`1e-323` 的**真值非零且
/// `< 2^-1000`** → **它们也是候选**，是否判违规由**确认环节**（实际解析器实测得 `0.0`
/// 才是下溢）裁决 —— 确认会放过它们。切勿把「是候选」误读为「判下溢」。
///
/// **返回值是「候选」而非「定论」**：调用方**必须**再用实际解析器确认 ——
/// 用 `serde_json::from_str::<f64>(&format!("{{\"x\":{}}}", cand.literal))` 解析，
/// 若得 `0.0` 而预筛已保证十进制真值非零，才是真下溢。
/// （`2^-1000` 远高于任何解析器的归零边界，保证**不漏候选**；同时远小于任何正常工艺
/// 参数，**不把正常值送进确认**。宽松只会多跑几次确认，对误判零成本。）
///
/// **返回 `Vec` 而非只返回第一个**：确认是调用方的开销，一次跑完避免反复扫描同一文本；
/// 且调用方常需在错误消息里报告**所有**问题字面量。空 `Vec` 表示无候选
/// （**不**代表 JSON 一定合法 —— 语法错误由调用方的解析器负责报错）。
///
/// 只扫描数字字面量位置：字符串内容（含 `\"`、`\\`、`\n` 等转义）与**键名**
/// 一律整体跳过，故 `{"note":"1e-400"}` 返回空。
///
/// # 示例
///
/// ```
/// use nctool_assets::json_num::scan_underflow_candidates;
///
/// let cands = scan_underflow_candidates(r#"{"a":1e-400,"b":2,"c":3e-500}"#);
/// assert_eq!(cands.len(), 2);
/// assert_eq!(cands[0].literal, "1e-400");
/// assert_eq!(cands[1].literal, "3e-500");
/// // 普通数值（真值 >= 2^-1000）不是候选。
/// assert!(scan_underflow_candidates(r#"{"x":1e-3,"y":123.456}"#).is_empty());
/// // `5e-324`（最小次正规数）**是**候选：其十进制真值非零且 < 2^-1000。
/// // 「是候选」≠「判下溢」——它由确认环节（`serde_json` 实测得非零）放过。
/// let sub = scan_underflow_candidates(r#"{"x":5e-324}"#);
/// assert_eq!(sub.len(), 1);
/// assert_eq!(sub[0].literal, "5e-324");
/// ```
pub fn scan_underflow_candidates(json: &str) -> Vec<UnderflowCandidate> {
    scan_number_tokens(json, true)
}

/// 扫描器状态（Normal / InString / InStringEscape 三态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    /// 在数字字面量位置抓取 token。
    Normal,
    /// 在双引号字符串内。
    InString,
    /// 刚吃掉字符串内的一个反斜杠，吞掉下一个转义字节。
    InStringEscape,
}

/// 扫描骨架：在文本中找出全部「数字字面量候选」。
///
/// 状态机：Normal / InString / InStringEscape 三态（§2）。字符串内容（含 `\"`、
/// `\\`、`\n` 等转义）与键名一律整体跳过。
fn scan_number_tokens(text: &str, only_underflow: bool) -> Vec<UnderflowCandidate> {
    let bytes = text.as_bytes();
    let mut out: Vec<UnderflowCandidate> = Vec::new();
    let (mut position, mut line, mut column) = (0, 1, 1);
    let mut i: usize = 0;
    let mut state = ScanState::Normal;

    while i < bytes.len() {
        match state {
            ScanState::Normal => {
                let b = bytes[i];
                if b == b'"' {
                    // 进入双引号字符串（键名与内容一律跳过）。
                    state = ScanState::InString;
                    i += 1;
                } else if b == b'-' || b.is_ascii_digit() {
                    // 数字 token：贪婪吃 `[0-9.eE+-]`。
                    let start = i;
                    i += 1;
                    while i < bytes.len() {
                        let c = bytes[i];
                        if c.is_ascii_digit()
                            || c == b'.'
                            || c == b'e'
                            || c == b'E'
                            || c == b'+'
                            || c == b'-'
                        {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    let token = &text[start..i];
                    let overflow = !only_underflow
                        && !token.contains(['.', 'e', 'E'])
                        && if token.starts_with('-') {
                            token.parse::<i64>().is_err()
                        } else {
                            token.parse::<u64>().is_err()
                        };
                    if overflow || only_underflow && is_prescreen_candidate(token) {
                        // Advance coordinates once across each prefix, preserving Unicode columns.
                        for ch in text[position..start].chars() {
                            if ch == '\n' {
                                line += 1;
                                column = 1;
                            } else {
                                column += 1;
                            }
                        }
                        position = start;
                        out.push(UnderflowCandidate {
                            literal: token.to_string(),
                            byte_offset: start,
                            line,
                            column,
                        });
                    }
                } else {
                    i += 1;
                }
            }
            ScanState::InString => {
                let b = bytes[i];
                if b == b'\\' {
                    state = ScanState::InStringEscape;
                    i += 1;
                } else if b == b'"' {
                    state = ScanState::Normal;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            ScanState::InStringEscape => {
                // 转义为 `\X` 两字符 → 只吞 1 字节。
                state = ScanState::InString;
                i += 1;
            }
        }
    }
    out
}

/// 单个数字 token 是否为**预筛候选**：真值非零 且 `|真值| < 2^-1000`。
///
/// **这不是最终裁决**——候选仍须由调用方用实际解析器确认（模块文档 / 附录 D4）。
/// token 非法（`parse_dec` 返回 `None`）时保守返回 `false`（不是候选，交给解析器报语法错）。
fn is_prescreen_candidate(token: &str) -> bool {
    match parse_dec(token) {
        Some(d) => !d.is_zero() && cmp_abs(&d, prescreen_threshold()) == Ordering::Less,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ───────────────────────── 测试辅助 ─────────────────────────

    /// 手工构造 `Decimal`（测试用）。
    fn dec(sig: &[u8], exp: i64) -> Decimal {
        Decimal {
            sig: sig.to_vec(),
            exp,
        }
    }

    /// `1eN` 的标准形式：`sig=[1]`, `exp=N`。
    fn one_e(n: i64) -> Decimal {
        dec(&[1], n)
    }

    /// 校验 `parse_dec` 的规范化结果。
    fn assert_parse(s: &str, sig: &[u8], exp: i64) {
        assert_eq!(parse_dec(s), Some(dec(sig, exp)), "解析 {s:?} 失败");
    }

    /// 生产预筛入口（供各用例复用）。
    fn is_cand(s: &str) -> bool {
        is_prescreen_candidate(s)
    }

    /// JSON 扫描命中数。
    fn hits(json: &str) -> usize {
        scan_underflow_candidates(json).len()
    }

    /// 取第一个候选（断言必然存在）。
    fn first(json: &str) -> UnderflowCandidate {
        scan_underflow_candidates(json)
            .into_iter()
            .next()
            .expect("应有候选")
    }

    // ───────────────────────── 预筛常量 ─────────────────────────

    /// `prescreen_threshold()` 必须等于 `2^-1000`（≈ `9.33e-302`）的真值。
    #[test]
    fn threshold_is_pow5_1000() {
        let t = prescreen_threshold();
        // 规范化：无前导零、无末尾零。
        assert_ne!(t.sig.first(), Some(&0), "出现前导零");
        assert_ne!(t.sig.last(), Some(&0), "末尾零未被剥离");
        assert!(!t.sig.is_empty());
        // `2^-1000 ≈ 9.3326e-302`，最高有效位阶 = -302（`sig.len()+exp == -301`）。
        assert_eq!(t.sig.len() as i128 + t.exp as i128, -301);
        // 与 `python3 decimal` 核对：`2^-1000` 的十进制前 12 位。
        assert_eq!(&t.sig[..12], &[9, 3, 3, 2, 6, 3, 6, 1, 8, 5, 0, 3]);

        // 交叉验证：`1e-300`（真值 > 2^-1000）不是候选；`1e-350`（<）是。
        assert!(!is_cand("1e-300"), "1e-300 > 2^-1000，不是候选");
        assert!(is_cand("1e-350"), "1e-350 < 2^-1000，是候选");
    }

    /// `pow5` 的小规模正确性（手工可核对）：`5^0..5^6`。
    #[test]
    fn pow5_small_values() {
        assert_eq!(pow5(0), vec![1]);
        assert_eq!(pow5(1), vec![5]);
        assert_eq!(pow5(2), vec![2, 5]);
        assert_eq!(pow5(3), vec![1, 2, 5]);
        assert_eq!(pow5(4), vec![6, 2, 5]);
        assert_eq!(pow5(5), vec![3, 1, 2, 5]);
        assert_eq!(pow5(6), vec![1, 5, 6, 2, 5]);
    }

    /// `OnceLock` 缓存：多次调用返回同一引用。
    #[test]
    fn threshold_is_cached() {
        let a = prescreen_threshold();
        let b = prescreen_threshold();
        assert!(std::ptr::eq(a, b), "prescreen_threshold() 未复用 OnceLock");
    }

    // ───────────────────────── parse_dec 规范化 ─────────────────────────

    #[test]
    fn parse_dec_normalizes_forms() {
        assert_parse("1", &[1], 0);
        assert_parse("123.456", &[1, 2, 3, 4, 5, 6], -3);
        assert_parse("1e5", &[1], 5);
        assert_parse("1e-400", &[1], -400);
        assert_parse("100000e-405", &[1], -400); // 尾数挪小数点
        assert_parse("1e-0350", &[1], -350); // 前导零指数
        assert_parse("1e-0000000400", &[1], -400);
        assert_parse("1E-400", &[1], -400); // 大写 E
        let no_exp = format!("0.{}1", "0".repeat(324));
        assert_parse(&no_exp, &[1], -325); // 无指数形式
        assert_parse("1200", &[1, 2], 2); // 末尾零提指数
        assert_parse("0.001", &[1], -3);
    }

    #[test]
    fn parse_dec_zero_is_empty_sig() {
        for s in [
            "0",
            "0.0",
            "-0.0",
            "0e0",
            "0e-400",
            "0.0000e-400",
            "0.000000000",
            "+0",
            "-0",
        ] {
            let d = parse_dec(s).unwrap_or_else(|| panic!("{s:?} 应可解析"));
            assert!(d.is_zero(), "{s:?} 真值应为 0，得 {d:?}");
        }
    }

    #[test]
    fn parse_dec_rejects_invalid_tokens() {
        for s in [
            "1e-400e", "01", "012", "1.", "1e", "1e+", "", "+", "-", ".", "1.2.3", "12abc", "1e-x",
        ] {
            assert!(parse_dec(s).is_none(), "{s:?} 应被拒绝");
        }
        // `0.5` 的前导零在小数点后，合法。
        assert!(parse_dec("0.5").is_some());
        assert!(parse_dec("0").is_some());
    }

    #[test]
    fn parse_dec_saturates_huge_exponent() {
        let d = parse_dec("1e99999999999999999999").unwrap();
        assert_eq!(d.sig, vec![1]);
        assert_eq!(d.exp, i64::MAX);
        let d = parse_dec("1e-99999999999999999999").unwrap();
        assert_eq!(d.sig, vec![1]);
        assert!(d.exp <= -i64::MAX, "负指数应饱和到极负，得 {}", d.exp);
        let d = parse_dec("0e-99999999999999999999").unwrap();
        assert!(d.is_zero());
    }

    // ───────────────────────── cmp_abs ─────────────────────────

    #[test]
    fn cmp_abs_orders() {
        assert_eq!(cmp_abs(&one_e(-400), &one_e(-324)), Ordering::Less);
        assert_eq!(cmp_abs(&one_e(-324), &one_e(-400)), Ordering::Greater);
        assert_eq!(cmp_abs(&one_e(-400), &one_e(-400)), Ordering::Equal);
        assert_eq!(cmp_abs(&Decimal::zero(), &one_e(-400)), Ordering::Less);
        assert_eq!(cmp_abs(&Decimal::zero(), &dec(&[], 5)), Ordering::Equal);
        assert_eq!(
            cmp_abs(&dec(&[9], -324), &dec(&[1], -325)),
            Ordering::Greater
        );
        assert_eq!(
            cmp_abs(&dec(&[2, 5], -324), &dec(&[2, 4], -324)),
            Ordering::Greater
        );
        assert_eq!(
            cmp_abs(&dec(&[1, 2], -1), &dec(&[1, 2], -1)),
            Ordering::Equal
        );
        assert_eq!(
            cmp_abs(&dec(&[1, 2], -3), &dec(&[1, 1], -3)),
            Ordering::Greater
        );
    }

    // ─────────────── §1.4 / §5.1 / §5.7：预筛候选（真值 < 2^-1000） ───────────────

    /// §5.1 正例 + §5.7：这些字面量的十进制真值都 `< 2^-1000` → 全部是候选。
    #[test]
    fn prescreen_true_cases() {
        let no_exp_324 = format!("0.{}1", "0".repeat(324));
        let no_exp_400 = format!("0.{}1", "0".repeat(400));
        let cases: Vec<(&str, String)> = vec![
            ("1e-400", "1e-400".to_string()),
            ("1e-324", "1e-324".to_string()),
            ("10e-325", "10e-325".to_string()),
            ("1e-0350", "1e-0350".to_string()),
            ("1e-0000000400", "1e-0000000400".to_string()),
            ("100000e-405", "100000e-405".to_string()),
            ("1000000000e-0000400", "1000000000e-0000400".to_string()),
            ("no_exp_324", no_exp_324),
            ("no_exp_400", no_exp_400),
            ("-1e-400", "-1e-400".to_string()),
            ("1E-400", "1E-400".to_string()),
            (
                "12345678901234567890e-423",
                "12345678901234567890e-423".to_string(),
            ),
            (
                "超长尾数 37 位",
                "1.23456789012345678901234567890123456789e-324".to_string(),
            ),
            (
                "★ 2.4703282292062328e-324",
                "2.4703282292062328e-324".to_string(),
            ),
            (
                "★ 2.4703282292062329e-324",
                "2.4703282292062329e-324".to_string(),
            ),
        ];
        for (name, s) in cases {
            assert!(is_cand(&s), "[{name}] {s:?} 应是预筛候选");
        }
    }

    /// §5.2 反例：真值为 0、或真值 `>= 2^-1000` → 不是候选。
    ///
    /// **注意（确认范式的关键）**：`5e-324` / `1e-323` / `2.5e-324` 这些**合法次正规数**
    /// 的真值**仍 `< 2^-1000`**（数量级 1e-324，远小于 1e-301），故**会被预筛列为候选**——
    /// 但经**确认步骤**（实际解析器）得非零 → **放过**。「是候选」≠「判违规」：
    /// 预筛只负责不漏报，裁决在确认；合法值的零误判由确认步骤保证。
    #[test]
    fn prescreen_false_cases() {
        // 真值为 0：绝不是候选（无论指数多离谱）。
        for s in [
            "0",
            "0.0",
            "-0.0",
            "0e0",
            "0e-400",
            "0.0000e-400",
            "0e-99999999999999999999",
        ] {
            assert!(!is_cand(s), "{s:?} 真值为 0，绝不是候选");
        }
        // 真值 >= 2^-1000（约 1e-301）：普通数 / 微米级工艺参数等。
        for s in [
            "1e-300", "1e5", "1E5", "123.456", "1", "-1", "1e-6", "0.001",
        ] {
            assert!(!is_cand(s), "{s:?} 真值 >= 2^-1000，不是候选");
        }
        // 无指数、300 个零：真值 = 1e-301，>= 2^-1000 → 不是候选。
        let no_exp_300 = format!("0.{}1", "0".repeat(300));
        assert!(!is_cand(&no_exp_300), "1e-301 应 >= 2^-1000");
        // 无指数、302 个零：真值 = 1e-303 < 2^-1000 → 是候选。
        let no_exp_302 = format!("0.{}1", "0".repeat(302));
        assert!(is_cand(&no_exp_302), "1e-303 < 2^-1000，应是候选");
    }

    /// 合法次正规数（`5e-324` 等）**是预筛候选** —— 预筛宽松不漏；最终是否判下溢
    /// 由调用方用实际解析器确认（`serde_json` 实测非零 → 放过）。
    #[test]
    fn legal_subnormals_are_prescreen_candidates() {
        for s in ["5e-324", "1e-323", "9.9e-324", "2.5e-324"] {
            assert!(is_cand(s), "{s:?} 真值 < 2^-1000，应是预筛候选");
        }
    }

    /// §5.7 **预筛不漏报专项**（QA 头号攻击点）。
    ///
    /// 对每个**真下溢**字面量（`str::parse` 实测归零），预筛**必须**把它列为候选，
    /// 否则确认步骤无从执行 → 静默漏报。这里用「`str::parse` 得 `0.0`」作为「真下溢」
    /// （std 正确舍入）的 ground truth，并断言预筛命中。覆盖无指数 / 前导零 /
    /// 超长尾数 / 挪小数点 等形式。
    ///
    /// **不含 `2.4703282292062328e-324`**：`str::parse`（正确舍入）对它得 `5e-324`（非零），
    /// 它只对**非正确舍入**的 `serde_json` 才归零 —— 该字面量的确认由 JSON 侧调用方
    /// （`serde_json` 实测）完成，不在 std ground truth 内。
    #[test]
    fn prescreen_no_false_negative_for_true_underflow() {
        let mut cases: Vec<(String, String)> = vec![
            ("1e-400".into(), "1e-400".into()),
            ("1e-324".into(), "1e-324".into()),
            ("1e-325".into(), "1e-325".into()),
            ("1e-500".into(), "1e-500".into()),
            ("1e-999".into(), "1e-999".into()),
            ("1e-0350".into(), "1e-0350".into()),
            ("1e-0000000400".into(), "1e-0000000400".into()),
            ("100000e-405".into(), "100000e-405".into()),
            ("1000000000e-400".into(), "1000000000e-400".into()),
            ("-1e-400".into(), "-1e-400".into()),
            ("1E-400".into(), "1E-400".into()),
            (
                "12345678901234567890e-423".into(),
                "12345678901234567890e-423".into(),
            ),
        ];
        // 无指数形式：`0.` + 330/400 个零 + `1`（真值 1e-331 / 1e-401）。
        cases.push(("no_exp_330".into(), format!("0.{}1", "0".repeat(330))));
        cases.push(("no_exp_400".into(), format!("0.{}1", "0".repeat(400))));

        for (name, s) in cases {
            // ground truth：`str::parse`（正确舍入）实测归零。
            let parsed: f64 = s.parse().expect("字面量应可被 std 解析");
            assert_eq!(parsed, 0.0, "[{name}] {s:?} 经 std 解析应归零（真下溢）");
            assert!(is_cand(&s), "[{name}] {s:?} 真下溢，预筛必须命中（防漏报）");
        }
    }

    // ───────────────────────── §5.3 JSON 扫描器回归 ─────────────────────────

    /// §5.3：字符串 / 键名 / 转义一律跳过（第一版假阳性回归）。
    #[test]
    fn json_scanner_string_and_key_regression() {
        // 命中 1 的用例。
        for (name, json) in [
            ("普通值", r#"{"x":1e-400}"#),
            ("混合", r#"{"x":1e-400,"n":"ok"}"#),
            ("数组元素", r#"{"arr":[1e-400,2]}"#),
            ("嵌套", r#"{"nested":{"x":1e-400}}"#),
            ("字符串放过+值命中", r#"{"x":"5e-324","y":1e-400}"#),
        ] {
            assert_eq!(hits(json), 1, "[{name}] {json:?} 应命中 1 次");
        }

        // 命中 0 的用例。
        for (name, json) in [
            ("字符串内容", r#"{"note":"1e-400"}"#),
            ("键名是字符串", r#"{"1e-400":"v"}"#),
            ("转义引号", r#"{"s":"say \"1e-400\""}"#),
            ("字符串内反斜杠", r#"{"s":"a\\1e-400"}"#),
            ("换行转义", r#"{"s":"line1\n1e-400"}"#),
            ("多层转义", r#"{"s":"\\\"1e-400"}"#),
            ("纯字符串数组", r#"["1e-400","2e-400"]"#),
        ] {
            assert_eq!(hits(json), 0, "[{name}] {json:?} 应命中 0 次");
        }
    }

    /// §5.5：非法数字 token → 非候选（交给 serde_json 报语法错）。
    #[test]
    fn json_scanner_illegal_number_tokens_are_not_candidates() {
        for (name, json) in [
            ("前导零", r#"{"x":01}"#),
            ("尾随 e", r#"{"x":1e-400e}"#),
            ("裸点", r#"{"x":.5}"#),
            ("前导正号", r#"{"x":+1}"#),
        ] {
            assert_eq!(hits(json), 0, "[{name}] {json:?} 不应成为候选");
        }
    }

    /// 返回**全部**候选（不因首个命中而短路）且顺序正确。
    #[test]
    fn json_scanner_returns_all_candidates() {
        let cands = scan_underflow_candidates(r#"{"a":1e-400,"b":2,"c":3e-500,"d":1e-999}"#);
        let lits: Vec<&str> = cands.iter().map(|c| c.literal.as_str()).collect();
        assert_eq!(lits, vec!["1e-400", "3e-500", "1e-999"]);
    }

    /// 定位信息：行 / 列与字节偏移。
    #[test]
    fn json_scanner_reports_location() {
        let json = "{\n  \"x\": 1e-400\n}";
        let hit = first(json);
        assert_eq!(hit.literal, "1e-400");
        assert_eq!(hit.byte_offset, json.find("1e-400").unwrap());
        assert_eq!(hit.line, 2);
        assert_eq!(hit.column, 8); // `  "x": ` 共 7 字符。
    }

    /// 合法 JSON（无候选）→ 空；保证不误伤普通文档。
    ///
    /// 注意：`5e-324` / `1e-323` 是**候选**（真值 < 2^-1000），故不在此列 ——
    /// 它们由确认步骤放过，见 `legal_subnormals_are_prescreen_candidates`。
    #[test]
    fn json_scanner_clean_documents() {
        for json in [
            r#"{}"#,
            r#"[]"#,
            r#"{"a":1,"b":2.5,"c":-3e10,"d":true,"e":null}"#,
            r#"{"x":1e-10,"y":0.0001}"#,
            r#""just a string""#,
        ] {
            assert_eq!(hits(json), 0, "{json:?} 不应有候选");
        }
    }

    // ───────────────────────── JSON 边界回归 ─────────────────────────

    /// JSON 整数精度：超出 u64（正）/ i64（负）范围的整数字面量被标记为越界；
    /// 边界值、浮点/指数形式与字符串内的数字不标记。
    #[test]
    fn json_integer_overflow_scan() {
        // 越界命中：正数超 u64::MAX、负数低于 i64::MIN。
        let big = scan_json_integer_overflows(r#"{"x":18446744073709551616}"#);
        assert_eq!(big.len(), 1, "正数超 u64 应命中");
        assert_eq!(big[0].literal, "18446744073709551616");
        assert_eq!(big[0].line, 1);
        assert_eq!(big[0].column, 6);
        let neg = scan_json_integer_overflows(r#"{"x":-9223372036854775809}"#);
        assert_eq!(neg.len(), 1, "负数低于 i64 应命中");
        assert_eq!(neg[0].literal, "-9223372036854775809");

        // 边界值合法：i64::MIN / i64::MAX / u64::MAX 不标记。
        for ok in [
            r#"{"x":-9223372036854775808}"#,
            r#"{"x":9223372036854775807}"#,
            r#"{"x":18446744073709551615}"#,
        ] {
            assert!(
                scan_json_integer_overflows(ok).is_empty(),
                "{ok:?} 应在范围内"
            );
        }

        // 浮点 / 指数形式不属整数通道（交由 serde_json 处理）。
        for f in [r#"{"x":1.5}"#, r#"{"x":1e400}"#, r#"{"x":1e-400}"#] {
            assert!(
                scan_json_integer_overflows(f).is_empty(),
                "{f:?} 非整数字面量"
            );
        }

        // 字符串内容 / 键名内的数字整体跳过。
        for s in [
            r#"{"n":"99999999999999999999"}"#,
            r#"{"99999999999999999999":"v"}"#,
        ] {
            assert!(
                scan_json_integer_overflows(s).is_empty(),
                "{s:?} 字符串内数字应跳过"
            );
        }
    }

    /// 非有限走向：溢出为 Inf 的十进制字面量（如 `1e400`）既不是整数越界，也不是
    /// 下溢候选 —— 由调用方的 `serde_json` 解析负责报错，扫描层不越权。
    #[test]
    fn json_non_finite_values_are_not_candidates() {
        for s in ["1e400", "1e308", "9e999"] {
            assert!(!is_cand(s), "{s:?} 非下溢候选");
        }
        let doc = r#"{"x":1e400}"#;
        assert!(
            scan_underflow_candidates(doc).is_empty(),
            "非有限值非下溢候选"
        );
        assert!(
            scan_json_integer_overflows(doc).is_empty(),
            "非有限值非整数越界"
        );
    }
}
