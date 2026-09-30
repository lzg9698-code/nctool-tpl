import { useAppStore, type PageId } from "../stores/appStore";

const ITEMS: { id: PageId; icon: string; label: string }[] = [
  { id: "render", icon: "⚡", label: "渲染" },
  { id: "templates", icon: "📋", label: "模板管理" },
  { id: "checks", icon: "🔍", label: "检查" },
  { id: "machine", icon: "⚙", label: "机床配置" },
  { id: "preset", icon: "🗂", label: "参数预设" },
  { id: "config", icon: "🔧", label: "设置" },
];

export default function Sidebar() {
  const page = useAppStore((s) => s.page);
  const setPage = useAppStore((s) => s.setPage);

  return (
    <aside className="sidebar">
      <div className="side-brand">
        <span className="icon">NC</span> nctool
      </div>
      {ITEMS.map((it) => (
        <div
          key={it.id}
          className={`nav-item${page === it.id ? " active" : ""}`}
          onClick={() => setPage(it.id)}
          role="button"
          tabIndex={0}
        >
          <span className="ic">{it.icon}</span> {it.label}
        </div>
      ))}
      <div className="side-footer">
        nctool-core v0.3.0
        <br />
        Tauri 2 · Phase 2
      </div>
    </aside>
  );
}
