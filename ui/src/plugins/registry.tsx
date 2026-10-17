// Composition seam: the shell selects only registered contribution keys.
// Retired machining workbenches are no longer registered; only the generic
// shared parameter library remains a built-in view.
import { lazy, type ComponentType } from "react";
import type { ViewProps } from "./ViewProps";
export const builtinViews: Record<string, ComponentType<ViewProps>> = {
  "parameter.library": lazy(() =>
    import("./ParameterViews").then((module) => ({
      default: module.ParameterLibrary,
    })),
  ),
};
