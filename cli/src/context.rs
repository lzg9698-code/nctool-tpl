//! 命令执行上下文：解析全局选项、构建模板注册表、解析机床配置。

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use nctool_core::machine::MachinePreset;
use nctool_core::manifest::{path_to_rel_key, ResolvedMeta, TemplateManifest, MANIFEST_FILE};
use nctool_core::pipeline::GCodeGenerator;
use nctool_core::registry::{TemplateEntry, TemplateSource};
use nctool_core::variables::VariableLibrary;
use nctool_core::{MachineConfig, ParameterSet};

use crate::args;
use crate::cli::GlobalArgs;
use crate::config;
use crate::output::{CliError, OutputStyle};

/// 注册表缓存键：模板目录 + 该目录树的最新 mtime。
///
/// `root = None` 表示"未配置模板目录"（仅内置模板，与磁盘无关）。
/// 把 `root` 一并入键是为了防住 `template_dir` 被改写后误命中旧缓存。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistryKey {
    root: Option<PathBuf>,
    stamp: SystemTime,
}

/// 命令执行上下文（由全局选项 + 配置文件解析而来）。
#[derive(Debug, Clone)]
pub struct Ctx {
    /// 结果输出风格
    pub style: OutputStyle,
    /// 详细输出
    pub verbose: bool,
    /// 模板目录（CLI --template-dir 优先，否则配置文件）
    pub template_dir: Option<PathBuf>,
    /// 默认机床（CLI --machine 优先，否则配置文件）
    pub default_machine: Option<String>,
    /// 一次性加载的层叠配置（含来源路径，供各命令复用，避免重复读盘）
    pub loaded: config::LoadedConfig,
    /// 模板注册表缓存（键 + 共享注册表），见 [`Ctx::build_registry`]。
    ///
    /// `RefCell` 而非 `OnceCell`：键会随目录指纹变化而更新。
    /// 单线程使用（CLI 一次性执行、Web 服务顺序处理请求），无需加锁。
    ///
    /// `pub(crate)`：本 crate 的测试会以结构体字面量构造 `Ctx`（需要精确指定
    /// `template_dir` 等字段），私有字段会让这些构造点全部编译失败。
    /// 对外仍不可见——它不是 API 的一部分。
    pub(crate) registry_cache: RefCell<Option<(RegistryKey, Rc<GCodeGenerator>)>>,
}

impl Ctx {
    /// 从全局选项解析上下文（读配置层叠）。
    pub fn from_global(g: &GlobalArgs) -> Result<Self, CliError> {
        let loaded = config::load()?;
        // 损坏的 TOML 已在配置层降级为空配置；主动提示用户，但不阻断
        // 当前命令。这样只读命令仍可使用内置模板/机床，用户也不会误以为
        // 配置已生效。
        for warning in &loaded.warnings {
            eprintln!("warning: {warning}");
        }
        Ok(Ctx {
            style: OutputStyle::from(&g.format),
            verbose: g.verbose,
            template_dir: g
                .template_dir
                .clone()
                .or_else(|| loaded.merged.template_dir.clone()),
            default_machine: g
                .machine
                .clone()
                .or_else(|| loaded.merged.default_machine.clone()),
            loaded,
            registry_cache: RefCell::new(None),
        })
    }

