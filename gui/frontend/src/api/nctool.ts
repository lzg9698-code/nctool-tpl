// Tauri `invoke` 封装：统一把 reject 规范化为 `CommandError`。
import { invoke } from "@tauri-apps/api/core";
import type {
  CommandError,
  ConfigDump,
  GenOptions,
  LintFinding,
  MachineSummary,
  PresetDeleteResult,
  PresetListData,
  PresetSaveResult,
  RenderData,
  TemplateDetail,
  TemplateSummary,
} from "../types";

export type { CommandError };

/** 把任意 reject 值规范化为 `CommandError`（Tauri 侧已序列化为 `{kind,message,status}`）。 */
function normalize(e: unknown): CommandError {
  if (e && typeof e === "object") {
    const o = e as Record<string, unknown>;
    if (typeof o.kind === "string" && typeof o.message === "string") {
      return {
        kind: o.kind,
        message: o.message,
        status: typeof o.status === "number" ? o.status : 500,
      };
    }
  }
  return { kind: "internal", message: String(e), status: 500 };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw normalize(e);
  }
}

/**
 * 全部命令入口。
 *
 * **硬约束**：`renderTemplate` / `validateTemplate` 的 `paramsJson` 是 **`string`**；
 * 调用点必须先 `JSON.stringify(params)` 再传入（保留下溢守卫，勘误 E10）。
 */
export const api = {
  listTemplates: (category?: string) =>
    call<{ templates: TemplateSummary[] }>("list_templates", {
      category: category ?? null,
    }),
  getTemplate: (name: string) =>
    call<{ template: TemplateDetail }>("get_template", { name }),
  listMachines: () => call<{ machines: MachineSummary[] }>("list_machines"),
  renderTemplate: (template: string, paramsJson: string, options: GenOptions) =>
    call<RenderData>("render_template", { template, paramsJson, options }),
  validateTemplate: (template: string, paramsJson: string) =>
    call<{ report: unknown }>("validate_template", { template, paramsJson }),
  getConfig: () => call<ConfigDump>("get_config"),
  saveNcFile: (path: string, content: string) =>
    call<void>("save_nc_file", { path, content }),

  // —— Phase 2 第一批 ——
  /** `GET /api/presets`（`?template=` 过滤）。列表项含扁平 `params` 取值，可直接应用。 */
  listPresets: (template?: string) =>
    call<PresetListData>("list_presets", { template: template ?? null }),
  /**
   * `POST /api/presets`。**`paramsJson` 必须是 `JSON.stringify(params)` 的结果**
   * （Rust 侧文本拼接，保留下溢守卫）。同名默认 409，确认覆盖后带 `force: true` 重试。
   */
  savePreset: (
    name: string,
    template: string,
    paramsJson: string,
    force = false,
  ) => call<PresetSaveResult>("save_preset", { name, template, paramsJson, force }),
  /** `POST /api/presets/delete`。 */
  deletePreset: (name: string) =>
    call<PresetDeleteResult>("delete_preset", { name }),
  /** GUI 专有：`GET /api/templates/{name}` 取源码 → `nctool_tpl::lint`（无级别）。 */
  lintTemplate: (template: string) =>
    call<LintFinding[]>("lint_template", { template }),
};
