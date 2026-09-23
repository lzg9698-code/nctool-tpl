//! 预设参数集（模块三）的存储策略：模型 + 文件后端 + 陈旧检测 + 跨模板交集。
//!
//! # 为什么这一层用 serde 全量往返
//!
//! 与 `templates.yaml`（有 24 行人工语义注释，必须定点文本编辑）相反，
//! `presets.yaml` 是**工具自有的新建文件**，不存在人工注释需要保全，
//! 因此可以直接 serde 序列化整份结构 —— 这是设计 §4 的「三种编辑策略」之一。
//!
//! # 陈旧检测
//!
//! 每个预设记录落盘时刻模板的 [`SpecFingerprint`]。载入时重算当前规格指纹，
//! 不一致即判为**陈旧**（[`StaleReport`]），并逐参数给出「失效 / 缺失必选」清单。
//! 之所以能用指纹而非逐字段比对：指纹已覆盖 `name`/`kind`/`required`/区间/
//! 白名单/`required_if`/`derive`/`default` 等全部实质字段（见 `SpecFingerprint`
//! 的实现，位于同级的 `spec_fingerprint` 子模块 —— 它是私有模块，
//! 此处**不能**写成文档链接，否则 `-D rustdoc::private_intra_doc_links` 会红）。
//!
//! # 写路径
//!
//! 所有落盘一律经 [`WriteKernel`]（原子写 + 乐观锁），本模块不自行拼接文件字节。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{ParamSpec, ParameterSet};

use super::{
    validate_asset_name, FileFingerprint, SpecFingerprint, WriteAction, WriteError, WriteKernel,
    WriteOutcome,
};

/// 预设文件默认文件名（落配置目录，**永不落模板根**）。
pub const PRESET_FILE: &str = "presets.yaml";

/// 当前支持的预设文件 schema 版本。
pub const PRESET_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 单个参数预设：绑定某模板的一组参数值 + 落盘规格指纹。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    /// 预设名（同时是文件内的唯一键，受 [`validate_asset_name`] 约束）。
    pub name: String,
    /// 绑定的模板键（如 `turning/demo_gcode.j2`）。
    pub template: String,
    /// 参数集（扁平值模型，不引入嵌套对象）。
    ///
    /// **序列化形状是扁平映射**（`x: 21.0`），而非 `ParameterSet` 的派生形状
    /// （`values: {x: ...}`）——后者会把内部结构泄漏成用户可见的文件格式。
    /// 用 `serde(with)` 做转换而不是改 `ParameterSet` 的 derive：后者是
    /// 共享模型，其序列化形状被清单 / 参数文件契约依赖，不能为预设一处的
    /// 观感去动全局。
    #[serde(with = "params_map")]
    pub params: ParameterSet,
    /// 创建时间（ISO 8601 UTC）。
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// 落盘时刻的模板规格指纹（`fnv1a64:<16 hex>`）。
    #[serde(rename = "specFingerprint")]
    pub spec_fingerprint: String,
}

/// `ParameterSet` ↔ 扁平映射（`{参数名: 值}`）的 serde 桥。
///
/// 复用 [`ParamValue`] 自身的带标签 / 裸标量双形式反序列化能力：
/// 写出的每个值仍是 `{type, value}`（无歧义），读入时也接受裸标量
/// （手写 `x: 21.0` 同样可用）。
mod params_map {
    use super::*;
    use std::collections::BTreeMap;

    pub fn serialize<S: serde::Serializer>(set: &ParameterSet, ser: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&set.values, ser)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(de: D) -> Result<ParameterSet, D::Error> {
        let values: BTreeMap<String, crate::model::ParamValue> =
            serde::Deserialize::deserialize(de)?;
        Ok(ParameterSet { values })
    }
}

/// 预设文件整体结构（版本 1）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetFile {
    /// schema 版本。
    pub version: u32,
    /// 预设列表（按名排序，保证往返稳定）。
    pub presets: Vec<Preset>,
}

impl Default for PresetFile {
    fn default() -> Self {
        Self {
            version: PRESET_SCHEMA_VERSION,
            presets: Vec::new(),
        }
    }
}

impl PresetFile {
    /// 按名查找预设。
    pub fn get(&self, name: &str) -> Option<&Preset> {
        self.presets.iter().find(|p| p.name == name)
    }

    /// 按名删除，返回是否删掉了东西。
    pub fn take(&mut self, name: &str) -> bool {
        let before = self.presets.len();
        self.presets.retain(|p| p.name != name);
        self.presets.len() != before
    }
}

/// 预设载入时的读取结果：内容 + 降级警告。
///
/// **降级哲学（沿用 D13）**：预设文件是工具自有产物，损坏 / 版本未知时
/// **不阻断**其它命令，而是回退为空内容 + 记警告，让用户能自救（备份后重建）。
#[derive(Debug, Clone)]
pub struct LoadOutcome {
    /// 解析所得（降级时为空文件）。
    pub file: PresetFile,
    /// 降级警告（正常读取时为空）。
    pub warnings: Vec<String>,
    /// 读取时是否发生了降级（内容不可信，**禁止在其上做写回**）。
    pub degraded: bool,
}