    /// 构建（或复用）模板注册表：内置模板 + 模板目录中的 `*.j2` 文件（**递归**）。
    ///
    /// 目录模板以**相对模板目录的路径**作为模板名（如 `turning/undercut.j2`），
    /// 使用 `/` 作分隔符。相对名唯一（同名文件在不同子目录下互不冲突），
    /// 且不与内置模板的扁平名冲突。
    ///
    /// 元数据按「清单 > 模板头部注释 > 文件名」三级回退解析（见
    /// [`nctool_core::manifest::ResolvedMeta`]）：分类、描述、可见性、
    /// 输出文件名/后缀均可在 `templates.yaml` 中声明，未声明时按目录名推断分类。
    ///
    /// 隐藏文件与目录（`.` 开头）、清单文件自身、以及符号链接逃逸路径一律跳过。
    ///
    /// # 缓存
    /// 构建一次要遍历目录、读取并解析**全部**模板源码，成本与模板数成正比；
    /// 而 Web UI 的每个请求都要用它。故按「模板目录 + 目录树最新 mtime」缓存：
    /// 指纹未变则复用同一份注册表。
    ///
    /// **指纹不可省**：若无条件长期缓存，用户改完模板仍会拿到旧注册表，
    /// 渲染出与图纸不符的 G-code —— 属于本项目零容忍的"静默产出错误程序"。
    ///
    /// 需要**可变**注册表（注册临时文件模板等）时请改用
    /// [`Self::build_registry_fresh`]：缓存中的注册表由所有调用方共享，
    /// 就地改动会污染后续调用。
    pub fn build_registry(&self) -> Result<Rc<GCodeGenerator>, CliError> {
        // 无模板目录 = 仅内置模板，与磁盘无关，恒定不变
        let Some(dir) = &self.template_dir else {
            let key = RegistryKey {
                root: None,
                stamp: SystemTime::UNIX_EPOCH,
            };
            if let Some((cached_key, cached)) = self.registry_cache.borrow().as_ref() {
                if *cached_key == key {
                    return Ok(Rc::clone(cached));
                }
            }
            let gen = Rc::new(GCodeGenerator::new());
            *self.registry_cache.borrow_mut() = Some((key, Rc::clone(&gen)));
            return Ok(gen);
        };

        let root = canonicalize_dir(dir)?;
        // 指纹取不到（IO 异常）时**放弃缓存**：宁可每次重算，也不拿陈旧注册表
        let Some(stamp) = tree_stamp(&root) else {
            return Ok(Rc::new(self.load_registry(&root)?));
        };
        let key = RegistryKey {
            root: Some(root.clone()),
            stamp,
        };
        if let Some((cached_key, cached)) = self.registry_cache.borrow().as_ref() {
            if *cached_key == key {
                return Ok(Rc::clone(cached));
            }
        }
        let gen = Rc::new(self.load_registry(&root)?);
        *self.registry_cache.borrow_mut() = Some((key, Rc::clone(&gen)));
        Ok(gen)
    }

    /// 每次重建注册表（不读缓存）。
    ///
    /// 供需要 `&mut GCodeGenerator` 的调用方使用 —— 典型是 `render` 的
    /// 「把临时文件模板注册进来」路径。走缓存会让该临时模板泄漏进共享注册表，
    /// 使后续调用看到一个本不该存在的模板名。
    pub fn build_registry_fresh(&self) -> Result<GCodeGenerator, CliError> {
        match &self.template_dir {
            None => Ok(GCodeGenerator::new()),
            Some(dir) => {
                let root = canonicalize_dir(dir)?;
                self.load_registry(&root)
            }
        }
    }

