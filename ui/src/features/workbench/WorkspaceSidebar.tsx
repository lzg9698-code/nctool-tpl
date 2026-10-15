import {
  AlertTriangle,
  BookOpen,
  Boxes,
  Braces,
  FileCode2,
  FileInput,
  FolderOpen,
  History,
  LoaderCircle,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  Search,
  Settings2,
  X,
} from "lucide-react";
import { api } from "../../lib/api";

import type { WorkbenchController } from "./useWorkbenchController";
export function WorkspaceSidebar({
  controller,
}: {
  controller: WorkbenchController;
}) {
  const {
    runList,
    items,
    page,
    setPage,
    query,
    setQuery,
    tag,
    setTag,
    sidebarCollapsed,
    catalogState,
    catalogError,
    imported,
    doc,
    fail,
    catalog,
    toggleSidebar,
    openTemplate,
    create,
    visible,
    tags,
    setRunList,
  } = controller;
  return (
    <aside className="sidebar">
      <button
        className="icon-button sidebar-toggle"
        onClick={toggleSidebar}
        aria-label={sidebarCollapsed ? "展开侧栏" : "折叠侧栏"}
        title={sidebarCollapsed ? "展开侧栏" : "折叠侧栏"}
      >
        {sidebarCollapsed ? (
          <PanelLeftOpen size={18} />
        ) : (
          <PanelLeftClose size={18} />
        )}
      </button>
      <div className="brand">
        <span className="brand-mark">
          <Boxes size={22} />
        </span>
        <div className="brand-name">
          NCtool<span>模板工作台</span>
        </div>
        <span className="version-label">2.2</span>
      </div>
      <div className="workspace-indicator">
        <span className="online-dot" />
        通用模板工作区
      </div>
      <div className="sidebar-section-title">工作空间</div>
      <nav className="sidebar-nav" aria-label="工作空间导航">
        <button
          aria-label={"模板库 · " + items.length + " 个模板"}
          title="模板库"
          className={"nav-item " + (page === "workbench" ? "active" : "")}
          onClick={() => setPage("workbench")}
        >
          <FolderOpen size={17} />
          <span className="nav-label">模板</span>
          <span className="nav-count">{items.length}</span>
        </button>
        <button
          aria-label="共享参数"
          title="共享参数"
          className={"nav-item " + (page === "parameters" ? "active" : "")}
          onClick={() => setPage("parameters")}
        >
          <Braces size={17} />
          <span className="nav-label">共享参数</span>
        </button>
        <button
          aria-label="历史"
          title="历史"
          className={"nav-item " + (page === "history" ? "active" : "")}
          onClick={() => {
            setPage("history");
            api("runs")
              .then((r) => setRunList(r.runs))
              .catch(fail);
          }}
        >
          <History size={17} />
          <span className="nav-label">历史</span>
          <span className="nav-count">{runList.length || ""}</span>
        </button>
        <button
          aria-label="设置"
          title="设置"
          className={"nav-item " + (page === "plugins" ? "active" : "")}
          onClick={() => setPage("plugins")}
        >
          <Settings2 size={17} />
          <span className="nav-label">设置</span>
        </button>
      </nav>
      <div className="sidebar-divider" />
      <div className="sidebar-section-title">
        我的模板
        <button className="icon-button" aria-label="新建模板" onClick={create}>
          <Plus size={15} />
        </button>
      </div>
      <button
        className="nav-item collapsed-create"
        aria-label="新建模板"
        title="新建模板"
        onClick={create}
      >
        <Plus size={18} />
      </button>
      <div className="search-field">
        <Search size={14} />
        <input
          aria-label="搜索模板"
          placeholder="搜索名称、标签…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        {query && (
          <button
            className="icon-button"
            aria-label="清除搜索"
            onClick={() => setQuery("")}
          >
            <X size={12} />
          </button>
        )}
      </div>
      {tags.length > 0 && (
        <div className="tag-filter">
          <select
            aria-label="按标签筛选模板"
            value={tag}
            onChange={(e) => setTag(e.target.value)}
          >
            <option value="">全部标签</option>
            {tags.map((label) => (
              <option key={label} value={label}>
                {label}
              </option>
            ))}
          </select>
        </div>
      )}
      <div className="filter-summary" role="status">
        <span>
          {catalogState === "loading"
            ? "正在读取模板…"
            : `显示 ${visible.length} / ${items.length} 个模板`}
        </span>
        {(query || tag) && (
          <button
            className="text-button"
            onClick={() => {
              setQuery("");
              setTag("");
            }}
          >
            清除筛选
          </button>
        )}
      </div>
      <div className="template-list">
        {catalogError && (
          <div className="catalog-error" role="alert">
            <AlertTriangle size={16} />
            <p>{catalogError}</p>
            <button className="text-button" onClick={() => void catalog()}>
              重新读取
            </button>
          </div>
        )}
        {catalogState === "loading" && !items.length && (
          <div className="sidebar-empty">
            <LoaderCircle size={18} className="spin" /> 正在读取模板
          </div>
        )}
        {visible.map((item) => (
          <button
            key={item.id}
            title={
              item.title + (item.description ? "\n" + item.description : "")
            }
            className={
              "template-item " + (doc?.id === item.id ? "selected" : "")
            }
            onClick={() => void openTemplate(item)}
          >
            <FileCode2 size={16} />
            <span>
              <strong>{item.title}</strong>
              {item.description && <small>{item.description}</small>}
            </span>
          </button>
        ))}
        {!items.length && catalogState === "ready" && (
          <div className="sidebar-empty">
            模板库为空
            <br />
            <button onClick={create}>创建第一个模板</button>
          </div>
        )}
        {items.length > 0 && !visible.length && catalogState !== "error" && (
          <div className="sidebar-empty">没有匹配的模板</div>
        )}
      </div>
      <div className="sidebar-bottom">
        <button
          className="nav-item"
          aria-label="导入模板"
          title="导入模板"
          onClick={() => imported.current?.click()}
        >
          <FileInput size={17} />
          <span className="nav-label">导入模板</span>
        </button>
        <button
          aria-label="模板语法"
          title="模板语法"
          className={"nav-item " + (page === "syntax-guide" ? "active" : "")}
          onClick={() => setPage("syntax-guide")}
        >
          <BookOpen size={17} />
          <span className="nav-label">模板语法</span>
        </button>
      </div>
    </aside>
  );
}
