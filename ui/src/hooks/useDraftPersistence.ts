import { useEffect, useRef } from "react";
import { isDirty, type Document } from "../lib/data";
import { saveDrafts } from "../lib/drafts";

export function useDraftPersistence(
  workspaceId: string | undefined,
  documents: Document[],
  active: string | null,
  onStorageFailure: () => void,
) {
  const latest = useRef({ workspaceId, documents, active, onStorageFailure });
  latest.current = { workspaceId, documents, active, onStorageFailure };

  useEffect(() => {
    if (!workspaceId) return;
    const timeout = window.setTimeout(() => {
      const current = latest.current;
      if (
        current.workspaceId &&
        !saveDrafts(current.workspaceId, current.documents, current.active)
      )
        current.onStorageFailure();
    }, 350);
    return () => window.clearTimeout(timeout);
  }, [workspaceId, documents, active]);

  useEffect(() => {
    function flushDrafts() {
      const current = latest.current;
      if (current.workspaceId)
        saveDrafts(current.workspaceId, current.documents, current.active);
    }
    function before(event: BeforeUnloadEvent) {
      flushDrafts();
      if (latest.current.documents.some(isDirty)) {
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
  }, []);
}
