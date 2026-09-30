//! `preset` 子命令：参数预设的保存 / 列表 / 查看 / 重命名 / 删除 / 导入导出 / 应用。
//!
//! # 单一写通道
//!
//! 全部落盘经 [`PresetStore`]（内部走 `core::asset::WriteKernel`），本模块
//! 不自行拼接文件字节——与 `templates` / `machine` 同一约定。
//!
//! # 文件位置
//!
//! 默认落**配置目录**（`%APPDATA%\nctool\presets.yaml` / `$XDG_CONFIG_HOME/nctool/presets.yaml`）。
//! `--file` 可指定其它位置，但**必须落在模板目录之外**（红线 9 / R-9）：
//! 预设进模板根会被注册表当成模板目录扫描。该约束由
//! [`ensure_outside_template_root`] 强制，违反即拒（退出码 2）。
//!
//! # 与手填值同一套校验（AC-3.9）
//!
//! `save` 与 `import` 都调用 [`check_param_values`]（core 的**正向集合**入口，
//! 只校验已提供参数的值），并有 `SpecFingerprint` 记录落盘规格——陈旧检测的基线。

use std::path::{Path, PathBuf};

use nctool_core::asset::{
    default_preset_path, ensure_outside_template_root, now_iso8601, CrossTemplateReport, Preset,
    PresetFile, PresetStore, SpecFingerprint, StaleReport, WriteError,
};
use nctool_core::validate::check_param_values;
use nctool_core::ParamSpec;

use crate::cli::{
    PresetApplyArgs, PresetArgs, PresetCommand, PresetExportArgs, PresetFileArgs, PresetImportArgs,
    PresetListArgs, PresetRenameArgs, PresetRmArgs, PresetSaveArgs, PresetShowArgs,
};
use crate::context::Ctx;
use crate::output::CliError;

/// `preset` 命令分发。
pub fn run(ctx: &Ctx, args: &PresetArgs) -> Result<(), CliError> {
    match &args.command {
        PresetCommand::Save(a) => save(ctx, a),
        PresetCommand::List(a) => list(ctx, a),
        PresetCommand::Show(a) => show(ctx, a),
        PresetCommand::Rename(a) => rename(ctx, a),
        PresetCommand::Rm(a) => rm(ctx, a),
        PresetCommand::Export(a) => export(ctx, a),
        PresetCommand::Import(a) => import(ctx, a),
        PresetCommand::Apply(a) => apply(ctx, a),
    }
}

// ---------------------------------------------------------------------------
// 路径解析
// ---------------------------------------------------------------------------

/// 解析预设文件路径并施加"不得落在模板根内"约束。
///
/// # 为什么校验是**无条件**的
///
/// 早期实现写成 `if let Some(root) = &ctx.template_dir { ... }`，于是**只要
/// 模板目录未配置（`None`），这条红线就被静默跳过**。实测踩到：把
/// `--template-dir` 放在子命令**之后**时它不会被 `GlobalArgs` 收下，
/// `ctx.template_dir` 仍为 `None`，结果 `./templates/presets.yaml` 被照写不误
/// ——正是本项目零容忍的"校验条件性失效"。
///
/// 现在改为：**始终**取候选模板根集合（显式配置的 + 默认 `./templates` +
/// 注册表实际发现的模板目录）逐个比对，命中任一即拒。宁可在极少数场景下
/// 多拒一次（用户换路径即可），也不能让红线存在"看配置类型才生效"的缺口。
///
/// `pub(crate)`：HTTP 层（`server.rs`）的 `/api/presets` 同样要写这个文件，
/// 必须走**同一条**红线检查，不能各判一份。
pub(crate) fn preset_path(ctx: &Ctx, args: &PresetFileArgs) -> Result<PathBuf, CliError> {
    let path = args.file.clone().unwrap_or_else(default_preset_path);
    let roots = candidate_template_roots(ctx);
    for root in &roots {
        ensure_outside_template_root(&path, root)
            .map_err(|reason| CliError::new("args", reason))?;
    }
    Ok(path)
}

