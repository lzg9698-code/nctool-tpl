//! 数据模型：参数规格与参数集、机床配置、渲染上下文构建。
//!
//! 所有类型均派生 `serde` 序列化，便于从 JSON/YAML 配置文件加载
//! （CLI 场景）或持久化。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 渲染上下文值类型。
///
/// 取自 `nctool_tpl::Value`（即 `minijinja::Value` 的再导出）——本 crate
/// **不直接依赖 minijinja**，避免与模板库解析到不同版本而产生两个不兼容的
/// `Value` 类型。详见 [`nctool_tpl::Value`] 的说明。
use nctool_tpl::Value;

/// 参数值：支持数值 / 整数 / 字符串 / 布尔四种类型。
///
/// G-code 参数绝大多数为数值（坐标、进给、转速），少量为字符串（刀具名、注释）
/// 或布尔（开/关开关）。
///
/// **整数型（`Integer`）用于程序号、刀具号、刀长补偿号等天然为整数的参数**：
/// 这类值若用 `Number` 承载，`nc_strip`/`nc_pad` 会因浮点表示输出 `T5.5` 这类
/// 非法字址，或被 `trunc()` 静默截断（`prog=1.7` → `O0001` 且不报错）。
/// 用独立类型承载可在校验层就拒绝非整数值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ParamValue {
    /// 数值参数（坐标、进给、转速等）
    Number(f64),
    /// 整数参数（程序号、刀具号、补偿号等天然为整数的量）
    Integer(i64),
    /// 字符串参数（刀具名、注释、文本类）
    String(String),
    /// 布尔参数（开关类）
    Bool(bool),
}

impl ParamValue {
    /// 数值视图：`Number` 返回其值，其余返回 `None`。
    pub fn as_number(&self) -> Option<f64> {
        match self {
            ParamValue::Number(v) => Some(*v),
            _ => None,
        }
    }

    /// 整数视图：`Integer` 返回其值；`Number` 为整值时也返回（如 `5.0` → `5`）。
    ///
    /// 非整值的 `Number`（如 `5.5`）返回 `None` —— 调用方可据此区分
    /// "整数值" 与 "恰好写成浮点的整数"。
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            ParamValue::Integer(v) => Some(*v),
            ParamValue::Number(v) if v.is_finite() && v.fract() == 0.0 => Some(*v as i64),
            _ => None,
        }
    }

    /// 数值视图（跨 Number / Integer）：用于 min/max 等区间比较。
    ///
    /// 非数值类型返回 `None`。`Integer` 转为 `f64`（i64 在 f64 的 53 位精度内
    /// 可能丢精度，但 CNC 参数的量级远远不到，可接受）。
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ParamValue::Number(v) => Some(*v),
            ParamValue::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// 字符串视图：`String` 返回其引用，其余返回 `None`。
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ParamValue::String(v) => Some(v.as_str()),
            _ => None,
        }
    }

    /// 布尔视图：`Bool` 返回其值，其余返回 `None`。
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParamValue::Bool(v) => Some(*v),
            _ => None,
        }
    }
}

/// 参数类型（用于校验）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    /// 数值
    Number,
    /// 整数（程序号、刀具号、补偿号等）
    Integer,
    /// 字符串
    String,
    /// 布尔
    Bool,
}

impl ParamKind {
    /// 该类型是否与给定参数值匹配。
    ///
    /// 匹配规则有意保持宽松、但守住整数语义：
    /// - `Number` 接受 `Number` 与 `Integer`（整数是数值的特例）
    /// - `Integer` 接受 `Integer`，以及**整值**的 `Number`（`5.0` 通过，`5.5` 拒绝）
    ///
    /// 第二条是 `Integer` 的存在意义：程序号/刀具号若允许 `5.5`，会产出
    /// `T5.5` 这类非法字址，或被 `nc_pad` 静默截断成 `O0001` 而不报错。
    pub fn matches(&self, value: &ParamValue) -> bool {
        match (self, value) {
            (ParamKind::Number, ParamValue::Number(_)) => true,
            (ParamKind::Number, ParamValue::Integer(_)) => true,
            (ParamKind::Integer, ParamValue::Integer(_)) => true,
            // 整值浮点数视为合法整数（CLI/JSON 常把 5 解析成 5.0）
            (ParamKind::Integer, ParamValue::Number(v)) => v.is_finite() && v.fract() == 0.0,
            (ParamKind::String, ParamValue::String(_)) => true,
            (ParamKind::Bool, ParamValue::Bool(_)) => true,
            _ => false,
        }
    }

