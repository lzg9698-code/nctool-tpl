//! JSON/YAML 数字字面量下溢**候选提取**（ERR-NUM-UNDERFLOW）。
//!
//! 背景：`serde_json` / `serde_yaml` 把**真值非零但过小**的十进制字面量（如 `1e-400`）
//! 静默解析为 `f64 0.0`，该 `0.0` 会一路写进 G-code，全程无报错 —— 属于「静默出错」
//! （错误程序上机床 = 撞刀 = 真实伤害），必须零容忍。
//!
//! `f64` 通道没有哨兵：`1e-400` 与 `0.0` 解析出的 `f64` 逐位相同（`to_bits() == 0`），
//! 解析完成后已无法区分。**故检测必须在文本层、基于十进制真值完成。**
//!
//! # 两段式：本模块只做「候选提取」，**不**做最终裁决
//!
//! 判据范式为「**扫描候选 + 实际解析器确认**」（见设计文档 §1.3、附录 D）：
//!
//! 1. **本模块（`core`）**：文本层扫描，把**十进制真值非零且 `|真值| < 2^-1000`** 的
//!    数字字面量提取为 [`UnderflowCandidate`]。`2^-1000 ≈ 1e-301` 仅作**宽松预筛**，
//!    比任何解析器的归零边界（约 `5e-324`）大 23 个数量级 → **不可能漏掉真下溢值**。
//! 2. **调用方**：对候选字面量**实测**该格式的解析器 ——
//!    JSON 用 `serde_json`（在 `cli` 是运行时依赖）、YAML 用 `serde_yaml`
//!    （在 `core` 是运行时依赖，见 [`confirm_underflow_yaml`]）、
//!    `--param` 用 `str::parse`。解析得 `0.0` 而预筛已保证真值非零 ⇒ **确认下溢** ⇒ 拒绝。
//!
//! **为什么不设「精确预测阈值」**：实测 `serde_json` 的十进制→f64 **不是正确舍入**
//! （`2.4703282292062328e-324` 在 `str::parse` / `serde_yaml` 得 `5e-324`，在
//! `serde_json` 却得 `0.0`），而 `serde_yaml` 与 `std` 一致。同一字面量在不同解析器下
//! 归零与否**不同**（阈值分裂）→ 任何**单一预测阈值**都会在某一侧漏报或误伤
//! （`2^-1075` 漏报 JSON；`2^-1074` 误伤 `nc_strip` 场景的合法次正规数）。
//! 「确认」范式下每个解析器用自己的真实行为裁决，**零漏报 + 零误判**。详见附录 D。
//!
//! # 零新 crate
//!
//! 扫描与预筛全部 `std` 手写；`serde_json` 在 `core` 仍是 **dev-dependency**
//! （本模块运行时**不**调用它）；YAML 确认用 `serde_yaml`（`core` 既有运行时依赖）。
//!
//! # 示例
//!
//! ```
//! use nctool_core::json_num::scan_underflow_candidates;
//!
//! // `1e-400` 成为候选（真值非零且 < 2^-1000），待调用方确认。
//! let cands = scan_underflow_candidates(r#"{"x":1e-400}"#);
//! assert_eq!(cands.len(), 1);
//! assert_eq!(cands[0].literal, "1e-400");
//! // 普通数不是候选。
//! assert!(scan_underflow_candidates(r#"{"x":123.456,"y":1e-3}"#).is_empty());
//! // 字符串内容不是数字字面量，不得误判。
//! assert!(scan_underflow_candidates(r#"{"note":"1e-400"}"#).is_empty());
//! ```

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
/// 接受的语法（**比 JSON/YAML 宽松**，因为本函数只服务于真值预筛）：
/// 可选前导 `+`/`-`、整数部分、可选小数部分、可选 `e`/`E` 指数（指数可带符号与前导零）。
///
/// 返回 `None` 的情形（**任何尾随垃圾都拒绝**）：
/// `1e-400e`、`1.`、`1e`、`1e+`、空串、纯符号等 —— 这些多为非法数字，本模块**不越权**
/// 报语法错，交给调用方的解析器兜底。
///
/// 关于**前导零整数** `01`：JSON/YAML 数字语法均**不允许**前导零，解析器会以语法错误拒绝。
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
/// use nctool_core::json_num::scan_underflow_candidates;
///
/// let cands = scan_underflow_candidates(r#"{"a":1e-400,"b":2,"c":3e-500}"#);
/// assert_eq!(cands.len(), 2);
/// assert_eq!(cands[0].literal, "1e-400");
/// assert_eq!(cands[1].literal, "3e-500");
/// // 普通数值（真值 >= 2^-1000）不是候选。
/// assert!(scan_underflow_candidates(r#"{"x":1e-3,"y":123.456}"#).is_empty());
/// // `5e-324`（最小次正规数）**是**候选：其十进制真值非零且 < 2^-1000。
/// // 「是候选」≠「判下溢」——它由确认环节放过（见 confirm_underflow_yaml）。
/// let sub = scan_underflow_candidates(r#"{"x":5e-324}"#);
/// assert_eq!(sub.len(), 1);
/// assert_eq!(sub[0].literal, "5e-324");
/// ```
pub fn scan_underflow_candidates(json: &str) -> Vec<UnderflowCandidate> {
    scan_number_tokens(json, LexMode::Json)
}

