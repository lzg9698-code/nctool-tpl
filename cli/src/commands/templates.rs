//! `templates` 子命令：列表 / 查看 / 新建 / 修改 / 派生 / 重命名。

use std::path::{Path, PathBuf};

use nctool_core::asset::{
    build_derived_source, last_component, manifest_append_entry, manifest_is_parseable,
    scan_stale_includes, sibling_rel_key, ManifestOutcome, TemplateWriter, WriteError, WriteKernel,
};
use nctool_core::manifest::{ResolvedMeta, TemplateManifest, MANIFEST_FILE};
use nctool_core::validate::{check_param_values, check_spec_consistency};
use nctool_core::variables::VariableLibrary;
use nctool_core::TemplateSource;
use nctool_tpl::extract_undeclared;

use crate::cli::{
    CategoryArg, ParamInputArgs, TemplatesArgs, TemplatesCommand, TemplatesDeriveArgs,
    TemplatesEditArgs, TemplatesListArgs, TemplatesNewArgs, TemplatesRenameArgs,
};
use crate::context::Ctx;
use crate::output::CliError;

/// `templates` 命令分发。
pub fn run(ctx: &Ctx, args: &TemplatesArgs) -> Result<(), CliError> {
    match &args.command {
        TemplatesCommand::List(a) => list(ctx, a),
        TemplatesCommand::Show(a) => show(ctx, &a.template),
        TemplatesCommand::New(a) => new(ctx, a),
        TemplatesCommand::Edit(a) => edit(ctx, a),
        TemplatesCommand::Derive(a) => derive(ctx, a),
        TemplatesCommand::Rename(a) => rename(ctx, a),
    }
}

fn list(ctx: &Ctx, args: &TemplatesListArgs) -> Result<(), CliError> {
    let gen = ctx.build_registry()?;
    // 默认只列可见模板；--machine 会额外放开该方案包内的模板，
    // 但仍尊重 --all（未指定 --all 时方案包内的隐藏模板依然不显示）。
    let entries = if args.machine.is_some() {
        gen.registry().list_for_machine(
            args.machine.as_deref(),
            args.category.map(CategoryArg::to_core),
            args.all,
        )
    } else if args.all {
        gen.registry().list(args.category.map(CategoryArg::to_core))
    } else {
        gen.registry()
            .list_visible(args.category.map(CategoryArg::to_core))
    };

    let hidden = entries.iter().filter(|e| !e.visible).count();

    // JSON 数据
    let data: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "category": CategoryArg::from_core(e.category),
                "description": e.description,
                "visible": e.visible,
                "output_filename": e.output_filename,
                "output_extension": e.output_extension,
                "machine": e.machine,
                "status": e.status.map(|s| s.label()),
                "source": match &e.source {
                    nctool_core::TemplateSource::Builtin => "builtin".to_string(),
                    nctool_core::TemplateSource::Memory => "memory".to_string(),
                    nctool_core::TemplateSource::File(p) => p.display().to_string(),
                },
            })
        })
        .collect();

    // 文本输出
    let mut text = format!("模板列表（{} 个", entries.len());
    if hidden > 0 {
        text.push_str(&format!("，其中隐藏 {hidden}"));
    }
    text.push_str("）\n");
    for e in &entries {
        // 隐藏模板加标记，让 --all 场景下的输出自解释
        let mark = if e.visible { "  " } else { "· " };
        text.push_str(&format!(
            "{mark}{:<32} {:<4} {:<5} {}\n",
            e.name,
            CategoryArg::from_core(e.category),
            e.output_extension,
            e.description
        ));
    }
    if !args.all && hidden == 0 {
        text.push_str("\n（隐藏模板未显示；用 --all 查看全部）\n");
    }
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// 解析模板源码与参数规格：`show` / `inspect` 共用。
///
/// 模板解析结果：名称、源码、参数规格（文件模板无规格）和系统变量。
type ResolvedSource = (
    String,
    String,
    Option<Vec<nctool_core::ParamSpec>>,
    Vec<String>,
);

