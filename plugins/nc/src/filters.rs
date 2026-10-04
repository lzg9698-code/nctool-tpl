//! 数学/NC 数值格式化过滤器（全部基于 Rust 标准库 `f64`，零额外依赖）。
//!
//! 所有过滤器对结果做有限性校验（NaN/Inf 一律转渲染错误）；NC 过滤器
//! 附带数量级上限防护（防巨量分配与饱和截断）。

// ---------------------------------------------------------------------------
// NC 数值格式化过滤器（G-code 专用）
// ---------------------------------------------------------------------------

/// `nc_fixed` 小数位上限：超过 f64 有效精度（约 17 位）即无意义，
/// 且巨型宽度会触发巨量字符串分配（分配失败是进程 abort，非可捕获错误）。
const MAX_NC_FIXED_DECIMALS: usize = 32;
/// `nc_pad` 宽度上限：防止模板笔误/恶意输入触发巨量分配。
const MAX_NC_PAD_WIDTH: usize = 1024;

/// 固定小数位：`{{ x | nc_fixed(3) }}` → `21.000`。
///
/// 用于需要固定精度的坐标值（如 `X21.000 Y15.500`）。非有限数（NaN/Inf）报错。
///
/// **舍入模式是「就近取偶」（banker's rounding）**，由 Rust 的 `{:.N}` 决定：
/// `0.125` 保留 2 位得 `0.12`、`0.375` 得 `0.38`。minijinja 的 `round` 过滤器
/// 用的是「半远离零」（`0.125 | round(2)` → `0.13`）——**两者在 .5 附近取向相反**。
/// 这是刻意的：本过滤器要逐值对齐源项目 Python 的 `f"{v:.2f}"`（同样是就近取偶），
/// 换成与 `round` 一致会让迁移过来的模板输出变化。需要舍入语义统一时，
/// 别改这里，改模板。
///
/// # ⚠️ 丢值即报错（ERR-NUM-PRECISION）
///
/// 真值非零却渲染为全零时**返回 `Err`**（不再静默产出错误 G-code —— 坐标静默变 0
/// 在机床上是撞刀）。判据是 **`value != 0.0 && 舍入后的输出串全为零`**
/// （**字符串判零**，与 [`filter_nc_signed`] 同源）。
///
/// ⚠️ **不要改用 `0.5 × 10^(-N)` 之类的量级式判据**：它在 tie 点与真实舍入不一致。
/// 例：`0.5 | nc_fixed(0)` 舍入后确实得 `"0"`（取偶），量级式 `|0.5| < 0.5` 却为假，
/// 会**放过**这个本该报错的值；`5e-7 | nc_fixed(6)`、`5e-8 | nc_fixed(7)` 同理 ——
/// 实测 N=0..=8 全部 tie 上，`0.5 * 10f64.powi(-N)` 的 f64 结果与字面量 `0.5e-N`
/// **逐位相同**（`v == t` 恒真），故 `<` 恒为假、量级式**一律放过**，
/// 而真实舍入在 N=0/6/7 恰好取到零串、在 N=1..5/8 舍入向上非零。
/// 详见设计文档 §3.1.0。**判据只能有一条，即字符串判零。**
///
/// **tie 点行为（正确且有意保留，勿"修"）**：
///   - `0.5 | nc_fixed(0)` → `Err`（舍入后确实得到 `"0"`）；
///     `5e-7 | nc_fixed(6)`、`5e-8 | nc_fixed(7)` 同理 → `Err`。
///   - `0.0005 | nc_fixed(3)` → `"0.001"`（舍入向上非零，不报错）；
///     `0.05 | nc_fixed(1)` → `"0.1"`、`0.005 | nc_fixed(2)` → `"0.01"` 同理
///     —— 这两组的 f64 真值略**高于**十进制中点，故舍入向上。
///
/// （设计 §1.5.1 / §9.3；判据实测见 §3.1.0）
///
/// 需要保留更小量级时：改用 `| nc_fixed(4)` 或 `| nc_strip`（后者用 Rust `Display`，
/// 不截断，但输出非固定位）。
///
/// **负零归一必须在舍入之后**（对齐 [`filter_nc_signed`] 的 P2-3 修正）：用字符串判零，
/// 舍入前浮点判零会漏掉 `-1e-4`（`-1e-4 != 0.0` 为真）而输出 `-0.000`。
pub(crate) fn filter_nc_fixed(value: f64, decimals: usize) -> Result<String, minijinja::Error> {
    if !value.is_finite() {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_fixed: 输入非有限数（NaN/Inf）",
        ));
    }
    if decimals > MAX_NC_FIXED_DECIMALS {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!("nc_fixed: 小数位 {decimals} 超出上限 {MAX_NC_FIXED_DECIMALS}"),
        ));
    }
    // 先按小数位定值、再判零 —— 归一必须在**舍入之后**（对齐 nc_signed 的 P2-3 修正）。
    // 舍入前判零（`if value == 0.0`）会漏掉 `-1e-4`：它 != 0.0，绕过归一，
    // 随后 `{:.*}` 把 `-0.0001` 舍成 `-0.000`，输出带负号的零串 —— 与本节
    // 「-0.0 归一到 +0.0」的承诺相悖。用字符串判零而不是浮点比较：
    // `-0.0005` 这类值在"乘再除"里会抖到另一侧。
    let mag = format!("{:.*}", decimals, value.abs());
    let is_zero = mag.chars().all(|c| c == '0' || c == '.');
    // ★ 硬失败（ERR-NUM-PRECISION）：真值非零却渲染为全零
    //   => 调用方声明的精度不足以表达该值（契约违背），必须报错而非静默产出错误 G-code。
    //   判据 = 「舍入后的输出串全为零」【FORM_A，字符串判零】。
    //   ⚠️ 不要写成量级式 |v| < 0.5*10^(-N) —— 它在 tie 点与真实舍入不一致（设计 §3.1.0）。
    if value != 0.0 && is_zero {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!(
                "nc_fixed: 值 {value} 在 {decimals} 位小数下归零（该值需要更多小数位才能表示）；\
                 精度不足会静默产出错误 G-code。请提高小数位（如 nc_fixed(4)）或改用 nc_strip。"
            ),
        ));
    }
    // 符号在此决定：走到这里说明「真值非零且舍入后非零」或「真值确为零」。
    // 后者（+0.0 / -0.0）在 `value < 0.0` 为假 → 输出无符号的 `mag`（如 "0.000"），
    // 即负零归一到 +0.0（控制器对负零处理不一致，"-0.000" 在图纸上无意义）。
    if value < 0.0 {
        Ok(format!("-{mag}"))
    } else {
        Ok(mag)
    }
}

