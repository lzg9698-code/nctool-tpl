import { Suspense, useEffect, useState } from "react";
import {
  App,
  Alert,
  Badge,
  Button,
  Drawer,
  Empty,
  Grid,
  Input,
  Layout,
  Menu,
  Select,
  Space,
  Spin,
  Tag,
  Typography,
  Upload,
} from "antd";
import {
  BookOutlined,
  CodeOutlined,
  HistoryOutlined,
  MenuOutlined,
  PlusOutlined,
  SettingOutlined,
  SlidersOutlined,
  UploadOutlined,
} from "@ant-design/icons";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { useQueries, useQuery } from "@tanstack/react-query";
import { useWorkspace, useSession } from "./context";
import {
  action,
  catalogQuery,
  queryClient,
  running,
  runQuery,
} from "../lib/api";
import { decode, errorText, type Data } from "../lib/json";
import { dirty, makeDraft } from "../state/session";
import { retired } from "../lib/execution";

export function Shell() {
  const { id, store, capabilities } = useWorkspace(),
    navigate = useNavigate(),
    location = useLocation(),
    { message } = App.useApp();
  const [search, setSearch] = useState(""),
    [tag, setTag] = useState<string>(),
    [drawer, setDrawer] = useState(false);
  const screens = Grid.useBreakpoint(),
    catalog = useQuery(catalogQuery(id)),
    tasks = useSession((state) => state.tasks),
    storageError = useSession((state) => state.storageError);
  const observations = useQueries({
    queries: tasks.map((task) => runQuery(id, task)),
  });
  const taskCount = observations.filter((query) =>
    running(query.data?.status),
  ).length;
  const terminal = observations
    .filter((query) => query.data && !running(query.data.status))
    .map((query) => query.data!.id)
    .join("|");
  useEffect(() => {
    if (!terminal) return;
    const completed = new Set(terminal.split("|"));
    store.setState((state) => ({
      tasks: state.tasks.filter((task) => !completed.has(task)),
    }));
    void queryClient.invalidateQueries({ queryKey: [id, "history"] });
  }, [terminal, store, id]);
  useEffect(() => {
    const flush = (event: BeforeUnloadEvent) => {
      if (
        store.getState().documents.some(dirty) ||
        Object.values(store.getState().auxiliary).some(
          (draft) => draft.raw !== draft.saved,
        )
      ) {
        event.preventDefault();
        event.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", flush);
    return () => window.removeEventListener("beforeunload", flush);
  }, [store]);
  const open = async (item: Data) => {
    const existing = store
      .getState()
      .documents.find((doc) => doc.id === item.id);
    if (existing) {
      navigate("/workbench/" + existing.key);
      setDrawer(false);
      return;
    }
    try {
      const result = await queryClient.fetchQuery({
        queryKey: [id, "template", item.id, item.fingerprint],
        staleTime: 0,
        queryFn: ({ signal }) =>
          action("template.read", { name: item.id }, signal),
      });
      const existingAfterRead = store
        .getState()
        .documents.find((doc) => doc.id === item.id);
      const asset = result.data.asset;
      const matched = (capabilities.document_actions || []).filter(
        (entry: Data) =>
          entry.ui_schema?.document_input?.match_metadata &&
          Object.hasOwn(
            asset.metadata || {},
            entry.ui_schema.document_input.match_metadata,
          ),
      );
      const draft =
        existingAfterRead ||
        makeDraft(
          asset,
          item.id,
          result.data.fingerprint,
          asset.metadata?.execution?.action ||
            (matched.length === 1 ? matched[0].id : undefined),
        );
      store.getState().add(draft);
      navigate("/workbench/" + draft.key);
      setDrawer(false);
    } catch (error) {
      message.error(errorText(error));
    }
  };
  const create = () => {
    const draft = makeDraft();
    store.getState().add(draft);
    navigate("/workbench/" + draft.key);
    setDrawer(false);
  };
  const items: Data[] = catalog.data?.data?.items || [];
  const visible = items.filter(
    (item) =>
      `${item.title} ${item.id} ${item.description} ${(item.tags || []).join(" ")}`
        .toLowerCase()
        .includes(search.toLowerCase()) &&
      (!tag || item.tags?.includes(tag)),
  );
  const nav = [
    ...(capabilities.workbenches || []).map((view) => ({
      key:
        "/engineering/" +
        encodeURIComponent(view.plugin) +
        "/" +
        encodeURIComponent(view.id),
      icon: <CodeOutlined aria-hidden />,
      label: view.title,
    })),
    {
      key: "/workbench",
      icon: <CodeOutlined aria-hidden />,
      label: "模板工作台",
    },
    ...(capabilities.actions.some(
      (entry: Data) => entry.id === "parameter.catalog",
    )
      ? [
          {
            key: "/parameters",
            icon: <SlidersOutlined aria-hidden />,
            label: "参数管理",
          },
        ]
      : []),
    {
      key: "/history",
      icon: <HistoryOutlined aria-hidden />,
      label: "执行记录",
    },
    ...(capabilities.panels || [])
      .filter(
        (panel: Data) =>
          !["template-editor", "parameter.library"].includes(panel.view) &&
          panel.id !== "presets" &&
          !retired(panel.id) &&
          panel.actions?.some((actionId: string) =>
            capabilities.actions.some((entry: Data) => entry.id === actionId),
          ),
      )
      .map((panel: Data) => ({
        key: "/actions/" + encodeURIComponent(panel.id),
        icon: <CodeOutlined aria-hidden />,
        label: panel.title,
      })),
    { key: "/help", icon: <BookOutlined aria-hidden />, label: "模板语法" },
    {
      key: "/plugins",
      icon: <SettingOutlined aria-hidden />,
      label: "插件与设置",
    },
  ];
  const sidebar = (
    <div className="sidebar">
      <div className="brand">
        <div className="brand-symbol">
          <CodeOutlined aria-hidden />
        </div>
        <div>
          <strong>NCtool</strong>
          <small>模板工作台</small>
        </div>
        <Tag bordered={false}>2.2</Tag>
      </div>
      <Menu
        theme="dark"
        mode="inline"
        selectedKeys={[
          nav.find((item) => location.pathname.startsWith(item.key))?.key ||
            "/workbench",
        ]}
        items={nav}
        onClick={({ key }) => {
          navigate(key);
          setDrawer(false);
        }}
      />
      <div className="catalog-heading">
        <span>模板库</span>
        <Button
          aria-label="新建模板"
          type="text"
          size="small"
          icon={<PlusOutlined aria-hidden />}
          onClick={create}
        />
      </div>
      <div className="catalog-filters">
        <Input.Search
          aria-label="搜索模板"
          placeholder="搜索模板、标识或标签"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
        <Select
          aria-label="筛选模板标签"
          allowClear
          placeholder="全部标签"
          value={tag}
          onChange={setTag}
          options={[...new Set(items.flatMap((item) => item.tags || []))].map(
            (value) => ({ value, label: value }),
          )}
        />
      </div>
      <div className="catalog-list" aria-label="模板列表">
        {catalog.isError ? (
          <Alert
            type="error"
            title="模板列表读取失败"
            action={<Button onClick={() => catalog.refetch()}>重试读取</Button>}
          />
        ) : catalog.isPending ? (
          <Spin />
        ) : visible.length ? (
          visible.map((item) => (
            <button
              className="catalog-item"
              key={item.id}
              title={`${item.title}\n${item.id}\n${item.description || ""}`}
              onClick={() => open(item)}
            >
              <CodeOutlined aria-hidden />
              <span>
                <strong>{item.title || item.id}</strong>
                <small>{item.description || item.id}</small>
              </span>
            </button>
          ))
        ) : (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description="暂无匹配模板"
          />
        )}
      </div>
      {(catalog.data?.diagnostics.length ?? 0) > 0 && (
        <Alert type="warning" title="部分模板读取失败，请检查工作区" />
      )}
      <div className="catalog-footer">
        <Typography.Text>
          {visible.length} / {items.length} 个模板
        </Typography.Text>
        <Button type="text" size="small" onClick={() => catalog.refetch()}>
          刷新
        </Button>
      </div>
      <Upload
        accept=".json,.j2,.jinja2,.txt"
        showUploadList={false}
        beforeUpload={async (file) => {
          try {
            if (file.size > 1024 * 1024)
              throw new Error("单个模板不能超过 1 MiB");
            const text = await file.text(),
              asset = file.name.endsWith(".json")
                ? decode(text)
                : { source: text, metadata: { title: file.name } };
            if (typeof asset.source !== "string")
              throw new Error("模板缺少 source 字段");
            const draft = makeDraft(asset);
            store.getState().add(draft);
            navigate("/workbench/" + draft.key);
            setDrawer(false);
            message.info("已打开为新草稿，保存后写入工作区");
          } catch (error) {
            message.error(errorText(error));
          }
          return false;
        }}
      >
        <Button block icon={<UploadOutlined aria-hidden />}>
          导入模板为草稿
        </Button>
      </Upload>
    </div>
  );
  return (
    <Layout className="app-layout">
      {screens.xl ? (
        <Layout.Sider width={264} theme="dark">
          {sidebar}
        </Layout.Sider>
      ) : (
        <Drawer
          title="NCtool"
          placement="left"
          open={drawer}
          onClose={() => setDrawer(false)}
          styles={{ body: { padding: 0, background: "#141e32" } }}
        >
          {sidebar}
        </Drawer>
      )}
      <Layout>
        <Layout.Header className="app-header">
          <Space wrap size={[12, 8]}>
            {!screens.xl && (
              <Button
                aria-label="打开导航"
                icon={<MenuOutlined aria-hidden />}
                onClick={() => setDrawer(true)}
              />
            )}
            <Typography.Text strong>工作空间</Typography.Text>
            <Tag color="success">本地服务已连接</Tag>
          </Space>
          <Space wrap size={[12, 8]}>
            <Badge count={taskCount} size="small">
              <Button
                icon={<HistoryOutlined aria-hidden />}
                onClick={() => navigate("/history")}
              >
                {taskCount ? "任务运行中" : "执行记录"}
              </Button>
            </Badge>
            <Button
              type="primary"
              icon={<PlusOutlined aria-hidden />}
              onClick={create}
            >
              新建模板
            </Button>
          </Space>
        </Layout.Header>
        {storageError && <Alert type="error" title={storageError} />}
        <Layout.Content className="app-content">
          <Suspense
            fallback={
              <div className="loading">
                <Spin tip="正在加载页面…" />
              </div>
            }
          >
            <Outlet />
          </Suspense>
        </Layout.Content>
        <Layout.Footer className="app-footer">
          <span>
            <Badge status="success" />
            工作区 {id.slice(0, 12)}
          </span>
          <span>
            新版草稿独立保存 ·{" "}
            {taskCount ? `${taskCount} 个任务执行中` : "准备就绪"}
          </span>
        </Layout.Footer>
      </Layout>
    </Layout>
  );
}
