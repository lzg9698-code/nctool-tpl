import type { Data, Run } from "../lib/data";

export interface ViewProps {
  panel: Data;
  capabilities: Data;
  start: (id: string, input: Data, title: string) => Promise<string | null>;
  runs: Record<string, Run>;
  fail: (error: Data) => void;
  workspaceId: string;
}
