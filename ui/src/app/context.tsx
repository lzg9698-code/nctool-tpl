import { createContext, useContext } from "react";
import { useStore } from "zustand";
import { useMutation } from "@tanstack/react-query";
import { api, queryClient } from "../lib/api";
import { type Data } from "../lib/json";
import type { Session, SessionStore } from "../state/session";

export interface WorkspaceContext {
  id: string;
  capabilities: Data;
  store: SessionStore;
}
export const Context = createContext<WorkspaceContext | null>(null);
export function useWorkspace(): WorkspaceContext {
  const context = useContext(Context);
  if (!context) throw new Error("工作区尚未连接");
  return context;
}
export function useSession<T>(selector: (state: Session) => T): T {
  return useStore(useWorkspace().store, selector);
}
export function useStartRun() {
  const { id, store } = useWorkspace();
  return useMutation({
    mutationFn: async ({
      action,
      input,
      title,
    }: {
      action: string;
      input: Data;
      title: string;
    }) => {
      const created = await api("runs", { action, input, title });
      queryClient.setQueryData([id, "run", created.id], {
        id: created.id,
        action,
        input,
        title,
        status: "queued",
      });
      store.getState().track(created.id);
      await queryClient.invalidateQueries({ queryKey: [id, "history"] });
      return created.id as string;
    },
  });
}