/// 返回 `(模板名, 源码, 参数规格, 系统变量)`；文件模板无规格 → `None`。
/// 优先级与 `render`/`validate` 一致：**已注册模板名（内置/目录）→ 文件路径**，
/// 保证"查看的源码"与"实际渲染的源码"是同一份。
pub fn resolve_source(ctx: &Ctx, name_or_path: &str) -> Result<ResolvedSource, CliError> {
    // 1) 已注册模板（内置 / 目录）
    let gen = ctx.build_registry()?;
    if let Some(entry) = gen.registry().get(name_or_path) {
        return Ok((
            entry.name.clone(),
            entry.source_text.clone(),
            Some(entry.params.clone()),
            gen.registry().system_vars().to_vec(),
        ));
    }
    // 2) 文件路径 → 读源码（名称用文件名）
    if let Some(path) = ctx.find_template_file(name_or_path) {
        let source = std::fs::read_to_string(&path)
            .map_err(|e| CliError::new("io", format!("读取模板失败 {}: {e}", path.display())))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| name_or_path.to_string());
        return Ok((name, source, None, gen.registry().system_vars().to_vec()));
    }
    Err(CliError::new(
        "template_not_found",
        format!("模板不存在: {name_or_path}"),
    ))
}

/// 提取模板变量（必选/可选 + 行列定位）。
///
/// 过滤系统注入变量（如 `machine`）——它们由管线注入上下文，不算外部必选参数。
/// `system_vars` 取自注册表（单一事实源，避免 CLI 侧常量与 core 漂移）。
pub fn extract_variables(
    source: &str,
    name: &str,
    system_vars: &[String],
) -> Result<Vec<nctool_tpl::Variable>, CliError> {
    let ast = nctool_tpl::parse(source, name)?;
    let vars = extract_undeclared(&ast);
    Ok(vars
        .into_iter()
        .filter(|v| !system_vars.iter().any(|s| s == &v.name))
        .collect())
}

fn show(ctx: &Ctx, name_or_path: &str) -> Result<(), CliError> {
    let (name, source, _, system_vars) = resolve_source(ctx, name_or_path)?;
    let vars = extract_variables(&source, &name, &system_vars)?;

    let required: Vec<_> = vars.iter().filter(|v| !v.optional).collect();
    let optional: Vec<_> = vars.iter().filter(|v| v.optional).collect();

    // JSON
    let var_json: Vec<serde_json::Value> = vars
        .iter()
        .map(|v| {
            serde_json::json!({
                "name": v.name,
                "optional": v.optional,
                "line": v.line,
                "col": v.col,
            })
        })
        .collect();
    let data = serde_json::json!({
        "name": name,
        "source": source,
        "variables": var_json,
        "required": required.len(),
        "optional": optional.len(),
    });

    // 文本
    let mut text = format!("模板: {name}\n");
    text.push_str(&format!("必选参数（{}）:\n", required.len()));
    for v in &required {
        text.push_str(&format!("  {}  行 {} 列 {}\n", v.name, v.line, v.col));
    }
    text.push_str(&format!("可选参数（{}）:\n", optional.len()));
    for v in &optional {
        text.push_str(&format!("  {}  行 {} 列 {}\n", v.name, v.line, v.col));
    }
    text.push_str("---- 源码 ----\n");
    text.push_str(&source);
    if !source.ends_with('\n') {
        text.push('\n');
    }

    ctx.style.print_ok(&text, data);
    Ok(())
}

/// 生成新模板骨架源码。
fn scaffold_source(name: &str, category: &str) -> String {
    format!(
        "( {name} 模板骨架 )\n\
         ( 分类: {category} )\n\
         ( 参数规格注释: 模板引用的变量即参数；无 default 兜底的为必选 )\n\
         ( 示例: 使用内置数学过滤器与 NC 数值格式化过滤器 )\n\
         \n\
         {{{{ machine.program_prefix }}}}{{{{ prog | nc_pad(machine.program_digits | int) }}}}\n\
         {{{{ machine.coordinate_system }}}}\n\
         G0 X{{{{ x | nc_fixed(3) }}}} Y{{{{ y | nc_fixed(3) }}}}\n\
         G1 Z{{{{ depth | nc_fixed(3) }}}} F{{{{ feed | nc_fixed(3) }}}}\n\
         M5\nM9\n\
         {{{{ machine.program_end }}}}\n",
        name = name,
        category = category,
    )
}