/// 陈旧检测报告：重算指纹与预设记录不一致时，逐参数给出失效清单。
///
/// 序列化为 **camelCase**（`fingerprintChanged` / `staleParams` / `missingRequired`），
/// 与 `Preset` 的 `specFingerprint` / `createdAt` 保持同一约定——同一份 JSON 里
/// 混用两种命名风格会让消费方必须记住"哪个字段是哪种"，极易写错。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaleReport {
    /// 记录指纹与当前规格指纹是否一致。
    pub fingerprint_changed: bool,
    /// 预设中有、但当前模板规格里**已不存在**的参数名。
    pub stale_params: Vec<String>,
    /// 当前模板**新增的必选**参数（预设里没有 → 应用后仍会因缺参失败）。
    pub missing_required: Vec<String>,
}

impl StaleReport {
    /// 是否陈旧（指纹变化 / 有失效参数 / 有新增必选，任一成立）。
    pub fn is_stale(&self) -> bool {
        self.fingerprint_changed
            || !self.stale_params.is_empty()
            || !self.missing_required.is_empty()
    }
}

/// 跨模板复用报告：把预设应用到**另一个**模板时的三分类。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossTemplateReport {
    /// 两边都有且类型兼容 → 可直接复用。
    pub reusable: Vec<String>,
    /// 两边都有但类型不匹配 → 需用户确认（可能被拒）。
    pub needs_confirm: Vec<String>,
    /// 目标模板需要但预设里没有的必选参数。
    pub missing: Vec<String>,
}

/// 一次预设查询的结果：预设 + 陈旧报告。
#[derive(Debug, Clone)]
pub struct PresetView {
    /// 预设本体。
    pub preset: Preset,
    /// 相对**当前**模板规格的陈旧报告。
    pub stale: StaleReport,
}

// ---------------------------------------------------------------------------
// 存储
// ---------------------------------------------------------------------------

/// 预设存储：文件后端的唯一入口。
///
/// 无状态，所有方法为关联函数；每次写都「读 → 改 → [`WriteKernel`] 落盘」，
/// 并以上一次读取的指纹作为乐观锁 `expect`，避免覆盖他人并发写入。
pub struct PresetStore;

impl PresetStore {
    /// 读取预设文件。
    ///
    /// 文件不存在 → 空内容（非降级）；解析失败 / 版本未知 → 降级 + 警告。
    pub fn load(path: &Path) -> Result<LoadOutcome, WriteError> {
        if !path.exists() {
            return Ok(LoadOutcome {
                file: PresetFile::default(),
                warnings: Vec::new(),
                degraded: false,
            });
        }
        let text = std::fs::read_to_string(path).map_err(|e| super::map_io(e, path))?;
        // ERR-NUM-UNDERFLOW：预设文件是 YAML，其中的数值（如 `value: 1e-400`）会被
        // serde_yaml 静默归零 → 参与渲染就是错误坐标。**先**在文本层做候选提取，
        // 再逐条用 serde_yaml 实测确认（本通道自己的解析器）。命中即硬失败，
        // **不**降级为"损坏文件"警告（否则 1e-400 会被静默当 0 用）。
        if let Some(err) = detect_underflow(path, &text) {
            return Err(err);
        }
        match serde_yaml::from_str::<PresetFile>(&text) {
            Ok(file) if file.version == PRESET_SCHEMA_VERSION => Ok(LoadOutcome {
                file,
                warnings: Vec::new(),
                degraded: false,
            }),
            Ok(file) => Ok(LoadOutcome {
                file: PresetFile::default(),
                warnings: vec![format!(
                    "预设文件版本 {} 不被支持（当前支持 {}）：已忽略内容，写入前请先升级 nctool",
                    file.version, PRESET_SCHEMA_VERSION
                )],
                degraded: true,
            }),
            Err(err) => Ok(LoadOutcome {
                file: PresetFile::default(),
                warnings: vec![format!(
                    "预设文件解析失败（{}）：已忽略内容。原文件未被改动，可手动修复或备份后删除",
                    err
                )],
                degraded: true,
            }),
        }
    }

