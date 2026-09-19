//! 机床配置层：内建机床预设。
//!
//! 不同机床（WFL/INDEX/通用）的 G-code 编程约定存在差异（程序号格式、轴配置、
//! 主轴转速上限、标准 G/M 代码等）。本模块提供内建预设，渲染时作为 `machine`
//! 变量注入上下文，模板通过 `{{ machine.xxx }}` 引用，实现"一套模板适配多机床"。
//!
//! > **注意**：预设中的配置值为**通用编程约定的默认示例**，仅用于模板开发和
//! > 测试。实际投产前，请按具体机床的操作手册核对 G/M 代码、轴配置与参数上限，
//! > 并通过 [`MachineConfig`] 覆盖或新增配置项。

use std::collections::BTreeMap;

use crate::model::MachineConfig;

/// 机床标识（字符串，支持自定义扩展）。
///
/// 内建标识：`generic`、`wfl_m65`、`index_ms40`。
pub type MachineId = String;

/// 内建机床预设。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MachinePreset {
    /// 通用 CNC 机床（最小编程约定，作为默认与测试基线）
    Generic,
    /// WFL M65 多任务车铣复合
    WflM65,
    /// INDEX MS40 车铣复合
    IndexMs40,
}

impl MachinePreset {
    /// 预设对应的机床标识字符串。
    pub fn id(&self) -> MachineId {
        match self {
            MachinePreset::Generic => "generic".to_string(),
            MachinePreset::WflM65 => "wfl_m65".to_string(),
            MachinePreset::IndexMs40 => "index_ms40".to_string(),
        }
    }

    /// 从标识字符串解析预设；非内建标识返回 `None`。
    pub fn from_id(id: &str) -> Option<MachinePreset> {
        match id {
            "generic" => Some(MachinePreset::Generic),
            "wfl_m65" => Some(MachinePreset::WflM65),
            "index_ms40" => Some(MachinePreset::IndexMs40),
            _ => None,
        }
    }

    /// 所有内建预设。
    pub fn all() -> [MachinePreset; 3] {
        [
            MachinePreset::Generic,
            MachinePreset::WflM65,
            MachinePreset::IndexMs40,
        ]
    }

    /// 生成预设对应的 [`MachineConfig`]。
    pub fn config(&self) -> MachineConfig {
        match self {
            MachinePreset::Generic => MachineConfig {
                id: self.id(),
                vendor: "Generic".to_string(),
                model: "CNC".to_string(),
                config: generic_config(),
            },
            MachinePreset::WflM65 => MachineConfig {
                id: self.id(),
                vendor: "WFL".to_string(),
                model: "M65".to_string(),
                config: {
                    let mut c = generic_config();
                    c.extend([
                        ("max_spindle_rpm".to_string(), "3500".to_string()),
                        (
                            "machine_type".to_string(),
                            "mill_turn_multitask".to_string(),
                        ),
                        (
                            "axes".to_string(),
                            "X Z C (turning) / X Y Z B C (milling)".to_string(),
                        ),
                    ]);
                    c
                },
            },
            MachinePreset::IndexMs40 => MachineConfig {
                id: self.id(),
                vendor: "INDEX".to_string(),
                model: "MS40".to_string(),
                config: {
                    let mut c = generic_config();
                    c.extend([
                        ("max_spindle_rpm".to_string(), "5000".to_string()),
                        (
                            "machine_type".to_string(),
                            "mill_turn_twin_spindle".to_string(),
                        ),
                        ("axes".to_string(), "X1 Z1 C1 / X2 Z2 C2 / Y B".to_string()),
                    ]);
                    c
                },
            },
        }
    }