    /// 遍历 `root` 并装载全部模板（不做缓存，`root` 须已规范化）。
    fn load_registry(&self, root: &Path) -> Result<GCodeGenerator, CliError> {
        let mut gen = GCodeGenerator::new();

        // 清单加载失败不阻断：清单是可选的，损坏时降级为「无清单」并告警，
        // 这样模板仍可用，用户也能看到问题所在。
        let manifest = match TemplateManifest::load(root) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("warning: {e}");
                TemplateManifest::empty()
            }
        };
        // 变量库同理：可选文件，损坏时降级为空库（参数规格退回头部声明）
        let library = match VariableLibrary::load(root) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("warning: {e}");
                VariableLibrary::empty()
            }
        };

        // 收集待注册模板：先完成整目录遍历再注册，避免遍历中途发现重名时报错
        // 而留下半成品注册表。
        let mut found: Vec<(String, std::path::PathBuf)> = Vec::new();
        collect_templates(root, root, &mut found, &mut BTreeSet::new())?;
        // 按路径排序，保证列表输出稳定（BTreeMap 只保证注册顺序后的键序，
        // 而注册顺序取决于文件系统返回顺序）
        found.sort_by(|a, b| a.0.cmp(&b.0));

        // 清单孤儿条目（P1-12）：清单里写了、但没有任何模板文件对得上的键。
        // 它们的 `params` / `visible` / `machine` / `output_extension` 全部静默失效 ——
        // 看起来约束齐全，实际一条都没上（`undercut.j2` vs `undercut_fs.j2` 这类
        // 笔误尤其容易发生）。提示而不阻断：清单是可选文件，为它挡住整个注册表
        // 得不偿失。
        let present: BTreeSet<String> = found.iter().map(|(k, _)| k.clone()).collect();
        for key in manifest.orphan_keys(&present) {
            eprintln!(
                "warning: 清单条目 {key} 未匹配到任何模板文件\
                 （其 params / visible / machine / output_extension 均不会生效，请核对拼写）"
            );
        }

        for (rel_key, canonical) in found {
            let source_text = std::fs::read_to_string(&canonical).map_err(|e| {
                CliError::new("io", format!("读取模板失败 {}: {e}", canonical.display()))
            })?;
            let rel_path = std::path::Path::new(&rel_key);
            let meta =
                ResolvedMeta::resolve(rel_path, &source_text, manifest.get(&rel_key), &library);

            // 头部 `{# PARAMS: #}` 里无法解析的行：提示但不阻断加载——
            // 静默跳过等于静默少一条参数约束（类型/白名单就不再校验了）。
            for warning in &meta.warnings {
                eprintln!("warning: {rel_key}: {warning}");
            }

            // 描述为空时补上来源，避免列表里出现空白描述。
            //
            // 用**相对键**而不是绝对磁盘路径：这个字符串会经 `/api/templates`
            // 原样返回给浏览器，绝对路径等于把用户名与项目目录结构一并送出去
            // （P1-17）。相对键也正是用户要传给 CLI 的那个名字，比路径更有用。
            let description = if meta.description.is_empty() {
                format!("文件模板: {rel_key}")
            } else {
                meta.description.clone()
            };

            let entry = TemplateEntry::new(
                rel_key.clone(),
                meta.category,
                description,
                TemplateSource::File(canonical),
                // 参数规格：头部 `{# PARAMS: #}` + 清单 `params` 覆盖层。
                // 此前恒为空切片，导致文件模板的类型/区间/白名单约束全部失效，
                // `validate` 对 `Z_START=abc` 这类错误直接放行。
                meta.params.clone(),
                source_text,
            )
            .with_visible(meta.visible)
            .with_output(meta.output_filename.clone(), meta.output_extension.clone())
            .with_machine(meta.machine.clone())
            .with_status(meta.status);

            // 与内置模板重名不算错误：内置名是扁平的单段名（如 `facing`），
            // 而目录模板名至少含一个扩展名点或路径分隔符，正常不会冲突；
            // 真冲突时上游 add_entry 会返回 Duplicate，这里转换为用户可读错误。
            gen.registry_mut()
                .add_entry(entry)
                .map_err(|e| CliError::new("template_register", format!("注册模板失败: {e}")))?;
        }
        Ok(gen)
    }

    /// 测试用最小上下文（无模板目录、无默认机床、文本输出）。
    ///
    /// 集中一处列出全部字段：新增 `Ctx` 字段时只需改这里，
    /// 而不必逐个修补散落在各模块测试里的结构体字面量。
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            style: OutputStyle::Text,
            verbose: false,
            template_dir: None,
            default_machine: None,
            loaded: Default::default(),
            registry_cache: RefCell::new(None),
        }
    }

    /// 解析机床配置：`--machine` / 配置默认值 / 内置 generic。
    ///
    /// 内置预设优先；否则查找配置文件中的自定义机床；都找不到则报错。
    pub fn resolve_machine(&self, explicit: Option<&str>) -> Result<MachineConfig, CliError> {
        let id = match explicit {
            Some(id) => id.to_string(),
            None => self
                .default_machine
                .clone()
                .unwrap_or_else(|| "generic".to_string()),
        };
        if let Some(preset) = MachinePreset::from_id(&id) {
            return Ok(preset.config());
        }
        // 自定义机床：复用启动时缓存的一次性配置加载（不再重复读盘）
        if let Some(m) = self.loaded.merged.machine.get(&id) {
            return Ok(m.clone());
        }
        Err(CliError::new(
            "machine_not_found",
            format!("未知机床 '{id}'（内置: generic/wfl_m65/index_ms40，或配置自定义机床）"),
        ))
    }

    /// 模板目录中是否存在指定文件模板（仅供 CLI 命令按路径定位）。
    ///
    /// HTTP 服务不得调用此方法；它只允许访问注册表中的逻辑模板名。
    pub fn find_template_file(&self, name_or_path: &str) -> Option<PathBuf> {
        let p = PathBuf::from(name_or_path);
        if p.is_file() {
            return Some(p);
        }
        if let Some(dir) = &self.template_dir {
            let candidate = dir.join(name_or_path);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }
}