    /// 全量写回预设文件（原子写 + 乐观锁）。
    ///
    /// `expect`：`None` = 要求文件不存在（首次创建）；`Some(fp)` = 要求指纹一致。
    /// 调用方应先 [`Self::load`] 并用 [`WriteKernel::read_fingerprint`] 取快照。
    ///
    /// **降级载入的路径禁止调用本函数**——那会把被忽略的内容永久覆盖掉。
    pub fn save(
        path: &Path,
        file: &PresetFile,
        expect: Option<FileFingerprint>,
    ) -> Result<WriteOutcome, WriteError> {
        if let Some(rel) = path.file_name().and_then(|s| s.to_str()) {
            validate_asset_name(rel).map_err(|reason| WriteError::PathEscape {
                rel: rel.to_string(),
                reason,
            })?;
        }
        let mut normalized = file.clone();
        normalized.version = PRESET_SCHEMA_VERSION;
        normalized.presets.sort_by(|a, b| a.name.cmp(&b.name));
        let text = serde_yaml::to_string(&normalized)
            .map_err(|e| WriteError::Corrupt(format!("预设序列化失败：{e}")))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| super::map_io(e, parent))?;
        }
        WriteKernel::write_guarded(path, text.as_bytes(), expect)
    }

    /// 插入或替换一个预设（按名），返回写结果。
    ///
    /// 目标文件存在则先取快照作为乐观锁 `expect`；不存在则 `expect = None`。
    pub fn upsert(path: &Path, preset: Preset) -> Result<WriteOutcome, WriteError> {
        let loaded = Self::load(path)?;
        let expect = WriteKernel::read_fingerprint(path)?;
        let mut file = if loaded.degraded {
            // 降级内容不可信：拒绝覆盖，避免把用户手写内容永久抹掉。
            return Err(WriteError::Corrupt(format!(
                "预设文件不可用（{}），为避免覆盖已拒绝写入",
                loaded.warnings.join("；")
            )));
        } else {
            loaded.file
        };
        match file.presets.iter_mut().find(|p| p.name == preset.name) {
            Some(slot) => *slot = preset,
            None => file.presets.push(preset),
        }
        Self::save(path, &file, expect)
    }

    /// 重命名预设（保留模板绑定与参数），返回写结果。
    ///
    /// 新名已存在 → [`WriteError::PathEscape`]（映射为 `name_conflict`(6)）；
    /// 旧名不存在 → [`WriteError::Corrupt`]（映射为 `not_found` 语义，由调用方定）。
    pub fn rename(path: &Path, old: &str, new: &str) -> Result<WriteOutcome, WriteError> {
        validate_asset_name(new).map_err(|reason| WriteError::PathEscape {
            rel: new.to_string(),
            reason,
        })?;
        let loaded = Self::load(path)?;
        if loaded.degraded {
            return Err(WriteError::Corrupt(format!(
                "预设文件不可用（{}），为避免覆盖已拒绝写入",
                loaded.warnings.join("；")
            )));
        }
        let expect = WriteKernel::read_fingerprint(path)?;
        let mut file = loaded.file;
        if file.get(new).is_some() {
            return Err(WriteError::PathEscape {
                rel: new.to_string(),
                reason: "同名预设已存在".to_string(),
            });
        }
        let Some(mut p) = file.get(old).cloned() else {
            return Err(WriteError::NotFound(format!("预设不存在：{old}")));
        };
        file.take(old);
        p.name = new.to_string();
        file.presets.push(p);
        Self::save(path, &file, expect)
    }
    /// 删除预设。
    pub fn remove(path: &Path, name: &str) -> Result<WriteOutcome, WriteError> {
        let loaded = Self::load(path)?;
        if loaded.degraded {
            return Err(WriteError::Corrupt(format!(
                "预设文件不可用（{}），为避免覆盖已拒绝写入",
                loaded.warnings.join("；")
            )));
        }
        let expect = WriteKernel::read_fingerprint(path)?;
        let mut file = loaded.file;
        if !file.take(name) {
            return Err(WriteError::NotFound(format!("预设不存在：{name}")));
        }
        // 动作要报 `Deleted` 而不是透传 `save` 的 `Updated`：删一个预设，
        // 消费方看到的应当是"删除"（详见 `WriteAction::Deleted` 的说明）。
        let mut out = Self::save(path, &file, expect)?;
        out.action = WriteAction::Deleted;
        Ok(out)
    }

    /// 导出单个预设为独立 YAML 文本（自包含，含指纹）。
    pub fn export_preset(preset: &Preset) -> Result<String, WriteError> {
        let file = PresetFile {
            version: PRESET_SCHEMA_VERSION,
            presets: vec![preset.clone()],
        };
        serde_yaml::to_string(&file)
            .map_err(|e| WriteError::Corrupt(format!("预设序列化失败：{e}")))
    }

    /// 从导出文本导入一个或多个预设，逐个经 `validate` 后返回。
    ///
    /// `validate` 由调用方注入（通常是「预设名合法性 + 参数值校验」的组合），
    /// 保证导入的预设与手工新建走**同一套校验**（AC-3.9）。
    pub fn import_presets<E, F>(text: &str, mut validate: F) -> Result<Vec<Preset>, E>
    where
        F: FnMut(&Preset) -> Result<(), E>,
        E: From<WriteError>,
    {
        // ERR-NUM-UNDERFLOW：导入的 YAML 同样在解析前拦下溢（否则 `1e-400` 被静默
        // 归零写回文件，静默损坏落盘）。导入来源无确定路径，用占位 `<导入文本>`。
        if let Some(err) = detect_underflow(Path::new("<导入文本>"), text) {
            return Err(E::from(err));
        }
        let file: PresetFile = serde_yaml::from_str(text)
            .map_err(|e| E::from(WriteError::Corrupt(format!("导入内容解析失败：{e}"))))?;
        if file.version != PRESET_SCHEMA_VERSION {
            return Err(E::from(WriteError::Corrupt(format!(
                "导入内容版本 {} 不被支持（当前支持 {}）",
                file.version, PRESET_SCHEMA_VERSION
            ))));
        }
        for p in &file.presets {
            validate_asset_name(&p.name).map_err(|reason| {
                E::from(WriteError::PathEscape {
                    rel: p.name.clone(),
                    reason,
                })
            })?;
            validate(p)?;
        }
        Ok(file.presets)
    }

    /// 计算某预设相对当前模板规格的陈旧报告。
    ///
    /// `specs` 为当前模板的**有效参数规格**（已合并来源覆盖层）。
    pub fn stale_report(preset: &Preset, specs: &[ParamSpec]) -> StaleReport {
        Self::stale_report_with_vars(preset, specs, None)
    }

    /// 陈旧检测的完整版本：额外用**模板引用的变量名**判定参数是否失效。
    ///
    /// # 为什么需要 `referenced_vars`
    ///
    /// 2 参版本只用 `specs` 判"参数还在不在模板里"。这对**声明了规格**的模板
    /// 成立，但对**没有规格的模板**（无 `{# PARAMS: #}`、变量库也没覆盖）则完全
    /// 失真：`specs` 为空表 → 每个参数都被判成"失效"。实测踩到：一个刚保存的
    /// 预设，在模板未改动的情况下 `list` 就显示 `[陈旧]` 且列出全部参数为失效
    /// —— **狼来了**，真正的失效反而被淹没。
    ///
    /// 因此当调用方能提供 `referenced_vars`（模板实际引用的变量名）时，
    /// 判定改为：**参数既不在规格中、也不被模板引用**，才算失效。
    /// 传 `None` 时退化为原语义（调用方无从获知变量表）。
    pub fn stale_report_with_vars(
        preset: &Preset,
        specs: &[ParamSpec],
        referenced_vars: Option<&BTreeSet<String>>,
    ) -> StaleReport {
        Self::stale_report_full(preset, specs, referenced_vars, &[])
    }

    /// 陈旧检测的**基底实现**：只用规格表 + 模板引用变量判定。
    ///
    /// 这是唯一的判定主体；`stale_report` / `stale_report_with_vars` /
    /// `stale_report_full` 都只是它的不同详略包装。**不要**让它们互相委托，
    /// 否则会形成 `full → with_vars → full` 的无限递归——实测表现为
    /// `thread 'main' has overflowed its stack`（栈溢出），比编译错误更难定位。
    fn stale_report_specs_only(
        preset: &Preset,
        specs: &[ParamSpec],
        referenced_vars: Option<&BTreeSet<String>>,
    ) -> StaleReport {
        let current = SpecFingerprint::of(specs);
        let spec_names: BTreeSet<&str> = specs.iter().map(|s| s.name.as_str()).collect();
        let stale_params: Vec<String> = preset
            .params
            .values
            .keys()
            .filter(|k| {
                if spec_names.contains(k.as_str()) {
                    return false;
                }
                // 无规格但模板仍引用 → 参数依然有效，不算失效。
                match referenced_vars {
                    Some(vars) => !vars.contains(k.as_str()),
                    None => true,
                }
            })
            .cloned()
            .collect();
        // "新增必选"同样要防止误报：只认**当前模板确实存在的必选参数**。
        // 无规格表时无从判定必选性，故此时不列（宁可漏报也不误报），
        // 由 `stale_report_full` 用模板必选变量补上。
        let missing_required: Vec<String> = specs
            .iter()
            .filter(|s| s.required && !preset.params.contains(&s.name))
            .map(|s| s.name.clone())
            .collect();
        StaleReport {
            fingerprint_changed: current != preset.spec_fingerprint,
            stale_params,
            missing_required,
        }
    }

    /// 陈旧检测的**完整**版本：规格表 + 模板引用变量 + 模板必选变量三者齐备。
    ///
    /// - `referenced_vars`：模板引用的全部变量名（判定"参数是否已失效"）
    /// - `required_vars`：模板中**无兜底**的变量名（判定"新增必选"）
    ///
    /// 二者都由上层从模板源码提取（`Analysis.variables` 的 `name` / `optional`），
    /// 使**没有声明规格**的模板也能得到准确的陈旧报告——此类模板 `specs` 为空表，
    /// 只用规格表判定会得出"全部参数失效、且永无新增必选"的错误结论。
    pub fn stale_report_full(
        preset: &Preset,
        specs: &[ParamSpec],
        referenced_vars: Option<&BTreeSet<String>>,
        required_vars: &[String],
    ) -> StaleReport {
        let mut out = Self::stale_report_specs_only(preset, specs, referenced_vars);
        // 规格表已能给出"新增必选"时以规格表为准（它带 `required_if` 等条件语义）；
        // 否则用模板必选变量补上——只看**预设里没有**的那些。
        if out.missing_required.is_empty() {
            let mut extra: Vec<String> = required_vars
                .iter()
                .filter(|n| !preset.params.contains(n.as_str()))
                .cloned()
                .collect();
            extra.sort();
            extra.dedup();
            out.missing_required = extra;
        }
        out
    }

    /// 计算把 `preset` 应用到 `target_specs` 目标模板时的三分类报告。
    pub fn cross_template_report(
        preset: &Preset,
        src_specs: &[ParamSpec],
        target_specs: &[ParamSpec],
    ) -> CrossTemplateReport {
        let src_kind: std::collections::HashMap<&str, &ParamSpec> =
            src_specs.iter().map(|s| (s.name.as_str(), s)).collect();
        let target_kind: std::collections::HashMap<&str, &ParamSpec> =
            target_specs.iter().map(|s| (s.name.as_str(), s)).collect();

        let mut reusable = Vec::new();
        let mut needs_confirm = Vec::new();
        for (name, value) in &preset.params.values {
            let Some(target) = target_kind.get(name.as_str()) else {
                continue; // 目标模板没有该参数：既不复用也不阻断（非必选时才安全，见下方 missing）
            };
            let same_kind = match src_kind.get(name.as_str()) {
                Some(src) => src.kind.matches(value) && target.kind.matches(value),
                None => target.kind.matches(value),
            };
            if same_kind {
                reusable.push(name.clone());
            } else {
                needs_confirm.push(name.clone());
            }
        }
        let mut missing: Vec<String> = target_specs
            .iter()
            .filter(|s| s.required && !preset.params.contains(&s.name))
            .map(|s| s.name.clone())
            .collect();
        reusable.sort();
        needs_confirm.sort();
        missing.sort();
        CrossTemplateReport {
            reusable,
            needs_confirm,
            missing,
        }
    }
}