    /// 全部机床清单：内置预设在前，配置文件中新增的（id 不与预设重名）在后。
    ///
    /// 存在意义：这份「预设 + 自定义、按预设 id 去重」的枚举规则此前在
    /// `cli/src/server.rs::machines_list`（HTTP）与
    /// `cli/src/commands/machine.rs::list`（CLI）各写一遍。两份漂移会让 Web UI
    /// 与 CLI 列出不同的机床集合。调用方各自决定展示哪些字段
    /// （HTTP 多带 `config`，CLI 走文本表格）。
    pub fn entries(custom: &BTreeMap<String, MachineConfig>) -> Vec<MachineEntry> {
        let mut out: Vec<MachineEntry> = Self::all()
            .into_iter()
            .map(|p| {
                let cfg = p.config();
                MachineEntry {
                    id: p.id().to_string(),
                    vendor: cfg.vendor,
                    model: cfg.model,
                    config: cfg.config,
                    builtin: true,
                }
            })
            .collect();

        for (id, m) in custom {
            if Self::from_id(id).is_none() {
                out.push(MachineEntry {
                    id: id.clone(),
                    vendor: m.vendor.clone(),
                    model: m.model.clone(),
                    config: m.config.clone(),
                    builtin: false,
                });
            }
        }
        out
    }
}

/// 机床清单条目，见 [`MachinePreset::entries`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineEntry {
    /// 机床标识（`generic` / `wfl_m65` / 配置文件里的自定义 id）
    pub id: String,
    /// 厂商
    pub vendor: String,
    /// 型号
    pub model: String,
    /// 完整编程约定键值
    pub config: BTreeMap<String, String>,
    /// 是否内置预设（`false` = 来自配置文件的自定义机床）
    pub builtin: bool,
}

impl MachineEntry {
    /// 展示用的一行文本（CLI `machine list` 用）。
    ///
    /// 自定义机床带 `(自定义)` 后缀 —— 与内置预设区分，避免用户以为它是预设。
    pub fn display_line(&self) -> String {
        if self.builtin {
            format!("  {:<12} {} {}", self.id, self.vendor, self.model)
        } else {
            format!("  {:<12} {} {} (自定义)", self.id, self.vendor, self.model)
        }
    }
}

/// 机床配置键的值类型（用于 schema 校验）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineKeyKind {
    /// 字符串（G/M 码、标识、描述）
    String,
    /// 整数（位宽、转速上限等）
    Integer,
    /// 枚举字符串（从给定候选中取值）
    Choice(&'static [&'static str]),
}

/// 机床配置键 schema：键名、值类型、默认值、描述。
///
/// 这是 `MachineConfig.config` 的权威键名清单（[`KNOWN_CONFIG_KEYS`]）。
/// 模板通过 `{{ machine.xxx }}` 引用这里列出的键；拼错键名不会报编译错，
/// 机床配置里出现未知键时 [`validate_config_keys`] 会给出警告。
#[derive(Debug, Clone, Copy)]
pub struct MachineKeySchema {
    /// 键名
    pub key: &'static str,
    /// 值类型
    pub kind: MachineKeyKind,
    /// 内建预设默认值（模板 `default` 兜底失效时的回退值）
    pub default: &'static str,
    /// 用途说明
    pub description: &'static str,
}

