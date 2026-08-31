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
}
