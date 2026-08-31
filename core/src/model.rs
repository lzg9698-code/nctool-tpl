//! 数据模型：参数、机床、刀具、零件与工序。
//!
//! 所有类型均派生 `serde` 序列化，便于从 JSON/YAML 配置文件加载
//! （CLI 场景）或持久化。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 参数值：支持数值 / 字符串 / 布尔三种类型。
///
/// G-code 参数绝大多数为数值（坐标、进给、转速），少量为字符串（刀具名、注释）
/// 或布尔（开/关开关）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ParamValue {
    /// 数值参数（坐标、进给、转速等）
    Number(f64),
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
    /// 字符串
    String,
    /// 布尔
    Bool,
}

impl ParamKind {
    /// 该类型是否与给定参数值匹配。
    pub fn matches(&self, value: &ParamValue) -> bool {
        matches!(
            (self, value),
            (ParamKind::Number, ParamValue::Number(_))
                | (ParamKind::String, ParamValue::String(_))
                | (ParamKind::Bool, ParamValue::Bool(_))
        )
    }

    /// 人类可读的类型名。
    pub fn label(&self) -> &'static str {
        match self {
            ParamKind::Number => "数值",
            ParamKind::String => "字符串",
            ParamKind::Bool => "布尔",
        }
    }
}

/// 参数规格：模板的元数据，描述一个参数的类型、必选性、默认值与用途。
///
/// 由模板注册表维护，用于渲染前的参数校验。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamSpec {
    /// 参数名（与模板中变量名一致）
    pub name: String,
    /// 参数类型
    pub kind: ParamKind,
    /// 是否必选（缺失即校验失败）
    pub required: bool,
    /// 默认值（可选参数缺失时的兜底；与 `required` 互斥）
    pub default: Option<ParamValue>,
    /// 用途说明（文档/错误提示用）
    pub description: String,
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
fn param_to_minijinja(v: &ParamValue) -> minijinja::Value {
    match v {
        ParamValue::Number(n) => minijinja::Value::from_serialize(n),
        ParamValue::String(s) => minijinja::Value::from_serialize(s),
        ParamValue::Bool(b) => minijinja::Value::from_serialize(b),
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
    pub fn to_minijinja_value(&self) -> minijinja::Value {
        let map: std::collections::BTreeMap<&str, minijinja::Value> = self
            .values
            .iter()
            .map(|(k, v)| (k.as_str(), param_to_minijinja(v)))
            .collect();
        minijinja::Value::from_serialize(&map)
    }
}

/// 机床配置：封装不同机床（WFL/INDEX/通用）的 G-code 编程约定差异。
///
/// 渲染时作为 `machine` 变量注入上下文，模板通过 `{{ machine.xxx }}` 引用，
/// 实现"一套模板适配多种机床"。
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

/// 刀具。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    /// 刀具号（T 指令）
    pub number: u32,
    /// 刀具名称（如 `D12_3F_LOT`）
    pub name: String,
    /// 直径（mm）
    pub diameter: f64,
    /// 备注
    pub comment: String,
}

/// 工序：一次具体的 G-code 生成任务。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Operation {
    /// 工序标识
    pub id: String,
    /// 工序名称
    pub name: String,
    /// 使用的模板名（模板注册表内）
    pub template: String,
    /// 工序参数
    pub params: ParameterSet,
    /// 工序使用的刀具
    pub tools: Vec<Tool>,
}

/// 零件：一组工序的集合，代表一个待加工零件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// 零件标识（如物料号）
    pub id: String,
    /// 零件名称
    pub name: String,
    /// 材料（如 `18CrNiMo7`）
    pub material: String,
    /// 零件级参数（可被子工序合并）
    pub params: ParameterSet,
    /// 工序列表
    pub operations: Vec<Operation>,
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
