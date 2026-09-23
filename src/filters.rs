//! 数学/NC 数值格式化过滤器（全部基于 Rust 标准库 `f64`，零额外依赖）。
//!
//! 所有过滤器对结果做有限性校验（NaN/Inf 一律转渲染错误）；NC 过滤器
//! 附带数量级上限防护（防巨量分配与饱和截断）。

/// 数学过滤器结果校验：`NaN`/`Inf` 一律转为渲染错误，防止非法数值进入 G-code。
pub(crate) fn checked_math(value: f64, filter: &'static str) -> Result<f64, minijinja::Error> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!("数学过滤器 `{filter}` 输出非有限数（NaN/Inf），拒绝渲染"),
        ))
    }
}

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
    // -0.0 归一到 +0.0：控制器对负零的处理不一致，而 "-0.000" 在图纸上无意义。
    // 同一份逻辑换个过滤器（nc_signed 早已归一）就输出不同字节，是更实际的问题。
    // `-0.0 == 0.0` 为真，故这一条同时覆盖 +0.0。
    let value = if value == 0.0 { 0.0 } else { value };
    Ok(format!("{:.*}", decimals, value))
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
    /// `-0.0001` 保留 3 位就是 `0.000`，舍入前判零漏掉它，仍输出 `-0.000`，
    /// 与本模块「-0.0 归一到 +0.000」的承诺相悖。
    #[test]
    fn nc_signed_normalizes_zero_after_rounding() {
        assert_eq!(filter_nc_signed(-0.0001, 3).unwrap(), "+0.000");
        assert_eq!(filter_nc_signed(-0.4, 0).unwrap(), "+0");
        // 舍入后真的非零 → 符号必须保留（别把归一做过火）
        assert_eq!(filter_nc_signed(-0.002, 3).unwrap(), "-0.002");
        assert_eq!(filter_nc_signed(-1.0, 0).unwrap(), "-1");
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