/// 扫描 YAML 文本，返回**全部**下溢候选字面量（按出现顺序）。
///
/// 与 [`scan_underflow_candidates`] 共用同一套预筛判据（`parse_dec` / `cmp_abs` /
/// `prescreen_threshold`），**仅词法跳过策略不同**，以适配 YAML 与 JSON 的语法差异：
///
/// - **注释** `# ...` 到行尾整体跳过（YAML 有，JSON 没有）。
/// - **单引号字符串** `'...'`（无转义；`''` 表一个 `'`）跳过。
/// - **双引号字符串** `"..."`（与 JSON 同转义）跳过。
/// - **块标量** `|` / `>` 之后的缩进行（多行字符串内容）整体跳过。
///
/// 返回语义与调用方义务同 [`scan_underflow_candidates`]，但**确认必须用 YAML 解析器**
/// （[`confirm_underflow_yaml`]）—— 见附录 D4：`serde_yaml` 与 `serde_json` 的
/// 十进制→f64 行为不同，**不得**复用 JSON 的确认结果。
///
/// # 示例
///
/// ```
/// use nctool_core::json_num::scan_underflow_candidates_yaml;
///
/// // 普通标量 → 候选。
/// assert_eq!(scan_underflow_candidates_yaml("x: 1e-400").len(), 1);
/// // 注释里的数字不是标量 → 无候选。
/// assert!(scan_underflow_candidates_yaml("# note: 1e-400\nx: 1").is_empty());
/// // 引号字符串不是数字 → 无候选。
/// assert!(scan_underflow_candidates_yaml("x: \"1e-400\"").is_empty());
/// ```
pub fn scan_underflow_candidates_yaml(yaml: &str) -> Vec<UnderflowCandidate> {
    scan_number_tokens(yaml, LexMode::Yaml)
}

/// 用 `serde_yaml` 实测确认候选字面量是否被**静默归零**。
///
/// 返回 `true` 表示**确认为下溢**（应拒绝）：「该字面量经 `serde_yaml` 解析得 `0.0`，
/// 而 [`scan_underflow_candidates_yaml`] 的预筛已保证其十进制真值非零」。
///
/// 本函数放在 `core` 是因为 `serde_yaml` 是 `core` 的**运行时依赖**（`core` 只对
/// `serde_json` 保持 dev-dependency 边界；本函数**不**调用 `serde_json`）。
///
/// # 与 JSON 确认的关系
///
/// **JSON 通道的确认不在此**：`serde_json` 在 `core` 是 dev-dependency，不能运行时调用。
/// JSON 侧确认须在持 `serde_json` 运行时依赖的调用方（`cli`）实现。
/// 两个解析器的阈值**不同**（`serde_yaml` 正确舍入、`serde_json` 不是），故**必须分别确认**。
///
/// # 语法错误
///
/// 若字面量经 `serde_yaml` 解析报错（语法非法），返回 `false`（**不**确认为下溢）——
/// 语法错误交给调用方的原解析路径报错，本函数**不越权**。
///
/// # 示例
///
/// ```
/// use nctool_core::json_num::{confirm_underflow_yaml, UnderflowCandidate};
///
/// let cand = UnderflowCandidate {
///     literal: "1e-400".to_string(),
///     byte_offset: 0,
///     line: 1,
///     column: 1,
/// };
/// // `serde_yaml` 把 `1e-400` 解析为 0.0 → 确认为下溢。
/// assert!(confirm_underflow_yaml(&cand));
///
/// // 合法次正规数 `5e-324`：`serde_yaml` 解析非零 → 不确认。
/// let ok = UnderflowCandidate {
///     literal: "5e-324".to_string(),
///     byte_offset: 0,
///     line: 1,
///     column: 1,
/// };
/// assert!(!confirm_underflow_yaml(&ok));
/// ```
pub fn confirm_underflow_yaml(cand: &UnderflowCandidate) -> bool {
    // 用文档映射形态 `x: <字面量>` 实测；与调用方的实际解析路径同构。
    let probe = format!("x: {}", cand.literal);
    let value: serde_yaml::Value = match serde_yaml::from_str(&probe) {
        Ok(v) => v,
        // 语法错 → 不确认（交给原解析路径报错）。
        Err(_) => return false,
    };
    // 取出 `x` 键对应的标量。
    let scalar = match value {
        serde_yaml::Value::Mapping(m) => m.get(serde_yaml::Value::String("x".to_string())).cloned(),
        // 顶层非映射（理论不可达，因 probe 恒为映射）→ 保守不确认。
        _ => None,
    };
    match scalar {
        // 只对浮点标量判下溢；整数/其他类型无「静默归零」语义。
        Some(serde_yaml::Value::Number(n)) => n.as_f64().map(|v| v == 0.0).unwrap_or(false),
        // 非数字 / 缺键 → 不确认。
        _ => false,
    }
}

