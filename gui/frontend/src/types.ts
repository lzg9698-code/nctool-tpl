// 领域类型与 HTTP 包络 `data` 形状（§3.4）逐字对齐。
// GUI 走 Tauri `invoke`（非 `/api/` 字面量），故不参与 check_api_parity 纳管。

/** 命令错误：`api/nctool.ts` 把 Tauri 的 reject 规范化为该形状后抛出。 */
export interface CommandError {
  kind: string;
  message: string;
  status: number;
}

/** `GET /api/templates` → `data.templates[]`（Phase 1 只用这三个字段，勘误 E4）。 */
export interface TemplateSummary {
  name: string;
  category: string;
  description: string;
}

/** `ParamSpec` 的前端投影（`server.rs::spec_json`）。 */
export interface ParamSpec {
  name: string;
  kind: "Number" | "Integer" | "String" | "Bool" | "List" | "Choice" | "Any";
  required: boolean;
  default: unknown;
  min: number | null;
  max: number | null;
  integer: boolean;
  unit: string | null;
  options: string[] | null;
  requiredIf: string | null;
  derive: string | null;
  desc: string;
  description: string;
}

/** 变量行列定位（`server.rs::vars_json`）。 */
export interface VarLoc {
  name: string;
  line: number;
  col: number;
}

/** `GET /api/templates/{name}` → `data.template`。 */
export interface TemplateDetail {
  name: string;
  category: string;
  description: string;
  builtin: boolean;
  source: string;
  params: ParamSpec[];
  variables: { required: VarLoc[]; optional: VarLoc[] };
}

/** `GET /api/machines` → `data.machines[]`。 */
export interface MachineSummary {
  id: string;
  vendor: string;
  model: string;
  config: Record<string, string>;
  builtin: boolean;
}

/** 生成选项：键名与 HTTP `options` **逐字对齐**（camelCase）。`machine` 走顶层。 */
export interface GenOptions {
  lineNumbers: boolean;
  lineStep: number;
  maxLine: number;
  addHeader: boolean;
  stripBlank: boolean;
  ascii: boolean;
  lenient: boolean;
  machine: string | null;
}

/** `POST /api/render` → `data`（`blocked` 时无 `warnings` / `template`）。 */
export interface RenderData {
  blocked: boolean;
  report: unknown;
  output: string;
  warnings?: string[];
  template?: string;
  machine: string;
}

/** `get_config` 命令的只读视图。 */
export interface ConfigDump {
  globalPath: string | null;
  projectPath: string | null;
  templateDir: string | null;
  defaultMachine: string | null;
  customMachines: string[];
  warnings: string[];
}

// ---------------------------------------------------------------------------
// Phase 2 第一批：参数预设（GET/POST /api/presets*）与静态检查（lint）
// ---------------------------------------------------------------------------

/**
 * `GET /api/presets` → `data.presets[]`（`server.rs::presets_list`）。
 *
 * 列表项含 `params` 取值（**扁平**形态 `{"x":21.0}`，与 `savePreset` 的 `params`
 * 入参互逆），消费方可直接从列表"应用"预设（回填表单 + 切到渲染页）。
 */
export interface PresetSummary {
  name: string;
  template: string;
  paramCount: number;
  createdAt: string;
  specFingerprint: string;
  /**
   * 预设的参数取值（扁平形态 `{"x":21.0}`）。
   *
   * 与 `savePreset` 的 `params` 入参**互逆**（`server.rs::params_flat_json`），
   * 可直接写入渲染页表单；键序为字典序（后端 `BTreeMap`）。
   */
  params: Record<string, unknown>;
  /** 模板可解析（能被注册表找到）→ true；模板缺失/语法坏 → false。 */
  resolvable: boolean;
  /** 相对当前模板规格是否陈旧；`resolvable=false` 时为 `null`。 */
  stale: boolean | null;
  /** 陈旧的具体参数名（`resolvable=false` 时为 `null`）。 */
  staleParams: string[] | null;
  /** 规格新增的必选参数中缺失的项（`resolvable=false` 时为 `null`）。 */
  missingRequired: string[] | null;
}

/** `list_presets` 命令 → `data`。 */
export interface PresetListData {
  /** 预设文件位置：`~/...` 脱敏形态，不含用户名绝对路径（P1-9） */
  path: string;
  /** 恒为 true：`path` 已脱敏，非完整绝对路径（P1-9） */
  pathRedacted?: boolean;
  presets: PresetSummary[];
  /** 损坏文件 / 版本未知时的降级警告（只读不被坏文件拦住，D13）�?*/
  warnings?: string[];
}

/** `save_preset` 命令 → `data`。 */
export interface PresetSaveResult {
  name: string;
  template: string;
  paramCount: number;
  /** 预设文件位置：`~/...` 脱敏形态（P1-9） */
  path: string;
  /** 恒为 true：`path` 已脱敏（P1-9） */
  pathRedacted?: boolean;
  action: string;
  specFingerprint: string;
  fileFingerprint: string;
}

/** `delete_preset` 命令 → `data`。 */
export interface PresetDeleteResult {
  name: string;
  /** 预设文件位置：`~/...` 脱敏形态（P1-9） */
  path: string;
  /** 恒为 true：`path` 已脱敏（P1-9） */
  pathRedacted?: boolean;
  action: string;
  fileFingerprint: string;
}

/**
 * `lint_template` 命令 → `data[]`（`nctool_tpl::LintFinding`）。
 *
 * **无级别字段**：lint 结果不得标 ERROR/WARN/INFO、不得算"通过率"（设计稿 §2c）。
 */
export interface LintFinding {
  line: number;
  col: number;
  filter: string;
  suggestion: string;
  message: string;
}

/** 一条校验问题（`core::validate::ValidationIssue` 的 JSON 视图）。 */
export interface ValidationIssue {
  /** `ValidationLevel::as_str()`：`"error"` / `"warning"` / `"info"`（小写）。 */
  level: "error" | "warning" | "info";
  /** 关联参数名；无参问题为 `null`（字段存在，值为 null）。 */
  param: string | null;
  message: string;
}

/**
 * `validate_template` 命令 → `data.report`（`ValidationReportJson`）。
 *
 * 注意：**没有 `infos` 计数字段** —— INFO 数需由 `issues` 自行统计；
 * `errors` / `warnings` 是稳定契约字段（可用于门禁）。
 */
export interface ValidationReport {
  template: string;
  ok: boolean;
  errors: number;
  warnings: number;
  issues: ValidationIssue[];
}