fn new(ctx: &Ctx, args: &TemplatesNewArgs) -> Result<(), CliError> {
    validate_template_name(&args.name)?;
    // 目录：显式 --dir 优先，否则配置模板目录，否则 ./templates
    let dir: PathBuf = match &args.dir {
        Some(d) => d.clone(),
        None => ctx
            .template_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("templates")),
    };
    std::fs::create_dir_all(&dir)
        .map_err(|e| CliError::new("io", format!("创建目录失败 {}: {e}", dir.display())))?;

    // 名字已带 .j2 时不再追加扩展名（避免生成 a.j2.j2）
    let file_name = if args.name.to_lowercase().ends_with(".j2") {
        args.name.clone()
    } else {
        format!("{}.j2", args.name)
    };
    let path = dir.join(&file_name);
    let source = scaffold_source(&args.name, CategoryArg::from_core(args.category.to_core()));

    // §7.15：内容写操作落盘前**必须**校验。`new` 不接受用户参数（L3 不适用），
    // 故只跑 L1 语法 + L2 规格自洽；**不向 stdout 增加级别声明**——声明义务只
    // 落在接受用户内容的 `edit` / `derive` 上。骨架由工具生成，L2 通常通过，
    // 此处的价值是让契约成真、并在骨架模板将来演进时拦住回归。
    let specs = resolve_specs(&dir, &file_name, &source)?;
    run_l1_l2(&source, &file_name, &specs)?;

    // 经写内核落盘：原子写（tmp + rename，无半成品）+ 不跟随符号链接。
    // ⚠️ 重名检测仍是**写前快照比对**（`write_guarded(expect=None)` 先
    // `read_snapshot` 再 `write_atomic`），**非** `O_EXCL` 原子创建——两个并发
    // 同名 `new` 仍可能双双通过检查、后 rename 者静默覆盖；要真原子，`create`
    // 需改走 `OpenOptions::create_new`。见设计 §6.7 / R-11。
    match TemplateWriter::create(&dir, &file_name, &source) {
        Ok(_) => {}
        Err(WriteError::Conflict { .. }) => {
            // 归 `template_duplicate`（退出码 6）：业务冲突（重名），非 IO 失败。
            return Err(CliError::new(
                "template_duplicate",
                format!("模板已存在，不覆盖: {}", path.display()),
            ));
        }
        Err(e) => return Err(map_write_err(e)),
    }

    // 追加清单条目（定点文本编辑）；清单缺失/损坏 → 降级为警告，不阻断。
    append_manifest_for_new(&dir, &file_name, &args.name);

    let text = format!("已创建模板: {}\n", path.display());
    let data = serde_json::json!({ "path": path.display().to_string(), "name": args.name });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// 为 `templates new` 追加一条最小清单条目（若清单存在且可用）。
///
/// 降级语义（D13）：清单不存在 / 无 `templates:` 块 → 打印警告 + 提示手补，
/// **不**阻断（模板文件已成功创建，且目录扫描仍能发现它）。
fn append_manifest_for_new(dir: &Path, file_name: &str, display_name: &str) {
    let path = dir.join(MANIFEST_FILE);
    if !path.exists() {
        eprintln!(
            "warning: 清单文件不存在（{}），未登记条目；如需元数据请手动补录",
            path.display()
        );
        return;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "warning: 清单读取失败（{}）：{e}；请手动补录条目",
                path.display()
            );
            return;
        }
    };
    // 改写前先试解析：损坏清单**不得静默改写**（一个字节都不改），降级为警告。
    // 定点文本编辑是纯文本操作，若不显式试解析，"解析失败"永远检测不到，
    // 文档承诺的降级行为就不存在。
    if !manifest_is_parseable(&text) {
        eprintln!(
            "warning: 清单解析失败（{}），未登记条目，且未改动清单；请手动补录",
            path.display()
        );
        return;
    }
    // body 行缩进"相对键行"（键行缩进 2 空格 → 字段行 2 空格 → 落盘后为 4 空格）
    let body = vec![
        format!("  name: \"{display_name}\""),
        "  status: unreviewed".to_string(),
    ];
    match manifest_append_entry(&text, file_name, &body) {
        Some(new_text) => {
            let expect = WriteKernel::read_fingerprint(&path).ok().flatten();
            if let Err(e) = WriteKernel::write_guarded(&path, new_text.as_bytes(), expect) {
                eprintln!("warning: 清单写入失败（模板文件已创建）：{e}；请手动补录条目");
            }
        }
        None => eprintln!(
            "warning: 清单缺少顶层 `templates:` 块（{}），未登记条目；请手动补录",
            path.display()
        ),
    }
}