/// 收集所有可能的模板根（去重后用于包含性校验）。
fn candidate_template_roots(ctx: &Ctx) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if !roots.iter().any(|r| r == &p) {
            roots.push(p);
        }
    };
    // ① 显式配置的模板目录（全局选项或 nctool.toml）
    if let Some(dir) = &ctx.template_dir {
        push(dir.clone());
    }
    // ② 约定默认位置：即使未配置，也按惯例保护 `./templates`
    push(PathBuf::from("templates"));
    // ③ 注册表实际发现的模板文件所在目录（覆盖自定义目录布局）
    if let Ok(gen) = ctx.build_registry() {
        for entry in gen.registry().list(None) {
            if let nctool_core::TemplateSource::File(p) = &entry.source {
                if let Some(parent) = p.parent() {
                    push(parent.to_path_buf());
                }
            }
        }
    }
    roots
}

/// 写内核错误 → CLI 错误。
///
/// **委托**共享映射 [`CliError::from_write_error`]（消除 preset / machine /
/// templates 三份漂移），preset 上下文经显式参数传入：`NotFound` →
/// `preset_not_found`(5)、`Corrupt` → `io`(3)（预设是工具自有**资产**，损坏属
/// IO/内容问题；`machine` 的 `nctool.toml` 损坏是**配置**问题 → `config`(4)`
/// ——二者有意不同，设计 D4）。
fn map_write_err(e: WriteError) -> CliError {
    // preset 语义经显式上下文传入：`NotFound` → `preset_not_found`(5)、
    // `Corrupt` → `io`(3)（预设是工具自有**资产**，损坏属 IO/内容问题；
    // machine 的 `nctool.toml` 损坏则是**配置**问题 → `config`(4)，设计 D4）。
    CliError::from_write_error(e, "preset_not_found", "io")
}

/// `WriteError` → `CliError` 的 `From` 落点（P1-11 移入本模块）。
///
/// 唯一消费者：[`PresetStore::import_presets`] 的 `E: From<WriteError>`
/// 泛型约束（见 `preset import`），故语义 = **预设口径**（委托
/// `map_write_err`：`Corrupt` → `io(3)`、`NotFound` → `preset_not_found(5)`）。
///
/// ⚠️ 其它命令族**不要**用 `.into()`/`?` 隐式转换：machine 的 `Corrupt` 应归
/// `config(4)`，必须显式调
/// `CliError::from_write_error(e, "machine_not_found", "config")`。
impl From<WriteError> for CliError {
    fn from(err: WriteError) -> Self {
        map_write_err(err)
    }
}

/// 载入预设文件；降级时把警告打到 stderr 并**继续**（只读命令不应被损坏文件拦住）。
pub(crate) fn load_lenient(path: &Path) -> Result<PresetFile, CliError> {
    let got = PresetStore::load(path).map_err(map_write_err)?;
    for w in &got.warnings {
        eprintln!("warning: {w}");
    }
    Ok(got.file)
}

// ---------------------------------------------------------------------------
// 各子命令
// ---------------------------------------------------------------------------

/// 对一个预设做陈旧检测：解析模板的规格表**与**引用变量表后交给 core 判定。
///
/// 模板不可解析（被重命名/删除/语法坏了）→ 返回 `None`，由调用方标记
/// "模板缺失"而不是假装检测通过。**不得**在此时退回 `StaleReport::default()`
/// ——那会让一个已经落空的预设显示成"新鲜"，属静默误报。
pub(crate) fn stale_of(ctx: &Ctx, p: &Preset) -> Option<StaleReport> {
    let specs = specs_of(ctx, &p.template).ok()?;
    let (vars, required) = template_vars(ctx, &p.template).ok()?;
    Some(PresetStore::stale_report_full(
        p,
        &specs,
        Some(&vars),
        &required,
    ))
}