    /// 人类可读的类型名。
    pub fn label(&self) -> &'static str {
        match self {
            ParamKind::Number => "数值",
            ParamKind::Integer => "整数",
            ParamKind::String => "字符串",
            ParamKind::Bool => "布尔",
        }
    }
}

/// 参数规格：模板的元数据，描述一个参数的类型、必选性、默认值与用途。
///
/// 由模板注册表维护，用于渲染前的参数校验与 CLI 帮助信息展示。
///
/// **必选性语义**：`required` 是**文档性声明**（用于帮助信息），实际的
/// 必选性由**模板引用**决定——模板引用了该参数且无 `default` 兜底时即必选。
/// 因此 `required` 与 `default` 的取值以模板实际引用情况为准，规格中的声明
/// 主要用于人类可读的说明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamSpec {
    /// 参数名（与模板中变量名一致）
    pub name: String,
    /// 参数类型
    pub kind: ParamKind,
    /// 是否必选（**文档性声明**；实际必选性由模板引用 + 是否有 default 决定）
    pub required: bool,
    /// 默认值（可选参数缺失时的兜底）。
    ///
    /// 与 `required` **无互斥约束**：`required` 仅是文档性声明，实际必选性由
    /// 模板引用决定；两者可同时声明（`required=true + default` 意为"文档上
    /// 必选，但缺失时可用默认值兜底"）。
    pub default: Option<ParamValue>,
    /// 数值下界（**含边界**）；仅对数值/整数参数生效，`None` 表示不限。
    ///
    /// CNC 关键约束（进给率 > 0、主轴转速 ≥ 0、切削深度 ≤ 0 等）在此表达；
    /// 校验通过即保证参数落在工艺允许区间内。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// 数值上界（**含边界**）；仅对数值/整数参数生效，`None` 表示不限。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// 是否要求取**整数值**（数值参数拒绝 `5.5` 这类带小数的值）。
    ///
    /// 用途：程序号、刀具号、刀长补偿号等天然为整数的参数。缺失此约束时，
    /// `prog=1.7` 会被 `nc_pad` 静默截断为 `O0001`、`tool_num=5.5` 会输出
    /// 非法字址 `T5.5`，两者都不报错 —— 这是本字段要堵住的问题。
    ///
    /// 与 [`ParamKind::Integer`] 的区别：本字段是**附加在现有类型上的约束**
    /// （如 `Number` + `integer=true`），而 `ParamKind::Integer` 是**独立类型**。
    /// 给新参数建模时优先用 `ParamKind::Integer`。
    #[serde(default, skip_serializing_if = "is_false")]
    pub integer: bool,
    /// 计量单位（如 `mm`、`mm/min`、`r/min`）。
    ///
    /// 仅用于文档展示与错误提示，**不做任何单位换算**（换算涉及模板内
    /// `nc_fixed` 精度与机床单位制，需由上层显式处理）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// 用途说明（文档/错误提示用）
    pub description: String,
}

/// `bool` 的 serde 跳过判定（`skip_serializing_if` 需要 `&bool → bool` 的函数）。
fn is_false(b: &bool) -> bool {
    !*b
}

impl ParamSpec {
    /// 构造最小规格（无约束、无默认值）。
    pub fn new(name: impl Into<String>, kind: ParamKind, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind,
            required: false,
            default: None,
            min: None,
            max: None,
            integer: false,
            unit: None,
            description: description.into(),
        }
    }

    /// 设置数值下界（含）。
    pub fn with_min(mut self, min: f64) -> Self {
        self.min = Some(min);
        self
    }

    /// 设置数值上界（含）。
    pub fn with_max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }

    /// 设置取值区间（含两端），等价于 `with_min(min).with_max(max)`。
    pub fn with_range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// 要求取整数值。
    pub fn require_integer(mut self) -> Self {
        self.integer = true;
        self
    }

    /// 设置计量单位（仅文档与错误提示用，不参与换算）。
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// 设置文档性必选标记。
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// 设置默认值。
    pub fn with_default(mut self, default: ParamValue) -> Self {
        self.default = Some(default);
        self
    }

    /// 单位后缀（用于错误提示与帮助信息），如 ` (mm/min)`；无单位则为空串。
    pub fn unit_suffix(&self) -> String {
        match &self.unit {
            Some(u) => format!(" {u}"),
            None => String::new(),
        }
    }
}

/// 参数集：一组具名参数值。
///
/// 键为参数名，值为 [`ParamValue`]。内部用 `BTreeMap` 保证顺序稳定、可序列化。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParameterSet {
    /// 参数名 → 参数值
    pub values: BTreeMap<String, ParamValue>,
}

