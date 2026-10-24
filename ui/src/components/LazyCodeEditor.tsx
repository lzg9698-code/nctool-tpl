import { lazy, Suspense, type ComponentProps } from "react";
import { Spin } from "antd";
const Editor = lazy(() =>
  import("./CodeEditor").then((module) => ({ default: module.CodeEditor })),
);
export function CodeEditor(
  props: ComponentProps<typeof import("./CodeEditor").CodeEditor>,
) {
  return (
    <Suspense
      fallback={
        <div className="code-editor loading-editor">
          <Spin tip="正在加载编辑器…" />
        </div>
      }
    >
      <Editor {...props} />
    </Suspense>
  );
}
