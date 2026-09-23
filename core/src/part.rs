//! 零件级批量生成：把「一个零件 = 多道工序」的描述展开为一段完整程序。
//!
//! # 为什么需要这一层
//!
//! `render` 一次只处理一个模板。真实零件（如法兰盘 = 端面 + 4 孔 + 切断）
//! 要拼多个模板的输出，此前只能靠 shell 脚本逐段调用再 `cat` 拼接，于是
//! `docs/REAL_PART_WALKTHROUGH.md` §5 的 E5 走查暴露了三处局限，本模块逐条解决：
//!
//! | 局限 | 症状 | 本模块的对策 |
//! |---|---|---|
//! | 5.1 行号跨工序不续编 | 每段 `N0010` 重来，拼起来是 `N0010 N0020 N0010 N0020` | [`PartSpec::generate`] 把上一段的末行号传给下一段（[`crate::GenerationOptions::line_number_start`]） |
//! | 5.2 错误不聚合 | 某段失败时，前面几段已写出的 G-code 留在文件里 | [`PartError::OperationsFailed`]：**任一工序失败即整体不交付**，调用方拿不到半成品 |
//! | 5.3 参数无继承 | `part_name` 在每段都得重传一遍 | [`PartSpec::params`] 顶层参数对所有工序可见，工序级同名参数覆盖之 |
//!
//! # 契约
//!
//! JSON 形状（与前端 `ui/src/31_script_api.part.html` 的 `doPart` mock 对齐，
//! 字段名用 `ops` 而非 `operations` —— 前端已按 `ops` 写死，改名会让两边不一致）：
//!
//! ```json
//! {
//!   "name": "FLANGE_DEMO",
//!   "default_machine": "wfl_m65",
//!   "params": { "part_name": "FLANGE_DEMO" },
//!   "ops": [
//!     { "template": "program_header", "params": { "prog": 1001 } },
//!     { "template": "drill_cycle", "params": { "x": 20, "y": 0 }, "machine": "generic" }
//!   ]
//! }
//! ```
//!
//! **注意 `params` 是「参数名 → 参数值」的扁平表**（[`ParameterSet`]），
//! 不是嵌套对象 —— 与 `nctool --params-file` 同一格式，刻意复用以免多一套 schema。

use serde::{Deserialize, Serialize};

use crate::machine::MachinePreset;
use crate::model::{MachineConfig, ParameterSet};
use crate::pipeline::{GCodeGenerator, GenerationOptions, PipelineError};

/// 零件定义：一个零件由若干**按顺序执行**的工序组成。
///
/// 字段名与前端契约一致（见模块文档）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartSpec {
    /// 零件名（仅用于呈现与错误文案，不参与渲染上下文）
    #[serde(default)]
    pub name: Option<String>,
    /// 程序级默认机床标识（如 `generic` / `wfl_m65`）。
    ///
    /// 工序级 `machine` 覆盖它；两者都没有时由调用方决定兜底（CLI 用配置里的
    /// 默认机床，HTTP 用 `generic`）。
    #[serde(default)]
    pub default_machine: Option<String>,
    /// 程序级参数：**对所有工序可见**，工序级同名参数覆盖之。
    ///
    /// 这是 E5 局限 5.3 的对策 —— `part_name` 之类"整份程序共用一个值"的参数
    /// 只需在这里写一次。
    #[serde(default)]
    pub params: ParameterSet,
    /// 工序列表（按数组顺序执行）。
    #[serde(default)]
    pub ops: Vec<PartOp>,
}

/// 单道工序：一个模板 + 它的参数（+ 可选机床与输出选项覆盖）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartOp {
    /// 模板名（内置模板名 / 目录模板名 / 文件路径，与 `render` 的解析规则一致）
    pub template: String,
    /// 工序级参数，覆盖 [`PartSpec::params`] 的同名项
    #[serde(default)]
    pub params: ParameterSet,
    /// 工序级机床标识，覆盖 [`PartSpec::default_machine`]
    #[serde(default)]
    pub machine: Option<String>,
    /// 工序级输出选项覆盖（如 `line_numbers`）。
    ///
    /// 只覆盖显式给出的字段，其余继承 [`PartOptions`]。
    #[serde(default)]
    pub options: Option<PartOpOptions>,
}

/// 工序级输出选项覆盖。字段与 [`PartOptions`] 同名同义，`None` 表示"不覆盖"。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PartOpOptions {
    /// 覆盖行号开关
    #[serde(default)]
    pub line_numbers: Option<bool>,
    /// 覆盖是否输出头部注释
    #[serde(default)]
    pub add_header_comment: Option<bool>,
    /// 覆盖是否删除空行
    #[serde(default)]
    pub strip_blank_lines: Option<bool>,
    /// 覆盖是否仅输出 ASCII
    #[serde(default)]
    pub ascii_only: Option<bool>,
}

/// 程序级输出选项（对全部工序生效，工序级可覆盖单个字段）。
///
/// `Default` 即"全关"：不开行号、不加头注释、不删空行、不转 ASCII、不宽松 ——
/// 与 `GenerationOptions` 的默认语义一致，单独渲染一段程序时行为不变。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartOptions {
    /// 是否生成行号。开启时**跨工序续编**（局限 5.1 的对策）。
    pub line_numbers: bool,
    /// 是否输出头部注释
    pub add_header_comment: bool,
    /// 是否删除空行
    pub strip_blank_lines: bool,
    /// 是否仅输出 ASCII
    pub ascii_only: bool,
    /// 校验失败时是否仍继续（宽松模式）
    pub lenient: bool,
}

