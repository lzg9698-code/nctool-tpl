import { ChevronRight, FileCode2, Moon, Sun } from "lucide-react";
import { useEffect, useState } from "react";
import type { WorkbenchController } from "./useWorkbenchController";

type Theme = "light" | "dark";
function initialTheme(): Theme {
  try {
    const saved = localStorage.getItem("nctool:theme");
    if (saved === "light" || saved === "dark") return saved;
  } catch {}
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}
export function WorkspaceHeader({
  controller,
}: {
  controller: WorkbenchController;
}) {
  const [theme, setTheme] = useState<Theme>(initialTheme);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.style.colorScheme = theme;
    try {
      localStorage.setItem("nctool:theme", theme);
    } catch {}
  }, [theme]);
  const { page, doc, loading, currentPanel } = controller;
  const title =
    page === "workbench"
      ? "模板工作台"
      : page === "parameters"
        ? "共享参数"
        : page === "history"
          ? "执行历史"
          : page === "plugins"
            ? "工作区设置"
            : page === "syntax-guide"
              ? "模板语法"
              : currentPanel?.title || "插件操作";
  return (
    <header className="workspace-header">
      <div className="workspace-heading">
        <FileCode2 size={18} aria-hidden="true" />
        <span>{title}</span>
        {page === "workbench" && doc && (
          <>
            <ChevronRight size={14} aria-hidden="true" />
            <strong title={doc.title}>{doc.title}</strong>
          </>
        )}
      </div>
      <div className="flex items-center gap-3">
        <span className="workspace-connection">
          {loading ? "正在连接" : "本地工作区"}
        </span>
        <button
          className="theme-toggle"
          aria-label={theme === "light" ? "切换深色主题" : "切换浅色主题"}
          title={theme === "light" ? "切换深色主题" : "切换浅色主题"}
          onClick={() => setTheme(theme === "light" ? "dark" : "light")}
        >
          {theme === "light" ? <Moon size={16} /> : <Sun size={16} />}
        </button>
      </div>
    </header>
  );
}
