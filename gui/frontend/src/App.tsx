import { useEffect } from "react";
import Sidebar from "./components/Sidebar";
import ChecksPage from "./pages/ChecksPage";
import ConfigPage from "./pages/ConfigPage";
import MachinePage from "./pages/MachinePage";
import PresetPage from "./pages/PresetPage";
import RenderPage from "./pages/RenderPage";
import TemplatesPage from "./pages/TemplatesPage";
import { useAppStore } from "./stores/appStore";

export default function App() {
  const page = useAppStore((s) => s.page);
  const status = useAppStore((s) => s.status);
  const selected = useAppStore((s) => s.selected);
  const warnings = useAppStore((s) => s.warnings);
  const configWarnings = useAppStore((s) => s.configWarnings);

  useEffect(() => {
    void useAppStore.getState().loadConfig();
  }, []);

  return (
    <div className="app-shell">
      <Sidebar />
      <div className="app-main">
        <div className="app-content">
          {page === "render" && <RenderPage />}
          {page === "templates" && <TemplatesPage />}
          {page === "checks" && <ChecksPage />}
          {page === "machine" && <MachinePage />}
          {page === "preset" && <PresetPage />}
          {page === "config" && <ConfigPage />}
        </div>
        <div className="statusbar">
          {status ? (
            <span className={`sb-item sb-${status.level}`}>● {status.message}</span>
          ) : (
            <span className="sb-item sb-ok">● 就绪</span>
          )}
          <span className="sb-item">模板: {selected ?? "（未选择）"}</span>
          {warnings.length > 0 && (
            <span className="sb-item sb-warn">⚠ 渲染警告 {warnings.length} 条</span>
          )}
          {configWarnings.length > 0 && (
            <span
              className="sb-item sb-warn"
              title={configWarnings.join("\n")}
            >
              ⚠ 配置警告 {configWarnings.length} 条
            </span>
          )}
          <span className="sb-item sb-right">UTF-8 · LF</span>
        </div>
      </div>
    </div>
  );
}
