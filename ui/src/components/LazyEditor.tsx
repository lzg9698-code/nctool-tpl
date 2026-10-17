import { Suspense, forwardRef, lazy } from "react";
import type { EditorHandle, EditorProps } from "./Editor";

// CodeMirror is the largest single asset (~140 kB gzip). The welcome screen, the
// template list and form-mode parameter entry never need it, so the real editor
// loads on demand; the fallback keeps the same reserved box to avoid layout jump.
const DeferredEditor = lazy(() =>
  import("./Editor").then((module) => ({ default: module.Editor })),
);
export type { EditorHandle };

export const Editor = forwardRef<EditorHandle, EditorProps>(function Editor(
  props,
  ref,
) {
  return (
    <Suspense
      fallback={<div className="code-editor editor-loading" aria-busy="true" />}
    >
      <DeferredEditor {...props} ref={ref} />
    </Suspense>
  );
});