/// 将 [`ParamValue`] 转为 minijinja 裸值（用于渲染上下文）。
fn param_to_minijinja(v: &ParamValue) -> Value {
    match v {
        ParamValue::Number(n) => Value::from_serialize(n),
        // 整数以 i64 裸值注入：`{{ tool_num }}` 输出 `5` 而非 `5.0`，
        // 且能被 `nc_pad`/`nc_strip` 安全格式化。
        ParamValue::Integer(n) => Value::from_serialize(n),
        ParamValue::String(s) => Value::from_serialize(s),
        ParamValue::Bool(b) => Value::from_serialize(b),
    }
}

impl ParameterSet {
    /// 创建空参数集。
    pub fn new() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    /// 设置数值参数。
    pub fn set_number(&mut self, name: impl Into<String>, value: f64) -> &mut Self {
        self.values.insert(name.into(), ParamValue::Number(value));
        self
    }

    /// 设置整数参数（程序号、刀具号、补偿号等）。
    ///
    /// 用于承载天然为整数的量，避免 `Number` 路径下的浮点格式化与静默截断。
    pub fn set_integer(&mut self, name: impl Into<String>, value: i64) -> &mut Self {
        self.values.insert(name.into(), ParamValue::Integer(value));
        self
    }

    /// 设置字符串参数。
    pub fn set_string(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.values
            .insert(name.into(), ParamValue::String(value.into()));
        self
    }

    /// 设置布尔参数。
    pub fn set_bool(&mut self, name: impl Into<String>, value: bool) -> &mut Self {
        self.values.insert(name.into(), ParamValue::Bool(value));
        self
    }

    /// 读取参数值。
    pub fn get(&self, name: &str) -> Option<&ParamValue> {
        self.values.get(name)
    }

    /// 是否包含指定参数。
    pub fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    /// 参数数量。
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// 合并另一个参数集（后者覆盖同名项）。
    pub fn merge(&mut self, other: &ParameterSet) {
        for (k, v) in &other.values {
            self.values.insert(k.clone(), v.clone());
        }
    }

    /// 转换为 minijinja 渲染上下文。
    ///
    /// 模板中通过变量名直接引用参数（如 `{{ x }}`）。数值/字符串/布尔均以
    /// **裸值**注入（数值→数字、字符串→字符串、布尔→布尔），而非 serde 的
    /// 带标签对象。机床配置等系统参数由调用方在渲染时单独合并。
    pub fn to_minijinja_value(&self) -> Value {
        let map: std::collections::BTreeMap<&str, Value> = self
            .values
            .iter()
            .map(|(k, v)| (k.as_str(), param_to_minijinja(v)))
            .collect();
        Value::from_serialize(&map)
    }
}

/// 机床配置：封装不同机床（WFL/INDEX/通用）的 G-code 编程约定差异。
///
/// 渲染时作为 `machine` 变量注入上下文，模板通过 `{{ machine.xxx }}` 引用：
/// `config` 的全部键值（字符串）以及元信息 `id` / `vendor` / `model` 均可引用，
/// 实现"一套模板适配多种机床"。
///
/// 注意：`config` 值均为字符串，模板中做数值比较需先转换（如 `| int`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MachineConfig {
    /// 机床唯一标识（如 `wfl_m65`、`index_ms40`、`generic`）
    pub id: String,
    /// 厂商（如 `WFL`、`INDEX`、`Generic`）
    pub vendor: String,
    /// 型号（如 `M65`、`MS40`）
    pub model: String,
    /// 编程约定键值对（如 `program_prefix`、`max_spindle_rpm`、`axes`）
    pub config: BTreeMap<String, String>,
}

impl MachineConfig {
    /// 读取配置项。
    pub fn get(&self, key: &str) -> Option<&str> {
        self.config.get(key).map(String::as_str)
    }
}

/// 应用参数规格的默认值兜底（渲染前）：规格声明了 `default`、且参数集未
/// 提供的参数，渲染前自动填入默认值（用户提供的值优先，不被覆盖）。
///
/// 校验层（将规格默认值视为已提供）与渲染层共用本函数，保证
/// "校验通过 ⇒ 渲染不因缺参失败" 的口径一致。
///
/// 优先级：**用户提供的值 > 规格默认值 > 模板内联 `default`**——规格默认值
/// 注入后，模板中的 `{{ x | default(v) }}` 内联兜底不再触发（且规格默认的
/// 数值是 f64，内联默认是字面量，两者格式化输出可能不同）。
pub(crate) fn apply_spec_defaults(specs: &[ParamSpec], params: &ParameterSet) -> ParameterSet {
    let mut effective = params.clone();
    for spec in specs {
        if !effective.contains(&spec.name) {
            if let Some(default) = &spec.default {
                effective.values.insert(spec.name.clone(), default.clone());
            }
        }
    }
    effective
}

