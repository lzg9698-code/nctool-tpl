// Composition seam: the shell selects only registered contribution keys.
import { lazy, type ComponentType } from "react";
import type { ViewProps } from "./NcViews";
export const builtinViews: Record<string, ComponentType<ViewProps>> = {
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
