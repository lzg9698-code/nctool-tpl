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

/// 参数值：支持数值 / 整数 / 字符串 / 布尔 / 列表五种类型。
///
/// G-code 参数绝大多数为数值（坐标、进给、转速），少量为字符串（刀具名、注释）
/// 或布尔（开/关开关）。
///
/// **整数型（`Integer`）用于程序号、刀具号、刀长补偿号等天然为整数的参数**：
/// 这类值若用 `Number` 承载，`nc_strip`/`nc_pad` 会因浮点表示输出 `T5.5` 这类
/// 非法字址，或被 `trunc()` 静默截断（`prog=1.7` → `O0001` 且不报错）。
/// 用独立类型承载可在校验层就拒绝非整数值。
///
/// **列表型（`List`）用于驱动模板中的循环**：批量工序（多个槽位、多道刀次、
/// 多个孔位）天然是列表。没有列表类型时，这类模板只能靠调用方逐次渲染再拼接，
/// 既无法在单次调用中完成，也无法整体校验。列表元素类型不限，可嵌套。
///
/// # 序列化形式
///
/// 序列化恒为**带标签形式**（`{ "type": "integer", "value": 8 }`），无歧义；
/// 反序列化则**额外接受裸标量**（`8` / `"闭口"` / `true` / `[1, 2]`），
/// 因为手写 YAML 里 `options: ["闭口", "左开口"]`、`values: [0, 8, 12.5]`
/// 才是自然写法，强制带标签会把配置文件变成机器码。两种形式混用亦可。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase", tag = "type", content = "value")]
pub enum ParamValue {
    /// 数值参数（坐标、进给、转速等）
    Number(f64),
    /// 整数参数（程序号、刀具号、补偿号等天然为整数的量）
    Integer(i64),
    /// 字符串参数（刀具名、注释、文本类）
    String(String),
    /// 布尔参数（开关类）
    Bool(bool),
    /// 列表参数（驱动循环：槽位列表、刀次列表、孔位列表等）
    List(Vec<ParamValue>),
}

/// 反序列化：同时接受**带标签形式**与**裸标量**（见 [`ParamValue`] 的序列化形式说明）。
///
/// 用 `deserialize_any` 实现，因此**只适用于自描述格式**（YAML / JSON）——
/// 本项目的模板清单、变量库与参数文件正是这两种，不涉及 bincode 等非自描述格式。
///
/// 带标签形式仍做**类型自洽校验**：`{type: integer, value: 8.5}` 报错而不是
/// 静默截断成 `8`——程序号被截断正是本项目要堵的一类静默错误。
impl<'de> Deserialize<'de> for ParamValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ParamValueVisitor;

        impl<'de> serde::de::Visitor<'de> for ParamValueVisitor {
            type Value = ParamValue;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(
                    "参数值：标量（数值/整数/字符串/布尔）、列表，或 {type, value} 带标签形式",
                )
            }

            fn visit_bool<E>(self, v: bool) -> Result<ParamValue, E> {
                Ok(ParamValue::Bool(v))
            }

            fn visit_i64<E>(self, v: i64) -> Result<ParamValue, E> {
                Ok(ParamValue::Integer(v))
            }

            fn visit_u64<E>(self, v: u64) -> Result<ParamValue, E>
            where
                E: serde::de::Error,
            {
                i64::try_from(v)
                    .map(ParamValue::Integer)
                    .map_err(|_| E::custom(format!("整数 {v} 超出 i64 范围")))
            }

            fn visit_f64<E>(self, v: f64) -> Result<ParamValue, E> {
                Ok(ParamValue::Number(v))
            }

            fn visit_str<E>(self, v: &str) -> Result<ParamValue, E> {
                Ok(ParamValue::String(v.to_string()))
            }

            fn visit_string<E>(self, v: String) -> Result<ParamValue, E> {
                Ok(ParamValue::String(v))
            }

            fn visit_unit<E>(self) -> Result<ParamValue, E>
            where
                E: serde::de::Error,
            {
                Err(E::custom(
                    "参数值不能为 null：请省略该键以使用默认值，或显式提供值",
                ))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<ParamValue, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element::<ParamValue>()? {
                    items.push(item);
                }
                Ok(ParamValue::List(items))
            }

            fn visit_map<A>(self, mut map: A) -> Result<ParamValue, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                use serde::de::Error as _;
                let mut kind: Option<String> = None;
                let mut value: Option<ParamValue> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => kind = Some(map.next_value()?),
                        "value" => value = Some(map.next_value()?),
                        other => {
                            return Err(A::Error::unknown_field(other, &["type", "value"]));
                        }
                    }
                }
                match (kind, value) {
                    (Some(kind), Some(value)) => {
                        coerce_tagged(&kind, value).map_err(A::Error::custom)
                    }
                    (None, _) => Err(A::Error::missing_field("type")),
                    (_, None) => Err(A::Error::missing_field("value")),
                }
            }
        }

        deserializer.deserialize_any(ParamValueVisitor)
    }
}