/// 校验并规范化模板目录：不存在或无法解析时报 IO 错误。
fn canonicalize_dir(dir: &Path) -> Result<PathBuf, CliError> {
    if !dir.exists() {
        return Err(CliError::new(
            "io",
            format!("模板目录不存在: {}", dir.display()),
        ));
    }
    std::fs::canonicalize(dir)
        .map_err(|e| CliError::new("io", format!("解析模板目录失败 {}: {e}", dir.display())))
}

/// 计算目录树的"最新修改时间"指纹：任一文件/子目录被增删改都会改变它。
///
/// 只读 `metadata`（不读文件内容），成本远低于"读取并解析全部模板"。
/// 目录自身的 mtime 也要计入——新增/删除文件只改父目录 mtime。
///
/// 任一环节 IO 失败返回 `None`，调用方据此**放弃缓存**（宁可重算，
/// 也不拿陈旧注册表去渲染 G-code）。
fn tree_stamp(root: &Path) -> Option<SystemTime> {
    let mut newest = std::fs::metadata(root).ok()?.modified().ok()?;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()? {
            let entry = entry.ok()?;
            // DirEntry::metadata 不跟随符号链接：与 collect_templates 的
            // "符号链接逃逸一律跳过"口径一致，链接目标的变化不影响指纹
            let meta = entry.metadata().ok()?;
            if let Ok(modified) = meta.modified() {
                if modified > newest {
                    newest = modified;
                }
            }
            if meta.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Some(newest)
}

/// 递归收集模板目录下的 `*.j2` 文件。
///
/// 跳过规则（安全相关，逐条都有理由）：
///
/// - **隐藏项**（`.` 开头，含目录）：避免扫到 `.git`、`.venv` 等无关目录
/// - **清单文件**（`templates.yaml`）：它是元数据而非模板，且无 `.j2` 后缀，
///   实际不会命中；显式跳过是为了语义清晰
/// - **符号链接逃逸**：`canonicalize` 后真实路径必须仍在 `root` 之内，
///   否则跳过。这是防路径遍历的关键一步——目录链接可以指向模板根之外
/// - **非 `*.j2` 文件**：模板扩展名约定
///
/// 输出 `(相对路径键, 规范化绝对路径)`；相对路径键统一用 `/` 分隔。
///
/// `visited` 记录**已进入的规范化目录**，用于断开目录环。**目录同样要过逃逸校验
/// 与环检测，缺一不可**：`Path::is_dir()` 跟随符号链接，而逃逸校验此前只在文件
/// 分支执行 —— 目录链接既不会被跳过，也没有任何东西阻止重入。只校验逃逸挡不住
/// 指向根**内部**的环（`ta/loop -> ta`），故还需 `visited`。
///
/// # 环的实测行为（2026-09-18，更正审查报告的推断）
///
/// 报告记为「栈溢出 → 进程 abort」。实测**不成立**：路径每层增长一段，`is_dir()`
/// 最终会在平台路径长度上限处失败并返回 `false`，该层被当成普通文件跳过，递归
/// 因此有界（Windows 上用 junction 造环实测：走 66 层后正常结束，无崩溃、无报错）。
/// 真实代价是每次构建注册表都白走这几十层（Linux 上限更高，量级更大），
/// 而 Web UI 下**每个请求**都会重走一遍。
///
/// 之所以仍然要显式断环，而不是依赖这个「路径长度兜底」：那是平台的偶然属性，
/// 不是设计。Windows 一旦启用长路径、或在路径前加 `\\?\` 前缀，兜底即失效，
/// 递归变成真正无界 —— 到那时才是报告描述的栈溢出。显式 `visited` 与平台无关。
fn collect_templates(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf)>,
    visited: &mut BTreeSet<PathBuf>,
) -> Result<(), CliError> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| CliError::new("io", format!("读取目录失败 {}: {e}", dir.display())))?;
    for entry in entries {
        let entry = entry
            .map_err(|e| CliError::new("io", format!("读取目录项失败 {}: {e}", dir.display())))?;
        let path = entry.path();

        // 隐藏项（文件或目录）一律跳过
        let is_hidden = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'));
        if is_hidden {
            continue;
        }

        if path.is_dir() {
            // 目录也先 canonicalize 再决定是否进入：逃逸校验原本只覆盖文件分支
            let real = match std::fs::canonicalize(&path) {
                Ok(p) if p.starts_with(root) && p.is_dir() => p,
                _ => continue,
            };
            // 已进过的目录不再进 —— 符号链接成环时这是唯一的终止条件
            if !visited.insert(real.clone()) {
                continue;
            }
            collect_templates(root, &real, out, visited)?;
            continue;
        }

        // 只认 .j2 扩展名（大小写不敏感）
        let is_j2 = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("j2"));
        if !is_j2 {
            continue;
        }
        // 清单文件不是模板（防御性检查，正常已被扩展名过滤）
        if path.file_name().is_some_and(|n| n == MANIFEST_FILE) {
            continue;
        }

        // 跟随符号链接前确认真实目标仍在模板根目录内，防止逃逸到目录外。
        let canonical = match std::fs::canonicalize(&path) {
            Ok(p) if p.starts_with(root) && p.is_file() => p,
            _ => continue,
        };
        let rel = match canonical.strip_prefix(root) {
            Ok(r) => r,
            // 理论不可达（上一步已保证 starts_with），保守跳过而非 panic
            Err(_) => continue,
        };
        out.push((path_to_rel_key(rel), canonical));
    }
    Ok(())
}