/// 扫描 YAML 文本中的下溢字面量并用 **serde_yaml** 实测确认，命中则返回错误。
///
/// 两段式（ERR-NUM-UNDERFLOW，设计 §4.1 / 附录 D4）：
/// 1. [`crate::json_num::scan_underflow_candidates_yaml`] 做文本层**候选提取**
///    （宽松预筛 `|真值| < 2^-1000`，绝不漏报，且跳过注释 / 字符串 / 块标量）；
/// 2. [`crate::json_num::confirm_underflow_yaml`] 用 **serde_yaml 实测**确认
///    （解析得 `0.0` 而预筛已保证十进制真值非零 ⇒ 确认为下溢）。
///
/// 返回 `None` ⇒ 无命中（含"候选但经确认均为合法值"）。`path` 用于错误定位，
/// 无确定来源时传占位路径。**不**降级为警告：下溢会污染参数值并产出错误 G-code。
fn detect_underflow(path: &Path, text: &str) -> Option<WriteError> {
    for cand in crate::json_num::scan_underflow_candidates_yaml(text) {
        if crate::json_num::confirm_underflow_yaml(&cand) {
            return Some(WriteError::NumUnderflow {
                path: path.to_path_buf(),
                literal: cand.literal,
                line: cand.line,
                column: cand.column,
            });
        }
    }
    None
}