/// 带标签形式的类型自洽校验：把 `value` 归一为 `kind` 声明的类型。
///
/// `Number` / `Integer` 之间允许无损互转（`{type: number, value: 8}`、
/// `{type: integer, value: 8.0}` 都合法）；其余类型必须严格一致。
///
/// 类型名**大小写不敏感**：序列化恒为小写（与 [`ParamKind`] 一致），但历史
/// 载荷里出现过 PascalCase（`{type: Integer, ...}`），按不敏感匹配可以继续读，
/// 避免一次纯粹的命名规范化变成破坏性变更。
fn coerce_tagged(kind: &str, value: ParamValue) -> Result<ParamValue, String> {
    let mismatch = |want: &str, got: &ParamValue| {
        format!("type={kind} 的值应为{want}，实际为{}", got.type_name())
    };
    match kind.to_ascii_lowercase().as_str() {
        "number" => match value {
            ParamValue::Number(_) => Ok(value),
            ParamValue::Integer(i) => Ok(ParamValue::Number(i as f64)),
            other => Err(mismatch("数值", &other)),
        },
        "integer" => match value {
            ParamValue::Integer(_) => Ok(value),
            ParamValue::Number(n) if n.is_finite() && n.fract() == 0.0 => {
                Ok(ParamValue::Integer(n as i64))
            }
            other => Err(mismatch("整数", &other)),
        },
        "string" => match value {
            ParamValue::String(_) => Ok(value),
            other => Err(mismatch("字符串", &other)),
        },
        "bool" => match value {
            ParamValue::Bool(_) => Ok(value),
            other => Err(mismatch("布尔", &other)),
        },
        "list" => match value {
            ParamValue::List(_) => Ok(value),
            other => Err(mismatch("列表", &other)),
        },
        other => Err(format!(
            "未知的参数值类型 '{other}'（可用：number / integer / string / bool / list）"
        )),
    }
}

impl ParamValue {
    /// 人类可读渲染（错误提示、帮助信息、`inspect` 共用）。
    ///
    /// 字符串加双引号，使 `"8"`（文本候选）与 `8`（数值候选）不会混淆——
    /// 这两者在本模型中是**不同**的候选项。列表无合理语义，降级为 `<列表>`。
    pub fn display(&self) -> String {
        match self {
            ParamValue::Number(n) => format!("{n}"),
            ParamValue::Integer(n) => format!("{n}"),
            ParamValue::String(s) => format!("\"{s}\""),
            ParamValue::Bool(b) => format!("{b}"),
            ParamValue::List(_) => "<列表>".to_string(),
        }
    }

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