/// 去尾零：`{{ x | nc_strip }}` → `21`（输入 21.0）或 `21.5`（输入 21.50）。
///
/// 用于不需要固定精度的数值，避免输出 `X21.0` 而期望 `X21`。非有限数报错。
pub(crate) fn filter_nc_strip(value: f64) -> Result<String, minijinja::Error> {
    if !value.is_finite() {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_strip: 输入非有限数（NaN/Inf）",
        ));
    }
    // -0.0 归一到 +0.0，理由同 nc_fixed：否则 `-0.0 | nc_strip` 输出 `-0`
    let value = if value == 0.0 { 0.0 } else { value };
    // Rust f64 Display 已自动去尾零：21.0 → "21"，21.50 → "21.5"
    Ok(format!("{}", value))
}

/// 带强制正号 + 固定小数位：`{{ x | nc_signed(3) }}` → `+21.000`（输入 21.0）、
/// `-4.500`（输入 -4.5）、`+0.000`（输入 0）。
///
/// 对应源项目 Jinja2 侧的 `fmt_coord` 过滤器
/// （`f"{float(v):+.3f}"`，强制显示正负号）。**部分数控系统的增量坐标
/// （如 `G91` 或 `AROT` 的旋转量）要求显式正号**，省略号会被控制器
/// 误判为绝对值——这类差异不会报错，只会加工出错误位置。
///
/// 与 [`filter_nc_fixed`] 的区别只有一个：正数与零也输出 `+`。
/// 因此**不要**用它格式化本来就不带符号语义的值（如直径、进给）。
///
/// # ⚠️ 丢值即报错（ERR-NUM-PRECISION，与 [`filter_nc_fixed`] 同步）
///
/// 真值非零却渲染为全零时同样**返回 `Err`**。判据与 [`filter_nc_fixed`] **完全同源**：
/// `value != 0.0 && 舍入后的输出串全为零`（字符串判零）。二者只差符号前缀，
/// 因此「是否报错」对同一输入**必须一致** —— 这正是 P2-3 只修一半留下的教训，
/// 本次以同一条判据消除分叉的可能。
///
/// `-0.0` 归一到 `+0.000`（既有行为，不变）；`-1e-4 | nc_signed(3)` 则与
/// `nc_fixed` 一样报错，而**不再**输出 `+0.000`（那曾是静默丢值）。
pub(crate) fn filter_nc_signed(value: f64, decimals: usize) -> Result<String, minijinja::Error> {
    if !value.is_finite() {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_signed: 输入非有限数（NaN/Inf）",
        ));
    }
    if decimals > MAX_NC_FIXED_DECIMALS {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!("nc_signed: 小数位 {decimals} 超出上限 {MAX_NC_FIXED_DECIMALS}"),
        ));
    }
    // 先按小数位定值、再判零 —— 归一必须在**舍入之后**：`-0.0001` 保留 3 位就是
    // `0.000`，舍入前判零（`-0.0001 != 0.0`）漏掉它，仍然输出带负号的 `-0.000`，
    // 与本节「-0.0 归一到 +0.000」的承诺相悖。控制器对负零的处理不一致，
    // 而 "-0.000" 在图纸上无意义。
    // 用字符串判零而不是再算一遍浮点：`-0.0005` 这类值在"乘再除"里会抖到另一侧。
    let mag = format!("{:.*}", decimals, value.abs());
    let is_zero = mag.chars().all(|c| c == '0' || c == '.');
    // ★ 硬失败（ERR-NUM-PRECISION）：与 filter_nc_fixed 同源判据（字符串判零）。
    //   真值非零却渲染为全零 => 精度不足以表达该值（契约违背），报错而非静默产出错误 G-code。
    //   禁止改成量级式 |v| < 0.5*10^(-N)：tie 点与真实舍入不一致（设计 §3.1.0）。
    if value != 0.0 && is_zero {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!(
                "nc_signed: 值 {value} 在 {decimals} 位小数下归零（该值需要更多小数位才能表示）；\
                 精度不足会静默产出错误 G-code。请提高小数位（如 nc_signed(4)）或改用 nc_strip。"
            ),
        ));
    }
    let sign = if value < 0.0 && !is_zero { "-" } else { "+" };
    Ok(format!("{sign}{mag}"))
}

