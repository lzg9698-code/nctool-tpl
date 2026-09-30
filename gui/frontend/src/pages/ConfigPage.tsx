import { useCallback, useEffect, useState } from "react";
import { api } from "../api/nctool";
import type { ConfigDump } from "../types";

export default function ConfigPage() {
  const [config, setConfig] = useState<ConfigDump | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const refresh = useCallback(async () => {
    setLoading(true);
    try { setConfig(await api.getConfig()); setError(null); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);

  return <div className="page">
    <div className="toolbar"><strong>设置与配置</strong><span className="spacer" />
      <button className="btn" disabled={loading} onClick={() => void refresh()}>{loading ? "读取中…" : "刷新"}</button>
    </div>
    <div className="page-body">
      {error && <div className="nc-error">读取配置失败：{error}</div>}
      {!config && !error && <div className="nc-empty">正在读取层叠配置…</div>}
      {config && <>
        <div className="config-grid">
          <section className="config-card"><h3>生效设置</h3>
            <div className="config-row"><span>模板目录</span><code>{config.templateDir ?? "仅使用内置模板"}</code></div>
            <div className="config-row"><span>默认机床</span><code>{config.defaultMachine ?? "未指定"}</code></div>
            <div className="config-row"><span>自定义机床</span><code>{config.customMachines.length ? config.customMachines.join(", ") : "无"}</code></div>
          </section>
          <section className="config-card"><h3>配置来源</h3>
            <div className="config-row"><span>项目配置</span><code>{config.projectPath ?? "未找到"}</code></div>
            <div className="config-row"><span>全局配置</span><code>{config.globalPath ?? "未找到"}</code></div>
            <p className="muted">配置由全局与项目配置合并生效；项目配置优先。机床条目可在“机床配置”页管理。</p>
          </section>
        </div>
        <h3 className="config-subtitle">配置诊断</h3>
        {config.warnings.length ? config.warnings.map((warning, i) => <div className="config-warning" key={`${i}-${warning}`}>{warning}</div>) : <div className="nc-empty">没有配置降级或解析警告。</div>}
      </>}
    </div>
  </div>;
}