/// 词法模式：JSON 与 YAML 共用扫描骨架，仅「字符串/注释/块标量」跳过策略不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexMode {
    Json,
    Yaml,
}

/// 扫描器状态（两种模式共用；`InComment` / `BlockScalar` 仅 YAML 进入）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    /// 在数字字面量位置抓取 token。
    Normal,
    /// 在双引号字符串内（JSON / YAML 双引号）。
    InString,
    /// 刚吃掉字符串内的一个反斜杠，吞掉下一个转义字节。
    InStringEscape,
    /// 在 YAML 单引号字符串内（无转义；`''` 表一个 `'`）。
    InSingleQuote,
    /// 在 YAML 注释内（到行尾）。
    InComment,
}

/// 扫描骨架：在文本中找出全部「数字字面量候选」。
///
/// JSON 模式：Normal / InString / InStringEscape 三态（§2 状态机）。
/// YAML 模式：额外处理行尾注释 `#`、单引号字符串 `'...'`、块标量 `|`/`>` 后的缩进行。
fn scan_number_tokens(text: &str, mode: LexMode) -> Vec<UnderflowCandidate> {
    let bytes = text.as_bytes();
    let mut out: Vec<UnderflowCandidate> = Vec::new();
    let mut i: usize = 0;
    let mut state = ScanState::Normal;
    // YAML 块标量：记录其后的缩进量；缩进行整体跳过，直到遇到缩进更小的行。
    let mut block_indent: Option<usize> = None;

    while i < bytes.len() {
        // YAML 块标量处理：跳过所有「缩进 >= block_indent」的行。
        if let Some(indent) = block_indent {
            let line_start = i;
            // 计算本行缩进（空格数；行首）。
            let mut ws = 0usize;
            while line_start + ws < bytes.len() && bytes[line_start + ws] == b' ' {
                ws += 1;
            }
            let blank = line_start + ws >= bytes.len()
                || bytes[line_start + ws] == b'\n'
                || bytes[line_start + ws] == b'\r';
            if !blank && ws >= indent {
                // 该行属于块标量内容 → 跳到行尾。
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            // 缩进不足（或空行）→ 块标量结束。
            block_indent = None;
        }

        match state {
            ScanState::Normal => {
                let b = bytes[i];
                if mode == LexMode::Yaml && b == b'#' {
                    // YAML 注释：到行尾。
                    state = ScanState::InComment;
                    i += 1;
                } else if b == b'"' {
                    // 进入双引号字符串（键名与内容一律跳过）。
                    state = ScanState::InString;
                    i += 1;
                } else if mode == LexMode::Yaml && b == b'\'' {
                    // YAML 单引号字符串。
                    state = ScanState::InSingleQuote;
                    i += 1;
                } else if mode == LexMode::Yaml
                    && (b == b'|' || b == b'>')
                    && next_nonspace_is_eol(text, i + 1)
                {
                    // 块标量头（`|` / `>` 可选带 chomping/indent 指示符，以行尾结束）。
                    // 记录其内容的缩进基准，并**跳过本行（含换行）**，使下一轮循环从
                    // 内容行行首开始判断。
                    block_indent = Some(next_line_indent(text, i + 1));
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1; // 越过行尾 `\n`，避免把本行误判为「空行 → 块标量结束」。
                    }
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
                    if is_prescreen_candidate(token) {
                        let (line, column) = line_col(text, start);
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
                // JSON/YAML 转义均为 `\X` 两字符 → 只吞 1 字节。
                state = ScanState::InString;
                i += 1;
            }
            ScanState::InSingleQuote => {
                // YAML 单引号：`''` 表一个 `'`（不闭合）。
                if bytes[i] == b'\'' {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                        i += 2; // 转义的引号，仍在字符串内。
                    } else {
                        state = ScanState::Normal;
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            ScanState::InComment => {
                if bytes[i] == b'\n' {
                    state = ScanState::Normal;
                }
                i += 1;
            }
        }
    }
    out
}

/// 从下标 `from` 起跳过行内空白后是否到达行尾（含 `\n` / `\r` / 串尾）。
///
/// 用于识别 YAML 块标量头（`key: |` / `key: >`）——`|`/`>` 之后只允许空白与行尾。
fn next_nonspace_is_eol(text: &str, from: usize) -> bool {
    let bytes = text.as_bytes();
    let mut j = from;
    while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
        j += 1;
    }
    j >= bytes.len() || bytes[j] == b'\n' || bytes[j] == b'\r'
}

/// 求下一行首个非空字符的缩进（空格数）。用于块标量内容的缩进基准。
fn next_line_indent(text: &str, from: usize) -> usize {
    let bytes = text.as_bytes();
    // 跳到下一行行首。
    let mut j = from;
    while j < bytes.len() && bytes[j] != b'\n' {
        j += 1;
    }
    if j < bytes.len() {
        j += 1; // 越过 `\n`。
    }
    let mut indent = 0usize;
    while j + indent < bytes.len() && bytes[j + indent] == b' ' {
        indent += 1;
    }
    indent
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

/// 由字节偏移 `offset` 计算 **1-based** `(行号, 列号)`。
///
/// 行号按 `\n` 计；列号按 **字符**（`char`）计（`offset` 之前同行的字符数 + 1），
/// 使多字节 UTF-8 下的列号对用户更直观、且稳定。
fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
    let line_start = prefix.rfind('\n').map(|p| p + 1).unwrap_or(0);
    // `text` 是合法 UTF-8；`offset` 落在 ASCII 数字 token 边界，故必为字符边界。
    let column = text[line_start..offset].chars().count() + 1;
    (line, column)
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

    /// YAML 扫描命中数。
    fn hits_yaml(yaml: &str) -> usize {
        scan_underflow_candidates_yaml(yaml).len()
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

    /// 合法次正规数（`5e-324` 等）**是预筛候选**，但由 YAML 确认步骤放过 ——
    /// 证明「预筛宽松不漏 + 确认零误判」的分层正确性。
    #[test]
    fn legal_subnormals_are_candidates_but_not_underflow() {
        for s in ["5e-324", "1e-323", "9.9e-324", "2.5e-324"] {
            assert!(is_cand(s), "{s:?} 真值 < 2^-1000，应是预筛候选");
            let cand = UnderflowCandidate {
                literal: s.to_string(),
                byte_offset: 0,
                line: 1,
                column: 1,
            };
            assert!(
                !confirm_underflow_yaml(&cand),
                "{s:?} 合法次正规数，确认步骤必须放过"
            );
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
    /// 它只对**非正确舍入**的 `serde_json` 才归零 —— 该字面量的确认由 JSON 侧做
    /// （见 `confirm_underflow_json` 的 cli 实现），不在 std ground truth 内。
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
    /// 它们由确认步骤放过，见 `legal_subnormals_are_candidates_but_not_underflow`。
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

    // ───────────────────────── §5.6 YAML 扫描器回归 ─────────────────────────

    /// §5.6：YAML 标量命中；引号串 / 注释 / 块标量跳过。
    ///
    /// 注意：`5e-324` 也是**预筛候选**（真值 < 2^-1000），故含它的输入候选数会 +1 ——
    /// 最终裁决（放过）在确认步骤，见 `yaml_scan_then_confirm`。
    #[test]
    fn yaml_scanner_hits_and_skips() {
        // 应命中 1（普通标量 / 前导零 / 挪小数点 / 行尾注释后）。
        assert_eq!(hits_yaml("params:\n  x: 1e-400"), 1, "普通标量");
        assert_eq!(hits_yaml("params:\n  x: 1e-0350"), 1, "前导零指数");
        assert_eq!(hits_yaml("params:\n  x: 100000e-405"), 1, "挪小数点");
        assert_eq!(
            hits_yaml("params:\n  x: 1e-400  # tail"),
            1,
            "行尾注释 + 命中"
        );
        // `1e-400` 与 `5e-324` 都是预筛候选 → 2 个（最终由确认区分）。
        assert_eq!(
            hits_yaml("params:\n  x: 1e-400\n  y: 5e-324"),
            2,
            "两者皆候选"
        );

        // 应命中 0（引号串 / 注释 / 块标量 / 普通值）。
        assert_eq!(hits_yaml("params:\n  x: \"1e-400\""), 0, "双引号字符串");
        assert_eq!(hits_yaml("params:\n  x: '1e-400'"), 0, "单引号字符串");
        assert_eq!(hits_yaml("# note: 1e-400\nparams:\n  x: 1"), 0, "注释");
        assert_eq!(hits_yaml("params:\n  note: |\n    1e-400"), 0, "块标量内容");
        assert_eq!(hits_yaml("params:\n  x: 1"), 0, "普通值");
    }

    /// YAML 单引号里的 `''` 转义不提前闭合。
    #[test]
    fn yaml_single_quote_escape() {
        assert_eq!(hits_yaml("x: 'it''s 1e-400'"), 0, "`''` 转义仍在串内");
    }

    /// YAML 注释：注释内（含行尾注释）的数字都不是标量 → 不命中。
    #[test]
    fn yaml_comment_until_eol() {
        assert_eq!(hits_yaml("# 1e-400 and 2e-500\nx: 1"), 0, "整行注释");
        // 行尾注释里的数字在注释区内 → 不命中；行内 `1` 不是候选。
        assert_eq!(
            hits_yaml("x: 1  # 1e-400 2e-500"),
            0,
            "行尾注释内的数字不命中"
        );
        // 注释结束后的下一行标量仍命中。
        assert_eq!(hits_yaml("x: 1  # 1e-400\ny: 1e-500"), 1, "注释下一行命中");
    }

    // ───────────────────────── YAML 确认（serde_yaml 实测） ─────────────────────────

    /// YAML 确认：`serde_yaml` 对 `1e-400` 归零 → 确认；对 `5e-324` 非零 → 不确认。
    #[test]
    fn yaml_confirmation_matches_serde_yaml() {
        let make = |lit: &str| UnderflowCandidate {
            literal: lit.to_string(),
            byte_offset: 0,
            line: 1,
            column: 1,
        };
        // 真下溢 → 确认。
        assert!(confirm_underflow_yaml(&make("1e-400")), "1e-400 应确认");
        assert!(confirm_underflow_yaml(&make("1e-324")), "1e-324 应确认");
        // 合法值 → 不确认。
        assert!(!confirm_underflow_yaml(&make("5e-324")), "5e-324 应放过");
        assert!(!confirm_underflow_yaml(&make("1e-323")), "1e-323 应放过");
        // **阈值分裂关键项**：`2.4703282292062328e-324` 在 YAML **非零**（bits=1）→ 放过。
        assert!(
            !confirm_underflow_yaml(&make("2.4703282292062328e-324")),
            "YAML 对 2.4703282292062328e-324 不归零（与 JSON 分裂）"
        );
        // 语法非法 → 不确认（交给原解析路径）。
        assert!(!confirm_underflow_yaml(&make("1e-400e")), "语法错不确认");
    }

    /// 端到端：YAML 扫描 + YAML 确认，复现 §5.6 期望结果。
    #[test]
    fn yaml_scan_then_confirm() {
        let confirmed = |y: &str| -> Vec<String> {
            scan_underflow_candidates_yaml(y)
                .into_iter()
                .filter(confirm_underflow_yaml)
                .map(|c| c.literal)
                .collect()
        };
        // `x: 1e-400` → 拒绝（确认）。
        assert_eq!(confirmed("params:\n  x: 1e-400"), vec!["1e-400"]);
        // `x: 1e-400 / y: 5e-324` → 拒绝（因 x）。
        assert_eq!(
            confirmed("params:\n  x: 1e-400\n  y: 5e-324"),
            vec!["1e-400"]
        );
        // `x: 5e-324` → 通过（候选仍会进，但确认放过）→ 无命中。
        assert!(confirmed("params:\n  x: 5e-324").is_empty());
        // `x: "1e-400"` → 通过（不是标量）。
        assert!(confirmed("params:\n  x: \"1e-400\"").is_empty());
        // §5.6 #10：`2.4703282292062328e-324` → YAML 确认放过（阈值分裂）。
        assert!(confirmed("params:\n  x: 2.4703282292062328e-324").is_empty());
    }
}
