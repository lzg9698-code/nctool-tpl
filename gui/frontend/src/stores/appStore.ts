import { create } from "zustand";
import { api } from "../api/nctool";
import { createDebouncer } from "../lib/debounce";
import type {
  CommandError,
  ConfigDump,
  GenOptions,
  MachineSummary,
  ParamSpec,
  TemplateSummary,
  VarLoc,
} from "../types";

export type PageId =
  | "render"
  | "templates"
  | "checks"
  | "machine"
  | "preset"
  | "config";

/** 生成选项默认值：**行号默认关闭**，与 CLI 默认输出一致。 */
export const defaultOptions: GenOptions = {
  lineNumbers: false,
  lineStep: 10,
  maxLine: 9999,
  addHeader: false,
  stripBlank: false,
  ascii: false,
  lenient: false,
  machine: null,
};

/** 300ms 防抖落点（模块级单例；模板/机床切换走 `renderNow` 不防抖）。 */
const debouncer = createDebouncer(300);

function errOf(e: unknown): CommandError {
  if (e && typeof e === "object" && "message" in e) {
    const o = e as { kind?: unknown; message?: unknown; status?: unknown };
    return {
      kind: typeof o.kind === "string" ? o.kind : "internal",
      message: String(o.message),
      status: typeof o.status === "number" ? o.status : 500,
    };
  }
  return { kind: "internal", message: String(e), status: 500 };
}

/** 由 `ParamSpec` 初始化表单值（默认值优先，其次按类型给中性值）。 */
function initialParams(spec: ParamSpec[]): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const s of spec) {
    if (s.default !== null && s.default !== undefined) out[s.name] = s.default;
    else if (s.kind === "Bool") out[s.name] = undefined;
    else out[s.name] = "";
  }
  return out;
}

export interface AppStore {
  page: PageId;
  setPage(p: PageId): void;

  // 模板 / 机床
  templates: TemplateSummary[];
  machines: MachineSummary[];
  loadTemplates(): Promise<void>;
  loadMachines(): Promise<void>;

  // 配置摘要（状态栏提示）
  configWarnings: string[];
  loadConfig(): Promise<void>;

  // 渲染上下文
  selected: string | null;
  spec: ParamSpec[];
  variables: { required: VarLoc[]; optional: VarLoc[] };
  selectTemplate(name: string): Promise<void>;

  params: Record<string, unknown>;
  setParam(name: string, value: unknown): void;
  resetParams(): void;
  /**
   * 应用一个预设：切到其绑定模板（若与当前不同）→ 用预设取值**整体覆盖**表单参数
   * → 立即渲染。与 Web UI 的 `loadPreset` 同一口径（`state.params = {...p.params}`）。
   */
  applyPreset(
    template: string,
    params: Record<string, unknown>,
  ): Promise<void>;

  options: GenOptions;
  setOption<K extends keyof GenOptions>(k: K, v: GenOptions[K]): void;

  // 渲染输出
  output: string;
  blocked: boolean;
  warnings: string[];
  renderError: { kind: string; message: string } | null;
  rendering: boolean;
  renderNow(): Promise<void>;
  scheduleRender(): void;

  status: { level: "ok" | "warn" | "err"; message: string } | null;
  setStatus(s: AppStore["status"]): void;
}

export const useAppStore = create<AppStore>((set, get) => ({
  page: "render",
  setPage: (p) => set({ page: p }),

  templates: [],
  machines: [],
  loadTemplates: async () => {
    try {
      const { templates } = await api.listTemplates();
      set({ templates });
    } catch (e) {
      set({
        status: { level: "err", message: `加载模板失败：${errOf(e).message}` },
      });
    }
  },
  loadMachines: async () => {
    try {
      const { machines } = await api.listMachines();
      set({ machines });
    } catch (e) {
      set({
        status: { level: "err", message: `加载机床失败：${errOf(e).message}` },
      });
    }
  },

  configWarnings: [],
  loadConfig: async () => {
    try {
      const cfg: ConfigDump = await api.getConfig();
      set({ configWarnings: cfg.warnings });
    } catch {
      // 配置摘要仅作提示，失败不打断（窗口须可用）
    }
  },

  selected: null,
  spec: [],
  variables: { required: [], optional: [] },
  selectTemplate: async (name) => {
    set({ selected: name, renderError: null });
    try {
      const { template } = await api.getTemplate(name);
      set({
        spec: template.params,
        variables: template.variables,
        params: initialParams(template.params),
        output: "",
        blocked: false,
        warnings: [],
      });
      // 模板切换立即渲染（不防抖）
      await get().renderNow();
    } catch (e) {
      const err = errOf(e);
      set({
        spec: [],
        params: {},
        renderError: { kind: err.kind, message: err.message },
        status: { level: "err", message: `加载模板失败：${err.message}` },
      });
    }
  },

  params: {},
  setParam: (name, value) => {
    set((s) => ({ params: { ...s.params, [name]: value } }));
    get().scheduleRender();
  },
  resetParams: () => {
    set((s) => ({ params: initialParams(s.spec) }));
    get().scheduleRender();
  },
  applyPreset: async (template, presetParams) => {
    // 绑定模板不同才切换：`selectTemplate` 会重置 params 并立即渲染一次，
    // 与 Web UI 的 `selectTemplate(p.template)` 一致（避免不必要的二次拉取）。
    if (get().selected !== template) {
      await get().selectTemplate(template);
    }
    // **整体覆盖**（不合并模板默认值）：与 Web UI `state.params = {...p.params}` 对齐
    // —— 预设缺省的参数交由模板的 `default()` 兜底，而不是在这里悄悄填默认值。
    set({ params: { ...presetParams } });
    await get().renderNow();
  },

  options: { ...defaultOptions },
  setOption: (k, v) => {
    set((s) => {
      // 显式拷贝 + 索引赋值：泛型键的 `{ ...s.options, [k]: v }` 会被 TS 推断成
      // 索引签名类型，无法赋回 `GenOptions`。
      const options: GenOptions = { ...s.options };
      options[k] = v;
      return { options };
    });
    get().scheduleRender();
  },

  output: "",
  blocked: false,
  warnings: [],
  renderError: null,
  rendering: false,
  renderNow: async () => {
    debouncer.cancel(); // 立即渲染前取消未触发的防抖
    const { selected, params, options } = get();
    if (!selected) return;
    set({ rendering: true });
    try {
      // ★ 硬约束：paramsJson 必须是 JSON.stringify(params) 的结果（保留下溢守卫）
      const paramsJson = JSON.stringify(params);
      const data = await api.renderTemplate(selected, paramsJson, options);
      set({
        output: data.output,
        blocked: data.blocked,
        warnings: data.warnings ?? [],
        renderError: null,
        rendering: false,
      });
    } catch (e) {
      // 渲染失败只更新 renderError，不打断编辑
      const err = errOf(e);
      set({
        output: "",
        blocked: false,
        warnings: [],
        renderError: { kind: err.kind, message: err.message },
        rendering: false,
      });
    }
  },
  scheduleRender: () => {
    debouncer.schedule(() => {
      void get().renderNow();
    });
  },

  status: null,
  setStatus: (s) => set({ status: s }),
}));