/// [`stale_of`] 的"指定目标模板"版本：`preset apply` 可经 `--template` 应用到
/// **另一个**模板，此时规格必须取目标模板的（调用方已解析好 `target_specs`）。
///
/// 语义与 [`stale_of`] **完全一致**：目标模板不可解析 → 返回 `None`，由调用方
/// 呈现为"检测跳过"，**不得**退回 `StaleReport::default()` —— 那会把一个已经落空
/// 的预设显示成"陈旧检测: 通过"，属静默误报。
///
/// # 可达性（实测更正，2026-09-27）
///
/// 审查报告（P1-6）称"目标模板有语法错误/被删/被改名时 `apply` 会输出检测通过"，
/// **该症状不成立**：`apply` 在更早的 `specs_of(ctx, &target)?` 处就已硬失败
/// （模板被删 → 退出码 5 `template_not_found`；模板语法坏 → 注册表构建失败）。
/// 因此本函数返回 `None` 目前是**防御性**的：它消除的是 `unwrap_or_default()`
/// 这个"吞掉 Err 假装空集"的写法本身，以及 `stale` 字段"跳过"与"通过"不可区分
/// 的契约缺陷（现为 `null`）。行为由
/// `apply_fails_loudly_when_target_template_is_unresolvable` 钉住。
pub(crate) fn stale_of_on(
    ctx: &Ctx,
    p: &Preset,
    target: &str,
    target_specs: &[ParamSpec],
) -> Option<StaleReport> {
    let (vars, required) = template_vars(ctx, target).ok()?;
    Some(PresetStore::stale_report_full(
        p,
        target_specs,
        Some(&vars),
        &required,
    ))
}

/// 解析某模板的**有效参数规格**（清单 / 变量库 / 头部 PARAMS 三层合并后的结果）。
pub(crate) fn specs_of(ctx: &Ctx, template: &str) -> Result<Vec<ParamSpec>, CliError> {
    let gen = ctx.build_registry()?;
    let entry = gen
        .registry()
        .get(template)
        .ok_or_else(|| CliError::new("template_not_found", format!("模板不存在: {template}")))?;
    Ok(entry.params.clone())
}

/// 模板的变量画像：`(全部引用变量, 无兜底的必选变量)`。
///
/// 判据取模板自身的变量提取结果，而非规格表：规格是"给参数加约束"的可选层，
/// 没有规格的模板照样可以有必选变量（此时 `specs_of` 返回空表）。若只用规格表
/// 判"参数是否属于该模板 / 是否算失效"，这类模板会得出完全错误的结论。
///
/// `Variable::optional == false` 即"无兜底的必选变量"（见 `nctool_tpl::extract`）。
///
/// 模板解析失败（语法错误）时返回 `Err`——此时无法判断参数归属，宁可拒绝保存。
pub(crate) fn template_vars(
    ctx: &Ctx,
    template: &str,
) -> Result<(std::collections::BTreeSet<String>, Vec<String>), CliError> {
    let gen = ctx.build_registry()?;
    let entry = gen
        .registry()
        .get(template)
        .ok_or_else(|| CliError::new("template_not_found", format!("模板不存在: {template}")))?;
    let analysis = entry.analysis().map_err(|e| {
        CliError::new(
            "validation",
            format!("模板 {template} 解析失败，无法确认参数归属：{e}"),
        )
    })?;
    let all = analysis.variables.iter().map(|v| v.name.clone()).collect();
    let required = analysis
        .variables
        .iter()
        .filter(|v| !v.optional)
        .map(|v| v.name.clone())
        .collect();
    Ok((all, required))
}