/// 单道工序的生成结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpOutcome {
    /// 工序在 `ops` 数组中的下标（从 0 起）
    pub index: usize,
    /// 模板名（回显，便于调用方定位）
    pub template: String,
    /// 该工序产出的 G-code（**已含后处理**）
    pub output: String,
    /// 该工序的末行号（续编游标，未开行号时等于起始值）
    pub end_line_number: u32,
}

/// 批量生成结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartOutcome {
    /// 逐工序结果，顺序与 `ops` 一致
    pub ops: Vec<OpOutcome>,
    /// 拼接后的完整程序（各工序按顺序首尾相接）
    pub program: String,
}

impl PartOutcome {
    /// 拼接后程序的非空行数。
    pub fn line_count(&self) -> usize {
        self.program
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count()
    }
}

/// 批量生成失败。
///
/// **聚合语义**（局限 5.2 的对策）：`generate` 走完**所有**工序再决定成败 ——
/// 不是遇到第一个错就返回。这样调用方能一次性看到"哪几道工序有问题"，
/// 而不是修一个跑一次又冒出一个。
#[derive(Debug)]
pub enum PartError {
    /// 一道或多道工序失败。**整体不交付**，`outcome` 为 `None`。
    ///
    /// 字段是 `Vec`（而非单个错误）正体现聚合：一次报全。
    OperationsFailed {
        /// 失败工序的 `(下标, 模板名, 错误描述)` 列表
        failures: Vec<OpFailure>,
    },
    /// 零件定义本身不合法（如 `ops` 为空）
    InvalidSpec(String),
}

/// 单道工序的失败描述。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpFailure {
    /// 工序下标（从 0 起）
    pub index: usize,
    /// 工序名（优先用模板名；模板名为空时用 `工序N`）
    pub name: String,
    /// 面向用户的错误描述（单行，可直接拼进提示）
    pub error: String,
}

