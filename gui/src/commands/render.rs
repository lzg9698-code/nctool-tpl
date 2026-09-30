//! 渲染命令：`render_template` —— 参数通道安全的关键落点。

use crate::commands::shared::{load_ctx, run_route, spawn_failed, CommandError};

/// 生成选项：字段名与 HTTP `options` 键**逐字对齐**（camelCase）。
///
/// `machine` 是**顶层**请求体字段（构造 body 时从 options 里取出），
/// 不参与 `options` 对象（勘误 E2）。
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GenOptions {
    /// → `lineNumbers`（默认关闭，与 CLI 默认输出一致）
    pub line_numbers: bool,
    /// → `lineStep`（默认 10）
    #[serde(default = "step_default")]
    pub line_step: u32,
    /// → `maxLine`（默认 9999）
    #[serde(default = "max_default")]
    pub max_line: u32,
    /// → `addHeader`
    pub add_header: bool,
    /// → `stripBlank`
    pub strip_blank: bool,
    /// → `ascii`
    pub ascii: bool,
    /// → `lenient`
    pub lenient: bool,
    /// 顶层字段（构造 body 时从 options 里取出）
    pub machine: Option<String>,
}

fn step_default() -> u32 {
    10
}

fn max_default() -> u32 {
    9999
}

/// 手写 `Default`：默认值必须与 CLI 的 `--line-step 10` / `--max-line 9999` 一致
/// （`#[derive(Default)]` 会给出 0，与 CLI 默认输出不一致）。
impl Default for GenOptions {
    fn default() -> Self {
        Self {
            line_numbers: false,
            line_step: step_default(),
            max_line: max_default(),
            add_header: false,
            strip_blank: false,
            ascii: false,
            lenient: false,
            machine: None,
        }
    }
}

impl GenOptions {
    /// 序列化为 HTTP `options` 对象文本：字段顺序**固定**（camelCase），不含 `machine`。
    ///
    /// 手写而非 `serde_json::to_value` + `to_string`：`serde_json` 默认的 `Map`
    /// 是 `BTreeMap`（键按字典序），经 `Value` 往返会打乱字段顺序，令请求体形状
    /// 不可预测。此处的输出是**契约**，必须逐字稳定。
    fn to_options_json(&self) -> String {
        format!(
            "{{\"lineNumbers\":{},\"lineStep\":{},\"maxLine\":{},\
             \"addHeader\":{},\"stripBlank\":{},\"ascii\":{},\"lenient\":{}}}",
            self.line_numbers,
            self.line_step,
            self.max_line,
            self.add_header,
            self.strip_blank,
            self.ascii,
            self.lenient
        )
    }
}

/// ★ 关键：`params_json` 以**文本**拼接进请求体，绝不 `from_str`→`to_string`。
///
/// 若把 `params_json` 先 `serde_json::from_str` 再 `to_string`，下溢字面量
/// （如 `1e-400`）会被静默归零成 `0.0`，`route` 的文本层下溢守卫随之失效
/// （勘误 E10）。原样拼接让守卫照常生效。
pub(crate) fn build_render_body(template: &str, params_json: &str, opts: &GenOptions) -> String {
    let mut body = format!(
        "{{\"template\":{t},\"params\":{p},\"options\":{o}",
        t = serde_json::to_string(template).expect("模板名序列化"),
        p = params_json, // ← 原样文本
        o = opts.to_options_json(),
    );
    if let Some(m) = opts.machine.as_deref() {
        body.push_str(&format!(
            ",\"machine\":{}",
            serde_json::to_string(m).expect("机床名序列化")
        ));
    }
    body.push('}');
    body
}

/// 渲染模板 → 返回 `data = {blocked, report, output, warnings?, template?, machine}`。
///
/// `blocked` 时**无** `warnings` / `template`（前端需判空）。
#[tauri::command]
pub async fn render_template(
    template: String,
    params_json: String,
    options: Option<GenOptions>,
) -> Result<serde_json::Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = load_ctx()?;
        let body = build_render_body(&template, &params_json, &options.unwrap_or_default());
        run_route(&ctx, "POST", "/api/render", "", body.as_bytes())
    })
    .await
    .map_err(spawn_failed)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use nctool_cli::config::LoadedConfig;
    use nctool_cli::context::Ctx;

    /// 仓库根下的 `templates/`（`CARGO_MANIFEST_DIR` = `.../rustjinja/gui`）。
    fn repo_templates() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("gui 的上级目录 = 仓库根")
            .join("templates")
    }

    fn ctx() -> Ctx {
        Ctx::for_embedded(Some(repo_templates()), None, LoadedConfig::default())
    }

    /// ① 请求体构造：camelCase / `machine` 顶层 / `params` 文本拼接（逐字相等）。
    #[test]
    fn build_render_body_shape() {
        let opts = GenOptions {
            line_numbers: true,
            line_step: 10,
            max_line: 9999,
            machine: Some("generic".into()),
            ..Default::default()
        };
        let body = build_render_body(
            "drill_cycle",
            r#"{"x":21,"y":15,"depth":-10,"feed":100}"#,
            &opts,
        );
        assert_eq!(
            body,
            r#"{"template":"drill_cycle","params":{"x":21,"y":15,"depth":-10,"feed":100},"options":{"lineNumbers":true,"lineStep":10,"maxLine":9999,"addHeader":false,"stripBlank":false,"ascii":false,"lenient":false},"machine":"generic"}"#
        );
    }

    /// ② 逐字节一致性：与 CLI golden 基线对拍（`tests/golden/drill_cycle_generic.nc`）。
    #[test]
    fn output_matches_cli_golden() {
        let body = build_render_body(
            "drill_cycle",
            r#"{"x":21,"y":15,"depth":-10,"feed":100}"#,
            &GenOptions::default(),
        );
        let data = run_route(&ctx(), "POST", "/api/render", "", body.as_bytes())
            .expect("渲染应成功（与 CLI 同一契约）");
        let out = data["output"].as_str().expect("data.output 应为字符串");
        let golden = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("gui 的上级目录 = 仓库根")
                .join("tests/golden/drill_cycle_generic.nc"),
        )
        .expect("读取 golden 基线")
        .replace("\r\n", "\n");
        assert_eq!(out, golden, "GUI 输出必须与 CLI golden 逐字节一致");
    }

    /// ③ 门槛项：JSON 下溢必须**硬失败**且不产出 G-code。
    #[test]
    fn underflow_is_hard_error() {
        let body = build_render_body(
            "drill_cycle",
            r#"{"x":1e-400,"y":15,"depth":-10,"feed":100}"#,
            &GenOptions::default(),
        );
        let err = run_route(&ctx(), "POST", "/api/render", "", body.as_bytes())
            .expect_err("下溢字面量必须硬失败，不得产出 G-code");
        assert_eq!(err.kind, "num_underflow");
        assert_eq!(err.status, 400);
    }
}
