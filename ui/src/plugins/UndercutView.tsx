import { TurningWorkbench } from "./TurningWorkbench";
import type { ViewProps } from "./NcViews";
export function UndercutView(props: ViewProps) { return <TurningWorkbench {...props} kind="undercut" />; }