/// 把预设文件路径的**默认位置**解析出来：配置目录（非模板根）。
///
/// - Windows：`%APPDATA%`
/// - Unix：`$XDG_CONFIG_HOME`，兜底 `$HOME/.config`
/// - 均缺失：当前目录（仍可通过 `--file` 显式指定）
pub fn default_preset_path() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
    };
    let base = base.or_else(|| {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|h| PathBuf::from(h).join(".config"))
    });
    match base {
        Some(dir) => dir.join("nctool").join(PRESET_FILE),
        None => PathBuf::from(PRESET_FILE),
    }
}

/// 校验「预设文件位置不得落在模板根内」（红线 9 / R-9）。
///
/// 相等或位于其下均拒绝：预设进模板根会被注册表当成模板目录扫描。
pub fn ensure_outside_template_root(
    preset_path: &Path,
    template_root: &Path,
) -> Result<(), String> {
    let a = normalize(preset_path);
    let b = normalize(template_root);
    if a.starts_with(&b) {
        return Err(format!(
            "预设文件不得落在模板目录内：{} 位于 {} 之下。\
             请改用配置目录（默认 %APPDATA%\\nctool\\presets.yaml），或指定模板目录之外的路径",
            preset_path.display(),
            template_root.display()
        ));
    }
    Ok(())
}

/// 路径归一（不依赖文件存在）：先转绝对路径，再逐段消解 `..`。
///
/// **为什么不能直接 `canonicalize`**——两个都会让红线**静默失效**的坑：
///
/// 1. **相对路径**：待校验的预设文件通常**还不存在**（"保存前校验"正是常态），
///    `canonicalize` 必然失败。若此时退回手写的相对路径 `templates/presets.yaml`，
///    而根 `templates` 能被 canonicalize 成绝对路径，两边 `starts_with` 比较
///    **恒假** → "预设不得落模板根"这条红线永不触发。故先按 cwd 定基转绝对。
/// 2. **Windows verbatim 前缀**：`canonicalize` 返回 `\\?\C://...` 形式，与
///    未 canonicalize 的 `C://...` 前缀不同，同样导致比较恒假。故末尾统一剥离。
fn normalize(p: &Path) -> PathBuf {
    // 先转绝对（不要求存在）：消除"一边相对、一边绝对"的口径差。
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    };
    let resolved = std::fs::canonicalize(&abs).unwrap_or_else(|_| {
        // 目标不存在：逐段压栈、`..` 弹栈（起点已是绝对路径）。
        let mut out = PathBuf::new();
        for comp in abs.components() {
            match comp {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other.as_os_str()),
            }
        }
        // 再试一次 canonicalize：父目录若存在，可顺带消掉大小写 / 8.3 短名差异。
        match (std::fs::canonicalize(&out), out.file_name()) {
            (Ok(base), Some(name)) => base.join(name),
            _ => out,
        }
    });
    strip_verbatim(resolved)
}

/// 剥掉 Windows `\\?\` verbatim 前缀（含 `\\?\UNC\` 形式）。
fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().into_owned();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest.to_string());
    }
    p
}

/// 生成 ISO 8601 UTC 时间戳（`YYYY-MM-DDTHH:MM:SSZ`），零新增依赖。
///
/// 用民用历算法从 Unix 秒换算，避免引入 `chrono`/`time`。
pub fn now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    iso8601_from_unix(secs)
}