/// 校验模板名：必须是单个合法文件名组件，禁止路径分隔符与 `..`，
/// 防止 `templates new` 逃出模板目录（路径穿越）。
///
/// 规则**已提升为单一来源** [`nctool_core::asset::validate_asset_name`]（模板名 /
/// 机床 id / 预设名共用），此处仅做"错误类型 + 退出码分类"的转发：名称非法一律
/// 归 `args`（退出码 2，与 clap 用法错误同族）。
fn validate_template_name(name: &str) -> Result<(), CliError> {
    nctool_core::asset::validate_asset_name(name).map_err(|reason| CliError::new("args", reason))
}

// ---------------------------------------------------------------------------
// templates edit / derive / rename
// ---------------------------------------------------------------------------

/// 本次实际执行到的校验级别（用于在输出里**明示**做了哪几级，杜绝静默降级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValidationTier {
    /// 只做了 L1 语法 + L2 规格自洽（未提供参数，未做 L3）。
    SyntaxAndSpec,
    /// L1 + L2 + L3（提供了参数，走了**值级**校验）。
    Full,
}

impl ValidationTier {
    /// 人类可读的级别声明（写进成功输出）。
    fn declaration(self) -> &'static str {
        match self {
            ValidationTier::SyntaxAndSpec => {
                "本次仅完成语法（L1）与规格自洽（L2）校验；未提供参数，未执行参数值校验（L3）。"
            }
            ValidationTier::Full => {
                "本次已完成语法（L1）/ 规格自洽（L2）/ 参数值（L3）校验（仅校验已提供参数的值，不检查缺失）。"
            }
        }
    }

    /// 机器可读的级别列表（JSON 契约字段 `validationLevels`）。
    ///
    /// 用**数组**而非拼接串（如 `"syntax+spec"`）：后者要求消费方解析字符串，
    /// 正是"按消息文本决策"的反面教材（D7）。
    fn levels(self) -> &'static [&'static str] {
        match self {
            ValidationTier::SyntaxAndSpec => &["L1", "L2"],
            ValidationTier::Full => &["L1", "L2", "L3"],
        }
    }
}

