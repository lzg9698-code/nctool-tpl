// Composition seam for built-in plugin views. External action panels continue
// through PluginPage; parameters are edited directly on each template.
import type { ComponentType } from "react";
import type { ViewProps } from "./ViewProps";
export const builtinViews: Record<string, ComponentType<ViewProps>> = {};
