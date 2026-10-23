import { lazy, useMemo } from "react";
import { Alert, App as AntApp, Button, ConfigProvider, Spin } from "antd";
import zhCN from "antd/locale/zh_CN";
import { QueryClientProvider, useQuery } from "@tanstack/react-query";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { api, queryClient } from "./lib/api";
import { errorText } from "./lib/json";
import { createSession } from "./state/session";
import { Context } from "./app/context";
import { Shell } from "./app/Shell";
import "./styles.css";

const Workbench = lazy(() => import("./features/workbench/Workbench"));
const ParametersPage = lazy(
  () => import("./features/parameters/ParametersPage"),
);
const HistoryPage = lazy(() => import("./features/history/HistoryPage"));
const PluginsPage = lazy(() => import("./features/plugins/PluginsPage"));
const ActionPage = lazy(() => import("./features/plugins/ActionPage"));
const HelpPage = lazy(() => import("./features/help/HelpPage"));
function Workspace() {
  const startup = useQuery({
    queryKey: ["workspace-bootstrap"],
    queryFn: async ({ signal }) => {
      const [workspace, capabilities] = await Promise.all([
        api("workspace", undefined, signal),
        api("capabilities", undefined, signal),
      ]);
      return { workspace, capabilities };
    },
    staleTime: Infinity,
    retry: 1,
  });
  const workspaceId = startup.data?.workspace.id;
  const store = useMemo(
    () => (workspaceId ? createSession(workspaceId) : null),
    [workspaceId],
  );
  if (startup.isError)
    return (
      <div className="connection-error">
        <Alert
          type="error"
          title="工作区连接失败"
          description={errorText(startup.error)}
          action={<Button onClick={() => startup.refetch()}>重新连接</Button>}
        />
      </div>
    );
  if (!startup.data || !store)
    return (
      <div className="loading">
        <Spin size="large" />
        <p>正在连接本地工作区…</p>
      </div>
    );
  return (
    <Context.Provider
      value={{
        id: workspaceId,
        capabilities: startup.data.capabilities,
        store,
      }}
    >
      <HashRouter>
        <Routes>
          <Route element={<Shell />}>
            <Route index element={<Navigate to="/workbench" replace />} />
            <Route path="workbench/:key?" element={<Workbench />} />
            <Route path="parameters" element={<ParametersPage />} />
            <Route path="history/:runId?" element={<HistoryPage />} />
            <Route path="plugins" element={<PluginsPage />} />
            <Route path="actions/:panelId" element={<ActionPage />} />
            <Route path="help" element={<HelpPage />} />
            <Route path="*" element={<Navigate to="/workbench" replace />} />
          </Route>
        </Routes>
      </HashRouter>
    </Context.Provider>
  );
}
export default function App() {
  return (
    <ConfigProvider
      locale={zhCN}
      componentSize="middle"
      theme={{
        token: {
          screenLGMax: 1099,
          screenXL: 1100,
          screenXLMin: 1100,
          colorPrimary: "#315bea",
          colorInfo: "#315bea",
          borderRadius: 8,
          colorBgLayout: "#f3f5f9",
          colorText: "#182238",
          fontFamily:
            '-apple-system, BlinkMacSystemFont, "Segoe UI", "Microsoft YaHei", sans-serif',
        },
        components: {
          Layout: { headerBg: "#ffffff", siderBg: "#141e32" },
          Menu: { darkItemBg: "#141e32", darkItemSelectedBg: "#315bea" },
          Button: { primaryShadow: "none" },
        },
      }}
    >
      <AntApp>
        <QueryClientProvider client={queryClient}>
          <Workspace />
        </QueryClientProvider>
      </AntApp>
    </ConfigProvider>
  );
}
