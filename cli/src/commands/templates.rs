//! `templates` 子命令：列表 / 查看 / 新建。

use std::path::PathBuf;

use nctool_tpl::extract_undeclared;

use crate::cli::{
    CategoryArg, TemplatesArgs, TemplatesCommand, TemplatesListArgs, TemplatesNewArgs,
};
use crate::context::Ctx;
use crate::output::CliError;

/// `templates` 命令分发。
pub fn run(ctx: &Ctx, args: &TemplatesArgs) -> Result<(), CliError> {
    match &args.command {
        TemplatesCommand::List(a) => list(ctx, a),
        TemplatesCommand::Show(a) => show(ctx, &a.template),
        TemplatesCommand::New(a) => new(ctx, a),
    }
}

fn list(ctx: &Ctx, args: &TemplatesListArgs) -> Result<(), CliError> {
    let gen = ctx.build_registry()?;
    let entries = gen.registry().list(args.category.map(CategoryArg::to_core));

    // JSON 数据
    let data: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "category": CategoryArg::from_core(e.category),
                "description": e.description,
            })
        })
        .collect();

    // 文本输出
    let mut text = format!("模板列表（{} 个）\n", entries.len());
    for e in &entries {
        text.push_str(&format!(
            "{:<24} {:<6} {}\n",
            e.name,
            CategoryArg::from_core(e.category),
            e.description
        ));
    }
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// 解析模板源码与参数规格：`show` / `inspect` / `validate` 共用。
///
/// 返回 `(模板名, 源码, 参数规格)`；文件模板无规格 → `None`，
/// 注册表模板 → `Some(params)`。
pub fn resolve_source(
    ctx: &Ctx,
    name_or_path: &str,
) -> Result<(String, String, Option<Vec<nctool_core::ParamSpec>>), CliError> {
    // 1) 文件路径 → 读源码（名称用文件名）
    if let Some(path) = ctx.find_template_file(name_or_path) {
        let source = std::fs::read_to_string(&path)
            .map_err(|e| CliError::new("io", format!("读取模板失败 {}: {e}", path.display())))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| name_or_path.to_string());
        return Ok((name, source, None));
    }
    // 2) 已注册模板（内置 / 目录）
    let gen = ctx.build_registry()?;
    let entry = gen.registry().get(name_or_path).ok_or_else(|| {
        CliError::new("template_not_found", format!("模板不存在: {name_or_path}"))
    })?;
    Ok((
        entry.name.clone(),
        entry.source_text.clone(),
        Some(entry.params.clone()),
    ))
}

/// 提取模板变量（必选/可选 + 行列定位）。
///
/// 过滤系统注入变量（如 `machine`）——它们由管线注入上下文，不算外部必选参数。
pub fn extract_variables(source: &str, name: &str) -> Result<Vec<nctool_tpl::Variable>, CliError> {
    let ast = nctool_tpl::parse(source, name)?;
    let vars = extract_undeclared(&ast);
    Ok(vars
        .into_iter()
        .filter(|v| !super::SYSTEM_VARS.contains(&v.name.as_str()))
        .collect())
}

fn show(ctx: &Ctx, name_or_path: &str) -> Result<(), CliError> {
    let (name, source, _) = resolve_source(ctx, name_or_path)?;
    let vars = extract_variables(&source, &name)?;

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
         O{{{{ prog | nc_pad(4) }}}}\n\
         G{{{{ machine.coordinate_system }}}}\n\
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

    let path = dir.join(format!("{}.j2", args.name));
    if path.exists() {
        return Err(CliError::new(
            "io",
            format!("模板已存在，不覆盖: {}", path.display()),
        ));
    }
    let source = scaffold_source(&args.name, CategoryArg::from_core(args.category.to_core()));
    std::fs::write(&path, source)
        .map_err(|e| CliError::new("io", format!("写入模板失败 {}: {e}", path.display())))?;

    let text = format!("已创建模板: {}\n", path.display());
    let data = serde_json::json!({ "path": path.display().to_string(), "name": args.name });
    ctx.style.print_ok(&text, data);
    Ok(())
}

/// 校验模板名：必须是单个合法文件名组件，禁止路径分隔符与 `..`，
/// 防止 `templates new` 逃出模板目录（路径穿越）。
fn validate_template_name(name: &str) -> Result<(), CliError> {
    if name.is_empty() {
        return Err(CliError::new("args", "模板名不能为空"));
    }
    if name == "." || name == ".." {
        return Err(CliError::new("args", format!("非法模板名: {name}")));
    }
    let components = std::path::Path::new(name).components().count();
    if components != 1 {
        return Err(CliError::new(
            "args",
            format!("模板名不能包含路径分隔符: {name}"),
        ));
    }
    Ok(())
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
