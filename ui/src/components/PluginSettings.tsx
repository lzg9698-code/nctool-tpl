import { useEffect, useRef, useState } from "react";
import {
  Boxes,
  Save,
  Plus,
  Trash2,
  Check,
  AlertTriangle,
  ShieldCheck,
  FolderOpen,
} from "lucide-react";
import { api } from "../lib/api";
import { clone, type Data } from "../lib/data";
import { Form } from "./Form";
import { PageTitle, JsonEditor } from "./WorkbenchUi";
export function PluginSettings({
  inventory,
  reload,
  notify,
  fail,
}: {
  inventory: Data;
  reload: () => Promise<Data>;
  notify: (text: string, error?: boolean) => void;
  fail: (error: Data) => void;
}) {
  const [config, setConfig] = useState<Data>(null),
    [token, setToken] = useState<Data>(null),
    [validation, setValidation] = useState<Data>(null),
    [problem, setProblem] = useState(""),
    [checking, setChecking] = useState(false),
    [jsonError, setJsonError] = useState(""),
    [source, setSource] = useState(""),
    [inspected, setInspected] = useState<Data>(null),
    [inspectedSource, setInspectedSource] = useState(""),
    [installing, setInstalling] = useState(false),
    [saving, setSaving] = useState(false);
  // Gate asynchronous results on the request they belong to: bumping the
  // revision (on every source edit) makes any in-flight inspect stale, so a
  // late reply for a previously typed directory is never shown or installed.
  const inspectRevision = useRef(0);
  // Tracks local draft edits so a save that races with editing keeps the newer
  // draft instead of overwriting it with the reloaded (older) config.
  const editRevision = useRef(0);
  useEffect(() => {
    void refreshConfig();
  }, []);
  async function refreshConfig(applyConfig = true) {
    try {
      const saved = await api("config");
      // Keep fingerprint semantics even when the draft changed mid-save: the
      // token still tracks the server's stored configuration, but a newer local
      // draft must not be clobbered by the reloaded (pre-edit) config.
      if (applyConfig) setConfig(saved.config);
      setToken(saved.fingerprint);
    } catch (e) {
      fail(e);
    }
  }
  function change(next: Data) {
    editRevision.current += 1;
    setConfig(next);
    setValidation(null);
    setProblem("");
  }
  async function validate(next = config) {
    if (jsonError) {
      setProblem("请先修复启动配置 JSON：" + jsonError);
      return null;
    }
    setChecking(true);
    setProblem("");
    try {
      const result = await api("config/validate", { config: next });
      setValidation(result);
      return result;
    } catch (e) {
      setValidation(null);
      setProblem((e as Error).message);
      return null;
    } finally {
      setChecking(false);
    }
  }
  async function save(next = config) {
    if (saving) return;
    setSaving(true);
    const draftRevision = editRevision.current;
    try {
      const checked = await validate(next);
      if (!checked) return;
      await api("config", { config: next, expected: token });
      await refreshConfig(editRevision.current === draftRevision);
      await reload();
      notify("配置已验证并保存，下次启动生效");
    } catch (e) {
      fail(e);
    } finally {
      setSaving(false);
    }
  }
  async function toggle(id: string, enabled: boolean) {
    const next = clone(config);
    next.enabled = next.enabled.filter((value: string) => value !== id);
    next.disabled = next.disabled.filter((value: string) => value !== id);
    if (enabled) next.enabled.push(id);
    else next.disabled.push(id);
    change(next);
    await validate(next);
  }
  return (
    <div className="page-content">
      <PageTitle
        title="插件与设置"
        description="先检查依赖、接口版本和参数规格，再保存启动配置。预检查不会执行插件程序。"
      />
      {inventory.restart_required && (
        <div className="stale-banner">
          启动配置已有变化，当前工作区仍使用启动时的能力。关闭并重新启动服务后生效。
        </div>
      )}
      <div className="settings-grid">
        <section className="panel">
          <div className="panel-header">
            <strong>已安装的能力</strong>
          </div>
          {config &&
            inventory.plugins.filter((plugin: Data) => !plugin.retired).map((plugin: Data) => {
              const id = plugin.id;
              const enabled =
                !config.disabled.includes(id) &&
                (id === "template" ||
                  config.enabled.includes(id) ||
                  (id === "parameters" && !config.disabled.includes("template")));
              return (
                <div className="plugin-config-card" key={id}>
                  <div className="plugin-card">
                    <div className="plugin-avatar">
                      <Boxes size={22} />
                    </div>
                    <div>
                      <strong>
                        {(
                          {
                            template: "通用模板",
                            parameters: "共享参数",
                            math: "数学扩展",
                          } as Record<string, string>
                        )[id] || id}
                      </strong>
                      <p>
                        {plugin.active
                          ? "正在运行"
                          : plugin.available
                            ? "已安装，尚未运行"
                            : "此构建未包含"}{" "}
                        · 配置{enabled ? "启用" : "停用"}
                      </p>
                    </div>
                    {plugin.available && (
                      <button
                        className="button secondary"
                        onClick={() => void toggle(id, !enabled)}
                      >
                        {enabled ? "设为停用" : "设为启用"}
                      </button>
                    )}
                  </div>
                  <div className="plugin-dependencies">
                    {plugin.descriptor?.requires?.length > 0 && (
                      <span>
                        依赖：
                        {plugin.descriptor.requires
                          .map(
                            (requirement: Data) =>
                              requirement.id + " @" + requirement.version,
                          )
                          .join("、")}
                      </span>
                    )}
                    {!plugin.builtin && !plugin.active && (
                      <button
                        className="text-button danger"
                        onClick={async () => {
                          if (!confirm("卸载这个已停用的插件？")) return;
                          try {
                            await api("plugins/uninstall", { id });
                            await reload();
                            await refreshConfig();
                            notify("插件已卸载");
                          } catch (e) {
                            fail(e);
                          }
                        }}
                      >
                        <Trash2 size={12} />
                        卸载
                      </button>
                    )}
                  </div>
                  {plugin.descriptor?.config_schema?.properties && (
                    <details className="plugin-fields">
                      <summary>插件配置</summary>
                      <Form
                        schema={plugin.descriptor.config_schema}
                        value={config.plugins[id] || {}}
                        onChange={(value) =>
                          change({
                            ...config,
                            plugins: { ...config.plugins, [id]: value },
                          })
                        }
                      />
                    </details>
                  )}
                </div>
              );
            })}
        </section>
        <section className="panel">
          <div className="panel-header">
            <strong>启动组合与预检查</strong>
          </div>
          <div className="panel-content">
            {config && (
              <>
                <label>
                  默认功能
                  <select
                    aria-label="默认功能"
                    value={config.profile}
                    onChange={(e) =>
                      change({ ...config, profile: e.target.value })
                    }
                  >
                    <option value="template">通用模板工作区</option>
                  </select>
                </label>
                <p className="hint">
                  通用模板工作区启用通用模板与共享参数。旧 NC
                  组合已移除；启动配置若仍指向它，配置检查会给出兼容错误，需要显式迁移。
                </p>
                <details className="advanced">
                  <summary>服务提供方和高级配置</summary>
                  <div className="plugin-json">
                    <JsonEditor
                      value={config}
                      onChange={change}
                      label="启动配置 JSON"
                      onError={setJsonError}
                    />
                  </div>
                </details>
                {problem && (
                  <div className="inline-error" role="alert">
                    <AlertTriangle size={14} />
                    {problem}
                  </div>
                )}
                {validation && (
                  <div className="validation-result" role="status">
                    <ShieldCheck size={16} />
                    <div>
                      <strong>配置检查通过</strong>
                      <p>
                        启动顺序：
                        {validation.plan.order.join(" → ") || "空组合"}
                      </p>
                    </div>
                  </div>
                )}
                <div className="settings-actions">
                  <button
                    className="button secondary"
                    onClick={() => void validate()}
                    disabled={checking || saving}
                  >
                    <Check size={14} />
                    {checking ? "检查中" : "检查配置"}
                  </button>
                  <button
                    className="button primary"
                    onClick={() => void save()}
                    disabled={checking || saving}
                  >
                    <Save size={14} />
                    验证并保存配置
                  </button>
                </div>
              </>
            )}
          </div>
        </section>
        <section className="panel plugin-install">
          <div className="panel-header">
            <strong>安装本地插件</strong>
          </div>
          <div className="panel-content">
            <label>
              插件目录
              <input
                aria-label="插件目录"
                value={source}
                onChange={(e) => {
                  inspectRevision.current += 1;
                  setSource(e.target.value);
                  setInspected(null);
                  setInspectedSource("");
                }}
                placeholder="包含 plugin.json 的本地目录"
              />
            </label>
            <p className="hint">
              目录使用本地服务所在机器的路径。安装仅验证和复制文件，不自动启用，也不执行入口程序。
            </p>
            <button
              className="button secondary"
              onClick={async () => {
                const requested = source;
                const revision = inspectRevision.current;
                try {
                  const result = await api("plugins/inspect", {
                    source: requested,
                  });
                  if (revision !== inspectRevision.current) return;
                  setInspected(result);
                  setInspectedSource(requested);
                  notify("插件描述已读取");
                } catch (e) {
                  if (revision === inspectRevision.current) fail(e);
                }
              }}
            >
              <FolderOpen size={14} />
              读取插件信息
            </button>
            {inspected && (
              <div className="plugin-preview">
                <strong>
                  {inspected.descriptor.id} · {inspected.descriptor.version}
                </strong>
                <p>
                  动作：
                  {inspected.actions
                    .map((entry: Data) => entry.title)
                    .join("、")}
                </p>
                <p>
                  依赖：
                  {inspected.descriptor.requires
                    .map((requirement: Data) => requirement.id)
                    .join("、") || "无"}
                </p>
                <button
                  className="button primary"
                  disabled={installing}
                  onClick={async () => {
                    setInstalling(true);
                    try {
                      await api("plugins/install", { source: inspectedSource });
                      setInspected(null);
                      setInspectedSource("");
                      setSource("");
                      await reload();
                      notify("插件已安装，请检查并保存启用配置");
                    } catch (e) {
                      fail(e);
                    } finally {
                      setInstalling(false);
                    }
                  }}
                >
                  <Plus size={14} />
                  安装插件
                </button>
              </div>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}
