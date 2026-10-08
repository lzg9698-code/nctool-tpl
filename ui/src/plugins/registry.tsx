// Composition seam: the shell selects only registered contribution keys.
import { lazy, type ComponentType } from "react";
import type { ViewProps } from "./NcViews";
export const builtinViews: Record<string, ComponentType<ViewProps>> = {
  "sync-turning.workbench": lazy(() => import("./SyncTurningView").then(module => ({ default: module.SyncTurningView }))),
  "undercut.workbench": lazy(() => import("./UndercutView").then(module => ({ default: module.UndercutView }))),
  "parameter.library": lazy(() => import("./ParameterViews").then(module => ({ default: module.ParameterLibrary }))),
  "nc.parameters": lazy(() => import("./ParameterViews").then(module => ({ default: module.ParameterLibrary }))),
  "nc.managed-process": lazy(() => import("./ParameterViews").then(module => ({ default: module.ManagedProcess }))),
  "nc.generate": lazy(() =>
    import("./NcViews").then((module) => ({ default: module.NcGenerate })),
  ),
  "nc.machines": lazy(() =>
    import("./NcViews").then((module) => ({ default: module.Machines })),
  ),
  "nc.process": lazy(() =>
    import("./NcViews").then((module) => ({ default: module.ProcessEditor })),
  ),
};
