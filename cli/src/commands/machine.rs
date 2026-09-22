//! `machine` 子命令：机床预设列表 / 查看配置 / 新建 / 编辑 / 删除 / 试渲染。
//!
//! # 单一写通道
//!
//! `add` / `edit` / `rm` 一律经 [`MachineWriter`]（内部走 `core::asset::WriteKernel`），
//! 本模块不自行拼接 `nctool.toml` 字节——与 `templates` / `preset` 同一约定。
//! 只读的 `list` / `show` 仍走 `config::load()`（serde `toml`，D13 降级哲学）。
//!
//! # 落盘目标
//!
//! 默认写**项目 `nctool.toml`**（`Ctx::project_config_path`：向上递归查找，未发现则
//! 于当前目录创建）。`--file` 可覆盖。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use nctool_core::asset::{
    is_builtin_machine, validate_asset_name, FileFingerprint, MachineWriter, WriteAction,
    WriteKernel,
};
use nctool_core::machine::MachinePreset;
use nctool_core::pipeline::GenerationOptions;
use nctool_core::MachineConfig;

use crate::cli::{
    MachineAddArgs, MachineArgs, MachineCommand, MachineEditArgs, MachineFileArgs, MachineRmArgs,
    MachineShowArgs, MachineTestArgs,
};
use crate::config;
use crate::context::{build_params, Ctx};
use crate::output::CliError;

/// `machine` 命令分发。
pub fn run(ctx: &Ctx, args: &MachineArgs) -> Result<(), CliError> {
    match &args.command {
        MachineCommand::List => list(ctx),
        MachineCommand::Show(a) => show(ctx, a),
        MachineCommand::Add(a) => add(ctx, a),
        MachineCommand::Edit(a) => edit(ctx, a),
        MachineCommand::Rm(a) => rm(ctx, a),
        MachineCommand::Test(a) => test(ctx, a),
    }
}

// ---------------------------------------------------------------------------
// 只读命令（走 config::load 的降级路径）
// ---------------------------------------------------------------------------

fn list(ctx: &Ctx) -> Result<(), CliError> {
    // 枚举规则来自 core 的单一来源（见 `MachinePreset::entries`）；
    // 文本行由 `MachineEntry::display_line` 提供，与 HTTP 侧同源。
    let entries = MachinePreset::entries(&ctx.loaded.merged.machine);
    let mut text = String::from("机床预设:\n");
    let mut presets: Vec<serde_json::Value> = Vec::new();
    for m in &entries {
        // CLI 列表有意不带完整 config（那是 `machine show` 的职责）
        presets.push(serde_json::json!({
            "id": m.id,
            "vendor": m.vendor,
            "model": m.model,
            "builtin": m.builtin,
        }));
        text.push_str(&m.display_line());
        text.push('\n');
    }
    let data = serde_json::json!({ "machines": presets });
    ctx.style.print_ok(&text, data);
    Ok(())
}

