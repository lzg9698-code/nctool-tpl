import { TurningWorkbench } from "./TurningWorkbench";
import type { ViewProps } from "./NcViews";
export function SyncTurningView(props: ViewProps) { return <TurningWorkbench {...props} kind="sync-turning" />; }
