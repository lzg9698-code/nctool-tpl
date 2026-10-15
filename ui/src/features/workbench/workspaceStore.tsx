import { createContext, useContext, useState, type ReactNode } from "react";
import { createStore, useStore } from "zustand";
import { useShallow } from "zustand/react/shallow";
import type { Data, Document, Run } from "../../lib/data";
import type { Item } from "./model";

type Update<T> = T | ((current: T) => T);
export interface WorkspaceState {
  workspace: Data;
  capabilities: Data;
  inventory: Data;
  items: Item[];
  documents: Document[];
  active: string | null;
  page: string;
  runList: Data[];
  runs: Record<string, Run>;
  setWorkspace: (update: Update<Data>) => void;
  setCapabilities: (update: Update<Data>) => void;
  setInventory: (update: Update<Data>) => void;
  setItems: (update: Update<Item[]>) => void;
  setDocuments: (update: Update<Document[]>) => void;
  setActive: (update: Update<string | null>) => void;
  setPage: (update: Update<string>) => void;
  setRunList: (update: Update<Data[]>) => void;
  setRuns: (update: Update<Record<string, Run>>) => void;
}
export function createWorkspaceStore() {
  return createStore<WorkspaceState>((set) => {
    function setter<K extends keyof WorkspaceState>(key: K) {
      return (update: Update<WorkspaceState[K]>) =>
        set(
          (state) =>
            ({
              [key]:
                typeof update === "function"
                  ? (update as (value: WorkspaceState[K]) => WorkspaceState[K])(
                      state[key],
                    )
                  : update,
            }) as Pick<WorkspaceState, K>,
        );
    }
    return {
      workspace: null,
      capabilities: { actions: [], panels: [] },
      inventory: { plugins: [] },
      items: [],
      documents: [],
      active: null,
      page: "workbench",
      runList: [],
      runs: {},
      setWorkspace: setter("workspace"),
      setCapabilities: setter("capabilities"),
      setInventory: setter("inventory"),
      setItems: setter("items"),
      setDocuments: setter("documents"),
      setActive: setter("active"),
      setPage: setter("page"),
      setRunList: setter("runList"),
      setRuns: setter("runs"),
    };
  });
}
const WorkspaceContext = createContext<ReturnType<
  typeof createWorkspaceStore
> | null>(null);
export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const [store] = useState(createWorkspaceStore);
  return (
    <WorkspaceContext.Provider value={store}>
      {children}
    </WorkspaceContext.Provider>
  );
}
export function useWorkspaceState() {
  const store = useContext(WorkspaceContext);
  if (!store) throw new Error("WorkspaceProvider is required");
  return useStore(
    store,
    useShallow((state) => state),
  );
}
