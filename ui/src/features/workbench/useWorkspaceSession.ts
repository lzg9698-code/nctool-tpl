import { useEffect, useRef } from "react";
import { api } from "../../lib/api";
import { isDirty, loadDrafts, saveDrafts } from "../../lib/drafts";
import type { WorkbenchController } from "./useWorkbenchController";
import type { WorkspaceState } from "./workspaceStore";
type Session = Pick<
  WorkbenchController,
  | "workspace"
  | "documents"
  | "active"
  | "runs"
  | "doc"
  | "saving"
  | "capabilities"
  | "catalog"
  | "fail"
  | "notify"
  | "saveDocument"
  | "generate"
  | "setInventory"
  | "setActive"
  | "setRecovered"
  | "setRuns"
  | "setRunList"
> &
  Pick<WorkspaceState, "setWorkspace" | "setCapabilities" | "setDocuments"> & {
    setLoading: (value: boolean) => void;
    setSidebarCollapsed: (value: boolean) => void;
    setPaneRatio: (value: { use: number; edit: number }) => void;
    /**
     * True only while a template workbench page is showing. Pages that render
     * their own surfaces (shared parameters, history, plugin workbenches,
     * syntax guide) keep full ownership of their own keys.
     */
    scopedShortcuts: boolean;
  };
// Surfaces that own the key first: fields being typed into stay editable
// shortcuts-free, and an open dialog or menu swallows everything behind it.
const editableSelector =
  'input, textarea, select, [contenteditable=""], [contenteditable="true"]';
function inEditable(node: unknown): boolean {
  return node instanceof Element && !!node.closest(editableSelector);
}
function shortcutBlocked(event: KeyboardEvent): boolean {
  if (document.querySelector('[role="dialog"], [role="menu"]')) return true;
  return inEditable(event.target) || inEditable(document.activeElement);
}
export function useWorkspaceSession({
  workspace,
  documents,
  active,
  runs,
  doc,
  saving,
  capabilities,
  catalog,
  fail,
  notify,
  saveDocument,
  generate,
  setWorkspace,
  setCapabilities,
  setInventory,
  setSidebarCollapsed,
  setPaneRatio,
  setDocuments,
  setActive,
  setRecovered,
  setRuns,
  setRunList,
  setLoading,
  scopedShortcuts,
}: Session) {
  const latest = useRef({ saveDocument, generate });
  latest.current = { saveDocument, generate };
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [w, c, i] = await Promise.all([
          api("workspace"),
          api("capabilities"),
          api("plugins"),
        ]);
        if (cancelled) return;
        setWorkspace(w);
        try {
          setSidebarCollapsed(
            localStorage.getItem("nctool:sidebar:" + w.id) === "collapsed",
          );
          const saved = JSON.parse(
            localStorage.getItem("nctool:layout:" + w.id) || "null",
          );
          if (
            saved &&
            saved.use >= 25 &&
            saved.use <= 70 &&
            saved.edit >= 25 &&
            saved.edit <= 70
          )
            setPaneRatio(saved);
        } catch {}

        setCapabilities(c);
        setInventory(i);
        const drafts = loadDrafts(w.id);
        setDocuments(drafts.documents);
        const visibleDrafts = drafts.documents;
        setActive(
          visibleDrafts.find((d) => d.key === drafts.active)?.key ||
            visibleDrafts[0]?.key ||
            null,
        );
        setRecovered(visibleDrafts.length > 0);
        for (const draft of drafts.documents) {
          if (draft.lastRunId) {
            try {
              const previous = await api("runs/" + draft.lastRunId);
              setRuns((r) => ({ ...r, [previous.id]: previous }));
            } catch {}
          }
        }
        await catalog();
        const records = await api("runs");
        setRunList(records.runs);
      } catch (error) {
        fail(error);
      } finally {
        setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);
  useEffect(() => {
    if (workspace) {
      const timeout = setTimeout(() => {
        if (!saveDrafts(workspace.id, documents, active))
          notify("浏览器草稿存储空间不足，请及时保存模板", true);
      }, 350);
      return () => clearTimeout(timeout);
    }
  }, [workspace, documents, active]);
  useEffect(() => {
    function flushDrafts() {
      if (workspace) saveDrafts(workspace.id, documents, active);
    }
    function before(event: BeforeUnloadEvent) {
      flushDrafts();
      if (documents.some(isDirty)) {
        event.preventDefault();
        event.returnValue = "";
      }
    }
    window.addEventListener("beforeunload", before);
    window.addEventListener("pagehide", flushDrafts);
    return () => {
      window.removeEventListener("beforeunload", before);
      window.removeEventListener("pagehide", flushDrafts);
    };
  }, [documents, workspace, active]);
  useEffect(() => {
    function keys(event: KeyboardEvent) {
      if (!(event.ctrlKey || event.metaKey)) return;
      // Both shortcuts share one scope: template workbench surface, no
      // blocking dialog or menu, focus not inside a text field.
      if (!scopedShortcuts || shortcutBlocked(event)) return;
      const key = event.key.toLowerCase();
      if (key === "s") {
        event.preventDefault();
        if (doc && !saving) void latest.current.saveDocument();
      }
      if (event.key === "Enter") {
        event.preventDefault();
        if (doc) void latest.current.generate();
      }
    }
    window.addEventListener("keydown", keys);
    return () => window.removeEventListener("keydown", keys);
  }, [doc, saving, capabilities, scopedShortcuts]);
  useEffect(() => {
    const pending = Object.values(runs).filter((r) =>
      ["queued", "running"].includes(r.status),
    );
    if (!pending.length) return;
    let cancelled = false;
    const timeout = setTimeout(async () => {
      for (const run of pending) {
        try {
          const fresh = await api("runs/" + run.id);
          if (cancelled) return;
          setRuns((current) => ({ ...current, [fresh.id]: fresh }));
          if (!["queued", "running"].includes(fresh.status)) {
            setRunList((await api("runs")).runs);
            if (fresh.status === "failed")
              notify(fresh.error?.message || "任务失败", true);
          }
        } catch (error) {
          if (!cancelled) fail(error);
        }
      }
    }, 500);
    return () => {
      cancelled = true;
      clearTimeout(timeout);
    };
  }, [runs]);
}