fn show(ctx: &Ctx, args: &MachineShowArgs) -> Result<(), CliError> {
    let m = ctx.resolve_machine(Some(&args.id))?;
    let mut text = format!("机床: {}\n", m.id);
    text.push_str(&format!("  厂商: {}\n  型号: {}\n", m.vendor, m.model));
    text.push_str("  配置:\n");
    for (k, v) in &m.config {
        text.push_str(&format!("    {:<24} {}\n", k, v));
    }
    // schema 告警（A4）：未知键 / 非法值提示，不阻断命令成功
    let warnings = nctool_core::machine::validate_config_keys(&m);
    if !warnings.is_empty() {
        text.push_str("  配置告警:\n");
        for w in &warnings {
            text.push_str(&format!("    ⚠ {w}\n"));
        }
    }
    let data = serde_json::json!({
        "id": m.id,
        "vendor": m.vendor,
        "model": m.model,
        "config": m.config,
        "warnings": warnings,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

// ---------------------------------------------------------------------------
// 写命令
// ---------------------------------------------------------------------------

/// `machine add`：新建自定义机床（默认以 generic 为基线）。
///
/// **全部校验都排在写盘动作之前**：早期实现把 id 合法性与 `preflight` 放在「首次
/// 创建写 `EXAMPLE_CONFIG`」之后，于是非法 id（或校验失败）的命令会先建出一份
/// `nctool.toml` 再报错 —— 失败的命令留下文件，与 AC-2.3 / AC-2.4 的「不落盘」
/// 相悖。语句顺序：① 内置保护 → ② id 合法 → ③ 组配置 → ④ 重名 → ⑤ 指纹预检
/// → ⑥ preflight → ⑦ 建文件 → ⑧ upsert。
fn add(ctx: &Ctx, args: &MachineAddArgs) -> Result<(), CliError> {
    // ① 内置保护（AC-2.2）：内置 3 预设不可改，拒绝且不落盘。
    reject_if_builtin(&args.id, "machine add <新id> --from <内置id>")?;

    // ② id 合法性：与 [`MachineWriter::upsert`] 内部同一份判定（`validate_asset_name`），
    //    单一来源、不在此另写一份规则；提前调用只为**在写盘之前**失败。
    validate_asset_name(&args.id)
        .map_err(|reason| CliError::new("args", format!("非法机床 id: {reason}")))?;

    // ③ 基线预设 → 目标配置（`--set` 覆盖）。
    let base = MachinePreset::from_id(&args.from)
        .ok_or_else(|| {
            CliError::new(
                "args",
                format!(
                    "未知基线预设：{}（可选：generic / wfl_m65 / index_ms40）",
                    args.from
                ),
            )
        })?
        .config();
    let base_keys = base.config.len();
    let mut cfg = MachineConfig {
        id: args.id.clone(),
        vendor: args.vendor.clone().unwrap_or(base.vendor),
        model: args.model.clone().unwrap_or(base.model),
        config: base.config,
    };
    apply_sets(&mut cfg, &args.set)?;

    // ④ 目标文件 + 重名检查（默认拒绝，`--force` 覆盖）。
    let path = machine_path(ctx, &args.file)?;
    let existing = load_machines(&path)?;
    let created = !existing.contains_key(&args.id);
    if !created && !args.force {
        return Err(CliError::new(
            "name_conflict",
            format!(
                "同名机床已存在：{}。可选：① --force 覆盖 ② machine edit {} 修改 ③ 换一个 id",
                args.id, args.id
            ),
        ));
    }

    // ⑤ `--expect-hash` 预检（格式 + 文件存在性）：先于建文件，避免"期望指纹但
    //    文件不存在"时误建文件。
    if let Some(want) = args.expect_hash.as_deref() {
        validate_fingerprint_format(want)?;
        if !path.exists() {
            return Err(write_conflict(format!(
                "期望指纹 {want}，但文件不存在：{}",
                path.display()
            )));
        }
    }

    // ⑥ 四重校验（缺键 / Choice / 整数 / 完整性）。纯函数：只读 `cfg` 与注册表，
    //    不碰目标文件，因此可以（也必须）排在创建文件之前。
    let warnings = preflight_or_fail(ctx, &cfg, &args.id)?;

    // ⑦ 首次创建：以 EXAMPLE_CONFIG 为初始内容（与 `config init` 产物一致；
    //    且 AC-2.10 的 golden 需要一份**带注释**的 nctool.toml 才有输入可验）。
    if !path.exists() {
        WriteKernel::write_atomic(&path, config::EXAMPLE_CONFIG.as_bytes())
            .map_err(|e| CliError::from_write_error(e, "machine_not_found"))?;
    }

    // ⑧ 落盘（乐观锁 expect = 当前指纹）。
    let expect = resolve_expect(&path, args.expect_hash.as_deref())?;
    let outcome = MachineWriter::upsert(&path, &cfg, expect)
        .map_err(|e| CliError::from_write_error(e, "machine_not_found"))?;

    let action = if created {
        "新建"
    } else {
        action_label(outcome.action)
    };
    let text = format!(
        "已保存机床: {}\n基线: {}（预填 {} 键，来自 {}）\n厂商/型号: {} / {}\n配置键: {} 个\n文件: {}\n动作: {}\n\
         提示：可用 `nctool machine test {} --template <模板> --param k=v` 试渲染验证。\n",
        cfg.id,
        args.from,
        base_keys,
        args.from,
        cfg.vendor,
        cfg.model,
        cfg.config.len(),
        path.display(),
        action,
        cfg.id,
    );
    let data = serde_json::json!({
        "id": cfg.id,
        "from": args.from,
        "baseKeys": base_keys,
        "vendor": cfg.vendor,
        "model": cfg.model,
        "configKeys": cfg.config.len(),
        "path": path.display().to_string(),
        "created": created,
        "action": outcome.action,
        "fileFingerprint": outcome.fingerprint,
        "warnings": warnings,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// `machine edit`：编辑既有自定义机床（合并式 upsert）。
fn edit(ctx: &Ctx, args: &MachineEditArgs) -> Result<(), CliError> {
    reject_if_builtin(&args.id, "machine add <新id> --from <内置id>")?;

    let path = machine_path(ctx, &args.file)?;
    let existing = load_machines(&path)?;
    let Some(mut cfg) = existing.get(&args.id).cloned() else {
        return Err(CliError::new(
            "machine_not_found",
            format!("机床不存在：{}（{}）", args.id, path.display()),
        ));
    };

    if let Some(v) = &args.vendor {
        cfg.vendor = v.clone();
    }
    if let Some(m) = &args.model {
        cfg.model = m.clone();
    }
    apply_sets(&mut cfg, &args.set)?;
    for k in &args.unset {
        cfg.config.remove(k);
    }

    let warnings = preflight_or_fail(ctx, &cfg, &args.id)?;
    let expect = resolve_expect(&path, args.expect_hash.as_deref())?;
    let outcome = MachineWriter::upsert(&path, &cfg, expect)
        .map_err(|e| CliError::from_write_error(e, "machine_not_found"))?;

    let text = format!(
        "已保存机床: {}\n厂商/型号: {} / {}\n配置键: {} 个\n文件: {}\n动作: {}\n\
         提示：可用 `nctool machine test {} --template <模板> --param k=v` 试渲染验证。\n",
        cfg.id,
        cfg.vendor,
        cfg.model,
        cfg.config.len(),
        path.display(),
        action_label(outcome.action),
        cfg.id,
    );
    let data = serde_json::json!({
        "id": cfg.id,
        "vendor": cfg.vendor,
        "model": cfg.model,
        "configKeys": cfg.config.len(),
        "path": path.display().to_string(),
        "action": outcome.action,
        "fileFingerprint": outcome.fingerprint,
        "warnings": warnings,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// `machine rm`：删除自定义机床。
fn rm(ctx: &Ctx, args: &MachineRmArgs) -> Result<(), CliError> {
    reject_if_builtin(&args.id, "machine add <新id> --from <内置id>")?;

    let path = machine_path(ctx, &args.file)?;
    if !args.yes {
        return Err(CliError::new(
            "args",
            format!(
                "删除是破坏性操作：{}。确认无误请加 --yes（该操作不可撤销）",
                args.id
            ),
        ));
    }
    let expect = resolve_expect(&path, args.expect_hash.as_deref())?;
    let outcome = MachineWriter::remove(&path, &args.id, expect)
        .map_err(|e| CliError::from_write_error(e, "machine_not_found"))?;

    let text = format!("已删除机床: {}\n文件: {}\n", args.id, path.display());
    let data = serde_json::json!({
        "id": args.id,
        "path": path.display().to_string(),
        // core 的 `remove` 已把动作修正为 `Deleted`（不透传 save 的 `Updated`）
        "action": outcome.action,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// `machine test`：试渲染验证（该机床 + 目标模板）。
///
/// **必须**走与 `commands::render` 完全同一条管线调用
/// （`gen.generate(...)` + [`GenerationOptions::default`]），保证 stdout 的 G-code
/// 与 `render --machine` **逐字节一致**（AC-2.7）。
fn test(ctx: &Ctx, args: &MachineTestArgs) -> Result<(), CliError> {
    // 机床：内置或自定义（不存在 → machine_not_found(5)）。
    let machine = ctx.resolve_machine(Some(&args.id))?;

    let gen = ctx.build_registry()?;
    let entry = gen.registry().get(&args.template).ok_or_else(|| {
        CliError::new(
            "template_not_found",
            format!("模板不存在: {}", args.template),
        )
    })?;
    let specs = entry.params.clone();
    let params = build_params(
        args.params.params_file.as_deref(),
        &args.params.param,
        &specs,
    )?;

    // 渲染前校验（与 render 同口径；有 Error 即阻断，退出码 1）
    let report = gen.registry().validate(&args.template, &params)?;
    if report.has_errors() {
        // 报告走 stderr：stdout 要留给 G-code（与 render 一致）
        eprintln!("{}", report.summary());
        return Err(CliError::new(
            "validation",
            "参数校验未通过（详见上方报告）",
        ));
    }
    let warnings: Vec<String> = report.warnings().map(|w| w.message.clone()).collect();

    // 与 render 完全同一条管线调用（AC-2.7）
    let out = gen.generate(
        &args.template,
        &params,
        &machine,
        &GenerationOptions::default(),
    )?;

    // R-8：安全文案不可弱化。写 stderr，保证 stdout 与 `render --machine` 逐字节一致。
    const NOTICE: &str = "试渲染不能替代真实空运行/工艺评审";
    eprintln!("提示：{NOTICE}。");

    let data = serde_json::json!({
        "template": args.template,
        "machine": args.id,
        "output": out,
        "warnings": warnings,
        "notice": NOTICE,
    });
    ctx.style.print_ok(&out, data);
    Ok(())
}

// ---------------------------------------------------------------------------
// 完整性所需键的收集（CLI 侧；core::asset 不依赖注册表）
// ---------------------------------------------------------------------------

/// 收集"该机床可见的全部模板"引用的 `machine.*` 键。
///
/// 返回 `(keys, warnings)`：模板解析失败只记 warning、**不阻断**
/// （否则一个无关模板的语法错误会让整台机床无法保存）。
///
/// 口径（单一来源）：
/// - 模板集合 = `registry.list_for_machine(Some(id), None, /*include_hidden=*/true)`
///   —— 通用模板（`machine=None`）+ 归属本机床的模板；新建机床尚无模板归属，
///   故实际为全部通用模板；隐藏模板也纳入（它们可被 `include`/程序调用）。
/// - 根名 = `registry.system_vars()` 里匹配到的注入变量名（默认 `"machine"`），
///   **不硬编码**（§7.10：系统注入变量名以 `system_vars()` 为单一来源）。
/// - 每个模板：`nctool_tpl::parse(&e.source_text, &e.name)` →
///   `nctool_tpl::extract_member_accesses(&ast, root)`（返回键按出现顺序去重）。
///
/// **注册表构建失败同样降级**（典型：`template_dir` 指向不存在的目录）：返回空集 +
/// 告警，**不阻断机床保存**——与 §10「一个无关模板的语法错误不应锁死机床编辑」
/// 同一取舍。机床编辑是配置管理操作，不应被模板目录缺失门禁；该问题会在
/// `render` / `machine test` 处以更明确的方式暴露（那两处仍走 `build_registry()?`）。
///
/// **为何必须在 CLI 侧现取 `parse`**：`extract_member_accesses` 需要 `&Ast`，
/// 而 `TemplateEntry` 只缓存 `Analysis`（`Ast` 借用源码，无法自引用存储）。
pub(crate) fn required_machine_keys(
    ctx: &Ctx,
    machine_id: &str,
) -> Result<(BTreeSet<String>, Vec<String>), CliError> {
    let gen = match ctx.build_registry() {
        Ok(gen) => gen,
        Err(e) => {
            return Ok((
                BTreeSet::new(),
                vec![format!(
                    "无法加载模板注册表，已跳过机床键完整性检查（保存不受阻；\
                     模板目录/模板问题会在 render 与 machine test 处暴露）：{e}"
                )],
            ));
        }
    };
    let registry = gen.registry();
    let root = registry
        .system_vars()
        .iter()
        .find(|s| s.as_str() == "machine")
        .or_else(|| registry.system_vars().first())
        .cloned()
        .unwrap_or_else(|| "machine".to_string());

    let mut keys = BTreeSet::new();
    let mut warnings = Vec::new();
    for entry in registry.list_for_machine(Some(machine_id), None, true) {
        match nctool_tpl::parse(&entry.source_text, &entry.name) {
            Ok(ast) => {
                for k in nctool_tpl::extract_member_accesses(&ast, &root) {
                    // 排除恒存在的元信息键（`machine.id/vendor/model`）：它们由
                    // 渲染上下文无条件注入，**不是** `config` 键——纳入"必需配置键"
                    // 会让任何引用元信息的模板被误判为缺键而阻断保存。
                    // 单一来源：`MachineConfig::META_KEYS`（与 `build_render_context` 对齐）。
                    if !nctool_core::MachineConfig::is_meta_key(&k) {
                        keys.insert(k);
                    }
                }
            }
            Err(e) => warnings.push(format!(
                "模板 {} 解析失败，已跳过其机床键收集（不影响保存，保存后 machine test 会暴露）：{e}",
                entry.name
            )),
        }
    }
    Ok((keys, warnings))
}

/// 收集所需键 → preflight；阻断则返回 `validation`(1)。通过时返回告警清单。
fn preflight_or_fail(
    ctx: &Ctx,
    cfg: &MachineConfig,
    machine_id: &str,
) -> Result<Vec<String>, CliError> {
    let (required, mut warnings) = required_machine_keys(ctx, machine_id)?;
    let report = MachineWriter::preflight(cfg, &required);
    if !report.can_save() {
        return Err(CliError::new(
            "validation",
            format!(
                "机床配置校验未通过，未落盘：\n{}",
                report
                    .blocking
                    .iter()
                    .map(|b| format!("  ✗ {b}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        ));
    }
    warnings.extend(report.warnings.iter().cloned());
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    Ok(warnings)
}

// ---------------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------------

/// 目标配置文件路径（`--file` 覆盖，否则项目 `nctool.toml`）。
fn machine_path(ctx: &Ctx, file: &MachineFileArgs) -> Result<PathBuf, CliError> {
    Ok(file
        .file
        .clone()
        .unwrap_or_else(|| ctx.project_config_path()))
}

/// 读目标文件的自定义机床（写路径的严格读取：损坏即报错，不静默覆盖）。
fn load_machines(
    path: &Path,
) -> Result<std::collections::BTreeMap<String, MachineConfig>, CliError> {
    MachineWriter::load(path).map_err(|e| CliError::from_write_error(e, "machine_not_found"))
}

/// 内置机床保护（AC-2.2）：内置 3 预设不可改，`args`(2)，不落盘。
fn reject_if_builtin(id: &str, hint: &str) -> Result<(), CliError> {
    if is_builtin_machine(id) {
        return Err(CliError::new(
            "args",
            format!("内置机床不可修改：{id}。如需定制，请派生一份自定义机床：`nctool {hint}`"),
        ));
    }
    Ok(())
}

/// 应用 `--set k=v` 覆盖到配置。
fn apply_sets(cfg: &mut MachineConfig, sets: &[String]) -> Result<(), CliError> {
    for kv in sets {
        let (k, v) = parse_set(kv)?;
        cfg.config.insert(k, v);
    }
    Ok(())
}

/// 解析 `k=v`（在**首个** `=` 处切分，值可含 `=`）。
fn parse_set(kv: &str) -> Result<(String, String), CliError> {
    match kv.split_once('=') {
        Some((k, v)) if !k.is_empty() => Ok((k.to_string(), v.to_string())),
        _ => Err(CliError::new(
            "args",
            format!("--set 参数格式应为 k=v（键不能为空），实际为 {kv:?}"),
        )),
    }
}

/// 计算乐观锁 `expect`：`--expect-hash` 显式给出时以它为准，否则用当前指纹。
///
/// `--expect-hash` 是**内容哈希**（`fnv1a64:<16hex>`），无法还原成完整
/// [`FileFingerprint`]（缺 mtime），故用作**预检**：与当前指纹比对一致后，
/// 实际写盘仍以当前快照为 `expect`。
fn resolve_expect(path: &Path, want: Option<&str>) -> Result<Option<FileFingerprint>, CliError> {
    let current = WriteKernel::read_fingerprint(path)
        .map_err(|e| CliError::from_write_error(e, "machine_not_found"))?;
    match want {
        None => Ok(current),
        Some(want) => match current {
            Some(fp) if fp.as_string() == want => Ok(Some(fp)),
            Some(fp) => Err(write_conflict(format!(
                "指纹不匹配（期望 {want}，实际 {}），文件已被外部修改，未写盘",
                fp.as_string()
            ))),
            None => Err(write_conflict(format!(
                "期望指纹 {want}，但文件不存在：{}",
                path.display()
            ))),
        },
    }
}

/// 校验 `--expect-hash` 的格式：`fnv1a64:<16 位十六进制>`；非法 → `args`(2)。
fn validate_fingerprint_format(s: &str) -> Result<(), CliError> {
    let ok = s
        .strip_prefix("fnv1a64:")
        .is_some_and(|hex| hex.len() == 16 && hex.chars().all(|c| c.is_ascii_hexdigit()));
    if ok {
        Ok(())
    } else {
        Err(CliError::new(
            "args",
            format!("--expect-hash 格式应为 fnv1a64:<16 位十六进制>，实际为 {s:?}"),
        ))
    }
}

/// `write_conflict`(6) 的便捷构造。
fn write_conflict(msg: impl Into<String>) -> CliError {
    CliError::new("write_conflict", msg.into())
}

/// 写动作的中文标签。
fn action_label(action: WriteAction) -> &'static str {
    match action {
        WriteAction::Created => "新建",
        WriteAction::Updated => "覆盖",
        WriteAction::Unchanged => "未变",
        WriteAction::Deleted => "删除",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_set_splits_at_first_equals() {
        assert_eq!(parse_set("a=1").unwrap(), ("a".into(), "1".into()));
        assert_eq!(parse_set("k=v=w").unwrap(), ("k".into(), "v=w".into()));
        assert_eq!(parse_set("k=").unwrap(), ("k".into(), "".into()));
        assert!(parse_set("noequals").is_err());
        assert!(parse_set("=v").is_err());
    }

    #[test]
    fn fingerprint_format_accepts_canonical_rejects_junk() {
        assert!(validate_fingerprint_format("fnv1a64:0000000000000000").is_ok());
        assert!(validate_fingerprint_format("fnv1a64:deadbeefdeadbeef").is_ok());
        assert!(validate_fingerprint_format("fnv1a64:abc").is_err());
        assert!(validate_fingerprint_format("deadbeefdeadbeef").is_err());
        assert!(validate_fingerprint_format("fnv1a64:gggggggggggggggg").is_err());
    }

    #[test]
    fn action_labels_cover_all_variants() {
        for (a, want) in [
            (WriteAction::Created, "新建"),
            (WriteAction::Updated, "覆盖"),
            (WriteAction::Unchanged, "未变"),
            (WriteAction::Deleted, "删除"),
        ] {
            assert_eq!(action_label(a), want);
        }
    }
}