/// 全部已知的机床配置键（含内建通用键与扩展键）。
///
/// 列表顺序即文档展示顺序。扩展键（如 `axes`）仅特定预设或自定义配置
/// 使用，`default` 仅是文档性说明；校验唯一关心的是键名已知、值合法。
pub const KNOWN_CONFIG_KEYS: &[MachineKeySchema] = &[
    MachineKeySchema {
        key: "program_prefix",
        kind: MachineKeyKind::String,
        default: "O",
        description: "程序号前缀（FANUC 风格 O/Mazak P 等）",
    },
    MachineKeySchema {
        key: "program_digits",
        kind: MachineKeyKind::Integer,
        default: "4",
        description: "程序号位数（前导零填充）",
    },
    MachineKeySchema {
        key: "line_number_prefix",
        kind: MachineKeyKind::String,
        default: "N",
        description: "行号前缀",
    },
    MachineKeySchema {
        key: "line_number_digits",
        kind: MachineKeyKind::Integer,
        default: "4",
        description: "行号位数",
    },
    MachineKeySchema {
        key: "coordinate_system",
        kind: MachineKeyKind::String,
        default: "G54",
        description: "工件坐标系（G54–G59）",
    },
    MachineKeySchema {
        key: "units",
        kind: MachineKeyKind::Choice(&["metric", "imperial"]),
        default: "metric",
        description: "单位制（metric→G21，imperial→G20）",
    },
    MachineKeySchema {
        key: "feed_mode",
        kind: MachineKeyKind::Choice(&["G94", "G95"]),
        default: "G94",
        description: "进给模式（G94 每分进给 / G95 每转进给）",
    },
    MachineKeySchema {
        key: "rapid",
        kind: MachineKeyKind::String,
        default: "G0",
        description: "快速移动 G 码",
    },
    MachineKeySchema {
        key: "linear",
        kind: MachineKeyKind::String,
        default: "G1",
        description: "线性插补 G 码",
    },
    MachineKeySchema {
        key: "clockwise_arc",
        kind: MachineKeyKind::String,
        default: "G2",
        description: "顺时针圆弧 G 码",
    },
    MachineKeySchema {
        key: "ccw_arc",
        kind: MachineKeyKind::String,
        default: "G3",
        description: "逆时针圆弧 G 码",
    },
    MachineKeySchema {
        key: "spindle_on",
        kind: MachineKeyKind::String,
        default: "M3",
        description: "主轴正转 M 码",
    },
    MachineKeySchema {
        key: "spindle_off",
        kind: MachineKeyKind::String,
        default: "M5",
        description: "主轴停止 M 码",
    },
    MachineKeySchema {
        key: "coolant_on",
        kind: MachineKeyKind::String,
        default: "M8",
        description: "冷却开 M 码",
    },
    MachineKeySchema {
        key: "coolant_off",
        kind: MachineKeyKind::String,
        default: "M9",
        description: "冷却关 M 码",
    },
    MachineKeySchema {
        key: "program_end",
        kind: MachineKeyKind::String,
        default: "M30",
        description: "程序结束 M 码（M30/M99 等）",
    },
    MachineKeySchema {
        key: "tool_change",
        kind: MachineKeyKind::String,
        default: "M6",
        description: "换刀 M 码",
    },
    MachineKeySchema {
        key: "max_spindle_rpm",
        kind: MachineKeyKind::Integer,
        default: "6000",
        description: "主轴最高转速（供文档与用户自检，模板可引用）",
    },
    MachineKeySchema {
        key: "machine_type",
        kind: MachineKeyKind::String,
        default: "generic",
        description: "机床类型标识（generic / mill_turn_multitask 等）",
    },
    // 扩展键：无通用默认值，仅特定预设使用
    MachineKeySchema {
        key: "axes",
        kind: MachineKeyKind::String,
        default: "",
        description: "轴配置描述（如 \"X Z C / X Y Z B C\"，文档用途）",
    },
];

