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
    // Rust f64 Display 已自动去尾零：21.0 → "21"，21.50 → "21.5"
    Ok(format!("{}", value))
}

/// 前导零填充：`{{ n | nc_pad(4) }}` → `0001`（输入 1）。
///
/// 用于程序号（`O0001`）、行号（`N0010`）等需要固定宽度的**非负**整数。
/// 输入为浮点数时截断小数部分取整。负数或非有限数报错
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
    // 避免静默输出错误的程序号/行号
    if value.trunc() > i64::MAX as f64 {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "nc_pad: 输入超出整数范围（截断后超过 i64 上限）",
        ));
    }
    let int_val = value.trunc() as i64;
    Ok(format!("{:0>width$}", int_val, width = width))
}