    /// 列表视图：`List` 返回其元素切片，其余返回 `None`。
    pub fn as_list(&self) -> Option<&[ParamValue]> {
        match self {
            ParamValue::List(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    /// 类型名（用于错误提示）。
    pub fn type_name(&self) -> &'static str {
        match self {
            ParamValue::Number(_) => "数值",
            ParamValue::Integer(_) => "整数",
            ParamValue::String(_) => "字符串",
            ParamValue::Bool(_) => "布尔",
            ParamValue::List(_) => "列表",
        }
    }

    /// 白名单成员比较：该值是否与候选项 `option` 等价。
    ///
    /// 规则：
    /// - 同变体同值 → 相等（`String` 区分大小写，`Number` 按 `f64` 相等）；
    /// - **数值/整数跨变体按数值比较**（`Integer(8)` 与 `Number(8.0)` 视为同一
    ///   候选项）。CLI/JSON 常把 `8` 解析成 `8.0`，若按变体严格区分，模板作者
    ///   声明的整数候选项会被用户输入的无害浮点写法拒绝——这是**假拒绝**，
    ///   只会打扰用户而不会放行错误值（`8.0 == 8` 在数值上恒等）；
    /// - 其余跨变体一律不等（`Bool(true)` ≠ `Number(1.0)`，避免把开关当数值）。
    ///
    /// 注意**不做字符串与数值的隐式转换**：候选项写 `"8"` 时，用户必须提供
    /// 字符串 `"8"`，提供 `8` 会被判为不在候选项内并报错。CNC 场景下静默
    /// 的类型归一化会掩盖参数模型错误。
    pub fn matches_option(&self, option: &ParamValue) -> bool {
        if self == option {
            return true;
        }
        match (self, option) {
            (ParamValue::Number(a), ParamValue::Integer(b)) => *a == *b as f64,
            (ParamValue::Integer(a), ParamValue::Number(b)) => *a as f64 == *b,
            _ => false,
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
    /// 列表（驱动循环的批量参数）。
    ///
    /// **只校验"是不是列表"，不校验元素类型**——元素类型由模板自身决定
    /// （`{% for p in passes %}{{ p.x }}` 要求元素是对象/结构），
    /// 静态声明元素类型会与模板的隐式结构约定重复且易漂移。
    /// 需要元素级校验时应在模板内用 `{% if %}` 显式表达。
    List,
    /// 枚举（取值必须落在 [`ParamSpec::options`] 声明的白名单内）。
    ///
    /// 与 [`ParamKind::String`] 的区别：`String` 接受任意字符串，`Choice`
    /// 只接受候选值。用于工艺图纸上**有限集合**的选项，例如闭口/左开口/右开口、
    /// 卡簧槽标准宽度 `0 / 8 / 10 / 12.5`、刀尖型号 `B4 / DM24`。
    /// 缺少此类型时，白名单只能写在模板注释里，非法选项会一路渲染成错误 G-code。
    ///
    /// **类型匹配有意放宽为"任意标量"**（字符串/数值/整数/布尔），因为真正的
    /// 约束是白名单而不是类型；数值枚举（`0 / 8 / 10 / 12.5`）必须能通过类型
    /// 检查，才能走到白名单比较那一步。`List` 不在此列——列表永远不可能是
    /// 扁平白名单的成员。
    ///
    /// 白名单为空或未声明时**不做成员检查**（视为不约束），见
    /// [`ParamSpec::accepts_option`]。
    Choice,
    /// 已声明但**未标注类型**：不做类型检查，其余约束（`options` / `min` /
    /// `max` / `integer` / `required_if` / `default`）照常生效。
    ///
    /// 存在的意义是让「只知道有这个参数、还不知道它是什么类型」的阶段也能声明
    /// 参数——典型场景是模板头部 `{# PARAMS: #}` 只写了 `name 必选 描述`
    /// 而没写类型（迁移模板里的常见形态）。此时若强行猜一个类型，会让合法输入
    /// 被误拒；若干脆不生成规格，则连白名单与条件必选都声明不了。
    ///
    /// **这是过渡态，不是目标态**：参数类型应从图纸/源变量库补齐，`Any` 只应
    /// 出现在尚未补全类型的迁移模板上。
    Any,
}

impl ParamKind {
    /// 该类型是否与给定参数值匹配。
    ///
    /// 匹配规则有意保持宽松、但守住整数语义：
    /// - `Number` 接受 `Number` 与 `Integer`（整数是数值的特例）
    /// - `Integer` 接受 `Integer`，以及**整值**的 `Number`（`5.0` 通过，`5.5` 拒绝）
    /// - `List` 只接受 `List`（任何元素类型，见 [`ParamKind::List`] 说明）
    /// - `Choice` 接受任意**标量**（字符串/数值/整数/布尔），拒绝 `List`
    /// - `Any` 接受一切（不约束类型，见 [`ParamKind::Any`] 说明）
    ///
    /// 第二条是 `Integer` 的存在意义：程序号/刀具号若允许 `5.5`，会产出
    /// `T5.5` 这类非法字址，或被 `nc_pad` 静默截断成 `O0001` 而不报错。
    ///
    /// `Choice` 只做"是不是标量"的粗筛，真正的白名单比较在
    /// [`ParamSpec::accepts_option`]（`matches` 拿不到 `options`）。
    pub fn matches(&self, value: &ParamValue) -> bool {
        match (self, value) {
            (ParamKind::Number, ParamValue::Number(_)) => true,
            (ParamKind::Number, ParamValue::Integer(_)) => true,
            (ParamKind::Integer, ParamValue::Integer(_)) => true,
            // 整值浮点数视为合法整数（CLI/JSON 常把 5 解析成 5.0）
            (ParamKind::Integer, ParamValue::Number(v)) => v.is_finite() && v.fract() == 0.0,
            (ParamKind::String, ParamValue::String(_)) => true,
            (ParamKind::Bool, ParamValue::Bool(_)) => true,
            (ParamKind::List, ParamValue::List(_)) => true,
            // 枚举：任意标量都先通过类型关，由白名单决定最终是否合法
            (ParamKind::Choice, ParamValue::List(_)) => false,
            (ParamKind::Choice, _) => true,
            // 未标注类型：不做类型判断
            (ParamKind::Any, _) => true,
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
            ParamKind::List => "列表",
            ParamKind::Choice => "枚举",
            ParamKind::Any => "未标注类型",
        }
    }
}

/// 条件必选：本参数仅在**控制参数**取到指定值时才必选。
///
/// # 为什么需要它
///
/// 变量提取是**静态**的：它不看模板里 `{% if side == "Right" %}` 的运行期取值，
/// 把各分支引用的变量全部视为必选。于是互斥参数（同一时刻只有一个分支可达）
/// 会被要求"全部提供"：
///
/// ```jinja
/// {% if side == "Right" %}
/// G1 Z{{ FS_Z_PLUS1 | nc_fixed(3) }}     {# 只有 Right 用得到 #}
/// {% else %}
/// G1 Z{{ FS_Z_MINUS1 | nc_fixed(3) }}    {# 只有 Left 用得到 #}
/// {% endif %}
/// ```
///
/// 实测：只做右侧槽的用户被要求再填一个左侧专用 Z 值，否则校验不通过。
/// 历史上靠 `| default(0)` 规避——但兜底值会**静默产出 `Z0` 这种错误坐标**，
/// 属于本项目零容忍的"渲染成功但结果错误"。
///
/// 本字段把"哪个分支用到哪个参数"这一事实从模板语义里显式声明出来，
/// 一处声明、全局受益（校验、`inspect`、UI 表单都能读同一份声明）。
///
/// # 语义
///
/// - 控制参数取值命中 [`RequiredIf::values`] → 本参数**必选**；
/// - 未命中 → 本参数**可缺失**（渲染时该分支不可达，未定义变量不会被求值）；
/// - 控制参数**无法判定**（未提供且无规格默认值）→ 保守判为**必选**
///   （分支可能被走到，宁可多要一个参数，也不能放过缺失）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredIf {
    /// 控制参数名（模板中引用的另一个参数）
    pub param: String,
    /// 触发值集合：控制参数取值命中其中之一时，本参数变为必选
    pub values: Vec<ParamValue>,
}

impl RequiredIf {
    /// 构造条件：`param` 取到 `values` 中任一值时本参数必选。
    pub fn new(param: impl Into<String>, values: impl Into<Vec<ParamValue>>) -> Self {
        Self {
            param: param.into(),
            values: values.into(),
        }
    }

    /// 该值是否命中触发条件（数值/整数跨变体按数值比较，见
    /// [`ParamValue::matches_option`]）。
    pub fn triggered_by(&self, value: &ParamValue) -> bool {
        self.values.iter().any(|v| value.matches_option(v))
    }

    /// 人类可读形式（如 `side = "Right"`），用于帮助信息与错误提示。
    pub fn display(&self) -> String {
        let values = self
            .values
            .iter()
            .map(render_option)
            .collect::<Vec<_>>()
            .join(" 或 ");
        format!("{} = {values}", self.param)
    }
}

/// 派生规则：本参数的值由**另一个参数的取值查表**得到，在 Rust 侧计算后注入。
///
/// 见 [`crate::derive`] 的模块文档（为什么要派生、为什么派生值恒胜、失败为何不静默）。
///
/// YAML 写法（`templates/variables.yaml` 或清单 `params`）：
///
/// ```yaml
/// - name: tip_depth
///   kind: number
///   derive:
///     from: tip_model
///     table:
///       - [B4, 8.51]
///       - [DM24, 29.61]
///     fallback: 29.61
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeriveRule {
    /// 源参数名（查表的主键来源）
    pub from: String,
    /// 查表：`(源值, 派生值)` 有序对。
    ///
    /// 键比较走 [`ParamValue::matches_option`]（数值/整数跨变体按数值相等），
    /// 因此表键写 `8` 也能命中调用方传来的 `8.0`。
    pub table: Vec<(ParamValue, ParamValue)>,
    /// 源参数缺失或取值未命中表项时的回退值。
    ///
    /// `None` = **报错**而不是取 0（见 [`crate::derive`] 的"失败不静默"）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<ParamValue>,
}

impl DeriveRule {
    /// 该规则的人类可读形式（如 `由 tip_model 查表（12 项，回退 29.61）`）。
    pub fn display(&self) -> String {
        let fallback = match &self.fallback {
            Some(v) => format!("，回退 {}", render_option(v)),
            None => "，无回退（未命中即报错）".to_string(),
        };
        format!("由 {} 查表（{} 项{fallback}）", self.from, self.table.len())
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
    /// 候选项白名单（枚举约束）。
    ///
    /// **声明了非空白名单，取值就必须落在其中**——本字段对**所有类型**生效，
    /// 而不只对 [`ParamKind::Choice`]：`Number + options` 表达数值枚举
    /// （`0 / 8 / 10 / 12.5`），`String + options` 表达文本枚举
    /// （`B4 / DM24`），`Choice + options` 是语义最明确的写法。
    ///
    /// `None` 或空列表表示**不做白名单检查**，因此旧规格（无此字段）行为不变。
    /// 非法值**不会被静默替换成默认值或首个候选项**——那会产出与图纸不符的
    /// G-code，违背"宁可渲染失败"的项目原则。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<ParamValue>>,
    /// 条件必选：仅当控制参数取到指定值时才必选（见 [`RequiredIf`]）。
    ///
    /// `None` = 无条件（必选性完全由模板引用与 `default` 决定，即既有语义）。
    /// 与 `default` 同时声明时 `default` 优先——有兜底值就不会"缺失"，
    /// 条件必选自然失去意义（不会报错，只是不生效）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_if: Option<RequiredIf>,
    /// 派生规则：声明后本参数由系统在 Rust 侧算好注入（见 [`DeriveRule`]）。
    ///
    /// 派生参数**不要求调用方提供**；调用方提供了也会被派生值覆盖并收到提示。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derive: Option<DeriveRule>,
    /// 用途说明（文档/错误提示用）
    pub description: String,
}

/// `bool` 的 serde 跳过判定（`skip_serializing_if` 需要 `&bool → bool` 的函数）。
fn is_false(b: &bool) -> bool {
    !*b
}

/// 候选项的可读渲染（错误提示与帮助信息用）。
///
/// 统一走 [`ParamValue::display`]，保证"报错里列出的候选值""inspect 列出的候选值"
/// 与"实际参与比较的候选值"三处口径一致。
fn render_option(v: &ParamValue) -> String {
    v.display()
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
            options: None,
            required_if: None,
            derive: None,
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

    /// 声明条件必选：`param` 取到 `values` 中任一值时本参数才必选。
    ///
    /// 见 [`RequiredIf`] 的语义说明（含"控制参数不可判定时保守判必选"）。
    pub fn required_when(
        mut self,
        param: impl Into<String>,
        values: impl Into<Vec<ParamValue>>,
    ) -> Self {
        self.required_if = Some(RequiredIf::new(param, values));
        self
    }

    /// 声明派生规则：本参数的值由 `rule` 在 Rust 侧算好注入（见 [`DeriveRule`]）。
    pub fn with_derive(mut self, rule: DeriveRule) -> Self {
        self.derive = Some(rule);
        self
    }

    /// 设置默认值。
    pub fn with_default(mut self, default: ParamValue) -> Self {
        self.default = Some(default);
        self
    }

    /// 设置候选项白名单（枚举约束）。
    ///
    /// ```ignore
    /// ParamSpec::new("U_FX", ParamKind::Choice, "越程槽形式")
    ///     .with_options([ParamValue::String("闭口".into()), ParamValue::String("左开口".into())])
    /// ```
    ///
    /// 传空集合等价于不声明白名单（不约束取值）。
    pub fn with_options(mut self, options: impl Into<Vec<ParamValue>>) -> Self {
        let options = options.into();
        self.options = if options.is_empty() {
            None
        } else {
            Some(options)
        };
        self
    }

    /// 该值是否通过白名单检查。
    ///
    /// 返回 `None` 表示**未声明有效白名单**（`options` 为 `None` 或空列表），
    /// 调用方不应据此报错；返回 `Some(true/false)` 才是白名单的判定结论。
    ///
    /// 空列表被视同未声明，是为了让"从配置里读到一个空列表"这种退化情形
    /// 表现为"不约束"而不是"拒绝一切值"——后者会让模板在无任何提示的情况下
    /// 完全不可用。
    pub fn accepts_option(&self, value: &ParamValue) -> Option<bool> {
        let options = self.options.as_ref()?;
        if options.is_empty() {
            return None;
        }
        Some(options.iter().any(|o| value.matches_option(o)))
    }

    /// 候选项的人类可读形式（如 `"闭口" / "左开口"`）；无有效白名单时为 `None`。
    ///
    /// 供错误提示、CLI 帮助与 UI 展示共用，保证"报错里列出的候选值"与
    /// "实际参与比较的候选值"是同一份数据。
    pub fn options_display(&self) -> Option<String> {
        let options = self.options.as_ref()?;
        if options.is_empty() {
            return None;
        }
        Some(
            options
                .iter()
                .map(render_option)
                .collect::<Vec<_>>()
                .join(" / "),
        )
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
        // 列表递归转换：元素可为任意类型（含嵌套列表），
        // 保证 `{% for x in items %}` 能用，且 `x` 保留原始类型语义
        // （整数仍是整数，不会被格式化成 `5.0`）。
        ParamValue::List(items) => {
            let converted: Vec<Value> = items.iter().map(param_to_minijinja).collect();
            Value::from_serialize(&converted)
        }
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

    // -------------------------------------------------------------------
    // ParamKind::Choice 与 ParamSpec.options 白名单
    // -------------------------------------------------------------------

    #[test]
    fn choice_matching_and_label() {
        assert_eq!(ParamKind::Choice.label(), "枚举");
        // 标量一律先通过类型关（真正约束是白名单）
        assert!(ParamKind::Choice.matches(&ParamValue::String("闭口".into())));
        assert!(ParamKind::Choice.matches(&ParamValue::Number(12.5)));
        assert!(ParamKind::Choice.matches(&ParamValue::Integer(8)));
        assert!(ParamKind::Choice.matches(&ParamValue::Bool(true)));
        // 列表不可能是扁平白名单的成员
        assert!(!ParamKind::Choice.matches(&ParamValue::List(vec![ParamValue::Number(1.0)])));
    }

    #[test]
    fn options_absent_means_unconstrained() {
        // 旧规格（无 options 字段）行为必须完全不变：不报白名单错误
        let spec = ParamSpec::new("x", ParamKind::Number, "X");
        assert_eq!(spec.accepts_option(&ParamValue::Number(123.0)), None);
        assert_eq!(spec.options_display(), None);

        // 空列表等同未声明（配置里读到空列表时表现为"不约束"而非"拒绝一切"）
        let spec = ParamSpec::new("x", ParamKind::Choice, "X").with_options(Vec::new());
        assert_eq!(spec.options, None);
        assert_eq!(
            spec.accepts_option(&ParamValue::String("任意".into())),
            None
        );
        assert_eq!(spec.options_display(), None);
    }

    #[test]
    fn options_accepts_only_listed_values() {
        let spec = ParamSpec::new("U_FX", ParamKind::Choice, "越程槽形式").with_options([
            ParamValue::String("闭口".into()),
            ParamValue::String("左开口".into()),
            ParamValue::String("右开口".into()),
        ]);
        assert_eq!(
            spec.accepts_option(&ParamValue::String("左开口".into())),
            Some(true)
        );
        assert_eq!(
            spec.accepts_option(&ParamValue::String("上开口".into())),
            Some(false)
        );
        // 字符串候选不因"看起来像数字"而放宽
        assert_eq!(spec.accepts_option(&ParamValue::Number(1.0)), Some(false));
        assert_eq!(
            spec.options_display().as_deref(),
            Some("\"闭口\" / \"左开口\" / \"右开口\"")
        );
    }

    #[test]
    fn numeric_option_matches_across_number_and_integer() {
        // 数值枚举：CLI/JSON 把 8 解析成 8.0 时不应假拒绝
        let spec = ParamSpec::new("U_Q", ParamKind::Choice, "槽宽系列").with_options([
            ParamValue::Integer(0),
            ParamValue::Integer(8),
            ParamValue::Integer(10),
            ParamValue::Number(12.5),
        ]);
        assert_eq!(spec.accepts_option(&ParamValue::Number(8.0)), Some(true));
        assert_eq!(spec.accepts_option(&ParamValue::Integer(8)), Some(true));
        assert_eq!(spec.accepts_option(&ParamValue::Number(12.5)), Some(true));
        assert_eq!(spec.accepts_option(&ParamValue::Number(12.6)), Some(false));
        assert_eq!(spec.accepts_option(&ParamValue::Integer(9)), Some(false));
        // 布尔不被当作数值候选（Bool(true) ≠ Number(1.0)）
        assert_eq!(spec.accepts_option(&ParamValue::Bool(true)), Some(false));
        assert_eq!(spec.options_display().as_deref(), Some("0 / 8 / 10 / 12.5"));
    }

    #[test]
    fn string_and_numeric_options_are_distinct() {
        // 文本 "8" 与数值 8 是不同候选项，不做隐式归一化
        let spec = ParamSpec::new("k", ParamKind::Choice, "k")
            .with_options([ParamValue::String("8".into())]);
        assert_eq!(
            spec.accepts_option(&ParamValue::String("8".into())),
            Some(true)
        );
        assert_eq!(spec.accepts_option(&ParamValue::Number(8.0)), Some(false));
    }

    // -------------------------------------------------------------------
    // ParamValue 的序列化形式（带标签 / 裸标量）
    // -------------------------------------------------------------------

    #[test]
    fn param_value_deserializes_from_bare_scalars() {
        // 手写 YAML 的自然写法：`options: ["闭口", 8, 12.5, true]`
        assert_eq!(
            serde_yaml::from_str::<ParamValue>("闭口").unwrap(),
            ParamValue::String("闭口".into())
        );
        assert_eq!(
            serde_yaml::from_str::<ParamValue>("8").unwrap(),
            ParamValue::Integer(8)
        );
        assert_eq!(
            serde_yaml::from_str::<ParamValue>("12.5").unwrap(),
            ParamValue::Number(12.5)
        );
        assert_eq!(
            serde_yaml::from_str::<ParamValue>("true").unwrap(),
            ParamValue::Bool(true)
        );
        // JSON 同理
        assert_eq!(
            serde_json::from_str::<ParamValue>("[1, \"a\", false]").unwrap(),
            ParamValue::List(vec![
                ParamValue::Integer(1),
                ParamValue::String("a".into()),
                ParamValue::Bool(false),
            ])
        );
    }

    #[test]
    fn param_value_deserializes_from_tagged_form() {
        // 带标签形式（序列化的输出形式）必须仍可读回
        let yaml = "{type: integer, value: 8}";
        assert_eq!(
            serde_yaml::from_str::<ParamValue>(yaml).unwrap(),
            ParamValue::Integer(8)
        );
        // 无损互转：整值浮点可声明为 integer，整数可声明为 number
        assert_eq!(
            serde_yaml::from_str::<ParamValue>("{type: integer, value: 8.0}").unwrap(),
            ParamValue::Integer(8)
        );
        assert_eq!(
            serde_yaml::from_str::<ParamValue>("{type: number, value: 8}").unwrap(),
            ParamValue::Number(8.0)
        );
        assert_eq!(
            serde_yaml::from_str::<ParamValue>("{type: list, value: [1, 2]}").unwrap(),
            ParamValue::List(vec![ParamValue::Integer(1), ParamValue::Integer(2)])
        );
    }

    #[test]
    fn param_value_tagged_form_rejects_type_mismatch() {
        // `{type: integer, value: 8.5}` 必须报错，而不是静默截断成 8 ——
        // 程序号被静默截断正是本项目要堵的一类错误
        assert!(serde_yaml::from_str::<ParamValue>("{type: integer, value: 8.5}").is_err());
        assert!(serde_yaml::from_str::<ParamValue>("{type: string, value: 8}").is_err());
        assert!(serde_yaml::from_str::<ParamValue>("{type: number, value: 闭口}").is_err());
        // 未知类型名与未知字段同样报错（不静默忽略）
        assert!(serde_yaml::from_str::<ParamValue>("{type: integre, value: 8}").is_err());
        assert!(serde_yaml::from_str::<ParamValue>("{kind: integer, value: 8}").is_err());
        assert!(serde_yaml::from_str::<ParamValue>("{type: integer}").is_err());
    }

    #[test]
    fn param_value_rejects_null_and_round_trips_tagged() {
        // null 无法表达参数值：省略键才能表达"用默认值"，显式 null 是写法错误
        assert!(serde_yaml::from_str::<ParamValue>("null").is_err());
        assert!(serde_yaml::from_str::<ParamValue>("~").is_err());

        // 序列化恒为带标签形式（无歧义），保证机器可读
        let json = serde_json::to_string(&ParamValue::Integer(8)).unwrap();
        assert_eq!(json, r#"{"type":"integer","value":8}"#);
        let back: ParamValue = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ParamValue::Integer(8));
    }

    // -------------------------------------------------------------------
    // RequiredIf 条件必选
    // -------------------------------------------------------------------

    #[test]
    fn required_if_trigger_matching() {
        let rif = RequiredIf::new("side", [ParamValue::String("Right".into())]);
        assert!(rif.triggered_by(&ParamValue::String("Right".into())));
        assert!(!rif.triggered_by(&ParamValue::String("Left".into())));
        assert_eq!(rif.display(), "side = \"Right\"");

        // 数值触发值：数值/整数跨变体按数值比较
        let rif2 = RequiredIf::new("U_Q", [ParamValue::Integer(0), ParamValue::Number(12.5)]);
        assert!(rif2.triggered_by(&ParamValue::Number(0.0)));
        assert!(rif2.triggered_by(&ParamValue::Number(12.5)));
        assert!(!rif2.triggered_by(&ParamValue::Number(8.0)));
        assert_eq!(rif2.display(), "U_Q = 0 或 12.5");
    }

    #[test]
    fn required_when_builder_sets_field() {
        let spec = ParamSpec::new("FS_Z_PLUS1", ParamKind::Number, "FS Z+1")
            .required_when("side", [ParamValue::String("Right".into())]);
        let rif = spec.required_if.as_ref().unwrap();
        assert_eq!(rif.param, "side");
        assert!(rif.triggered_by(&ParamValue::String("Right".into())));
    }

    #[test]
    fn required_if_round_trips_yaml() {
        let spec = ParamSpec::new("FS_Z_MINUS1", ParamKind::Number, "FS Z-1")
            .required_when("side", [ParamValue::String("Left".into())]);
        let yaml = serde_yaml::to_string(&spec).unwrap();
        assert!(yaml.contains("required_if"), "{yaml}");
        let back: ParamSpec = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back, spec);
    }

    #[test]
    fn any_kind_matches_everything() {
        assert_eq!(ParamKind::Any.label(), "未标注类型");
        for v in [
            ParamValue::Number(1.0),
            ParamValue::Integer(1),
            ParamValue::String("x".into()),
            ParamValue::Bool(true),
            ParamValue::List(vec![]),
        ] {
            assert!(ParamKind::Any.matches(&v), "Any 应接受 {v:?}");
        }
    }
}