/// 校验机床配置：未知键与非法值 → 告警列表。
///
/// 模板拼错 `machine.xxx` 键名不会报错（只在缺失时回退默认值），因此
/// **机床配置侧**必须兜一道：未知键极可能是"模板引用了配置里根本不存在的
/// 键"的症状（如模板 `machine.feed_modd`，配置里超量补了 `feed_modd`
/// 也不会报错——但两者一致时错误被掩盖）。返回非空即说明配置可疑。
///
/// 注意：**自定义机床配置可以合法携带未知键**（新机床的扩展约定），
/// 本函数仅提示，不阻断；调用方（如 `nctool machine show`）展示给用户判断。
pub fn validate_config_keys(cfg: &MachineConfig) -> Vec<String> {
    let mut warnings = Vec::new();
    for (k, v) in &cfg.config {
        match KNOWN_CONFIG_KEYS.iter().find(|s| s.key == k) {
            None => warnings.push(format!(
                "未知配置键: {k}（{{{{ machine.{k} }}}} 引用前请确认拼写；内建键见 `nctool machine show`）"
            )),
            Some(s) => match s.kind {
                MachineKeyKind::Integer => {
                    if v.parse::<i64>().is_err() {
                        warnings.push(format!(
                            "配置键 {k} 期望整数，实际为 {v:?}（模板做 | int 转换时会失败或取默认值）"
                        ));
                    }
                }
                MachineKeyKind::Choice(opts) => {
                    if !opts.contains(&v.as_str()) {
                        warnings.push(format!(
                            "配置键 {k} 取值应属于 {}，实际为 {v:?}",
                            opts
                                .iter()
                                .map(|o| format!(r#""{o}""#))
                                .collect::<Vec<_>>()
                                .join("/")
                        ));
                    }
                }
                MachineKeyKind::String => {
                    // 空串不是"合法的空约定"：这些键要么被模板直接插值（会插出
                    // 空片段），要么参与 `starts_with` 判定 —— 空前缀会让
                    // `starts_with("")` 恒真，导致整份程序一行都不编号且无告警
                    // （见 `pipeline::non_empty_config`）。
                    if v.trim().is_empty() {
                        warnings.push(format!(
                            "配置键 {k} 为空字符串（将静默回退默认值 {:?}；\
                             请直接删除该键或填写有效值）",
                            s.default
                        ));
                    }
                }
            },
        }
    }
    warnings
}

/// 通用编程约定默认值（所有预设的基础）。
fn generic_config() -> BTreeMap<String, String> {
    [
        ("program_prefix", "O"),
        ("program_digits", "4"),
        ("line_number_prefix", "N"),
        ("line_number_digits", "4"),
        ("coordinate_system", "G54"),
        ("units", "metric"),
        ("feed_mode", "G94"),
        ("rapid", "G0"),
        ("linear", "G1"),
        ("clockwise_arc", "G2"),
        ("ccw_arc", "G3"),
        ("spindle_on", "M3"),
        ("spindle_off", "M5"),
        ("coolant_on", "M8"),
        ("coolant_off", "M9"),
        ("program_end", "M30"),
        ("tool_change", "M6"),
        ("max_spindle_rpm", "6000"),
        ("machine_type", "generic"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 守卫（第四轮批次 D）：机床枚举规则（预设 + 自定义、按预设 id 去重）是
    /// CLI 与 HTTP 共用的**单一来源**，在此钉住。此前两处各写一遍。
    #[test]
    fn entries_lists_presets_then_custom_without_duplicates() {
        let mk = |id: &str, vendor: &str| MachineConfig {
            id: id.to_string(),
            vendor: vendor.to_string(),
            model: "M".to_string(),
            config: BTreeMap::new(),
        };
        let mut custom = BTreeMap::new();
        // 与预设重名的自定义项必须被忽略 —— 否则列表里会出现两个 `generic`
        custom.insert("generic".to_string(), mk("generic", "影子"));
        custom.insert("my_lathe".to_string(), mk("my_lathe", "SIEG"));

        let e = MachinePreset::entries(&custom);
        let ids: Vec<&str> = e.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["generic", "wfl_m65", "index_ms40", "my_lathe"]);
        assert!(e[..3].iter().all(|x| x.builtin), "预设应标 builtin");
        assert!(!e[3].builtin);
        assert_eq!(e[3].vendor, "SIEG", "自定义项应取配置里的厂商，不是影子值");

        // 预设条目必须带完整 config（HTTP 侧要用它渲染机床面板）
        assert!(e[0].config.contains_key("program_prefix"));

        // 文本行：自定义项带后缀，预设不带
        assert!(e[3].display_line().contains("(自定义)"));
        assert!(!e[0].display_line().contains("自定义"));
    }

    #[test]
    fn preset_ids_roundtrip() {
        for p in MachinePreset::all() {
            assert_eq!(MachinePreset::from_id(&p.id()), Some(p));
        }
        assert_eq!(MachinePreset::from_id("unknown"), None);
    }

    #[test]
    fn generic_config_has_basic_keys() {
        let c = MachinePreset::Generic.config();
        assert_eq!(c.get("program_prefix"), Some("O"));
        assert_eq!(c.get("linear"), Some("G1"));
        assert_eq!(c.get("coordinate_system"), Some("G54"));
    }

    #[test]
    fn machine_presets_share_base_and_extend() {
        let base = MachinePreset::Generic.config();
        let wfl = MachinePreset::WflM65.config();
        let idx = MachinePreset::IndexMs40.config();
        // 继承通用键
        for key in ["program_prefix", "linear", "coordinate_system"] {
            assert_eq!(wfl.get(key), base.get(key));
            assert_eq!(idx.get(key), base.get(key));
        }
        // 扩展键
        assert_eq!(wfl.vendor, "WFL");
        assert_eq!(idx.vendor, "INDEX");
        assert!(wfl.get("max_spindle_rpm").is_some());
        assert!(idx.get("axes").is_some());
    }

    #[test]
    fn custom_config_overrides() {
        // 自定义机床：从 generic 基础复制并覆盖
        let mut c = MachinePreset::Generic.config();
        c.id = "my_custom".into();
        c.config.insert("max_spindle_rpm".into(), "8000".into());
        assert_eq!(c.get("max_spindle_rpm"), Some("8000"));
        assert_eq!(c.get("linear"), Some("G1"));
    }

    #[test]
    fn generic_config_matches_schema_defaults() {
        // schema 与 generic 预设不得漂移：每个 schema 键的默认值必须等于
        // generic_config() 中的实际值（axes 为扩展键，不在 generic 中）
        let generic = generic_config();
        for s in KNOWN_CONFIG_KEYS {
            if s.key == "axes" {
                assert!(!generic.contains_key(s.key), "扩展键不应出现在 generic");
                continue;
            }
            assert_eq!(
                generic.get(s.key).map(String::as_str),
                Some(s.default),
                "schema 默认值与 generic 预设不一致（键: {}）",
                s.key
            );
        }
        // 反向：generic 的每个键都必须出现在 schema 中
        for k in generic.keys() {
            assert!(
                KNOWN_CONFIG_KEYS.iter().any(|s| s.key == k),
                "generic 预设含有 schema 未登记的键: {k}"
            );
        }
    }

    #[test]
    fn preset_keys_are_all_registered_in_schema() {
        for p in MachinePreset::all() {
            for k in p.config().config.keys() {
                assert!(
                    KNOWN_CONFIG_KEYS.iter().any(|s| s.key == k),
                    "预设 {} 含未登记键: {k}",
                    p.id()
                );
            }
        }
    }

    #[test]
    fn builtin_configs_produce_no_warnings() {
        for p in MachinePreset::all() {
            assert!(
                validate_config_keys(&p.config()).is_empty(),
                "内建预设 {} 不应有配置告警",
                p.id()
            );
        }
    }

    #[test]
    fn unknown_key_warns() {
        // 回归：模板 machine.feed_modd 拼错与配置新增 feed_modd 一致时，
        // 校验层看不出错——机床配置侧必须提示未知键
        let mut c = MachinePreset::Generic.config();
        c.config.insert("feed_modd".into(), "G94".into());
        let warnings = validate_config_keys(&c);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("feed_modd"));
    }

    #[test]
    fn empty_string_value_warns() {
        // 回归（P1）：此前 `MachineKeyKind::String => {}` 不做任何校验，
        // 空串能通过。而空前缀会让 `starts_with("")` 恒真，整份程序的行号
        // 静默消失（`pipeline::non_empty_config` 已回退默认值，这里补上提示）
        let mut c = MachinePreset::Generic.config();
        c.config.insert("line_number_prefix".into(), "".into());
        c.config.insert("program_prefix".into(), "   ".into()); // 纯空白同样算空
        let warnings = validate_config_keys(&c);
        assert_eq!(warnings.len(), 2, "两个空值都应告警: {warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("line_number_prefix")));
        assert!(warnings.iter().any(|w| w.contains("program_prefix")));
        // 消息要给出回退值，否则用户不知道会变成什么
        assert!(
            warnings.iter().any(|w| w.contains("N")),
            "应提示回退默认值: {warnings:?}"
        );
    }

    #[test]
    fn invalid_value_warns() {
        let mut c = MachinePreset::Generic.config();
        c.config.insert("program_digits".into(), "abc".into());
        c.config.insert("units".into(), "imperialish".into());
        let warnings = validate_config_keys(&c);
        assert_eq!(warnings.len(), 2, "两条非法值都应告警: {warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("program_digits")));
        assert!(warnings.iter().any(|w| w.contains("units")));
    }
}