/// `templates edit`：修改并保存模板源码（保存前分级校验 + 乐观锁）。
fn edit(ctx: &Ctx, args: &TemplatesEditArgs) -> Result<(), CliError> {
    let root = template_root(ctx)?;
    let (rel_key, path) = locate_editable(ctx, &args.template)?;

    // 1) 打开时快照（乐观锁基线）
    let snapshot = WriteKernel::read_fingerprint(&path)
        .map_err(|e| CliError::new("io", format!("读取模板指纹失败：{e}")))?;
    let Some(snap) = snapshot else {
        return Err(CliError::new(
            "io",
            format!("模板文件不存在：{}", path.display()),
        ));
    };

    // 2) --expect-hash：与当前指纹比对，不一致即判并发冲突（不写盘）
    if let Some(want) = &args.expect_hash {
        let actual = snap.as_string();
        if &actual != want {
            return Err(CliError::new(
                "write_conflict",
                format!(
                    "并发冲突：指纹不匹配（期望 {want}，实际 {actual}），文件已被外部修改，未写盘。\
                     可选：① 覆盖（去掉 --expect-hash 以当前内容为基线重存）\
                     ② 放弃 ③ 另存为新模板（templates derive）"
                ),
            ));
        }
    }

    // 3) 取得新源码（--from-file 或 $EDITOR）
    let new_source = obtain_new_source(args, &path)?;

    // 4) 分级校验（L1 总是 / L2 总是 / L3 仅当提供了参数）
    let specs = resolve_specs(&root, &rel_key, &new_source)?;
    let tier = run_tiered_validation(&new_source, &args.template, &specs, &args.params)?;

    // 5) 经写内核落盘（乐观锁 expect = 快照指纹）
    let outcome =
        TemplateWriter::save(&root, &rel_key, &new_source, Some(snap)).map_err(map_write_err)?;

    // 6) 报告
    let text = format!(
        "已保存模板: {}\n动作: {:?}\n{}\n",
        outcome.path.display(),
        outcome.action,
        tier.declaration()
    );
    let data = serde_json::json!({
        "path": outcome.path.display().to_string(),
        "action": outcome.action,
        "fingerprint": outcome.fingerprint,
        // 数组（非拼接串）：消费方按元素判断，无需解析字符串（D7）。
        "validationLevels": tier.levels(),
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// `templates derive`：以源模板为蓝本派生新模板。
fn derive(ctx: &Ctx, args: &TemplatesDeriveArgs) -> Result<(), CliError> {
    validate_template_name(&args.new)?;
    let root = template_root(ctx)?;
    let (src_key, src_path) = locate_editable(ctx, &args.src)?;
    let src_source = std::fs::read_to_string(&src_path)
        .map_err(|e| CliError::new("io", format!("读取源模板失败 {}: {e}", src_path.display())))?;

    let new_source = build_derived_source(&src_source, &args.new, &src_key, !args.no_derive_note);
    let dst_key = sibling_rel_key(&src_key, &args.new);

    // 走写前分级校验（L1/L2 必做；提供了参数则 L3）
    let specs = resolve_specs(&root, &src_key, &new_source)?;
    let tier = run_tiered_validation(&new_source, &args.new, &specs, &args.params)?;

    let report = TemplateWriter::derive(&root, &src_key, &dst_key, &new_source)
        .map_err(|e| map_create_err(e, &dst_key))?;

    let manifest_note = describe_manifest(&report.manifest);
    let text = format!(
        "已派生模板: {}\n源模板: {}\n清单: {}\n{}\n",
        report.file.path.display(),
        src_key,
        manifest_note,
        tier.declaration()
    );
    let data = serde_json::json!({
        "path": report.file.path.display().to_string(),
        "source": src_key,
        "key": dst_key,
        "action": report.file.action,
        "manifest": manifest_note,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// `templates rename`：重命名模板（同步清单键 + 警告 include 引用）。
fn rename(ctx: &Ctx, args: &TemplatesRenameArgs) -> Result<(), CliError> {
    validate_template_name(&args.new)?;
    let root = template_root(ctx)?;
    let (old_key, _old_path) = locate_editable(ctx, &args.old)?;
    let new_key = sibling_rel_key(&old_key, &args.new);

    let report = TemplateWriter::rename(&root, &old_key, &new_key)
        .map_err(|e| map_create_err(e, &new_key))?;

    // 扫描 include 引用（只警告、不自动改）
    let scan = collect_template_sources(ctx)?;
    let old_name = last_component(&old_key);
    let stale = scan_stale_includes(&scan, &old_key, old_name);

    let manifest_note = describe_manifest(&report.manifest);
    let mut text = format!(
        "已重命名模板: {} → {}\n清单: {}\n",
        old_key,
        report.file.path.display(),
        manifest_note
    );
    if stale.is_empty() {
        text.push_str("include 引用检查：无引用旧名的模板。\n");
    } else {
        text.push_str("include 引用检查：以下模板仍引用旧名（**未自动改写**，请手动处理）：\n");
        for k in &stale {
            text.push_str(&format!("  - {k}\n"));
        }
    }
    let data = serde_json::json!({
        "path": report.file.path.display().to_string(),
        "old": old_key,
        "new": new_key,
        "manifest": manifest_note,
        "stale_includes": stale,
    });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// 解析模板根目录（写操作必须有明确的模板根）。
fn template_root(ctx: &Ctx) -> Result<PathBuf, CliError> {
    ctx.template_dir.clone().ok_or_else(|| {
        CliError::new(
            "args",
            "未配置模板目录，无法定位模板文件（请用 --template-dir 指定）",
        )
    })
}

/// 定位**可编辑**（磁盘）模板：返回 `(相对键, 绝对路径)`。
///
/// 内置 / 内存模板没有磁盘文件，拒绝编辑（提示先派生）。
fn locate_editable(ctx: &Ctx, name: &str) -> Result<(String, PathBuf), CliError> {
    let gen = ctx.build_registry()?;
    let entry = gen
        .registry()
        .get(name)
        .ok_or_else(|| CliError::new("template_not_found", format!("模板不存在: {name}")))?;
    match &entry.source {
        TemplateSource::File(p) => Ok((entry.name.clone(), p.clone())),
        _ => Err(CliError::new(
            "args",
            format!("模板 {name} 没有磁盘文件（内置/内存模板不可编辑；请先 `templates derive` 派生一份）"),
        )),
    }
}

/// 取得新源码：优先 `--from-file`，否则 `--editor` / `$EDITOR` / `$VISUAL`。
///
/// `$EDITOR` 模式在临时副本上编辑，退出后读回；与 `--from-file` 走**同一条**
/// 写路径（同一套校验与乐观锁）。
fn obtain_new_source(args: &TemplatesEditArgs, target: &Path) -> Result<String, CliError> {
    if let Some(f) = &args.from_file {
        return std::fs::read_to_string(f).map_err(|e| {
            CliError::new("io", format!("读取 --from-file 失败 {}: {e}", f.display()))
        });
    }

    let editor = args
        .editor
        .clone()
        .or_else(|| std::env::var("EDITOR").ok())
        .or_else(|| std::env::var("VISUAL").ok())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            CliError::new(
                "args",
                "需要 --from-file <文件> 或设置 --editor/$EDITOR 以交互编辑",
            )
        })?;

    let current = std::fs::read_to_string(target)
        .map_err(|e| CliError::new("io", format!("读取模板失败 {}: {e}", target.display())))?;
    let tmp = std::env::temp_dir().join(format!(
        "nctool-edit-{}-{}.j2",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&tmp, &current)
        .map_err(|e| CliError::new("io", format!("写入临时文件失败 {}: {e}", tmp.display())))?;

    // `$EDITOR` 可能带参数（如 `code --wait`）：按空白切分，首段为程序。
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    let status = std::process::Command::new(program)
        .args(parts)
        .arg(&tmp)
        .status()
        .map_err(|e| CliError::new("io", format!("启动编辑器 {program} 失败: {e}")))?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(CliError::new(
            "io",
            format!("编辑器退出码非 0（{status}），已放弃保存"),
        ));
    }
    let edited = std::fs::read_to_string(&tmp)
        .map_err(|e| CliError::new("io", format!("读取编辑结果失败: {e}")))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(edited)
}

/// 解析模板的**有效参数规格**（头部 `{# PARAMS: #}` + 变量库 + 清单覆盖层）。
///
/// 与注册表加载时的口径一致（[`ResolvedMeta::resolve`] 单一来源）。
fn resolve_specs(
    root: &Path,
    rel_key: &str,
    source: &str,
) -> Result<Vec<nctool_core::ParamSpec>, CliError> {
    let manifest = match TemplateManifest::load(root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("warning: {e}");
            TemplateManifest::empty()
        }
    };
    let library = match VariableLibrary::load(root) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("warning: {e}");
            VariableLibrary::empty()
        }
    };
    let meta = ResolvedMeta::resolve(Path::new(rel_key), source, manifest.get(rel_key), &library);
    Ok(meta.params)
}

/// 保存前的**分级校验**：L1 语法（总是）/ L2 规格自洽（总是）/ L3 参数值（可选）。
///
/// 任何一级出现 Error → 阻断（不落盘），返回 [`CliError`]（kind = `validation`）。
/// 只做到 L1/L2 时返回 [`ValidationTier::SyntaxAndSpec`]，由调用方在输出里
/// **明示**"未做参数值校验"——静默降级不可接受（P3）。
///
/// L3 用**正向集合**入口 [`check_param_values`]：只校验**已提供**参数的值，
/// **不查缺失**（"缺参数"是使用期问题，不是模板缺陷）。校验语义全部留在 core，
/// CLI 不自行判断"哪些 kind 算阻断"（与 D19 同一精神）。
fn run_tiered_validation(
    source: &str,
    name: &str,
    specs: &[nctool_core::ParamSpec],
    params: &ParamInputArgs,
) -> Result<ValidationTier, CliError> {
    // ---- L1 语法 + L2 规格自洽（总是；与 `new` 共用同一实现） ----
    run_l1_l2(source, name, specs)?;

    // ---- L3 参数值校验（仅当用户提供了参数；只查已提供值，不查缺失） ----
    let provided = !params.param.is_empty() || params.params_file.is_some();
    if !provided {
        return Ok(ValidationTier::SyntaxAndSpec);
    }
    let pset = crate::context::build_params(params.params_file.as_deref(), &params.param, specs)?;
    let report = check_param_values(specs, &pset);
    if report.has_errors() {
        return Err(CliError::new(
            "validation",
            format!("参数值校验失败（L3）：\n{}", report.summary()),
        ));
    }
    Ok(ValidationTier::Full)
}

/// **L1 语法 + L2 规格自洽**（两者都不依赖参数值）。任一 Error → 阻断。
///
/// `edit` / `derive` 的 L1/L2 与 `new` 的骨架校验**共用此函数**（单一来源，
/// 避免两处判定漂移）。`new` 不接受用户参数，故其校验到此为止。
fn run_l1_l2(source: &str, name: &str, specs: &[nctool_core::ParamSpec]) -> Result<(), CliError> {
    // ---- L1 语法 ----
    if let Err(err) = nctool_tpl::parse(source, name) {
        let loc = match &err {
            nctool_tpl::TplError::Parse { line, col, .. } => {
                format!("（第 {line} 行 第 {col} 列）")
            }
            _ => String::new(),
        };
        return Err(CliError::new(
            "validation",
            format!("语法校验失败（L1）{loc}：{err}"),
        ));
    }

    // ---- L2 规格自洽（不依赖参数值） ----
    let report = check_spec_consistency(specs);
    if report.has_errors() {
        return Err(CliError::new(
            "validation",
            format!("规格自洽校验失败（L2）：\n{}", report.summary()),
        ));
    }
    Ok(())
}

/// 收集全部已注册模板的 `(键, 源码)`，供 include 引用扫描。
fn collect_template_sources(ctx: &Ctx) -> Result<Vec<(String, String)>, CliError> {
    let gen = ctx.build_registry()?;
    Ok(gen
        .registry()
        .list(None)
        .iter()
        .map(|e| (e.name.clone(), e.source_text.clone()))
        .collect())
}

/// 把清单处理结果转成人类可读的一句话。
fn describe_manifest(m: &ManifestOutcome) -> String {
    match m {
        ManifestOutcome::Written(out) => format!("已写入 {}", out.path.display()),
        ManifestOutcome::NoEntry => "清单中无该条目，未改动".to_string(),
        ManifestOutcome::Degraded(reason) => format!("降级（{reason}）"),
    }
}

/// 把写内核错误映射为 CLI 错误（写冲突 → 6；只读 / IO → 3；越界 → 2）。
fn map_write_err(e: WriteError) -> CliError {
    match e {
        WriteError::Conflict { path, .. } => CliError::new(
            "write_conflict",
            format!(
                "写入冲突：{} 已被外部修改，未覆盖。\
                 可选：① 覆盖（以当前内容为基线重存）② 放弃 ③ 另存为新模板",
                path.display()
            ),
        ),
        WriteError::ReadOnly { path } => {
            CliError::new("io", format!("目标只读或无写入权限：{}", path.display()))
        }
        WriteError::PathEscape { rel, reason } => {
            CliError::new("args", format!("路径越界被拒绝：{rel}（{reason}）"))
        }
        // 目标不可用（如同名目录占位）≠ "数据损坏"：payload 已自述，不再加误导前缀。
        WriteError::Corrupt(m) => CliError::new("io", m),
        WriteError::Io(e) => CliError::new("io", format!("写入失败：{e}")),
        _ => CliError::new("io", format!("写入失败：{e}")),
    }
}

/// 派生 / 重命名的写错误映射：目标已存在 → `name_conflict`(6)；其余同 [`map_write_err`]。
fn map_create_err(e: WriteError, key: &str) -> CliError {
    match e {
        WriteError::Conflict { .. } => CliError::new(
            "name_conflict",
            format!("目标已存在，不覆盖：{key}（如需覆盖请先重命名或删除）"),
        ),
        other => map_write_err(other),
    }
}

#[cfg(test)]
mod tests {
    use super::validate_template_name;

    #[test]
    fn valid_names() {
        assert!(validate_template_name("my_op").is_ok());
        assert!(validate_template_name("my.op").is_ok());
        assert!(validate_template_name("钻_孔循环").is_ok());
    }

    #[test]
    fn path_traversal_rejected() {
        assert!(validate_template_name("../evil").is_err());
        assert!(validate_template_name("a/b").is_err());
        assert!(validate_template_name("..").is_err());
        assert!(validate_template_name(".").is_err());
        assert!(validate_template_name("").is_err());
        assert!(validate_template_name(r"..\evil").is_err());
        assert!(validate_template_name("sub\\evil").is_err());
    }
}