impl std::fmt::Display for PartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartError::InvalidSpec(msg) => write!(f, "零件定义不合法：{msg}"),
            PartError::OperationsFailed { failures } => {
                write!(f, "{} 道工序失败：", failures.len())?;
                for (i, fail) in failures.iter().enumerate() {
                    if i > 0 {
                        write!(f, "；")?;
                    }
                    write!(f, "工序{}（{}）：{}", fail.index + 1, fail.name, fail.error)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for PartError {}

/// 解析机床标识为 [`MachineConfig`]。
///
/// `None` / 内建标识 / 未知标识三种情形：
/// - `None` → 用 `fallback_id`（调用方的默认机床）；`fallback_id` 也解析不了则 generic
/// - 内建标识 → 对应预设
/// - 未知标识 → **报错而不是静默用 generic**：写错机床名的后果是整份程序按错误的
///   编程约定生成（程序号格式、行号前缀都不同），静默降级等于产出错误程序。
pub fn resolve_machine(
    id: Option<&str>,
    fallback_id: Option<&str>,
) -> Result<(MachineConfig, String), String> {
    let requested = match id {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => match fallback_id {
            Some(s) if !s.trim().is_empty() => s.trim(),
            _ => return Ok((MachinePreset::Generic.config(), "generic".to_string())),
        },
    };
    match MachinePreset::from_id(requested) {
        Some(preset) => {
            let cfg = preset.config();
            let id = cfg.id.clone();
            Ok((cfg, id))
        }
        None => {
            // 列出内建标识，让用户不必翻文档就知道有哪些可用值
            let known: Vec<String> = MachinePreset::all().iter().map(|p| p.id()).collect();
            Err(format!(
                "未知机床标识 `{requested}`（可用：{}）",
                known.join(" / ")
            ))
        }
    }
}

/// 合并「程序级参数 + 工序级参数」，工序级覆盖同名项。
///
/// 覆盖是**按参数名整体替换**，不是深合并 —— 参数值是 [`crate::ParamValue`]
/// 的扁平模型（JSON 对象被拒绝），没有可深合并的结构。
fn merge_params(base: &ParameterSet, over: &ParameterSet) -> ParameterSet {
    let mut merged = base.clone();
    for (k, v) in &over.values {
        merged.values.insert(k.clone(), v.clone());
    }
    merged
}

/// 把程序级选项与工序级覆盖合成该工序的 [`GenerationOptions`]。
fn op_options(
    base: &PartOptions,
    over: Option<&PartOpOptions>,
    line_number_start: u32,
) -> GenerationOptions {
    let mut opts = GenerationOptions {
        line_numbers: base.line_numbers,
        add_header_comment: base.add_header_comment,
        strip_blank_lines: base.strip_blank_lines,
        ascii_only: base.ascii_only,
        line_number_start,
        ..Default::default()
    };
    if let Some(o) = over {
        if let Some(v) = o.line_numbers {
            opts.line_numbers = v;
        }
        if let Some(v) = o.add_header_comment {
            opts.add_header_comment = v;
        }
        if let Some(v) = o.strip_blank_lines {
            opts.strip_blank_lines = v;
        }
        if let Some(v) = o.ascii_only {
            opts.ascii_only = v;
        }
    }
    opts
}

impl PartSpec {
    /// 校验零件定义的基本形状（`ops` 非空；每个 op 的 `template` 非空）。
    ///
    /// 与生成分离：调用方可在动手渲染前先挡掉明显不合法的定义。
    pub fn validate_shape(&self) -> Result<(), PartError> {
        if self.ops.is_empty() {
            return Err(PartError::InvalidSpec(
                "ops 为空 —— 零件至少要有一道工序".to_string(),
            ));
        }
        for (i, op) in self.ops.iter().enumerate() {
            if op.template.trim().is_empty() {
                return Err(PartError::InvalidSpec(format!(
                    "工序{} 的 template 为空",
                    i + 1
                )));
            }
        }
        Ok(())
    }

    /// 批量生成：逐工序渲染并拼接为一份完整程序。
    ///
    /// # 语义
    ///
    /// 1. **参数继承**：每道工序的生效参数 = [`Self::params`] ← 覆盖 `op.params`
    /// 2. **机床覆盖**：`op.machine` > [`Self::default_machine`] > `fallback_machine`
    /// 3. **行号续编**：上游工序的末行号作为下游的 `line_number_start`，
    ///    使 `N0010 / N0020` 跨工序连续，而非每段重来
    /// 4. **事务语义**：**任一道工序失败 → 整体失败**，不返回半成品。
    ///    仍会跑完所有工序以聚合全部失败项（见 [`PartError`]）
    ///
    /// # 参数
    /// - `fallback_machine`：`ops` 与 `default_machine` 都未指定时的机床标识
    /// - `opts`：程序级输出选项
    pub fn generate(
        &self,
        gen: &GCodeGenerator,
        fallback_machine: Option<&str>,
        opts: &PartOptions,
    ) -> Result<PartOutcome, PartError> {
        self.validate_shape()?;

        let mut outcomes: Vec<OpOutcome> = Vec::with_capacity(self.ops.len());
        let mut failures: Vec<OpFailure> = Vec::new();
        // 行号游标：跨工序传递（局限 5.1）。未开行号时恒为 0，无副作用。
        let mut cursor: u32 = 0;

        for (index, op) in self.ops.iter().enumerate() {
            let display_name = if op.template.trim().is_empty() {
                format!("工序{}", index + 1)
            } else {
                op.template.clone()
            };
            // 机床：工序级 > 程序级 > 调用方兜底
            let machine_id = op.machine.as_deref().or(self.default_machine.as_deref());
            let (machine, _resolved_id) = match resolve_machine(machine_id, fallback_machine) {
                Ok(v) => v,
                Err(e) => {
                    failures.push(OpFailure {
                        index,
                        name: display_name,
                        error: e,
                    });
                    continue;
                }
            };
            let params = merge_params(&self.params, &op.params);
            let op_opts = op_options(opts, op.options.as_ref(), cursor);

            let result = if opts.lenient {
                gen.generate_lenient(&op.template, &params, &machine, &op_opts)
                    .map(|out| (out, cursor))
            } else {
                gen.generate_with_cursor(&op.template, &params, &machine, &op_opts)
            };
            match result {
                Ok((output, end_line_number)) => {
                    cursor = end_line_number;
                    outcomes.push(OpOutcome {
                        index,
                        template: op.template.clone(),
                        output,
                        end_line_number,
                    });
                }
                Err(err) => failures.push(OpFailure {
                    index,
                    name: display_name,
                    error: describe_pipeline_error(&err),
                }),
            }
        }

        // 事务语义（局限 5.2）：有任何失败就整体不交付 —— 半份程序被误当成
        // 完整程序送上机床，比什么都不产出危险得多。
        if !failures.is_empty() {
            return Err(PartError::OperationsFailed { failures });
        }

        let program: String = outcomes.iter().map(|o| o.output.as_str()).collect();
        Ok(PartOutcome {
            ops: outcomes,
            program,
        })
    }
}

/// 把管线错误渲染成单行、面向用户的描述。
///
/// 校验类错误**只取首行摘要**：完整报告是多行的，塞进单行 `error` 字段会把
/// 聚合后的错误列表冲散；完整报告由调用方按需另行展示。
fn describe_pipeline_error(err: &PipelineError) -> String {
    match err {
        PipelineError::TemplateNotFound(name) => format!("模板不存在: {name}"),
        PipelineError::Validation(report) => {
            let first = report
                .issues
                .first()
                .map(|i| i.message.clone())
                .unwrap_or_else(|| "参数校验未通过".to_string());
            format!("参数校验未通过：{first}")
        }
        PipelineError::Derive(e) => format!("派生参数失败：{e}"),
        PipelineError::Render(e) => format!("渲染失败：{e}"),
        PipelineError::Registry(e) => format!("注册表错误：{e}"),
    }
}

/// 兼容性别名：`ops` 字段在旧设计文档中叫 `operations`。
///
/// 仅用于文档/迁移提示，不参与序列化（`serde` 走 [`PartSpec`] 的字段名）。
pub const LEGACY_OPS_FIELD: &str = "operations";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::TemplateCategory;

    /// 构建一个带临时文件的生成器。
    ///
    /// **测试模板一律用 `| default(...)` 把引用的变量标记为可选**：本项目里
    /// 裸引用 `{{ x }}` 表示"必选参数"，缺了会被校验拦下。测试关心的是编排
    /// 逻辑（继承/续编/事务），不是校验，故用可选形式让模板专注在要验证的点上；
    /// 需要验证校验行为的用例另行显式传值。
    ///
    /// 临时目录名用**原子自增序号**而非 `files.len()`：后者会让所有"2 个模板"
    /// 的用例共用同一目录并互相覆盖（测试并行运行时尤其明显，症状是拿到别的
    /// 用例写的模板内容）。
    fn gen_with(files: &[(&str, &str)]) -> GCodeGenerator {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "nctool_part_tests_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut g = GCodeGenerator::new();
        for (name, src) in files {
            let path = dir.join(format!("{name}.j2"));
            std::fs::write(&path, src).unwrap();
            g.registry_mut()
                .add_file(
                    (*name).to_string(),
                    TemplateCategory::General,
                    format!("测试模板 {name}"),
                    &path,
                    vec![],
                )
                .unwrap();
        }
        g
    }

    /// 从 JSON 字面量解析零件定义。
    ///
    /// **JSON 解析留在测试/调用方层**（`serde_json` 在 core 里是 dev-dependency）：
    /// core 的类型实现了 `Deserialize`，谁读文件谁解析即可，不必为"反序列化自己"
    /// 把 JSON 依赖提升成 core 的运行时依赖。
    fn spec_from(json: &str) -> PartSpec {
        serde_json::from_str(json).expect("测试用的 JSON 应能解析")
    }

    // ---- 解析与形状校验 ----

    #[test]
    fn parses_minimal_spec() {
        let part = spec_from(r#"{"name":"P","ops":[{"template":"a"}]}"#);
        assert_eq!(part.name.as_deref(), Some("P"));
        assert_eq!(part.ops.len(), 1);
        assert_eq!(part.ops[0].template, "a");
        assert!(part.ops[0].machine.is_none());
    }

    #[test]
    fn parses_full_spec_with_all_fields() {
        let part = spec_from(
            r#"{
                "name":"FLANGE",
                "default_machine":"wfl_m65",
                "params":{"part_name":"FLANGE_DEMO"},
                "ops":[{"template":"a","params":{"prog":1001},"machine":"generic",
                        "options":{"line_numbers":true,"ascii_only":true}}]
            }"#,
        );
        assert_eq!(part.default_machine.as_deref(), Some("wfl_m65"));
        assert!(part.params.get("part_name").is_some());
        let op = &part.ops[0];
        assert_eq!(op.machine.as_deref(), Some("generic"));
        let o = op.options.as_ref().unwrap();
        assert_eq!(o.line_numbers, Some(true));
        assert_eq!(o.ascii_only, Some(true));
        assert_eq!(
            o.strip_blank_lines, None,
            "未给出的字段应保持 None（不覆盖）"
        );
    }

    #[test]
    fn empty_ops_is_invalid() {
        let part = spec_from(r#"{"name":"P","ops":[]}"#);
        match part.validate_shape() {
            Err(PartError::InvalidSpec(msg)) => assert!(msg.contains("至少要有一道工序"), "{msg}"),
            other => panic!("应报 InvalidSpec，实得 {other:?}"),
        }
    }

    #[test]
    fn blank_template_is_invalid_and_names_the_position() {
        let part = spec_from(r#"{"ops":[{"template":"a"},{"template":"  "}]}"#);
        match part.validate_shape() {
            Err(PartError::InvalidSpec(msg)) => {
                assert!(msg.contains("工序2"), "错误应指出是第几道工序: {msg}")
            }
            other => panic!("应报 InvalidSpec，实得 {other:?}"),
        }
    }

    #[test]
    fn missing_ops_key_defaults_to_empty_then_fails_shape_check() {
        // `ops` 缺失（而非空数组）也要走同一条校验，不能因 serde 默认值而漏判
        let part = spec_from(r#"{"name":"P"}"#);
        assert!(part.ops.is_empty());
        assert!(part.validate_shape().is_err());
    }

    #[test]
    fn malformed_json_reports_parse_failure() {
        let err = serde_json::from_str::<PartSpec>("{not json").unwrap_err();
        assert!(err.to_string().contains("key must be a string"), "{err}");
    }

    // ---- 机床解析 ----

    #[test]
    fn resolve_machine_prefers_explicit_then_fallback_then_generic() {
        let (cfg, id) = resolve_machine(Some("wfl_m65"), Some("index_ms40")).unwrap();
        assert_eq!(id, "wfl_m65", "显式指定优先");
        assert_eq!(cfg.id, "wfl_m65");

        let (_, id) = resolve_machine(None, Some("index_ms40")).unwrap();
        assert_eq!(id, "index_ms40", "未指定时用调用方兜底");

        let (_, id) = resolve_machine(None, None).unwrap();
        assert_eq!(id, "generic", "都没有时用 generic");
    }

    #[test]
    fn blank_machine_id_is_treated_as_absent() {
        // 空串不是"合法的机床名"，与未指定同等对待
        let (_, id) = resolve_machine(Some("   "), Some("wfl_m65")).unwrap();
        assert_eq!(id, "wfl_m65");
        let (_, id) = resolve_machine(Some(""), None).unwrap();
        assert_eq!(id, "generic");
    }

    /// 未知机床标识必须**报错**，不能静默降级为 generic —— 静默降级会让整份
    /// 程序按错误的编程约定生成（程序号格式、行号前缀都不同）。
    #[test]
    fn unknown_machine_is_an_error_listing_valid_values() {
        let err = resolve_machine(Some("siemens_840d"), None).unwrap_err();
        assert!(err.contains("未知机床标识"), "{err}");
        assert!(err.contains("siemens_840d"), "应回显用户写的值: {err}");
        for known in MachinePreset::all() {
            assert!(
                err.contains(&known.id()),
                "应列出可用标识 {}: {err}",
                known.id()
            );
        }
    }

    // ---- 参数继承与覆盖 ----

    #[test]
    fn merge_params_op_overrides_part_level() {
        let mut base = ParameterSet::new();
        base.set_number("a", 1.0);
        base.set_number("shared", 1.0);
        let mut over = ParameterSet::new();
        over.set_number("shared", 2.0);
        over.set_number("b", 3.0);

        let merged = merge_params(&base, &over);
        assert_eq!(
            merged.get("a"),
            Some(&crate::ParamValue::Number(1.0)),
            "程序级独有项应保留"
        );
        assert_eq!(
            merged.get("shared"),
            Some(&crate::ParamValue::Number(2.0)),
            "工序级应覆盖程序级同名项"
        );
        assert_eq!(
            merged.get("b"),
            Some(&crate::ParamValue::Number(3.0)),
            "工序级独有项应加入"
        );
    }

    #[test]
    fn merge_params_with_empty_over_returns_base() {
        let mut base = ParameterSet::new();
        base.set_number("a", 1.0);
        assert_eq!(merge_params(&base, &ParameterSet::new()), base);
    }

    /// 参数继承的端到端证据：顶层 `part_name` 被两道工序同时看到，而工序级
    /// `prog` 只影响自己那一道。
    #[test]
    fn part_params_are_visible_to_every_op_and_op_params_win() {
        let g = gen_with(&[
            (
                "hdr",
                "( part={{ part_name | default(\"-\") }} prog={{ prog | default(0) }} )\n",
            ),
            (
                "body",
                "( part={{ part_name | default(\"-\") }} prog={{ prog | default(0) }} )\n",
            ),
        ]);
        let part = spec_from(
            r#"{
                "params":{"part_name":"FLANGE","prog":1001},
                "ops":[
                    {"template":"hdr"},
                    {"template":"body","params":{"prog":2002}}
                ]
            }"#,
        );
        let out = part.generate(&g, None, &PartOptions::default()).unwrap();
        assert!(
            out.ops[0].output.contains("part=FLANGE prog=1001"),
            "第一道应同时看到继承来的两个参数: {}",
            out.ops[0].output
        );
        assert!(
            out.ops[1].output.contains("part=FLANGE prog=2002"),
            "第二道的 prog 应被工序级覆盖，part_name 仍继承: {}",
            out.ops[1].output
        );
    }

    // ---- 行号跨工序续编（局限 5.1）----

    /// 核心回归：不开行号时各段独立，开了以后必须**连续**而非每段重来。
    #[test]
    fn line_numbers_continue_across_operations() {
        let g = gen_with(&[("a", "G0 X1\nG1 X2\n"), ("b", "G0 Y1\nG1 Y2\n")]);
        let part = spec_from(r#"{"ops":[{"template":"a"},{"template":"b"}]}"#);
        let opts = PartOptions {
            line_numbers: true,
            ..Default::default()
        };
        let out = part.generate(&g, None, &opts).unwrap();

        assert_eq!(out.ops[0].output, "N0010 G0 X1\nN0020 G1 X2\n");
        assert_eq!(out.ops[0].end_line_number, 20);
        // 关键：第二段从 N0030 起，而不是回到 N0010
        assert_eq!(
            out.ops[1].output, "N0030 G0 Y1\nN0040 G1 Y2\n",
            "行号应跨工序续编；每段重来会让拼接后的程序出现重复行号"
        );
        assert_eq!(out.ops[1].end_line_number, 40);

        // 拼接后整份程序的行号严格递增且不重复
        let nums: Vec<u32> = out
            .program
            .lines()
            .filter_map(|l| {
                l.strip_prefix('N')
                    .and_then(|r| r.split_whitespace().next())
            })
            .filter_map(|d| d.parse::<u32>().ok())
            .collect();
        assert_eq!(
            nums,
            vec![10, 20, 30, 40],
            "整份程序行号应唯一递增: {nums:?}"
        );
    }

    /// 段内若有**不参与编号**的行（程序号行），续编必须按"实际写入的末行号"
    /// 推进，而不是按段内行数推算 —— 否则后续所有段整体错位。
    #[test]
    fn continuation_ignores_non_numbered_program_line() {
        let g = gen_with(&[
            // 程序号行 O1001 不编号，其后两行编号
            ("hdr", "O1001\nG0 X1\nG1 X2\n"),
            ("body", "G0 Y1\n"),
        ]);
        let part = spec_from(r#"{"ops":[{"template":"hdr"},{"template":"body"}]}"#);
        let opts = PartOptions {
            line_numbers: true,
            ..Default::default()
        };
        let out = part.generate(&g, None, &opts).unwrap();

        assert_eq!(
            out.ops[0].output, "O1001\nN0010 G0 X1\nN0020 G1 X2\n",
            "程序号行应保持原样不编号"
        );
        assert_eq!(out.ops[0].end_line_number, 20);
        assert_eq!(
            out.ops[1].output, "N0030 G0 Y1\n",
            "续编应基于末行号 20，而不是按段内行数（3 行）推算"
        );
    }

    /// 不参与编号的行**不应**让游标前进 —— 若实现改成按行数递增，此用例会红。
    #[test]
    fn segment_with_no_numbered_line_does_not_advance_cursor() {
        let g = gen_with(&[("only_prog", "O1001\nO1002\n"), ("body", "G0 X1\n")]);
        let part = spec_from(r#"{"ops":[{"template":"only_prog"},{"template":"body"}]}"#);
        let opts = PartOptions {
            line_numbers: true,
            ..Default::default()
        };
        let out = part.generate(&g, None, &opts).unwrap();
        assert_eq!(out.ops[0].end_line_number, 0, "一行都未编号，游标不动");
        assert_eq!(
            out.ops[1].output, "N0010 G0 X1\n",
            "后一段仍从 N0010 开始（前一段确实没占用任何行号）"
        );
    }

    #[test]
    fn line_numbers_off_leaves_output_untouched() {
        let g = gen_with(&[("a", "G0 X1\n"), ("b", "G0 Y1\n")]);
        let part = spec_from(r#"{"ops":[{"template":"a"},{"template":"b"}]}"#);
        let out = part.generate(&g, None, &PartOptions::default()).unwrap();
        assert_eq!(out.ops[0].output, "G0 X1\n");
        assert_eq!(out.ops[1].output, "G0 Y1\n");
        assert_eq!(out.ops[0].end_line_number, 0);
    }

    /// 工序级 `options.line_numbers` 可单独开启/关闭，且未覆盖的字段沿用程序级。
    #[test]
    fn op_options_override_only_the_named_fields() {
        let g = gen_with(&[("a", "G0 X1\n"), ("b", "G0 Y1\n")]);
        // 程序级关行号、开头部注释；第一道工序单独开行号
        let part = spec_from(
            r#"{"ops":[
                {"template":"a","options":{"line_numbers":true}},
                {"template":"b"}
            ]}"#,
        );
        let opts = PartOptions {
            line_numbers: false,
            add_header_comment: true,
            ..Default::default()
        };
        let out = part.generate(&g, None, &opts).unwrap();
        assert!(
            out.ops[0].output.contains("N0010 G0 X1"),
            "工序级应覆盖为开行号: {}",
            out.ops[0].output
        );
        assert!(
            out.ops[0].output.contains("nctool generated G-code"),
            "未覆盖的字段（头部注释）应沿用程序级: {}",
            out.ops[0].output
        );
        assert!(
            !out.ops[1].output.contains("N0010"),
            "第二道未覆盖，应保持程序级的关行号: {}",
            out.ops[1].output
        );
        assert!(
            out.ops[1].output.contains("nctool generated G-code"),
            "第二道的头部注释也应沿用: {}",
            out.ops[1].output
        );
    }

    // ---- 事务语义与错误聚合（局限 5.2）----

    #[test]
    fn any_op_failure_fails_the_whole_part_without_partial_output() {
        let g = gen_with(&[("ok", "G0 X1\n"), ("needs", "G0 X{{ missing }}\n")]);
        let part = spec_from(r#"{"ops":[{"template":"ok"},{"template":"needs"}]}"#);
        let err = part
            .generate(&g, None, &PartOptions::default())
            .unwrap_err();
        match err {
            PartError::OperationsFailed { failures } => {
                assert_eq!(failures.len(), 1);
                assert_eq!(failures[0].index, 1);
                assert_eq!(failures[0].name, "needs");
                assert!(
                    failures[0].error.contains("参数校验未通过")
                        || failures[0].error.contains("missing"),
                    "错误应说明原因: {}",
                    failures[0].error
                );
            }
            other => panic!("应报 OperationsFailed，实得 {other:?}"),
        }
        // 注意：PartError 里没有 outcome 字段 —— 类型系统保证调用方拿不到半成品
    }

    /// 错误**一次性报全**，而不是遇到第一个就停（否则修一个跑一次又冒一个）。
    #[test]
    fn failures_are_aggregated_not_short_circuited() {
        let g = gen_with(&[
            ("ok", "G0 X1\n"),
            ("bad1", "G0 {{ m1 }}\n"),
            ("bad2", "G0 {{ m2 }}\n"),
        ]);
        let part = spec_from(
            r#"{"ops":[
                {"template":"ok"},
                {"template":"bad1"},
                {"template":"bad2"}
            ]}"#,
        );
        match part
            .generate(&g, None, &PartOptions::default())
            .unwrap_err()
        {
            PartError::OperationsFailed { failures } => {
                assert_eq!(failures.len(), 2, "两道失败工序应全部报出: {failures:?}");
                assert_eq!(failures[0].index, 1);
                assert_eq!(failures[1].index, 2);
            }
            other => panic!("应报 OperationsFailed，实得 {other:?}"),
        }
    }

    #[test]
    fn missing_template_is_reported_per_op() {
        let g = gen_with(&[("a", "G0 X1\n")]);
        let part = spec_from(r#"{"ops":[{"template":"a"},{"template":"nope"}]}"#);
        match part
            .generate(&g, None, &PartOptions::default())
            .unwrap_err()
        {
            PartError::OperationsFailed { failures } => {
                assert_eq!(failures.len(), 1);
                assert!(
                    failures[0].error.contains("模板不存在"),
                    "应明确指出模板不存在: {}",
                    failures[0].error
                );
                assert!(failures[0].error.contains("nope"), "{}", failures[0].error);
            }
            other => panic!("应报 OperationsFailed，实得 {other:?}"),
        }
    }

    /// 机床标识错误也走同一条聚合通道，且**指出是第几道工序**。
    #[test]
    fn unknown_op_machine_is_aggregated_with_position() {
        let g = gen_with(&[("a", "G0 X1\n"), ("b", "G0 Y1\n")]);
        let part =
            spec_from(r#"{"ops":[{"template":"a","machine":"nope_machine"},{"template":"b"}]}"#);
        match part
            .generate(&g, None, &PartOptions::default())
            .unwrap_err()
        {
            PartError::OperationsFailed { failures } => {
                assert_eq!(failures.len(), 1, "只有第一道机床写错");
                assert_eq!(failures[0].index, 0);
                assert!(
                    failures[0].error.contains("未知机床标识"),
                    "{}",
                    failures[0].error
                );
            }
            other => panic!("应报 OperationsFailed，实得 {other:?}"),
        }
    }

    /// 宽松模式放行"参数缺失"（不再硬拦），与单模板宽松语义一致。
    ///
    /// 这里**不放 NaN**：NaN 的硬失败语义由
    /// [`Self::lenient_mode_still_hard_fails_on_nan_for_every_op`] 单独验证，
    /// 两个关注点混在一个用例里会让失败原因难以判读。
    #[test]
    fn lenient_mode_allows_missing_params() {
        let g = gen_with(&[
            ("a", "G0 X{{ missing_arg }}\n"),
            ("b", "G0 Y{{ missing_arg }}\n"),
        ]);
        let part = spec_from(r#"{"ops":[{"template":"a"},{"template":"b"}]}"#);

        // 严格模式：缺必选参数 → 整体失败
        match part
            .generate(&g, None, &PartOptions::default())
            .unwrap_err()
        {
            PartError::OperationsFailed { failures } => {
                assert_eq!(failures.len(), 2, "严格模式下两道都缺参: {failures:?}");
            }
            other => panic!("应报 OperationsFailed，实得 {other:?}"),
        }

        // 宽松模式：缺参数降级放行 → 两道都通过，未定义变量渲染为空
        let lenient = PartOptions {
            lenient: true,
            ..Default::default()
        };
        let ok = part
            .generate(&g, None, &lenient)
            .expect("缺参在宽松模式应放行");
        assert_eq!(ok.ops.len(), 2);
        assert!(
            ok.ops[0].output.contains("G0 X\n"),
            "未定义变量渲染为空: {:?}",
            ok.ops[0].output
        );
        assert!(
            ok.ops[1].output.contains("G0 Y\n"),
            "{:?}",
            ok.ops[1].output
        );
    }

    /// 宽松模式**仍然硬拦 NaN**，且因继承而对每一道工序都拦。
    #[test]
    fn lenient_mode_still_hard_fails_on_nan_for_every_op() {
        // 两道工序都引用同一个程序级参数，故 NaN 对两者都是硬失败
        let g = gen_with(&[("a", "G0 X{{ bad }}\n"), ("b", "G0 Y{{ bad }}\n")]);
        let mut part = spec_from(r#"{"ops":[{"template":"a"},{"template":"b"}]}"#);
        part.params.set_number("bad", f64::NAN);

        let lenient = PartOptions {
            lenient: true,
            ..Default::default()
        };
        match part.generate(&g, None, &lenient).unwrap_err() {
            PartError::OperationsFailed { failures } => {
                assert_eq!(
                    failures.len(),
                    2,
                    "程序级 NaN 参数对所有工序可见，两道都该失败（这也顺带证明继承生效）: {failures:?}"
                );
                assert_eq!(failures[0].index, 0);
                assert_eq!(failures[1].index, 1);
                for f in &failures {
                    assert!(
                        f.error.contains("NaN") || f.error.contains("非有限数"),
                        "错误应说明是 NaN: {}",
                        f.error
                    );
                }
            }
            other => panic!("应报 OperationsFailed，实得 {other:?}"),
        }
    }

    #[test]
    fn lenient_all_failures_resolved_succeeds() {
        let g = gen_with(&[("a", "G0 X{{ missing_arg }}\n"), ("b", "G0 Y1\n")]);
        let part = spec_from(r#"{"ops":[{"template":"a"},{"template":"b"}]}"#);
        let lenient = PartOptions {
            lenient: true,
            ..Default::default()
        };
        let out = part.generate(&g, None, &lenient).unwrap();
        assert_eq!(out.ops.len(), 2);
        assert!(
            out.ops[0].output.contains("G0 X\n"),
            "未定义变量渲染为空: {:?}",
            out.ops[0].output
        );
    }

    // ---- 拼接与结果 ----

    #[test]
    fn program_is_ops_concatenated_in_order() {
        let g = gen_with(&[("a", "A1\nA2\n"), ("b", "B1\n")]);
        let part = spec_from(r#"{"ops":[{"template":"a"},{"template":"b"}]}"#);
        let out = part.generate(&g, None, &PartOptions::default()).unwrap();
        assert_eq!(out.program, "A1\nA2\nB1\n", "应按 ops 顺序首尾相接");
        assert_eq!(out.line_count(), 3);
        assert_eq!(out.ops.len(), 2);
    }

    #[test]
    fn single_op_part_works() {
        let g = gen_with(&[("a", "X1\n")]);
        let part = spec_from(r#"{"ops":[{"template":"a"}]}"#);
        let out = part.generate(&g, None, &PartOptions::default()).unwrap();
        assert_eq!(out.program, "X1\n");
    }

    #[test]
    fn line_count_ignores_blank_lines() {
        let g = gen_with(&[("a", "A1\n\n\nA2\n")]);
        let part = spec_from(r#"{"ops":[{"template":"a"}]}"#);
        let out = part.generate(&g, None, &PartOptions::default()).unwrap();
        assert_eq!(out.line_count(), 2, "空行不计入");
    }

    /// 机床覆盖的端到端证据：工序级机床覆盖生效，
    /// 且续编游标跨工序推进（第一道 1 行 → N0010，第二道接 N0020）。
    ///
    /// **注意内建三预设的 `line_number_prefix` 当前都是 `N`**（已实测），
    /// 因此无法靠前缀区分"用了哪台机床"。故本用例的机床覆盖证据取自
    /// [`PartOutcome`] 里逐工序的游标推进，而非输出文本的前缀 —— 后者
    /// 在当前预设下证明不了覆盖生效，只会给一个永远通过的假断言。
    #[test]
    fn op_machine_overrides_part_default_affecting_output() {
        let g = gen_with(&[("a", "G0 X1\n"), ("b", "G0 Y1\n")]);

        let part = spec_from(
            r#"{"default_machine":"generic","ops":[
                {"template":"a"},
                {"template":"b","machine":"wfl_m65"}
            ]}"#,
        );
        let opts = PartOptions {
            line_numbers: true,
            ..Default::default()
        };
        let out = part.generate(&g, None, &opts).unwrap();
        // 第一道：1 行 → N0010，游标 10
        assert_eq!(out.ops[0].output, "N0010 G0 X1\n");
        assert_eq!(out.ops[0].end_line_number, 10);
        // 第二道接续编到 20（工序级机床 wfl_m65 被接受并成功渲染）
        assert_eq!(
            out.ops[1].output, "N0020 G0 Y1\n",
            "第二道应续编到 20: {}",
            out.ops[1].output
        );
        assert_eq!(out.ops[1].end_line_number, 20);
    }

    /// 工序级机床写成**未知标识**时必须失败 —— 这证明 `op.machine` 真的被读取
    /// 并参与解析，而不是被忽略后静默沿用程序级默认。
    ///
    /// 这是"机床覆盖生效"的**否定式证据**，比文本前缀更可靠：前缀在内建预设下
    /// 三者相同，证不了任何事。
    #[test]
    fn op_machine_is_actually_consulted_not_ignored() {
        let g = gen_with(&[("a", "G0 X1\n")]);
        let part = spec_from(
            r#"{"default_machine":"generic","ops":[{"template":"a","machine":"no_such_machine"}]}"#,
        );
        match part
            .generate(&g, None, &PartOptions::default())
            .unwrap_err()
        {
            PartError::OperationsFailed { failures } => {
                assert_eq!(failures.len(), 1);
                assert!(
                    failures[0].error.contains("未知机床标识"),
                    "工序级机床若被忽略，这里会静默成功: {}",
                    failures[0].error
                );
            }
            other => panic!("应报 OperationsFailed，实得 {other:?}"),
        }
    }

    #[test]
    fn fallback_machine_used_when_spec_has_none() {
        let g = gen_with(&[("a", "G0 X1\n")]);
        let part = spec_from(r#"{"ops":[{"template":"a"}]}"#);
        // 传一个不存在的兜底机床 → 应报错（证明兜底参数确实被用了）
        let err = part.generate(&g, Some("nonexistent"), &PartOptions::default());
        assert!(err.is_err(), "兜底机床名无效时应失败，证明该参数被消费");
        // 传有效兜底 → 成功
        let out = part.generate(&g, Some("index_ms40"), &PartOptions::default());
        assert!(out.is_ok());
    }

    // ---- 错误展示 ----

    #[test]
    fn error_display_lists_every_failure_with_position_and_name() {
        let err = PartError::OperationsFailed {
            failures: vec![
                OpFailure {
                    index: 0,
                    name: "a".to_string(),
                    error: "模板不存在: a".to_string(),
                },
                OpFailure {
                    index: 2,
                    name: "c".to_string(),
                    error: "参数校验未通过：缺 x".to_string(),
                },
            ],
        };
        let msg = err.to_string();
        assert!(msg.contains("2 道工序失败"), "{msg}");
        assert!(msg.contains("工序1（a）"), "下标 0 应展示为工序1: {msg}");
        assert!(msg.contains("工序3（c）"), "下标 2 应展示为工序3: {msg}");
        assert!(msg.contains("模板不存在: a"), "{msg}");
    }

    #[test]
    fn invalid_spec_display_mentions_the_reason() {
        let err = PartError::InvalidSpec("ops 为空".to_string());
        assert!(err.to_string().contains("零件定义不合法：ops 为空"));
    }

    #[test]
    fn error_implements_std_error_for_question_mark_chaining() {
        fn takes_std_error(_: &dyn std::error::Error) {}
        takes_std_error(&PartError::InvalidSpec("x".to_string()));
    }

    #[test]
    fn op_failure_serializes_with_stable_field_names() {
        let f = OpFailure {
            index: 1,
            name: "drill".to_string(),
            error: "boom".to_string(),
        };
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["index"], serde_json::json!(1));
        assert_eq!(v["name"], serde_json::json!("drill"));
        assert_eq!(v["error"], serde_json::json!("boom"));
    }

    // ---- 选项构建 ----

    #[test]
    fn op_options_inherits_program_level_when_no_override() {
        let base = PartOptions {
            line_numbers: true,
            add_header_comment: true,
            strip_blank_lines: true,
            ascii_only: true,
            lenient: true,
        };
        let o = op_options(&base, None, 50);
        assert!(o.line_numbers && o.add_header_comment && o.strip_blank_lines && o.ascii_only);
        assert_eq!(o.line_number_start, 50, "续编游标必须透传");
        assert_eq!(o.format, crate::OutputFormat::Gcode);
    }

    #[test]
    fn op_options_partial_override_keeps_other_program_level_values() {
        let base = PartOptions {
            line_numbers: false,
            add_header_comment: true,
            strip_blank_lines: true,
            ascii_only: true,
            lenient: false,
        };
        let over = PartOpOptions {
            line_numbers: Some(true),
            ..Default::default()
        };
        let o = op_options(&base, Some(&over), 0);
        assert!(o.line_numbers, "被覆盖的字段取工序级");
        assert!(o.add_header_comment, "未覆盖的字段取程序级");
        assert!(o.strip_blank_lines);
        assert!(o.ascii_only);
    }

    #[test]
    fn default_part_options_are_all_off() {
        let o = PartOptions::default();
        assert!(!o.line_numbers && !o.add_header_comment && !o.strip_blank_lines);
        assert!(!o.ascii_only && !o.lenient);
    }
}