/// 前导零填充：`{{ n | nc_pad(4) }}` → `0001`（输入 1）。
///
/// 用于程序号（`O0001`）、行号（`N0010`）等需要固定宽度的**非负整数**。
/// **不接受小数**：传 `1.7` 直接报错而不是截断成 `1`——静默截断会让调用方
/// 以为写的是 1.7，在下游完全不可见（详见下方 `fract()` 检查）。需要取整
/// 请显式用 `| int` / `| round`。负数或非有限数同样报错
/// （负数会拼出 `O-001` 这类非法 G-code）。
pub(crate) fn filter_nc_pad(value: f64, width: usize) -> Result<String, minijinja::Error> {
    if !value.is_finite() {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_pad: 输入非有限数（NaN/Inf）",
        ));
    }
    if value < 0.0 {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_pad: 输入为负数（程序号/行号不可为负）",
        ));
    }
    if width == 0 {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_pad: 宽度不能为 0",
        ));
    }
    if width > MAX_NC_PAD_WIDTH {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!("nc_pad: 宽度 {width} 超出上限 {MAX_NC_PAD_WIDTH}"),
        ));
    }
    // i64 `as` 转换对超范围值是饱和的（1e300 → i64::MAX）：超界直接报错，
    // 避免静默输出错误的程序号/行号。
    //
    // 边界说明（避免过度承诺）：入参本已是 `f64`，(2^53, 2^63) 区间的整数在
    // 进本函数前就已不可精确表示，此检查**管不到那种精度损失**，只能守住
    // “饱和到 `i64::MAX`”这条前沿。NC 程序号/行号实际量级远低于 2^53，故无实际影响。
    //
    // **必须是 `>=` 而不是 `>`**：`i64::MAX as f64` 恰为 2^63（i64::MAX 本身在
    // f64 里不可表示），用 `>` 会让 2^63 通过检查，随后 `as i64` 把它**饱和**成
    // `i64::MAX` —— 静默产出一个错误的程序号，正是本检查要拦的东西。
    if value.trunc() >= i64::MAX as f64 {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_pad: 输入超出整数范围（截断后达到或超过 i64 上限）",
        ));
    }
    // 拒绝小数输入：程序号/行号传 `1.7` 时 `trunc()` 会静默产出 `O0001`，
    // 调用方以为写的是 1.7。这类"渲染成功但结果错误"比直接报错危险得多，
    // 因为它在下游是不可见的。需要取整请显式用 `| int` / `| round` / `| round(0)`。
    if value.fract() != 0.0 {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!(
                "nc_pad: 输入 {value} 不是整数（程序号/行号不接受小数；\
                 请先用 `| int` 或 `| round` 显式取整）"
            ),
        ));
    }
    let int_val = value.trunc() as i64;
    Ok(format!("{:0>width$}", int_val, width = width))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 负零归一：`-0.0` 与 `0.0` 必须输出**同一串字节**。
    ///
    /// 此前只有 `nc_signed` 归一，`nc_fixed` / `nc_strip` 会把 `-0.0` 渲染成
    /// `-0.000` / `-0` —— 同一份逻辑在模板里换个过滤器就输出不同字节，
    /// 而控制器对负零的处理并不一致。
    #[test]
    fn negative_zero_normalized_in_all_numeric_filters() {
        assert_eq!(filter_nc_fixed(-0.0, 3).unwrap(), "0.000");
        assert_eq!(filter_nc_strip(-0.0).unwrap(), "0");
        assert_eq!(filter_nc_signed(-0.0, 3).unwrap(), "+0.000");
        // 逐对相等（防止"两边一起错成别的样子"也算过）
        assert_eq!(
            filter_nc_fixed(-0.0, 3).unwrap(),
            filter_nc_fixed(0.0, 3).unwrap()
        );
        assert_eq!(
            filter_nc_strip(-0.0).unwrap(),
            filter_nc_strip(0.0).unwrap()
        );
    }

    /// 回归（P2-3）：`nc_signed` 的负零归一必须在**舍入之后**。
    /// `-0.0` 保留 3 位仍是 `0.000`，必须输出 `+0.000` 而非 `-0.000`。
    ///
    /// **ERR-NUM-PRECISION 补充**：本测试原先用 `-0.0001 | nc_signed(3)` 验证归一，
    /// 但 `-0.0001` 的**真值非零** —— 在新硬失败判据下它应**报错**（精度不足以表达），
    /// 而非被"归一"成 `+0.000`（那正是静默丢值）。故改用真零 `-0.0` 钉住归一语义，
    /// 真值非零的负小值改由 `nc_signed_errors_on_nonzero_that_renders_as_zero` 钉住报错。
    #[test]
    fn nc_signed_normalizes_zero_after_rounding() {
        // 真零（含负零）：舍入后仍为零串，且真值为零 → 不报错，归一到 +0.000
        assert_eq!(filter_nc_signed(-0.0, 3).unwrap(), "+0.000");
        assert_eq!(filter_nc_signed(-0.0, 0).unwrap(), "+0");
        // 舍入后真的非零 → 符号必须保留（别把归一做过火）
        assert_eq!(filter_nc_signed(-0.002, 3).unwrap(), "-0.002");
        assert_eq!(filter_nc_signed(-1.0, 0).unwrap(), "-1");
    }

    // -----------------------------------------------------------------------
    // ERR-NUM-PRECISION：nc_fixed / nc_signed 丢值即报错（硬失败）
    //
    // 判据 = `value != 0.0 && 舍入后的输出串全为零`（字符串判零，FORM_A）。
    // 禁止量级式判据（设计 §3.1.0）。以下测试是这一判据的**正反例钉子**。
    // -----------------------------------------------------------------------

    /// 硬失败正例（设计 §9.1）：真值非零但 `{:.*}` 渲染为全零 → `Err`。
    ///
    /// **反向验证**：若把实现里的硬失败分支删掉（退回旧的 `Ok(format!(...))`），
    /// 下列每个 `is_err()` 都会变成 `Ok("0.000" / "0.0" / "0")`，**测试立即变红**。
    #[test]
    fn nc_fixed_errors_on_nonzero_that_renders_as_zero() {
        // nc_fixed(3)：阈值附近与内部的真值非零值
        assert!(filter_nc_fixed(1e-4, 3).is_err(), "1e-4 主缺陷");
        assert!(filter_nc_fixed(4e-4, 3).is_err());
        assert!(filter_nc_fixed(4.9e-4, 3).is_err());
        assert!(filter_nc_fixed(5e-324, 3).is_err(), "最小次正规数");
        assert!(filter_nc_fixed(1e-10, 3).is_err());
        // 负值：原缺陷输出 "-0.000"（负零串）
        assert!(filter_nc_fixed(-1e-4, 3).is_err(), "原负零串");
        assert!(filter_nc_fixed(-4.9e-4, 3).is_err());
        // 整数位：U_RC=0.04 的真实路径是 nc_fixed(1)
        assert!(filter_nc_fixed(0.04, 1).is_err(), "U_RC 实路径");
        assert!(filter_nc_fixed(0.049, 1).is_err());
        assert!(filter_nc_fixed(-0.04, 1).is_err());
        // N=0
        assert!(filter_nc_fixed(0.4, 0).is_err());
        assert!(filter_nc_fixed(0.049, 0).is_err());
    }

    /// `nc_signed` 同步硬失败（设计 §9.1）：与 `nc_fixed` 同源的判定。
    ///
    /// **反向验证**：删掉 `filter_nc_signed` 里新增的硬失败分支，
    /// 下列 `is_err()` 会变 `Ok("+0.000" / "+0.0")`，测试立即变红。
    #[test]
    fn nc_signed_errors_on_nonzero_that_renders_as_zero() {
        assert!(filter_nc_signed(1e-4, 3).is_err());
        assert!(filter_nc_signed(-0.04, 1).is_err());
        assert!(filter_nc_signed(4.9e-4, 3).is_err());
    }

    /// 真零路径不得误伤（设计 §9.2，最易误伤的分支）：`0.0` / `-0.0` 必须 `Ok`。
    ///
    /// **反向验证**：把判据误写成 `if is_zero { Err }`（漏掉 `value != 0.0` 前提），
    /// 下列断言全部变 `Err`，测试立即变红。
    #[test]
    fn nc_fixed_allows_true_zero() {
        assert_eq!(filter_nc_fixed(0.0, 0).unwrap(), "0");
        assert_eq!(filter_nc_fixed(0.0, 3).unwrap(), "0.000");
        assert_eq!(filter_nc_fixed(-0.0, 3).unwrap(), "0.000");
        assert_eq!(filter_nc_fixed(-0.0, 0).unwrap(), "0");
        // 逐字节相等（防止"两边一起错成别的样子"也算过）
        assert_eq!(
            filter_nc_fixed(-0.0, 3).unwrap(),
            filter_nc_fixed(0.0, 3).unwrap()
        );
    }

    /// 边界反例（设计 §9.2）：舍入向上、结果非零 → 必须 `Ok`，不得误报。
    ///
    /// **反向验证**：把判据误写成量级式 `|v| < 0.5*10^(-N)`（FORM_B），
    /// 下列值会因 `0.0005f64` 真值恰等于阈值而使 `0.0005 < 0.0005` 为假 —— 单个值
    /// 不会出错，但 `N=0` 的 `0.5` 会差异（见下面的 tie 钉子测试）。
    /// 本测试主要钉住「舍入向上即放过」这条语义。
    #[test]
    fn nc_fixed_allows_rounding_up_at_boundary() {
        assert_eq!(filter_nc_fixed(0.0005, 3).unwrap(), "0.001");
        assert_eq!(filter_nc_fixed(0.05, 1).unwrap(), "0.1");
        assert_eq!(filter_nc_fixed(0.005, 2).unwrap(), "0.01");
        assert_eq!(filter_nc_fixed(-0.0005, 3).unwrap(), "-0.001");
        assert_eq!(filter_nc_fixed(-0.05, 1).unwrap(), "-0.1");
    }

    /// 正常值路径（设计 §9.2）：均须 `Ok` 且字节精确。
    ///
    /// **反向验证**：任何"顺手放宽"的改动若把正常值也判成丢值，此处立即变红。
    #[test]
    fn nc_fixed_allows_normal_values() {
        assert_eq!(filter_nc_fixed(0.001, 3).unwrap(), "0.001");
        assert_eq!(filter_nc_fixed(-0.001, 3).unwrap(), "-0.001");
        assert_eq!(filter_nc_fixed(21.0, 3).unwrap(), "21.000");
        assert_eq!(filter_nc_fixed(0.4, 3).unwrap(), "0.400");
        assert_eq!(filter_nc_fixed(1.0, 0).unwrap(), "1");
        assert_eq!(filter_nc_fixed(1200.0, 0).unwrap(), "1200");
        assert_eq!(filter_nc_fixed(200.0, 0).unwrap(), "200");
        assert_eq!(filter_nc_fixed(-1.0, 0).unwrap(), "-1");
        assert_eq!(filter_nc_signed(-0.0, 3).unwrap(), "+0.000");
        assert_eq!(filter_nc_signed(0.001, 3).unwrap(), "+0.001");
    }

    /// ★ tie 点钉子（设计 §9.3，**必须 `Err`，不得被"修"掉**）。
    ///
    /// 三个 tie：`0.5|0`、`5e-7|6`、`5e-8|7`。它们的十进制值恰是 `0.5×10^(-N)`，
    /// 但 f64 真值有的**恰等于**、有的**略小于**该十进制值，导致：
    ///   - 字符串判零（FORM_A）：`{:.*}` 取偶后确实是零串 → **必须 Err**（正确）；
    ///   - 量级式（FORM_B）：`|v| < 0.5×10^(-N)` 为假 → 会**放过**（错的）。
    ///
    /// 这**不是 bug**：`0.5 | nc_fixed(0)` 的真实来源 `U_RTRF=1`（进给 1 mm/min）
    /// 本身即不合理工艺值。裁定不加豁免（设计 §1.5.1 / §13 决策点 6）。
    ///
    /// **反向验证**：把判据改成量级式 `|v| < 0.5*10f64.powi(-(N as i32))`，
    /// 下面前两个断言立即变红（它们会变 `Ok`）。这是本缺陷最容易复发的点。
    #[test]
    fn nc_fixed_tie_points_must_error() {
        // N=0：0.5f64 恰为 0.5，{:0} 取偶给 "0" → 零串 → Err
        assert!(
            filter_nc_fixed(0.5, 0).is_err(),
            "0.5|nc_fixed(0)：取偶舍入给 0，与 0.4 同分支；设计 §1.5.1"
        );
        // N=6：0.0000005 的 f64 真值 = 4.99999999999999977374e-7，{:6} 给 "0.000000"
        assert!(
            filter_nc_fixed(5e-7, 6).is_err(),
            "5e-7|nc_fixed(6)：设计 §3.1.0 分歧点 2"
        );
        // N=7：同理
        assert!(
            filter_nc_fixed(5e-8, 7).is_err(),
            "5e-8|nc_fixed(7)：设计 §3.1.0 分歧点 3"
        );
        // 反向钉子：同为 tie 但舍入向上 → 必须 Ok（不得误报）
        assert_eq!(filter_nc_fixed(0.05, 1).unwrap(), "0.1");
        assert_eq!(filter_nc_fixed(0.005, 2).unwrap(), "0.01");
        assert_eq!(filter_nc_fixed(0.0005, 3).unwrap(), "0.001");
    }

    /// `nc_fixed` 与 `nc_signed` 判据**同源**（设计 §9.6 第 7 条）：
    /// 对同一组输入，「是否报错」必须完全一致（二者只差符号前缀）。
    ///
    /// **反向验证**：若只给其中一个加硬失败（P2-3 的历史错误），
    /// 下列 `assert_eq!(a.is_err(), b.is_err())` 会在分歧点立即变红。
    #[test]
    fn nc_fixed_and_nc_signed_share_the_same_criterion() {
        let cases: &[(f64, usize)] = &[
            (1e-4, 3),
            (4e-4, 3),
            (4.9e-4, 3),
            (5e-324, 3),
            (0.0, 3),
            (-0.0, 3),
            (0.0005, 3),
            (-0.0005, 3),
            (0.001, 3),
            (-0.001, 3),
            (0.05, 1),
            (0.5, 0),
            (5e-7, 6),
            (5e-8, 7),
            (0.4, 0),
            (21.0, 3),
        ];
        for &(v, n) in cases {
            let fixed_err = filter_nc_fixed(v, n).is_err();
            let signed_err = filter_nc_signed(v, n).is_err();
            assert_eq!(
                fixed_err, signed_err,
                "nc_fixed({v}e,{n}) 与 nc_signed({v}e,{n}) 判据必须一致"
            );
        }
    }

    /// 回归（P2-4）：上界检查差一。
    ///
    /// `i64::MAX as f64` 恰为 2^63（`i64::MAX` 本身在 f64 里不可表示），
    /// 用 `>` 会让 2^63 通过检查、被 `as i64` **饱和**成 `i64::MAX` ——
    /// 静默输出一个错误的程序号，正是该检查要拦的东西。
    #[test]
    fn nc_pad_rejects_exact_i64_upper_bound() {
        let two_pow_63 = i64::MAX as f64;
        assert_eq!(
            two_pow_63 as i64,
            i64::MAX,
            "前提：`as i64` 对 2^63 是饱和而非回绕"
        );
        assert!(
            filter_nc_pad(two_pow_63, 20).is_err(),
            "2^63 必须报错，而不是饱和成 i64::MAX 后输出"
        );
        // 正常量级不受影响
        assert_eq!(filter_nc_pad(12345.0, 6).unwrap(), "012345");
    }

    /// P2-5：文档说「输入为浮点数时截断小数部分取整」，实现却是**拒绝**小数。
    /// 钉住真实行为，免得有人照文档"修"成截断 —— 截断会让 `N{{ n | nc_pad(4) }}`
    /// 在 `n=1.7` 时静默产出 `N0001`。
    #[test]
    fn nc_pad_rejects_fractional_input() {
        assert!(filter_nc_pad(1.7, 4).is_err());
        assert!(filter_nc_pad(-1.0, 4).is_err());
    }
}