/// Unix 秒 → ISO 8601 UTC 字符串（供 `now_iso8601` 与测试共用）。
pub fn iso8601_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Howard Hinnant 的 `civil_from_days`：天数（自 1970-01-01）→ (年, 月, 日)。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ParamValue;

    fn tmpdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nctool_preset_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample() -> Preset {
        let mut params = ParameterSet::new();
        params.set_number("x", 21.0);
        params.set_integer("tool", 3);
        Preset {
            name: "钻孔".into(),
            template: "drilling/x.j2".into(),
            params,
            created_at: "2026-09-21T00:00:00Z".into(),
            spec_fingerprint: "fnv1a64:0000000000000000".into(),
        }
    }

    /// 构造一份含指定数值字面量的合法预设文件 YAML（用于下溢测试）。
    ///
    /// 字段名用 serde 契约的 camelCase（`createdAt` / `specFingerprint`）。
    fn preset_yaml(literal: &str) -> String {
        format!(
            "version: 1\n\
             presets:\n\
             \x20 - name: gap\n\
             \x20   template: turning/a.j2\n\
             \x20   params:\n\
             \x20     x:\n\
             \x20       type: number\n\
             \x20       value: {literal}\n\
             \x20   createdAt: \"2026-09-21T00:00:00Z\"\n\
             \x20   specFingerprint: \"fnv1a64:0000000000000000\"\n"
        )
    }

    #[test]
    fn load_missing_file_is_empty_not_degraded() {
        let dir = tmpdir("missing");
        let got = PresetStore::load(&dir.join("nope.yaml")).unwrap();
        assert!(!got.degraded);
        assert!(got.file.presets.is_empty());
        assert_eq!(got.file.version, PRESET_SCHEMA_VERSION);
    }

    #[test]
    fn corrupt_file_degrades_with_warning() {
        let dir = tmpdir("corrupt");
        let p = dir.join("presets.yaml");
        std::fs::write(&p, "presets: [ : : oops").unwrap();
        let got = PresetStore::load(&p).unwrap();
        assert!(got.degraded, "损坏内容必须降级");
        assert!(!got.warnings.is_empty());
        // 关键：降级时原文件不得被改动
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "presets: [ : : oops");
    }

    /// ERR-NUM-UNDERFLOW：预设文件里的 `1e-400` 必须**硬失败**（不得降级为 warning）
    /// —— 否则会被静默归零成 `0.0` 当作"生效参数"喂给渲染（错误坐标）。
    #[test]
    fn underflow_literal_is_hard_error() {
        let dir = tmpdir("preset_underflow");
        let p = dir.join("presets.yaml");
        std::fs::write(&p, preset_yaml("1e-400")).unwrap();
        let err = PresetStore::load(&p).expect_err("含 1e-400 的预设文件必须硬失败");
        match err {
            WriteError::NumUnderflow {
                literal,
                line,
                column,
                ..
            } => {
                assert_eq!(literal, "1e-400");
                assert!(line >= 1 && column >= 1);
            }
            other => panic!("应为 NumUnderflow，得到: {other:?}"),
        }
        // 原文件不得被改动（硬失败而非"忽略内容"）
        assert!(std::fs::read_to_string(&p).unwrap().contains("1e-400"));
    }

    /// 负向对照：`5e-324`（合法最小次正规数）→ 照常加载，不得误拒。
    #[test]
    fn legal_subnormal_is_accepted() {
        let dir = tmpdir("preset_subnormal");
        let p = dir.join("presets.yaml");
        std::fs::write(&p, preset_yaml("5e-324")).unwrap();
        let got = PresetStore::load(&p).expect("合法次正规数不得被误拒");
        assert!(!got.degraded);
        assert_eq!(
            got.file.get("gap").unwrap().params.get("x"),
            Some(&ParamValue::Number(5e-324))
        );
    }

    /// 导入通道同口径：`1e-400` → `import_presets` 返回 Err。
    #[test]
    fn import_rejects_underflow_literal() {
        let err = PresetStore::import_presets::<WriteError, _>(&preset_yaml("1e-400"), |_| Ok(()))
            .unwrap_err();
        assert!(matches!(err, WriteError::NumUnderflow { .. }), "{err:?}");
    }

    /// 导入通道负向对照：`5e-324` → 正常导入。
    #[test]
    fn import_accepts_legal_subnormal() {
        let got = PresetStore::import_presets::<WriteError, _>(&preset_yaml("5e-324"), |_| Ok(()))
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].params.get("x"), Some(&ParamValue::Number(5e-324)));
    }

    #[test]
    fn unknown_version_degrades_and_write_is_refused() {
        let dir = tmpdir("version");
        let p = dir.join("presets.yaml");
        std::fs::write(&p, "version: 99\npresets: []\n").unwrap();
        let got = PresetStore::load(&p).unwrap();
        assert!(got.degraded);
        assert!(got.warnings[0].contains("99"));
        // 未知版本下写回被拒（否则会把未来格式永久抹掉）
        let err = PresetStore::upsert(&p, sample()).unwrap_err();
        assert!(matches!(err, WriteError::Corrupt(_)), "{err:?}");
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "version: 99\npresets: []\n"
        );
    }

    #[test]
    fn upsert_roundtrip_preserves_values_and_fingerprint() {
        let dir = tmpdir("roundtrip");
        let p = dir.join("presets.yaml");
        PresetStore::upsert(&p, sample()).unwrap();
        let got = PresetStore::load(&p).unwrap();
        assert!(!got.degraded);
        let one = got.file.get("钻孔").expect("应能按名取回");
        assert_eq!(one.template, "drilling/x.j2");
        assert_eq!(one.spec_fingerprint, "fnv1a64:0000000000000000");
        assert_eq!(one.params.get("x"), Some(&ParamValue::Number(21.0)));
        assert_eq!(one.params.get("tool"), Some(&ParamValue::Integer(3)));
    }

    #[test]
    fn upsert_is_idempotent_and_sorted() {
        let dir = tmpdir("sorted");
        let p = dir.join("presets.yaml");
        let mut b = sample();
        b.name = "b".into();
        let mut a = sample();
        a.name = "a".into();
        PresetStore::upsert(&p, b).unwrap();
        PresetStore::upsert(&p, a).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        let ia = text.find("- name: a").unwrap();
        let ib = text.find("- name: b").unwrap();
        assert!(ia < ib, "预设应按名排序写出：\n{text}");
    }

    #[test]
    fn rename_preserves_payload_and_rejects_duplicate() {
        let dir = tmpdir("rename");
        let p = dir.join("presets.yaml");
        PresetStore::upsert(&p, sample()).unwrap();
        PresetStore::rename(&p, "钻孔", "钻孔2").unwrap();
        let got = PresetStore::load(&p).unwrap();
        assert!(got.file.get("钻孔").is_none());
        let two = got.file.get("钻孔2").unwrap();
        assert_eq!(two.template, "drilling/x.j2");
        assert_eq!(two.params.get("tool"), Some(&ParamValue::Integer(3)));

        PresetStore::upsert(&p, sample()).unwrap();
        let err = PresetStore::rename(&p, "钻孔", "钻孔2").unwrap_err();
        assert!(matches!(err, WriteError::PathEscape { .. }), "{err:?}");
    }

    #[test]
    fn rename_rejects_illegal_new_name() {
        let dir = tmpdir("rename_bad");
        let p = dir.join("presets.yaml");
        PresetStore::upsert(&p, sample()).unwrap();
        let err = PresetStore::rename(&p, "钻孔", "../evil").unwrap_err();
        assert!(matches!(err, WriteError::PathEscape { .. }), "{err:?}");
    }

    #[test]
    fn remove_missing_reports_error() {
        let dir = tmpdir("remove");
        let p = dir.join("presets.yaml");
        PresetStore::upsert(&p, sample()).unwrap();
        PresetStore::remove(&p, "钻孔").unwrap();
        assert!(PresetStore::load(&p).unwrap().file.presets.is_empty());
        assert!(PresetStore::remove(&p, "钻孔").is_err());
    }

    #[test]
    fn stale_report_flags_removed_param_and_new_required() {
        use crate::model::ParamKind;
        use crate::validate::spec;
        let mut preset = sample();
        preset.spec_fingerprint = "fnv1a64:deadbeefdeadbeef".into();
        let specs = vec![
            spec("x", ParamKind::Number, true, None, ""),
            spec("fresh", ParamKind::Number, true, None, ""),
        ];
        let r = PresetStore::stale_report(&preset, &specs);
        assert!(r.is_stale());
        assert!(r.fingerprint_changed);
        assert_eq!(r.stale_params, vec!["tool"], "被删除的参数应列入失效");
        assert_eq!(r.missing_required, vec!["fresh"], "新增必选应列入缺失");
    }

    #[test]
    fn stale_report_clean_when_fingerprint_matches() {
        use crate::model::ParamKind;
        use crate::validate::spec;
        let specs = vec![
            spec("x", ParamKind::Number, true, None, ""),
            spec("tool", ParamKind::Integer, false, None, ""),
        ];
        let mut preset = sample();
        preset.spec_fingerprint = SpecFingerprint::of(&specs);
        let r = PresetStore::stale_report(&preset, &specs);
        assert!(!r.is_stale(), "{r:?}");
    }

    #[test]
    fn cross_template_splits_three_categories() {
        use crate::model::ParamKind;
        use crate::validate::spec;
        let src_specs = vec![
            spec("x", ParamKind::Number, true, None, ""),
            spec("tool", ParamKind::Integer, false, None, ""),
        ];
        let target_specs = vec![
            // x 类型一致 → reusable；tool 变成 String → needs_confirm
            spec("x", ParamKind::Number, true, None, ""),
            spec("tool", ParamKind::String, false, None, ""),
            spec("depth", ParamKind::Number, true, None, ""),
        ];
        let r = PresetStore::cross_template_report(&sample(), &src_specs, &target_specs);
        assert_eq!(r.reusable, vec!["x"]);
        assert_eq!(r.needs_confirm, vec!["tool"]);
        assert_eq!(r.missing, vec!["depth"]);
    }

    #[test]
    fn export_import_roundtrip_keeps_fingerprint() {
        let text = PresetStore::export_preset(&sample()).unwrap();
        assert!(text.contains("specFingerprint"));
        let back = PresetStore::import_presets::<WriteError, _>(&text, |_| Ok(())).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0], sample());
    }

    /// 钉住**落盘形状**：参数必须是扁平映射，不得出现 `values:` 包装。
    ///
    /// `ParameterSet` 的派生序列化会写成 `values: {x: ...}`，把内部结构泄漏成
    /// 用户可见的文件格式。这条断言是防它复发的闸门——若有人把 `serde(with)`
    /// 去掉，往返测试仍会通过（对称），只有本测试会红。
    #[test]
    fn on_disk_shape_is_flat_not_wrapped() {
        let text = PresetStore::export_preset(&sample()).unwrap();
        assert!(
            !text.contains("values:"),
            "参数集不得以 `values:` 包装形式落盘：\n{text}"
        );
        assert!(text.contains("x:"), "参数名应直接出现在映射里：\n{text}");
        assert!(text.contains("specFingerprint:"), "{text}");
        assert!(text.contains("createdAt:"), "{text}");
    }

    /// 手写裸标量形式也应可读（`x: 21.0` 而非 `{type, value}`）。
    #[test]
    fn handwritten_scalar_params_are_accepted() {
        let text = "version: 1\npresets:\n- name: hand\n  template: a.j2\n  params:\n    x: 21.0\n    tool: 3\n  createdAt: \"2026-01-01T00:00:00Z\"\n  specFingerprint: \"fnv1a64:0\"\n";
        let got = PresetStore::import_presets::<WriteError, _>(text, |_| Ok(())).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].params.get("x"), Some(&ParamValue::Number(21.0)));
        assert_eq!(got[0].params.get("tool"), Some(&ParamValue::Integer(3)));
    }

    /// 陈旧 / 跨模板报告的 JSON 字段为 **camelCase**，与 `Preset` 同风格。
    #[test]
    fn report_json_uses_camel_case() {
        let stale = StaleReport {
            fingerprint_changed: true,
            stale_params: vec!["a".into()],
            missing_required: vec!["b".into()],
        };
        let v = serde_json::to_value(&stale).unwrap();
        assert_eq!(v["fingerprintChanged"], true);
        assert_eq!(v["staleParams"][0], "a");
        assert_eq!(v["missingRequired"][0], "b");

        let cross = CrossTemplateReport {
            reusable: vec!["r".into()],
            needs_confirm: vec!["c".into()],
            missing: vec!["m".into()],
        };
        let v = serde_json::to_value(&cross).unwrap();
        assert_eq!(v["reusable"][0], "r");
        assert_eq!(v["needsConfirm"][0], "c");
        assert_eq!(v["missing"][0], "m");
    }

    #[test]
    fn import_rejects_illegal_name_and_bad_version() {
        let text = "version: 1\npresets:\n  - name: \"../evil\"\n    template: a.j2\n    params: {}\n    createdAt: x\n    specFingerprint: y\n";
        let err = PresetStore::import_presets::<WriteError, _>(text, |_| Ok(())).unwrap_err();
        assert!(matches!(err, WriteError::PathEscape { .. }), "{err:?}");

        let bad = "version: 42\npresets: []\n";
        let err = PresetStore::import_presets::<WriteError, _>(bad, |_| Ok(())).unwrap_err();
        assert!(matches!(err, WriteError::Corrupt(_)), "{err:?}");
    }

    #[test]
    fn import_runs_injected_validator() {
        let text = PresetStore::export_preset(&sample()).unwrap();
        let err = PresetStore::import_presets::<WriteError, _>(&text, |_p| {
            Err(WriteError::Corrupt("参数非法".into()))
        })
        .unwrap_err();
        assert!(matches!(err, WriteError::Corrupt(ref m) if m.contains("参数非法")));
    }

    #[test]
    fn ensure_outside_template_root_rejects_inside_path() {
        let dir = tmpdir("guard");
        let inside = dir.join("presets.yaml");
        assert!(ensure_outside_template_root(&inside, &dir).is_err());
        assert!(ensure_outside_template_root(&dir.join("sub/x.yaml"), &dir).is_err());
        let outside = dir.parent().unwrap().join("elsewhere.yaml");
        assert!(ensure_outside_template_root(&outside, &dir).is_ok());
    }

    /// 回归（实测缺陷）：**相对路径**必须与相对根一起正确判定。
    ///
    /// 原实现直接对入参 `canonicalize`：预设文件**尚不存在**（保存前校验的常态）
    /// → canonicalize 失败 → 退回手写的**相对**路径，而根是**绝对**路径，
    /// `starts_with` **恒假** → 红线静默失效，预设被写进模板根。
    /// 实测已复现（`preset save --file ./templates/presets.yaml` 照写不误）。
    #[test]
    fn ensure_outside_template_root_handles_relative_paths() {
        let cwd = std::env::current_dir().unwrap();
        // 相对根 + 相对文件（文件不存在，正是"保存前"的形态）
        assert!(
            ensure_outside_template_root(
                Path::new("templates/presets.yaml"),
                Path::new("templates")
            )
            .is_err(),
            "相对路径形态下红线必须生效（否则预设会被写进模板根）"
        );
        // 带 `./` 前缀
        assert!(ensure_outside_template_root(
            Path::new("./templates/presets.yaml"),
            Path::new("templates")
        )
        .is_err());
        // 相对根 + 绝对文件
        assert!(ensure_outside_template_root(
            &cwd.join("templates/x.yaml"),
            Path::new("templates")
        )
        .is_err());
        // 相对根 + 绝对文件，且确实在根外
        assert!(
            ensure_outside_template_root(&cwd.join("outside/x.yaml"), Path::new("templates"))
                .is_ok()
        );
        // `..` 不得把判定带偏
        assert!(ensure_outside_template_root(
            Path::new("templates/../templates/x.yaml"),
            Path::new("templates")
        )
        .is_err());
    }

    #[test]
    fn iso8601_matches_known_instants() {
        assert_eq!(iso8601_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_from_unix(1_000_000_000), "2001-09-09T01:46:40Z");
        // 闰年 2 月 29 日边界
        assert_eq!(iso8601_from_unix(1_709_164_800), "2024-02-29T00:00:00Z");
        assert!(now_iso8601().ends_with('Z'));
    }

    #[test]
    fn default_preset_path_is_outside_template_dir_name() {
        let p = default_preset_path();
        assert!(p.ends_with(PRESET_FILE), "{}", p.display());
        // 关键红线：不得落在名为 templates 的路径下
        assert!(
            !p.to_string_lossy().contains("templates"),
            "默认预设路径不得落在模板根：{}",
            p.display()
        );
    }
}