/// 构建渲染上下文：`params`（裸值）+ `machine`（机床配置对象）。
///
/// [`ParamValue`] 以**裸值**注入（数值→数字、字符串→字符串、布尔→布尔），
/// 使模板能直接以 `{{ x }}` 引用参数。数值直接经 minijinja 序列化，**不经过
/// JSON 中间层**，因此 NaN/Inf 不会被静默篡改（校验层已拒绝它们进入管线）。
///
/// `machine` 对象包含 `config` 的全部键值（**字符串**，如 `{{ machine.rapid }}`）
/// 以及元信息 `id` / `vendor` / `model`。若 `config` 中存在同名键，元信息优先。
/// 注意 `config` 值均为字符串，模板中做数值比较需先转换（如 `| int`）。
pub(crate) fn build_render_context(params: &ParameterSet, machine: &MachineConfig) -> Value {
    let mut map: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    for (k, v) in &params.values {
        map.insert(k.clone(), param_to_minijinja(v));
    }
    // 注入 machine 对象（config 键值 + 元信息，模板通过 {{ machine.xxx }} 引用）
    let mut machine_obj: std::collections::BTreeMap<&str, Value> =
        std::collections::BTreeMap::new();
    for (k, v) in &machine.config {
        machine_obj.insert(k.as_str(), Value::from(v.as_str()));
    }
    machine_obj.insert("id", Value::from(machine.id.as_str()));
    machine_obj.insert("vendor", Value::from(machine.vendor.as_str()));
    machine_obj.insert("model", Value::from(machine.model.as_str()));
    map.insert("machine".to_string(), Value::from_serialize(&machine_obj));
    Value::from_serialize(&map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_value_views() {
        let n = ParamValue::Number(21.5);
        let s = ParamValue::String("D12".to_string());
        let b = ParamValue::Bool(true);
        assert_eq!(n.as_number(), Some(21.5));
        assert_eq!(n.as_str(), None);
        assert_eq!(s.as_str(), Some("D12"));
        assert_eq!(b.as_bool(), Some(true));
        assert_eq!(s.as_number(), None);
    }

    #[test]
    fn param_kind_matches() {
        assert!(ParamKind::Number.matches(&ParamValue::Number(1.0)));
        assert!(ParamKind::String.matches(&ParamValue::String("a".into())));
        assert!(ParamKind::Bool.matches(&ParamValue::Bool(true)));
        assert!(!ParamKind::Number.matches(&ParamValue::String("a".into())));
        assert!(!ParamKind::Bool.matches(&ParamValue::Number(1.0)));
    }

    #[test]
    fn parameter_set_fluent_api() {
        let mut ps = ParameterSet::new();
        ps.set_number("x", 21.0)
            .set_string("tool", "D12")
            .set_bool("coolant", true);
        assert_eq!(ps.len(), 3);
        assert_eq!(ps.get("x"), Some(&ParamValue::Number(21.0)));
        assert!(ps.contains("coolant"));
        assert!(!ps.contains("missing"));
    }

    #[test]
    fn parameter_set_merge_overrides() {
        let mut a = ParameterSet::new();
        a.set_number("x", 1.0).set_number("y", 2.0);
        let mut b = ParameterSet::new();
        b.set_number("y", 99.0).set_number("z", 3.0);
        a.merge(&b);
        assert_eq!(a.get("x"), Some(&ParamValue::Number(1.0)));
        assert_eq!(a.get("y"), Some(&ParamValue::Number(99.0)));
        assert_eq!(a.get("z"), Some(&ParamValue::Number(3.0)));
    }

    #[test]
    fn parameter_set_to_minijinja_value() {
        let mut ps = ParameterSet::new();
        ps.set_number("x", 21.0);
        let v = ps.to_minijinja_value();
        let x_val = v.get_attr("x").unwrap();
        assert!(x_val.is_number());
        assert_eq!(f64::try_from(x_val.clone()).ok(), Some(21.0));
    }

    #[test]
    fn machine_config_get() {
        let mut m = MachineConfig {
            id: "generic".into(),
            vendor: "Generic".into(),
            model: "CNC".into(),
            config: BTreeMap::new(),
        };
        m.config.insert("max_spindle_rpm".into(), "4000".into());
        assert_eq!(m.get("max_spindle_rpm"), Some("4000"));
        assert_eq!(m.get("missing"), None);
    }

    #[test]
    fn param_kind_label() {
        assert_eq!(ParamKind::Number.label(), "数值");
        assert_eq!(ParamKind::String.label(), "字符串");
        assert_eq!(ParamKind::Bool.label(), "布尔");
    }
}