/// 构造参数集：`--params-file` + `--param`（显式参数优先）。
///
/// 渲染上下文的构建（参数裸值 + `machine` 注入）与宽松渲染均已收编到
/// `nctool-core` 管线（`GCodeGenerator::generate` / `generate_lenient`），
/// CLI 不再持有副本，保证两条路径输出逐字节一致。
pub fn build_params(
    params_file: Option<&std::path::Path>,
    params: &[String],
    specs: &[nctool_core::ParamSpec],
) -> Result<ParameterSet, CliError> {
    args::build_parameter_set(params_file, params, specs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个只含单个模板的临时目录，返回 (目录, 模板文件路径)。
    fn temp_template_dir(tag: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("nctool_ctx_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("turning")).expect("建临时目录");
        let tpl = dir.join("turning").join("a.j2");
        std::fs::write(&tpl, "G0 X{{ x }}\n").expect("写模板");
        (dir, tpl)
    }

    /// 把**文件**的 mtime 推后若干秒。
    ///
    /// 指纹只到 mtime，而部分文件系统的时间戳粒度较粗——两次写入若落在同一
    /// 刻度内，指纹不变、缓存不失效，测试会随机失败。显式推后即消除该不确定性。
    ///
    /// 只对文件有效：Windows 下以写方式打开**目录**必然返回
    /// `PermissionDenied`（os error 5），不要传目录进来。
    fn bump_mtime(path: &Path, secs: u64) {
        assert!(path.is_file(), "bump_mtime 只接受文件: {}", path.display());
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("打开文件");
        f.set_modified(SystemTime::now() + std::time::Duration::from_secs(secs))
            .expect("设置 mtime");
    }

    /// 回归（P1-2）：目录符号链接**成环**时必须断开，不能无限重入。
    /// `Path::is_dir()` 跟随符号链接，而逃逸校验原先只在文件分支执行，目录层
    /// 既无校验也无 visited 集合。
    ///
    /// 环指向根**内部**（`turning/a/loop -> turning/a`）：逃逸校验拦不住它
    /// （目标确实在 root 之内），只有 visited 集合能终止递归。
    ///
    /// 注：在路径长度受限的平台上，`is_dir()` 会先在上限处失败而「碰巧」终止
    /// （见 `collect_templates` 的实测注记），所以本用例断言的是**结果正确**
    /// 而非「原本会崩」——它锁住的是「环不产生重复收集、也不吞掉正常模板」。
    ///
    /// 仅 unix 跑：Windows 建目录符号链接需要管理员或开发者模式，放进三平台 CI
    /// 会变成「看环境的偶发红」。Windows 的等价情形已用 junction 手工验证过。
    #[cfg(unix)]
    #[test]
    fn symlink_cycle_is_broken_and_templates_still_collected() {
        let (dir, _tpl) = temp_template_dir("symlink_cycle");
        let a = dir.join("turning").join("a");
        std::fs::create_dir_all(&a).expect("建子目录");
        std::os::unix::fs::symlink(&a, a.join("loop")).expect("建目录符号链接");

        let root = std::fs::canonicalize(&dir).expect("规范化临时目录");
        let mut out = Vec::new();
        // 能返回即说明环被断开；溢出会直接 abort，连断言都到不了
        collect_templates(&root, &root, &mut out, &mut BTreeSet::new()).expect("遍历应正常返回");
        assert!(
            out.iter().any(|(k, _)| k == "turning/a.j2"),
            "环之外的模板仍应被收集到: {out:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 目录符号链接**逃逸到根之外**时必须跳过。
    ///
    /// 此前目录层完全不校验，逃逸目录会被照常递归 —— 修 P1-2 时给目录补上了
    /// 与文件分支同一套 `canonicalize` + `starts_with(root)`，此用例把新口径钉住。
    #[cfg(unix)]
    #[test]
    fn symlink_escaping_root_is_skipped() {
        let outside = std::env::temp_dir().join(format!("nctool_outside_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).expect("建根外目录");
        let leaked = outside.join("leaked.j2");
        std::fs::write(&leaked, "; 不应被收集\n").expect("写根外模板");

        let (dir, _tpl) = temp_template_dir("symlink_escape");
        std::os::unix::fs::symlink(&outside, dir.join("turning").join("out"))
            .expect("建目录符号链接");

        let root = std::fs::canonicalize(&dir).expect("规范化临时目录");
        let mut out = Vec::new();
        collect_templates(&root, &root, &mut out, &mut BTreeSet::new()).expect("遍历应正常返回");
        assert!(
            !out.iter().any(|(k, _)| k.contains("leaked")),
            "逃逸到根之外的模板不得被收集: {out:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn ctx_default_machine_is_generic() {
        let ctx = Ctx::for_test();
        let m = ctx.resolve_machine(None).unwrap();
        assert_eq!(m.id, "generic");
    }

    #[test]
    fn resolve_builtin_preset() {
        let ctx = Ctx::for_test();
        let wfl = ctx.resolve_machine(Some("wfl_m65")).unwrap();
        assert_eq!(wfl.vendor, "WFL");
        let idx = ctx.resolve_machine(Some("index_ms40")).unwrap();
        assert_eq!(idx.vendor, "INDEX");
    }

    #[test]
    fn unknown_machine_errors() {
        let ctx = Ctx::for_test();
        assert!(ctx.resolve_machine(Some("no_such")).is_err());
    }

    #[test]
    fn registry_is_reused_while_directory_is_unchanged() {
        let (dir, _tpl) = temp_template_dir("cache_hit");
        let mut ctx = Ctx::for_test();
        ctx.template_dir = Some(dir.clone());

        let first = ctx.build_registry().expect("首次构建");
        let second = ctx.build_registry().expect("二次构建");
        assert!(
            Rc::ptr_eq(&first, &second),
            "目录树未变时必须复用同一份注册表（否则每请求都在重读全部模板）"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_is_rebuilt_after_template_edit() {
        let (dir, tpl) = temp_template_dir("cache_invalidate");
        let mut ctx = Ctx::for_test();
        ctx.template_dir = Some(dir.clone());

        let first = ctx.build_registry().expect("首次构建");
        assert!(first.registry().get("turning/a.j2").is_some());

        // 改内容 + 推后 mtime：必须重建，且新内容要真正生效——
        // 用旧注册表会渲染出与图纸不符的 G-code，是本项目零容忍的失败模式
        std::fs::write(&tpl, "G0 Z{{ z }}\n").expect("改写模板");
        bump_mtime(&tpl, 2);

        let second = ctx.build_registry().expect("改后重建");
        assert!(!Rc::ptr_eq(&first, &second), "目录树变化后必须重建注册表");
        let entry = second.registry().get("turning/a.j2").expect("模板仍在");
        assert!(
            entry.source_text.contains("Z{{ z }}"),
            "重建后必须读到新内容，实际为: {}",
            entry.source_text
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_is_rebuilt_when_template_added() {
        let (dir, _tpl) = temp_template_dir("cache_added");
        let mut ctx = Ctx::for_test();
        ctx.template_dir = Some(dir.clone());

        let first = ctx.build_registry().expect("首次构建");
        let added = dir.join("turning").join("b.j2");
        std::fs::write(&added, "G0 Y{{ y }}\n").expect("新增模板");
        // 推后新文件的 mtime 而非父目录：目录无法以写方式打开（Windows 下必然
        // 拒绝访问），而指纹取全树最大值，新文件足够新即可让指纹变化
        bump_mtime(&added, 2);

        let second = ctx.build_registry().expect("新增后重建");
        assert!(!Rc::ptr_eq(&first, &second), "新增模板必须使缓存失效");
        assert!(
            second.registry().get("turning/b.j2").is_some(),
            "新增模板必须出现在重建后的注册表中"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fresh_registry_does_not_pollute_shared_cache() {
        let (dir, _tpl) = temp_template_dir("fresh_isolation");
        let mut ctx = Ctx::for_test();
        ctx.template_dir = Some(dir.clone());

        let cached = ctx.build_registry().expect("缓存构建");

        // 模拟 render 的「就地注册临时文件模板」：必须落在独立副本上，
        // 否则该模板名会泄漏进共享缓存，后续调用会看到一个本不存在的模板
        let mut fresh = ctx.build_registry_fresh().expect("独立构建");
        fresh
            .registry_mut()
            .add_memory(
                "tmp_ad_hoc",
                nctool_core::registry::TemplateCategory::General,
                "临时模板",
                "G0 X0\n",
                vec![],
            )
            .expect("注册临时模板");

        let cached_again = ctx.build_registry().expect("再次取缓存");
        assert!(
            Rc::ptr_eq(&cached, &cached_again),
            "缓存不应被 fresh 构建替换"
        );
        assert!(
            cached_again.registry().get("tmp_ad_hoc").is_none(),
            "临时模板不得泄漏进共享缓存"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