/// `preset save`：保存当前参数为命名预设。
fn save(ctx: &Ctx, args: &PresetSaveArgs) -> Result<(), CliError> {
    let path = preset_path(ctx, &args.file)?;
    let specs = specs_of(ctx, &args.template)?;

    // 1) 构造参数集（复用与 render/validate 完全相同的归一逻辑）
    let params = crate::context::build_params(
        args.params.params_file.as_deref(),
        &args.params.param,
        &specs,
    )?;
    if params.is_empty() {
        return Err(CliError::new(
            "args",
            "未提供任何参数（请用 --param k=v 或 --params-file）；空预设无意义",
        ));
    }

    // 2) L3 值级校验：与「手填参数后渲染」走同一套（AC-3.9）
    let report = check_param_values(&specs, &params);
    if report.has_errors() {
        return Err(CliError::new(
            "validation",
            format!(
                "参数值校验失败（预设值须与手填值同规）：\n{}",
                report.summary()
            ),
        ));
    }

    // 3) 参数名必须确实是该模板使用的变量——否则一落盘就是脏数据。
    //
    //    判据是**模板引用的变量**（`referenced_vars`）而非规格表：规格是可选层，
    //    没有规格的模板照样有必选变量。只用规格表判会让这类模板存不进任何预设。
    let (known, _required) = template_vars(ctx, &args.template)?;
    let unknown: Vec<&str> = params
        .values
        .keys()
        .filter(|k| !known.contains(k.as_str()))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        let mut hint: Vec<&str> = known.iter().map(String::as_str).collect();
        hint.sort_unstable();
        return Err(CliError::new(
            "args",
            format!(
                "参数不属于模板 {}：{}\n该模板使用的变量：{}",
                args.template,
                unknown.join(", "),
                if hint.is_empty() {
                    "（无）".to_string()
                } else {
                    hint.join(", ")
                }
            ),
        ));
    }

    // 4) 同名处理：默认拒绝（不静默覆盖用户既有预设）
    let existing = load_lenient(&path)?;
    if existing.get(&args.name).is_some() && !args.force {
        return Err(CliError::new(
            "name_conflict",
            format!(
                "同名预设已存在：{}。可选：① --force 覆盖 ② preset rename 改名 ③ 换一个名字",
                args.name
            ),
        ));
    }

    // 5) 落盘
    let spec_fingerprint = SpecFingerprint::of(&specs);
    let preset = Preset {
        name: args.name.clone(),
        template: args.template.clone(),
        params: params.clone(),
        created_at: now_iso8601(),
        spec_fingerprint: spec_fingerprint.clone(),
    };
    let outcome = PresetStore::upsert(&path, preset).map_err(map_write_err)?;
    let action = match outcome.action {
        nctool_core::asset::WriteAction::Created => "新建",
        nctool_core::asset::WriteAction::Updated => "覆盖",
        nctool_core::asset::WriteAction::Unchanged => "未变",
        // `save` 不会产生 `Deleted`（只有 `remove` 会），但枚举是穷尽的：
        // 不给全分支就要写 `_`，那会把将来新增的变体也一并静默吞掉。
        nctool_core::asset::WriteAction::Deleted => "删除",
    };

    let text = format!(
        "已保存预设: {}\n绑定模板: {}\n参数: {} 个（{}）\n文件: {}\n动作: {}\n\
         规格指纹: {}\n\
         提示：预设值须与手填值同规；上机前请按工艺要求复核，未经真实工艺评审。\n",
        args.name,
        args.template,
        params.len(),
        params.values.keys().cloned().collect::<Vec<_>>().join(", "),
        path.display(),
        action,
        outcome.fingerprint.as_deref().unwrap_or("-"),
    );
    let data = serde_json::json!({
        "name": args.name,
        "template": args.template,
        "paramCount": params.len(),
        "path": path.display().to_string(),
        "action": outcome.action,
        // 区分两个指纹：`specFingerprint` 是**规格**指纹（陈旧检测基线），
        // `fileFingerprint` 是**文件内容**指纹（乐观锁）。二者同名易混。
        "specFingerprint": spec_fingerprint,
        "fileFingerprint": outcome.fingerprint,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// `preset list`：列出预设（含陈旧标记）。
fn list(ctx: &Ctx, args: &PresetListArgs) -> Result<(), CliError> {
    let path = preset_path(ctx, &args.file)?;
    let file = load_lenient(&path)?;
    let items: Vec<&Preset> = file
        .presets
        .iter()
        .filter(|p| args.template.as_deref().is_none_or(|t| p.template == t))
        .collect();

    if items.is_empty() {
        let text = format!("预设列表（0 个）\n文件: {}\n", path.display());
        ctx.style.print_ok(
            &text,
            serde_json::json!({ "path": path.display().to_string(), "presets": [] }),
        );
        return Ok(());
    }

    // 规格解析失败的模板不阻断列表：陈旧一栏标记为 `unknown`。
    let mut rows = Vec::new();
    let mut text = format!(
        "预设列表（{} 个）\n文件: {}\n\n",
        items.len(),
        path.display()
    );
    for p in &items {
        let stale = stale_of(ctx, p);
        let flag = match &stale {
            Some(r) if r.is_stale() => " [陈旧]",
            Some(_) => "",
            None => " [模板缺失]",
        };
        text.push_str(&format!(
            "  {}{}  模板: {}  参数: {} 个\n",
            p.name,
            flag,
            p.template,
            p.params.len()
        ));
        if args.verbose {
            if let Some(r) = &stale {
                if !r.stale_params.is_empty() {
                    text.push_str(&format!("      失效参数: {}\n", r.stale_params.join(", ")));
                }
                if !r.missing_required.is_empty() {
                    text.push_str(&format!(
                        "      新增必选（预设未含）: {}\n",
                        r.missing_required.join(", ")
                    ));
                }
            }
        }
        rows.push(serde_json::json!({
            "name": p.name,
            "template": p.template,
            "paramCount": p.params.len(),
            "createdAt": p.created_at,
            "specFingerprint": p.spec_fingerprint,
            // 结构化字段（非拼接串）：消费方无需解析文本（D7）
            "resolvable": stale.is_some(),
            "stale": stale.as_ref().map(|r| r.is_stale()),
            "staleParams": stale.as_ref().map(|r| r.stale_params.clone()),
            "missingRequired": stale.as_ref().map(|r| r.missing_required.clone()),
        }));
    }
    let data = serde_json::json!({
        "path": path.display().to_string(),
        "presets": rows,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// `preset show`：查看单个预设内容 + 陈旧报告。
fn show(ctx: &Ctx, args: &PresetShowArgs) -> Result<(), CliError> {
    let path = preset_path(ctx, &args.file)?;
    let file = load_lenient(&path)?;
    let Some(p) = file.get(&args.name).cloned() else {
        return Err(CliError::new(
            "preset_not_found",
            format!("预设不存在：{}（文件 {}）", args.name, path.display()),
        ));
    };

    let mut text = format!(
        "预设: {}\n绑定模板: {}\n创建时间: {}\n规格指纹: {}\n参数（{} 个）:\n",
        p.name,
        p.template,
        p.created_at,
        p.spec_fingerprint,
        p.params.len()
    );
    let mut values = serde_json::Map::new();
    for (k, v) in &p.params.values {
        text.push_str(&format!("  {k} = {}\n", v.display()));
        values.insert(
            k.clone(),
            serde_json::to_value(v).unwrap_or(serde_json::Value::Null),
        );
    }

    let mut data = serde_json::json!({
        "name": p.name,
        "template": p.template,
        "createdAt": p.created_at,
        "specFingerprint": p.spec_fingerprint,
        "params": values,
    });

    match stale_of(ctx, &p) {
        Some(r) => {
            text.push_str(&stale_line(&r));
            data["stale"] = serde_json::to_value(&r).unwrap_or(serde_json::Value::Null);
        }
        None => {
            text.push_str(&format!(
                "陈旧检测: 跳过（模板 {} 当前不可解析；可能已被重命名或删除）\n",
                p.template
            ));
            data["stale"] = serde_json::Value::Null;
        }
    }
    text.push_str("提示：上机前请按工艺要求复核，未经真实工艺评审。\n");
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// 陈旧报告 → 一行人类可读描述。
fn stale_line(r: &StaleReport) -> String {
    if !r.is_stale() {
        return "陈旧检测: 通过（规格指纹一致，无失效参数）\n".to_string();
    }
    let mut out = String::from("陈旧检测: ");
    if r.fingerprint_changed {
        out.push_str("规格指纹已变化");
    }
    if !r.stale_params.is_empty() {
        out.push_str(&format!("；失效参数: {}", r.stale_params.join(", ")));
    }
    if !r.missing_required.is_empty() {
        out.push_str(&format!(
            "；新增必选（未含）: {}",
            r.missing_required.join(", ")
        ));
    }
    out.push('\n');
    out
}

/// `preset rename`：重命名（保留模板绑定与参数）。
fn rename(ctx: &Ctx, args: &PresetRenameArgs) -> Result<(), CliError> {
    let path = preset_path(ctx, &args.file)?;
    let outcome = PresetStore::rename(&path, &args.old, &args.new).map_err(map_write_err)?;
    let text = format!(
        "已重命名预设: {} → {}\n文件: {}\n（模板绑定与参数已保留）\n",
        args.old,
        args.new,
        path.display()
    );
    let data = serde_json::json!({
        "old": args.old,
        "new": args.new,
        "path": path.display().to_string(),
        "action": outcome.action,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// `preset rm`：删除预设。
fn rm(ctx: &Ctx, args: &PresetRmArgs) -> Result<(), CliError> {
    let path = preset_path(ctx, &args.file)?;
    if !args.yes {
        return Err(CliError::new(
            "args",
            format!(
                "删除是破坏性操作：{}。确认无误请加 --yes（该操作不可撤销，\
                 建议先 `preset export {}` 备份）",
                args.name, args.name
            ),
        ));
    }
    let outcome = PresetStore::remove(&path, &args.name).map_err(map_write_err)?;
    let text = format!("已删除预设: {}\n文件: {}\n", args.name, path.display());
    let data = serde_json::json!({
        "name": args.name,
        "path": path.display().to_string(),
        // 这里是 `Deleted`（core 的 `remove` 已修正为不透传 `save` 的动作）
        "action": outcome.action,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// `preset export`：导出为独立 YAML（可用 `--out` 落盘，或写 stdout）。
fn export(ctx: &Ctx, args: &PresetExportArgs) -> Result<(), CliError> {
    let path = preset_path(ctx, &args.file)?;
    let file = load_lenient(&path)?;

    let picked: Vec<Preset> = match &args.name {
        Some(n) => match file.get(n) {
            Some(p) => vec![p.clone()],
            None => {
                return Err(CliError::new(
                    "preset_not_found",
                    format!("预设不存在：{n}（文件 {}）", path.display()),
                ))
            }
        },
        None => file.presets.clone(),
    };
    if picked.is_empty() {
        return Err(CliError::new("args", "没有可导出的预设"));
    }

    let bundle = PresetFile {
        version: nctool_core::asset::PRESET_SCHEMA_VERSION,
        presets: picked.clone(),
    };
    let text = serde_yaml::to_string(&bundle)
        .map_err(|e| CliError::new("io", format!("预设序列化失败：{e}")))?;

    match &args.out {
        Some(out) => {
            std::fs::write(out, &text).map_err(|e| {
                CliError::new("io", format!("写入导出文件失败 {}: {e}", out.display()))
            })?;
            let msg = format!("已导出 {} 个预设 → {}\n", picked.len(), out.display());
            ctx.style.print_ok(
                &msg,
                serde_json::json!({
                    "out": out.display().to_string(),
                    "count": picked.len(),
                    "names": picked.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
                }),
            );
        }
        None => {
            // 纯文本通道给 YAML 本体（可直接重定向），JSON 通道给结构化包络。
            ctx.style.print_ok(
                &text,
                serde_json::json!({
                    "count": picked.len(),
                    "names": picked.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
                    "yaml": text,
                }),
            );
        }
    }
    Ok(())
}

/// `preset import`：从导出文件导入。
fn import(ctx: &Ctx, args: &PresetImportArgs) -> Result<(), CliError> {
    let path = preset_path(ctx, &args.file)?;
    let raw = if args.input == "-" {
        // stdin **无法预检长度**，此前是完全无界的 `read_to_string`（P1-1）
        crate::args::read_stdin_capped("stdin")?
    } else {
        crate::args::read_text_capped(Path::new(&args.input), "导入文件")?
    };

    // 导入的每个预设都要过与 `save` 同一套值级校验（AC-3.9）；
    // 模板不可解析 → 视作"规格为空"，此时只做有限性校验（不阻断合法历史预设）。
    let presets = PresetStore::import_presets::<CliError, _>(&raw, |p| {
        let specs = match specs_of(ctx, &p.template) {
            Ok(s) => s,
            Err(e) => {
                // 不阻断导入（合法的历史预设可能引用已改名/删除的模板），
                // 但**不能无声**：降级后只做有限性校验，用户必须知道
                // "这组参数没有经过类型/区间/白名单校验"。
                eprintln!(
                    "warning: 预设「{}」的模板 {} 规格不可解析（{e}），已按无规格处理（仅做有限性校验）",
                    p.name, p.template
                );
                Vec::new()
            }
        };
        let report = check_param_values(&specs, &p.params);
        if report.has_errors() {
            return Err(CliError::new(
                "validation",
                format!("预设「{}」参数值非法：\n{}", p.name, report.summary()),
            ));
        }
        Ok(())
    })?;

    let mut existing = load_lenient(&path)?;
    let conflicts: Vec<String> = presets
        .iter()
        .filter(|p| existing.get(&p.name).is_some())
        .map(|p| p.name.clone())
        .collect();
    if !conflicts.is_empty() && !args.force {
        return Err(CliError::new(
            "name_conflict",
            format!(
                "以下预设已存在，未导入：{}。可选：① --force 覆盖 ② 先 preset rename 改名",
                conflicts.join(", ")
            ),
        ));
    }

    let imported: Vec<String> = presets.iter().map(|p| p.name.clone()).collect();
    for p in presets {
        existing.take(&p.name);
        existing.presets.push(p);
    }

    // 经写内核落盘（乐观锁 expect = 导入前快照）
    let expect = nctool_core::asset::WriteKernel::read_fingerprint(&path).map_err(map_write_err)?;
    let outcome = PresetStore::save(&path, &existing, expect).map_err(map_write_err)?;

    let text = format!(
        "已导入 {} 个预设: {}\n文件: {}\n",
        imported.len(),
        imported.join(", "),
        path.display()
    );
    let data = serde_json::json!({
        "imported": imported,
        "count": imported.len(),
        "path": path.display().to_string(),
        "action": outcome.action,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// `preset apply`：应用预设（校验 + 差异预览；跨模板需显式确认）。
fn apply(ctx: &Ctx, args: &PresetApplyArgs) -> Result<(), CliError> {
    let path = preset_path(ctx, &args.file)?;
    let file = load_lenient(&path)?;
    let Some(p) = file.get(&args.name).cloned() else {
        return Err(CliError::new(
            "preset_not_found",
            format!("预设不存在：{}（文件 {}）", args.name, path.display()),
        ));
    };

    let target = args.template.clone().unwrap_or_else(|| p.template.clone());
    let target_specs = specs_of(ctx, &target)?;
    let cross = target != p.template;

    // 跨模板：先出交集报告；未 --confirm 时**不写不渲染**，只展示（AC-3.6）
    let mut cross_report: Option<CrossTemplateReport> = None;
    let mut text = String::new();
    if cross {
        let src_specs = match specs_of(ctx, &p.template) {
            Ok(s) => s,
            Err(e) => {
                // 源模板不可解析 → 交集报告里"可复用/需确认"会整体偏保守，
                // 必须让用户知道这份报告是在缺源规格的前提下算出来的。
                eprintln!(
                    "warning: 源模板 {} 规格不可解析（{e}），跨模板报告已按无规格处理（结果偏保守）",
                    p.template
                );
                Vec::new()
            }
        };
        let r = PresetStore::cross_template_report(&p, &src_specs, &target_specs);
        text.push_str(&format!(
            "跨模板应用: {} → {}\n\
             可复用: {}\n需确认（类型不匹配）: {}\n目标模板缺失必选: {}\n",
            p.template,
            target,
            list_or_none(&r.reusable),
            list_or_none(&r.needs_confirm),
            list_or_none(&r.missing),
        ));
        let needs_confirm = !r.missing.is_empty() || !r.needs_confirm.is_empty();
        if needs_confirm && !args.confirm {
            text.push_str(
                "\n未写入任何内容：存在需确认项（见上）。确认无误请加 --confirm 重试。\n\
                 提示：缺失必选参数会导致渲染失败，请先用 --param 补足或改用其它预设。\n",
            );
            let data = serde_json::json!({
                "applied": false,
                "needsConfirm": true,
                "target": target,
                "crossTemplate": r,
            });
            ctx.style.print_ok(&text, data);
            return Ok(());
        }
        cross_report = Some(r);
    }

    // 值级校验（与手填值同规）——目标模板规格下的重新校验
    let report = check_param_values(&target_specs, &p.params);
    if report.has_errors() {
        return Err(CliError::new(
            "validation",
            format!(
                "预设值在目标模板（{}）下校验失败：\n{}",
                target,
                report.summary()
            ),
        ));
    }

    // 陈旧检测：目标模板不可解析时**必须显式跳过**，不得静默显示"通过"
    // （P1-6：此前 `template_vars(...).unwrap_or_default()` 把 Err 吞成空集，
    // 于是陈旧检测拿到空变量集，"检测通过"是假象）。
    let stale = stale_of_on(ctx, &p, &target, &target_specs);
    text.push_str(&format!(
        "预设: {}\n目标模板: {}\n参数: {}\n",
        p.name,
        target,
        p.params
            .values
            .iter()
            .map(|(k, v)| format!("{k}={}", v.display()))
            .collect::<Vec<_>>()
            .join("  "),
    ));
    match &stale {
        Some(report) => text.push_str(&stale_line(report)),
        None => text.push_str(&format!(
            "陈旧检测: 跳过（目标模板 {target} 当前不可解析；可能已被重命名或删除，\
             请先修复模板再上机）\n"
        )),
    }
    text.push_str(
        "提示：以上为**生效参数**，与手填值走同一套校验；\n\
         上机前请按工艺要求复核，未经真实工艺评审。\n",
    );

    let data = serde_json::json!({
        "applied": true,
        "name": p.name,
        "target": target,
        "crossTemplate": cross_report,
        "params": p.params.values.iter()
            .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or(serde_json::Value::Null)))
            .collect::<serde_json::Map<_, _>>(),
        // 跳过检测时为 null（而不是 `{}`）：消费方须能区分"检测通过"与"没检测"
        "stale": match &stale {
            Some(r) => serde_json::to_value(r).unwrap_or(serde_json::Value::Null),
            None => serde_json::Value::Null,
        },
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// 列表为空时输出 `（无）`，便于纯文本通道阅读。
fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "（无）".to_string()
    } else {
        items.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_line_reports_clean_and_stale() {
        let clean = StaleReport::default();
        assert!(stale_line(&clean).contains("通过"));

        let stale = StaleReport {
            fingerprint_changed: true,
            stale_params: vec!["old".into()],
            missing_required: vec!["fresh".into()],
        };
        let line = stale_line(&stale);
        assert!(line.contains("规格指纹已变化"), "{line}");
        assert!(line.contains("old"), "{line}");
        assert!(line.contains("fresh"), "{line}");
    }

    #[test]
    fn list_or_none_handles_empty() {
        assert_eq!(list_or_none(&[]), "（无）");
        assert_eq!(list_or_none(&["a".into(), "b".into()]), "a, b");
    }

    #[test]
    fn map_write_err_classifies_duplicate_name_as_name_conflict() {
        let e = map_write_err(WriteError::PathEscape {
            rel: "x".into(),
            reason: "同名预设已存在".into(),
        });
        assert_eq!(e.kind, "name_conflict");
        assert_eq!(e.exit_code(), 6);

        let e = map_write_err(WriteError::PathEscape {
            rel: "../x".into(),
            reason: "越界".into(),
        });
        assert_eq!(e.kind, "args");
        assert_eq!(e.exit_code(), 2);
    }

    #[test]
    fn map_write_err_conflict_and_io_codes() {
        let e = map_write_err(WriteError::Conflict {
            path: PathBuf::from("p.yaml"),
            expected: None,
            actual: None,
        });
        assert_eq!(e.kind, "write_conflict");
        assert_eq!(e.exit_code(), 6);

        // "预设不存在"走**独立变体**（不再靠匹配消息文本），退出码 5
        let e = map_write_err(WriteError::NotFound("预设不存在：x".into()));
        assert_eq!(e.kind, "preset_not_found");
        assert_eq!(e.exit_code(), 5);

        let e = map_write_err(WriteError::Corrupt("预设文件不可用".into()));
        assert_eq!(e.kind, "io");
        assert_eq!(e.exit_code(), 3);
    }
}
