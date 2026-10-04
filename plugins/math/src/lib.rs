//! Optional numerical extensions; never registered by the template engine.
use nctool_plugin_sdk::*;
fn checked_math(value: f64, name: &'static str) -> Result<f64, nctool_tpl::EngineError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(nctool_tpl::EngineError::new(
            nctool_tpl::EngineErrorKind::InvalidOperation,
            format!("{name}: non-finite result (NaN/Inf)"),
        ))
    }
}
pub fn configure(renderer: &mut nctool_tpl::Renderer) -> Result<(), nctool_tpl::TplError> {
    renderer.add_filter("sin", |v: f64| checked_math(v.sin(), "sin"))?;
    renderer.add_filter("cos", |v: f64| checked_math(v.cos(), "cos"))?;
    renderer.add_filter("tan", |v: f64| checked_math(v.tan(), "tan"))?;
    renderer.add_filter("asin", |v: f64| checked_math(v.asin(), "asin"))?;
    renderer.add_filter("acos", |v: f64| checked_math(v.acos(), "acos"))?;
    renderer.add_filter("atan", |v: f64| checked_math(v.atan(), "atan"))?;
    renderer.add_filter("sqrt", |v: f64| checked_math(v.sqrt(), "sqrt"))?;
    renderer.add_filter("exp", |v: f64| checked_math(v.exp(), "exp"))?;
    renderer.add_filter("ln", |v: f64| checked_math(v.ln(), "ln"))?;
    renderer.add_filter("log10", |v: f64| checked_math(v.log10(), "log10"))?;
    renderer.add_filter("pow", |v: f64, e: f64| checked_math(v.powf(e), "pow"))?;
    renderer.add_filter("floor", |v: f64| checked_math(v.floor(), "floor"))?;
    renderer.add_filter("ceil", |v: f64| checked_math(v.ceil(), "ceil"))?;
    // 角度制三角函数（工艺图纸按度输入）：`_d` 后缀 = degrees。
    // 见上方「角度制 vs 弧度制」——新模板一律用这组，避免静默错坐标。
    renderer.add_filter("sin_d", |v: f64| {
        checked_math(v.to_radians().sin(), "sin_d")
    })?;
    renderer.add_filter("cos_d", |v: f64| {
        checked_math(v.to_radians().cos(), "cos_d")
    })?;
    renderer.add_filter("tan_d", |v: f64| {
        checked_math(v.to_radians().tan(), "tan_d")
    })?;
    // 反三角以度输出（`asin_d(0.5)` → `30`）
    renderer.add_filter("asin_d", |v: f64| {
        checked_math(v.asin().to_degrees(), "asin_d")
    })?;
    renderer.add_filter("acos_d", |v: f64| {
        checked_math(v.acos().to_degrees(), "acos_d")
    })?;
    renderer.add_filter("atan_d", |v: f64| {
        checked_math(v.atan().to_degrees(), "atan_d")
    })?;
    Ok(())
}
pub struct MathPlugin;
impl Plugin for MathPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        Self::definition()
    }
    fn actions(&self) -> Vec<ActionDescriptor> {
        Self::action_definitions()
    }
    fn invoke(&self, _action: &str, _input: Value, _ctx: &Context) -> PluginResult<ActionResult> {
        Ok(ActionResult::data(json!({"extension":"math"})))
    }
    fn extensions(&self) -> Vec<RenderExtension> {
        vec![RenderExtension::new("math", configure)]
    }
}

impl MathPlugin {
    pub fn definition() -> PluginDescriptor {
        let mut d = PluginDescriptor::builtin("math");
        d.provides = vec![Service::new("math.ready", "math.info")];
        d
    }
}

impl MathPlugin {
    pub fn action_definitions() -> Vec<ActionDescriptor> {
        vec![ActionDescriptor::new(
            "math.info",
            "Math extensions",
            object_schema(),
        )]
    }
}
