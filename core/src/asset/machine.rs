//! 机床配置（模块二）的写策略：项目 `nctool.toml` 的 `toml_edit` 合并式 upsert。
//!
//! # 为什么这一层用 `toml_edit` 而非 serde 全量往返
//!
//! `nctool.toml` 是**用户手写**的项目配置：它带有说明注释、`template_dir`、
//! `default_machine` 等其它段。若用 serde 反序列化再整体写回，注释与段序会被
//! 抹掉——用户下次打开文件时自己的注释没了，且无从察觉。`toml_edit` 保留格式
//! 与注释，只改 `[machine.<id>]` 一处（AC-2.10）。
//!
//! # 单一写通道
//!
//! 所有落盘一律经 [`WriteKernel`]（原子写 + 乐观锁），本模块不自行拼接文件字节。
//!
//! # 与只读路径的分工
//!
//! CLI 的**只读**命令（`machine list/show`）仍走 `config::load()`（serde `toml`，
//! D13 降级哲学：损坏配置降级为空 + 警告，不阻断）。本模块的 [`MachineWriter::load`]
//! 只服务**写路径**的自身读取与测试：损坏时**报错**（[`WriteError::Corrupt`]）而不是
//! 降级为空——否则一次损坏的读取会让 upsert 把用户全部机床配置静默覆盖掉。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, Value};

use crate::machine::{MachineKeyKind, MachinePreset, KNOWN_CONFIG_KEYS};
use crate::model::MachineConfig;

use super::{map_io, validate_asset_name, WriteAction, WriteError, WriteKernel, WriteOutcome};

/// 项目配置文件默认文件名（写目标 = 项目根 `nctool.toml`）。
pub const CONFIG_FILE: &str = "nctool.toml";

/// 内置预设判定（`generic` / `wfl_m65` / `index_ms40`）——不可改（AC-2.2）。
///
/// 转发 [`MachinePreset::from_id`]，避免 CLI / UI / core 各写一份内置清单。
pub fn is_builtin_machine(id: &str) -> bool {
    MachinePreset::from_id(id).is_some()
}

// ---------------------------------------------------------------------------
// 报告类型
// ---------------------------------------------------------------------------

/// 完整性报告：模板引用的 `machine.*` 键集合 vs 配置键集合之差集。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletenessReport {
    /// 模板引用但配置缺失 → **阻断**（AC-2.4）。
    pub missing_keys: Vec<String>,
    /// 配置里有、schema 里没有 → **仅告警**（扩展键语义，AC-2.8）。
    pub unknown_keys: Vec<String>,
    /// = `!missing_keys.is_empty()`。
    pub blocking: bool,
}

/// 保存前总校验报告（值级阻断 + 完整性阻断 + 未知键告警）。
///
/// 结构化字段与 `blocking` / `warnings` 文本**并存**：文本供人读，结构化字段
/// 供消费方分支（**D7：禁止用消息文本做决策**）。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineSaveReport {
    /// 人类可读阻断原因（缺键 / Choice 非法 / 整数键非法）。
    pub blocking: Vec<String>,
    /// 仅告警（未知键 / 空串 / `line_number_digits` 将被夹紧的提示）。
    pub warnings: Vec<String>,
    /// 结构化：缺失的 `machine.*` 键。
    pub missing_keys: Vec<String>,
    /// 结构化：未知键。
    pub unknown_keys: Vec<String>,
    /// 结构化：值非法的键名（Choice 越界 / 整数键不可解析）。
    pub invalid_values: Vec<String>,
}

impl MachineSaveReport {
    /// 是否可落盘（`blocking` 为空）。
    pub fn can_save(&self) -> bool {
        self.blocking.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 写策略
// ---------------------------------------------------------------------------

/// 机床写操作（模块二）唯一入口。无状态，全部关联函数。
pub struct MachineWriter;

impl MachineWriter {
    /// 读全部自定义机床（`[machine.<id>]`）。
    ///
    /// - 文件不存在 → 空表（**非**错误）；
    /// - 解析失败 → [`WriteError::Corrupt`]（**不静默返回空**——调用方据此拒绝覆盖）。
    ///
    /// 注：CLI 的**只读**命令（`machine list/show`）仍走 `config::load()`（serde `toml`，
    /// D13 降级哲学）；本函数服务**写路径**的自身读取与测试。
    pub fn load(path: &Path) -> Result<BTreeMap<String, MachineConfig>, WriteError> {
        let Some(text) = read_text_if_exists(path)? else {
            return Ok(BTreeMap::new());
        };
        let doc: DocumentMut = text.parse().map_err(|e| corrupt(path, &e))?;
        Ok(extract_machines(&doc))
    }

    /// 合并式 upsert：只改 `[machine.<id>]` 段，其余段与注释**字节不变**（AC-2.10）。
    ///
    /// 步骤：① [`validate_asset_name`]（拒空 / `.` / `..` / 分隔符 / 盘符前缀 / 控制字符）；
    /// ② 读现有文本（缺 → 空文档）；③ [`DocumentMut`] 解析（失败 → [`WriteError::Corrupt`]）；
    /// ④ 写入 `[machine.<id>]`（`id`/`vendor`/`model` + `config` 子表）；
    /// ⑤ [`WriteKernel::write_guarded`]。
    ///
    /// `expect`：`None` = 要求写前**文件不存在**；`Some(fp)` = 要求指纹一致。
    /// 调用方通常传 [`WriteKernel::read_fingerprint`]（`--expect-hash` 时改用显式指纹）。
    ///
    /// 本函数**不做**"重名拒绝"——那是命令层策略（`machine add` 默认拒绝、`--force` 才覆盖）。
    /// 与 [`PresetStore::upsert`](super::PresetStore::upsert) 的写语义一致：把"要不要覆盖"
    /// 留给命令层，便于 `edit` 复用同一写函数。
    pub fn upsert(
        path: &Path,
        cfg: &MachineConfig,
        expect: Option<super::FileFingerprint>,
    ) -> Result<WriteOutcome, WriteError> {
        validate_asset_name(&cfg.id).map_err(|reason| WriteError::PathEscape {
            rel: cfg.id.clone(),
            reason,
        })?;

        let output = match read_text_if_exists(path)? {
            // 文件不存在：渲染一个只含 `[machine.<id>]` 的文档（`machine` 为隐式表）。
            None => {
                let mut doc = DocumentMut::new();
                insert_machine_entry(&mut doc, path, cfg)?;
                doc.to_string()
            }
            Some(text) => {
                let mut doc: DocumentMut = text.parse().map_err(|e| corrupt(path, &e))?;
                // `[machine]` 存在但**不是表** → 拒绝：否则会把它当普通值静默覆盖。
                if doc.get("machine").is_some_and(|i| !i.is_table()) {
                    return Err(WriteError::Corrupt(format!(
                        "{} 的 [machine] 段不是表（可能被写成了普通键），拒绝写入",
                        path.display()
                    )));
                }
                let exists = doc
                    .get("machine")
                    .and_then(Item::as_table)
                    .is_some_and(|t| t.contains_key(&cfg.id));
                if exists {
                    // 原地替换：`Table::insert` 保留位置，其余段与注释不受影响。
                    insert_machine_entry(&mut doc, path, cfg)?;
                    doc.to_string()
                } else {
                    // 追加新段：把**原文逐字节**作为前缀，仅追加渲染好的 `[machine.<id>]`。
                    //
                    // 为何不直接 `doc.to_string()` 整篇重排：`toml_edit` 把"最后一段之后
                    // 的注释"存为**文档尾注释**，一旦在 `[machine]` 下追加新子表，尾注释
                    // 会被挤到新段之后——注释位置漂移，违反 AC-2.10。把原文当字节前缀
                    // 追加则让行内注释 / 段序 / 尾注释全部原样保留（新段本就是文件末尾的
                    // 顶层 `[machine.<id>]`，追加合法且不扰动既有内容）。
                    let mut fresh = DocumentMut::new();
                    insert_machine_entry(&mut fresh, path, cfg)?;
                    let mut out = text;
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str(&fresh.to_string());
                    out
                }
            }
        };

        WriteKernel::write_guarded(path, output.as_bytes(), expect)
    }

    /// 删除 `[machine.<id>]` 段。
    ///
    /// 段不存在 → [`WriteError::NotFound`]（→ `machine_not_found`(5)）。
    /// 返回的 `WriteOutcome.action` 覆写为 [`WriteAction::Deleted`]（同
    /// [`PresetStore::remove`](super::PresetStore::remove)）——删一个机床，
    /// 消费方看到的应当是"删除"，而不是内部"改内容再落盘"透传出来的 `Updated`。
    pub fn remove(
        path: &Path,
        id: &str,
        expect: Option<super::FileFingerprint>,
    ) -> Result<WriteOutcome, WriteError> {
        let text = read_text_if_exists(path)?.ok_or_else(|| {
            WriteError::NotFound(format!("机床不存在：{id}（{} 尚未创建）", path.display()))
        })?;
        let mut doc: DocumentMut = text.parse().map_err(|e| corrupt(path, &e))?;

        let removed = doc
            .get_mut("machine")
            .and_then(|i| i.as_table_mut())
            .and_then(|t| t.remove(id));
        if removed.is_none() {
            return Err(WriteError::NotFound(format!(
                "机床不存在：{id}（{} 无该段）",
                path.display()
            )));
        }
        // `[machine]` 变空则一并移除：否则会留下一个空的 `[machine]` 表头，
        // 是本次删除的可见残留。仅当"刚被我们清空"时才会走到，不会误删用户的结构。
        if doc
            .get("machine")
            .and_then(|i| i.as_table())
            .is_some_and(|t| t.is_empty())
        {
            doc.remove("machine");
        }

        let mut out = WriteKernel::write_guarded(path, doc.to_string().as_bytes(), expect)?;
        out.action = WriteAction::Deleted;
        Ok(out)
    }

    /// 纯函数：完整性差集（**不落盘**、**不读盘**）。
    ///
    /// `required_keys` 由调用方（CLI）从注册表收集（`core::asset` 不依赖注册表）。
    pub fn check_completeness(
        cfg: &MachineConfig,
        required_keys: &BTreeSet<String>,
    ) -> CompletenessReport {
        let present: BTreeSet<&str> = cfg.config.keys().map(String::as_str).collect();
        let missing_keys: Vec<String> = required_keys
            .iter()
            .filter(|k| !present.contains(k.as_str()))
            .cloned()
            .collect();
        let schema_keys: BTreeSet<&str> = KNOWN_CONFIG_KEYS.iter().map(|s| s.key).collect();
        let unknown_keys: Vec<String> = cfg
            .config
            .keys()
            .filter(|k| !schema_keys.contains(k.as_str()))
            .cloned()
            .collect();
        let blocking = !missing_keys.is_empty();
        CompletenessReport {
            missing_keys,
            unknown_keys,
            blocking,
        }
    }

    /// 纯函数：保存前总校验。
    ///
    /// = 值级判定（遍历 [`KNOWN_CONFIG_KEYS`] 的 `MachineKeyKind`——**单一来源**，
    ///   禁止在别处再抄一份 `units`/`feed_mode` 白名单）+ [`Self::check_completeness`]
    ///   的差集（缺键阻断 / 未知键告警）。
    ///
    /// `line_number_digits` **超大值不阻断**（夹紧在 `pipeline::postprocess`，AC-2.9）——
    /// 只产生一条"将被夹紧"的 warning；写层**不复制**夹紧逻辑。
    pub fn preflight(cfg: &MachineConfig, required_keys: &BTreeSet<String>) -> MachineSaveReport {
        let mut report = MachineSaveReport::default();
        let completeness = Self::check_completeness(cfg, required_keys);

        // 未知键：仅告警（扩展键合法，AC-2.8）。
        for k in &completeness.unknown_keys {
            report.warnings.push(format!(
                "未知配置键: {k}（扩展键允许；引用前请确认拼写，内建键见 `nctool machine show`）"
            ));
        }

        // 值级：按 schema 的 `MachineKeyKind` 判定（单一来源）。
        for (k, v) in &cfg.config {
            let Some(schema) = KNOWN_CONFIG_KEYS.iter().find(|s| s.key == k) else {
                continue; // 未知键已在上面告警
            };
            match schema.kind {
                MachineKeyKind::Choice(opts) => {
                    if !opts.contains(&v.as_str()) {
                        report.invalid_values.push(k.clone());
                        report.blocking.push(format!(
                            "配置键 {k} 取值应属于 {}，实际为 {v:?}",
                            opts.iter()
                                .map(|o| format!(r#""{o}""#))
                                .collect::<Vec<_>>()
                                .join("/")
                        ));
                    }
                }
                MachineKeyKind::Integer => match v.trim().parse::<i64>() {
                    Ok(n) => {
                        // `line_number_digits` 的安全上限由 `pipeline::postprocess`
                        // 强制（`clamp(1, MAX_LINE_NUMBER_DIGITS)`）。这里**只提示**，
                        // 不阻断、也不复制夹紧逻辑（AC-2.9）。
                        if k == "line_number_digits" {
                            let bound = crate::pipeline::MAX_LINE_NUMBER_DIGITS as i64;
                            if n > bound || n < 1 {
                                report.warnings.push(format!(
                                    "配置键 line_number_digits={n} 超出安全范围，\
                                     渲染时会被夹紧到 [1, {bound}]（写层不阻断；\
                                     真正的夹紧在 pipeline 后处理）"
                                ));
                            }
                        }
                    }
                    Err(_) => {
                        report.invalid_values.push(k.clone());
                        report.blocking.push(format!(
                            "配置键 {k} 期望整数，实际为 {v:?}（模板做 | int 转换时会失败或取默认值）"
                        ));
                    }
                },
                MachineKeyKind::String => {
                    if v.trim().is_empty() {
                        report.warnings.push(format!(
                            "配置键 {k} 为空字符串（将静默回退默认值 {:?}；\
                             请直接删除该键或填写有效值）",
                            schema.default
                        ));
                    }
                }
            }
        }

        // 完整性：缺键阻断（AC-2.4）。
        for k in &completeness.missing_keys {
            report.blocking.push(format!(
                "缺失被模板引用的机床键: {k}（请用 --set {k}=... 补齐）"
            ));
        }
        report.missing_keys = completeness.missing_keys;
        report.unknown_keys = completeness.unknown_keys;
        report
    }
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/// 读文件文本；不存在返回 `None`。
fn read_text_if_exists(path: &Path) -> Result<Option<String>, WriteError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(map_io(e, path)),
    }
}

/// 在文档中写入 `[machine.<id>]`（`id`/`vendor`/`model` + `config` 子表）。
///
/// `[machine]` 缺失时创建为**隐式**表（否则会多出一行空的 `[machine]` 表头）；
/// 已存在即用 [`Table::insert`] 替换（保留位置）。`[machine]` 存在但非表 → `Corrupt`。
fn insert_machine_entry(
    doc: &mut DocumentMut,
    path: &Path,
    cfg: &MachineConfig,
) -> Result<(), WriteError> {
    let mut entry = Table::new();
    entry.insert("id", Item::Value(Value::from(cfg.id.as_str())));
    entry.insert("vendor", Item::Value(Value::from(cfg.vendor.as_str())));
    entry.insert("model", Item::Value(Value::from(cfg.model.as_str())));
    let mut config = Table::new();
    for (k, v) in &cfg.config {
        config.insert(k, Item::Value(Value::from(v.as_str())));
    }
    entry.insert("config", Item::Table(config));

    if doc.get("machine").is_none() {
        let mut table = Table::new();
        table.set_implicit(true);
        doc.insert("machine", Item::Table(table));
    }
    let machine = doc
        .get_mut("machine")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| {
            WriteError::Corrupt(format!(
                "{} 的 [machine] 段不是表（可能被写成了普通键），拒绝写入",
                path.display()
            ))
        })?;
    // 保留被替换表的 **decor**（表头之前的注释块 + 表头行内注释）。
    //
    // `toml_edit` 把紧邻表头的注释存为该表的 `decor`；`Table::insert` 换入新表时
    // 连同旧 decor 一起丢弃 —— 表现为 `machine edit` **静默删掉用户的注释**。
    // 最典型的一例：`machine add` 首次创建写入的 `EXAMPLE_CONFIG` 示例头（19 行
    // 注释）恰是文件第一个表的前缀，一次 `edit` 就会全数消失（实测 43 行 → 25 行）。
    //
    // 只保 decor，不保表**内**的注释：那属于设计 §10 已登记的
    // 「`[machine.<id>]` 段内旧注释被替换」。
    if let Some(old) = machine.get(&cfg.id).and_then(Item::as_table) {
        *entry.decor_mut() = old.decor().clone();
        if let (Some(old_config), Some(new_config)) = (
            old.get("config").and_then(Item::as_table),
            entry.get_mut("config").and_then(Item::as_table_mut),
        ) {
            *new_config.decor_mut() = old_config.decor().clone();
        }
    }

    machine.insert(&cfg.id, Item::Table(entry));
    Ok(())
}

/// 构造 TOML 解析失败的 [`WriteError::Corrupt`]（统一文案）。
///
/// 写路径的读取**不降级**：损坏即报错并拒绝覆盖——否则一次损坏的读取会让
/// `upsert` 把用户全部机床配置静默覆盖掉（对照 `config::load` 的 D13 降级哲学）。
fn corrupt(path: &Path, err: &toml_edit::TomlError) -> WriteError {
    WriteError::Corrupt(format!(
        "{} 解析失败（{}）：为避免覆盖已拒绝写入，请先修复或备份后删除该文件",
        path.display(),
        err
    ))
}

/// 从已解析文档提取 `[machine.<id>]` 段为 [`MachineConfig`] 映射。
fn extract_machines(doc: &DocumentMut) -> BTreeMap<String, MachineConfig> {
    let mut out = BTreeMap::new();
    let Some(machine) = doc.get("machine").and_then(Item::as_table) else {
        return out;
    };
    for (id, item) in machine.iter() {
        let Some(table) = item.as_table() else {
            continue; // `[machine]` 下的非表项（如 `machine = "x"`）跳过
        };
        let mut cfg = MachineConfig {
            id: id.to_string(),
            vendor: String::new(),
            model: String::new(),
            config: BTreeMap::new(),
        };
        if let Some(s) = table.get("id").and_then(Item::as_str) {
            cfg.id = s.to_string();
        }
        if let Some(s) = table.get("vendor").and_then(Item::as_str) {
            cfg.vendor = s.to_string();
        }
        if let Some(s) = table.get("model").and_then(Item::as_str) {
            cfg.model = s.to_string();
        }
        // `config` 有两种**语义等价**的写法：标准子表 `[machine.<id>.config]`
        // 与行内表 `config = { linear = "G1" }`。读路径（serde，见
        // `cli::config::load`）两种都认 —— 写路径若只认子表，`machine edit`
        // 就会把行内表里的键当成"不存在"，在注册表降级（模板目录里有坏模板）
        // 导致 preflight 放行时**静默丢掉**它们。两个读者必须对同一份文件
        // 得到同一个答案。
        match table.get("config") {
            Some(Item::Table(config)) => {
                for (k, v) in config.iter() {
                    if let Some(s) = item_as_config_string(v) {
                        cfg.config.insert(k.to_string(), s);
                    }
                }
            }
            Some(Item::Value(Value::InlineTable(inline))) => {
                for (k, v) in inline.iter() {
                    if let Some(s) = value_as_config_string(v) {
                        cfg.config.insert(k.to_string(), s);
                    }
                }
            }
            _ => {} // 无 `config` 键（合法：机床可以没有配置）
        }
        out.insert(id.to_string(), cfg);
    }
    out
}

/// 配置值取字符串（子表形态）。
fn item_as_config_string(item: &Item) -> Option<String> {
    item.as_value().and_then(value_as_config_string)
}

/// 配置值取字符串：字符串取内部文本，其余标量取**无装饰**文本（`4` / `true` 等）。
///
/// 为什么不能直接 `Value::to_string()`：`toml_edit` 把 `key = value` 里 `=` 之后的
/// 空白存为该值的 **decor**，渲染时会一并输出 —— `max_spindle_rpm = 4200` 读出来
/// 是 `" 4200"`（行内表更糟，末值还会带上尾随空格）。这不是"格式差异"：值会经
/// `machine.<key>` 注入渲染，多一个空格就进了 G-code，`| int` 还会直接失败。
/// 逐变体取 `value()` 才是真正的"原样文本"。
fn value_as_config_string(v: &Value) -> Option<String> {
    Some(match v {
        Value::String(s) => s.value().clone(),
        Value::Integer(i) => i.value().to_string(),
        Value::Float(f) => f.value().to_string(),
        Value::Boolean(b) => b.value().to_string(),
        Value::Datetime(d) => d.value().to_string(),
        // 数组 / 嵌套表不是配置键的取值形态。取原样文本（去装饰），让"值不合法"
        // 这件事在 preflight / 渲染处暴露，而不是在这里猜用户的意图。
        other => other.to_string().trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nctool_machine_{tag}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample(id: &str) -> MachineConfig {
        let mut config = BTreeMap::new();
        config.insert("program_prefix".to_string(), "O".to_string());
        config.insert("linear".to_string(), "G1".to_string());
        MachineConfig {
            id: id.to_string(),
            vendor: "HERO".to_string(),
            model: "X9".to_string(),
            config,
        }
    }

    #[test]
    fn is_builtin_machine_matches_presets_only() {
        for id in ["generic", "wfl_m65", "index_ms40"] {
            assert!(is_builtin_machine(id), "{id} 应判为内置");
        }
        assert!(!is_builtin_machine("hero_x9"));
    }

    #[test]
    fn load_missing_file_is_empty_not_error() {
        let dir = tmpdir("missing");
        let got = MachineWriter::load(&dir.join("nctool.toml")).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn load_corrupt_file_is_error_not_empty() {
        let dir = tmpdir("corrupt");
        let p = dir.join("nctool.toml");
        std::fs::write(&p, "this is [ not toml").unwrap();
        let err = MachineWriter::load(&p).unwrap_err();
        assert!(matches!(err, WriteError::Corrupt(_)), "{err:?}");
    }

    #[test]
    fn upsert_then_load_roundtrip() {
        let dir = tmpdir("roundtrip");
        let p = dir.join("nctool.toml");
        MachineWriter::upsert(&p, &sample("hero_x9"), None).unwrap();
        let got = MachineWriter::load(&p).unwrap();
        let m = got.get("hero_x9").expect("应能取回");
        assert_eq!(m.vendor, "HERO");
        assert_eq!(m.get("linear"), Some("G1"));
    }

    #[test]
    fn upsert_preserves_other_sections_and_comments() {
        let dir = tmpdir("preserve");
        let p = dir.join("nctool.toml");
        let original = "# 顶部注释\ntemplate_dir = \"templates\"\n\n[machine.existing]\nid = \"existing\"\nvendor = \"ACME\"\n\n[machine.existing.config]\nlinear = \"G1\"\n";
        std::fs::write(&p, original).unwrap();

        MachineWriter::upsert(&p, &sample("hero_x9"), Some(fingerprint(&p))).unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        // 原有内容（含注释与既有段）逐字节保留为前缀
        assert!(
            after.starts_with(original),
            "其余段与注释必须逐字节不变：\n--- before ---\n{original}\n--- after ---\n{after}"
        );
        assert!(after.contains("[machine.hero_x9]"));
    }

    #[test]
    fn remove_deletes_section_and_reports_deleted() {
        let dir = tmpdir("remove");
        let p = dir.join("nctool.toml");
        MachineWriter::upsert(&p, &sample("a"), None).unwrap();
        MachineWriter::upsert(&p, &sample("b"), Some(fingerprint(&p))).unwrap();
        let out = MachineWriter::remove(&p, "a", Some(fingerprint(&p))).unwrap();
        assert_eq!(out.action, WriteAction::Deleted);
        let got = MachineWriter::load(&p).unwrap();
        assert!(!got.contains_key("a"));
        assert!(got.contains_key("b"));
    }

    #[test]
    fn remove_missing_is_not_found() {
        let dir = tmpdir("remove_missing");
        let p = dir.join("nctool.toml");
        MachineWriter::upsert(&p, &sample("a"), None).unwrap();
        let err = MachineWriter::remove(&p, "ghost", Some(fingerprint(&p))).unwrap_err();
        assert!(matches!(err, WriteError::NotFound(_)), "{err:?}");
    }

    #[test]
    fn upsert_rejects_illegal_id() {
        let dir = tmpdir("bad_id");
        let p = dir.join("nctool.toml");
        for bad in ["../evil", "a/b", "a\\b", ""] {
            let err = MachineWriter::upsert(&p, &sample(bad), None).unwrap_err();
            assert!(
                matches!(err, WriteError::PathEscape { .. }),
                "{bad}: {err:?}"
            );
        }
    }

    #[test]
    fn check_completeness_splits_missing_and_unknown() {
        let cfg = sample("hero");
        let mut required = BTreeSet::new();
        required.insert("linear".to_string());
        required.insert("rapid".to_string()); // 缺
        let r = MachineWriter::check_completeness(&cfg, &required);
        assert_eq!(r.missing_keys, vec!["rapid"]);
        assert!(r.blocking);
        assert!(r.unknown_keys.is_empty());
    }

    #[test]
    fn preflight_blocks_illegal_choice_and_missing() {
        let mut cfg = sample("hero");
        cfg.config.insert("units".to_string(), "inch".to_string());
        let mut required = BTreeSet::new();
        required.insert("linear".to_string());
        required.insert("coordinate_system".to_string()); // 缺
        let r = MachineWriter::preflight(&cfg, &required);
        assert!(!r.can_save());
        assert!(r.invalid_values.contains(&"units".to_string()));
        assert!(r.missing_keys.contains(&"coordinate_system".to_string()));
    }

    #[test]
    fn preflight_allows_unknown_key_and_clamps_huge_digits_as_warning() {
        let mut cfg = sample("hero");
        cfg.config.insert("my_ext".to_string(), "1".to_string());
        cfg.config
            .insert("line_number_digits".to_string(), "1000000000".to_string());
        let r = MachineWriter::preflight(&cfg, &BTreeSet::new());
        assert!(r.can_save(), "扩展键与超大行号位数都不应阻断: {r:?}");
        assert!(r.unknown_keys.contains(&"my_ext".to_string()));
        assert!(r.warnings.iter().any(|w| w.contains("夹紧")));
    }

    /// 从路径读指纹（测试辅助；文件必须存在）。
    fn fingerprint(p: &Path) -> super::super::FileFingerprint {
        WriteKernel::read_fingerprint(p)
            .unwrap()
            .expect("文件应存在")
    }

    #[test]
    fn upsert_on_comment_only_file_keeps_comments_at_top() {
        // 模拟首次创建：CLI 先写入 EXAMPLE_CONFIG（纯注释），再 upsert。
        // 注释必须仍逐字节留在**顶部**，且不出现空 `[machine]` 表头。
        let dir = tmpdir("comment_only");
        let p = dir.join("nctool.toml");
        let example =
            "# nctool 配置示例\n# 配置层级：项目 ./nctool.toml\n\n# template_dir = \"templates\"\n";
        std::fs::write(&p, example).unwrap();

        MachineWriter::upsert(&p, &sample("hero_x9"), Some(fingerprint(&p))).unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        assert!(
            after.starts_with(example),
            "注释必须逐字节保留在顶部：\n--- before ---\n{example}\n--- after ---\n{after}"
        );
        assert!(
            !after.contains("\n[machine]\n"),
            "不应出现空的 [machine] 表头：\n{after}"
        );
        assert!(after.contains("[machine.hero_x9]"));
        // 回读一致
        assert_eq!(MachineWriter::load(&p).unwrap()["hero_x9"].vendor, "HERO");
    }

    #[test]
    fn upsert_into_plain_file_appends_and_keeps_existing_bytes() {
        let dir = tmpdir("plain");
        let p = dir.join("nctool.toml");
        let original = "template_dir = \"templates\"\n";
        std::fs::write(&p, original).unwrap();
        MachineWriter::upsert(&p, &sample("hero_x9"), Some(fingerprint(&p))).unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        assert!(after.starts_with(original), "{after}");
        assert!(after.contains("[machine.hero_x9]"));
        assert!(!after.contains("\n[machine]\n"), "{after}");
    }
}
