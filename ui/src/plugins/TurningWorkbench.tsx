import { ap2Conditions, fillSyncAp2 } from "../lib/syncAp2";
import { ChoiceTabs } from "../components/ChoiceTabs";
import { TrajectoryPresetEditor } from "../components/TrajectoryPresetEditor";
import { SpecializedTemplateEditor } from "../components/SpecializedTemplateEditor";
import { loadTemplateCatalog } from "../lib/templateCatalog";
import { useEffect, useRef, useState } from "react";
import { ArrowUp, Copy, Trash2 } from "lucide-react";
import { api, action } from "../lib/api";
import {
  DraftNumber,
  download,
  encode,
  numberText,
  parseData,
  safeNumber,
  type Data,
} from "../lib/data";
import { cloneDraft, decodeDraft, encodeDraft } from "../lib/drafts";
import { useManagedDependencies } from "../lib/managedDependencies";
import {
  changeGrooveType,
  emptyRow,
  importTurning,
  initialTurning,
  turningRequest,
  type TurningKind,
} from "../lib/turningModel";
import { PageTitle, RunDetail } from "../components/WorkbenchUi";
import type { ViewProps } from "./NcViews";
const labels: Record<string, string> = {
  sta_x: "起始 X",
  sta_z: "起始 Z",
  end_x: "结束 X",
  end_z: "结束 Z",
  ap_2: "ap2",
  d1: "D1",
  z: "Z",
  d2: "D2",
  c: "C",
  angle: "角度",
  clearance_x: "X 接近余量 (mm)",
  retract_extra: "X 退刀余量 (mm)",
  left_entry_angle: "左侧耦合进刀角 (°)",
  left_entry_feed: "左侧进刀进给",
  left_retract_feed: "左侧退刀进给",
  right_entry_feed: "右侧进刀进给",
  right_retract_feed: "右侧退刀进给",
  approach_extra: "接近附加余量 (mm)",
  cut_feed: "切削进给",
  fs_exit_extension: "FS 退出延长 (mm)",
};
function Numeric({
  label,
  value,
  change,
}: {
  label: string;
  value: Data;
  change: (v: Data) => void;
}) {
  return (
    <label>
      {label}
      <input
        aria-label={label}
        inputMode="decimal"
        value={numberText(value)}
        onChange={(e) =>
          change(
            e.target.value === "" ? undefined : new DraftNumber(e.target.value),
          )
        }
      />
    </label>
  );
}
function PathPreview({ previews }: { previews: Data[] }) {
  const [zoom, setZoom] = useState(1);
  const points = previews.flatMap((p) => p.points || []);
  if (!points.length) return <p className="hint">填入原始参数后计算预览。</p>;
  const xs = points.map((p) => safeNumber(p.z)),
    ys = points.map((p) => safeNumber(p.x));
  const minX = Math.min(...xs),
    minY = Math.min(...ys),
    dx = Math.max(1, Math.max(...xs) - minX),
    dy = Math.max(1, Math.max(...ys) - minY);
  const width = 520,
    height = 250,
    scale = Math.min(460 / dx, 190 / dy);
  return (
    <div className="turning-path">
      <div className="toolbar">
        <button onClick={() => setZoom((z) => Math.min(4, z * 1.25))}>
          放大
        </button>
        <button onClick={() => setZoom((z) => Math.max(0.25, z / 1.25))}>
          缩小
        </button>
        <button onClick={() => setZoom(1)}>适应范围</button>
      </div>
      <svg
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label="轨迹坐标示意"
      >
        <g transform={`translate(260 125) scale(${zoom}) translate(-260 -125)`}>
          {previews.map((p, i) => (
            <polyline
              key={i}
              fill="none"
              stroke={i % 2 ? "#bd741d" : "#157267"}
              strokeWidth="2"
              points={(p.points || [])
                .map(
                  (v: Data) =>
                    `${30 + (safeNumber(v.z) - minX) * scale},${220 - (safeNumber(v.x) - minY) * scale}`,
                )
                .join(" ")}
            />
          ))}
        </g>
        <text x="12" y="18">
          X 直径 / Z，mm
        </text>
      </svg>
      <p className="hint">{previews[0].label}</p>
    </div>
  );
}
function FragmentDiff({ before, after }: { before: string; after: string }) {
  const oldLines = before.split("\n"),
    newLines = after.split("\n");
  const changes = Array.from(
    { length: Math.max(oldLines.length, newLines.length) },
    (_, i) => ({
      line: i + 1,
      before: oldLines[i] || "",
      after: newLines[i] || "",
    }),
  ).filter((r) => r.before !== r.after);
  return (
    <details>
      <summary>轨迹片段变化：{changes.length} 行（后处理前）</summary>
      <pre>
        {changes
          .slice(0, 200)
          .map((r) => `${r.line} - ${r.before}\n  + ${r.after}`)
          .join("\n") || "与上次生成的轨迹片段一致"}
        {changes.length > 200 && "\n其余变化请检查完整程序。"}
      </pre>
    </details>
  );
}
function StandardSection({
  dimensions,
  type,
}: {
  dimensions: Data;
  type: string;
}) {
  if (!dimensions) return null;
  return (
    <div className="turning-standard-section">
      <svg viewBox="0 0 300 95" role="img" aria-label={type + "标准截面示意"}>
        <path
          d={
            type === "ES"
              ? "M10 30 H95 L115 65 Q125 80 140 70 H190 L220 30 H285"
              : "M10 30 H95 L115 65 Q125 80 140 70 L165 30 H285"
          }
          fill="none"
          stroke="#157267"
          strokeWidth="3"
        />
        <text x="12" y="16">
          {type} 标准截面示意（非比例）
        </text>
        <text x="20" y="92">
          r={numberText(dimensions.r)} / t1={numberText(dimensions.t1)} / f=
          {numberText(dimensions.f)} mm
        </text>
      </svg>
    </div>
  );
}
export function TurningWorkbench(props: ViewProps & { kind: TurningKind }) {
  const { kind } = props,
    sync = kind === "sync-turning",
    title = sync ? "同步粗车" : "越程槽";
  const storageKey = `nctool:turning:${props.workspaceId}:${kind}`;
  const [draft, setDraft] = useState<Data>(() => {
    try {
      return decodeDraft(localStorage.getItem(storageKey) || "");
    } catch {
      return initialTurning(kind);
    }
  });
  const [standardAssets, setStandardAssets] = useState<Data[]>([]);
  const [templates, setTemplates] = useState<Data[]>([]);
  const [description, setDescription] = useState<Data>(),
    [tables, setTables] = useState<Data>({}),
    [presets, setPresets] = useState<Data[]>([]),
    [sessions, setSessions] = useState<Data[]>([]),
    [machines, setMachines] = useState<string[]>([]);
  const [preview, setPreview] = useState<Data>({}),
    [message, setMessage] = useState(""),
    [pending, setPending] = useState(false),
    [recommendations, setRecommendations] = useState<Data>({});
  const [sideRuns, setSideRuns] = useState<Data>({});
  const [runId, setRunId] = useState<string | null>(null),
    [runSignature, setRunSignature] = useState(""),
    [runTarget, setRunTarget] = useState("all"),
    [modes, setModes] = useState<Data>({ right: "code", left: "code" }),
    [channels, setChannels] = useState<Data>({ right: "0", left: "0" }),
    [activeSide, setActiveSide] = useState("right");
  const [sessionId, setSessionId] = useState(""),
    [sessionExpected, setSessionExpected] = useState<string | null>(null),
    [sessionName, setSessionName] = useState("");
  const [imported, setImported] = useState<Data>();
  const current = useRef(draft);
  current.current = draft;
  const serial = (d: Data) => {
    try {
      return encode(turningRequest(d, kind, "all"));
    } catch {
      return "invalid:" + encodeDraft(d);
    }
  };
  const sideSignature = (value: Data, side: string) => {
    try {
      return encode(turningRequest(value, kind, side));
    } catch {
      return "invalid:" + encodeDraft(value);
    }
  };
  const sideDependencies = {
    right: useManagedDependencies(props.runs[sideRuns.right?.id]),
    left: useManagedDependencies(props.runs[sideRuns.left?.id]),
  };
  const signature = serial(draft),
    run = runId ? props.runs[runId] : undefined;
  const dependency = useManagedDependencies(run);
  const stale =
    runSignature !==
      (runTarget === "all" ? signature : sideSignature(draft, runTarget)) ||
    dependency.stale;
  function previewSideSignature(value: Data, side: string) {
    const next = cloneDraft(value);
    if (next.sides[side]?.trajectory)
      delete next.sides[side].trajectory.confirmed_for;
    return sideSignature(next, side);
  }
  const prepared = {
    sides: Object.fromEntries(
      ["right", "left"].map((side) => {
        const record = sideRuns[side],
          sideRun = props.runs[record?.id];
        const valid =
          preview[side]?.signature === previewSideSignature(draft, side);
        const currentRun =
          record?.signature === sideSignature(draft, side) &&
          !sideDependencies[side as "right" | "left"].stale;
        return [
          side,
          valid
            ? preview[side].data
            : currentRun && sideRun?.status === "succeeded"
              ? sideRun.result?.data.prepared.sides[side]
              : undefined,
        ];
      }),
    ),
  };
  function patch(next: Data) {
    setDraft(next);
    current.current = next;
    localStorage.setItem(storageKey, encodeDraft(next));
    setMessage("");
  }
  function changeSide(side: string, changes: Data) {
    const next = cloneDraft(draft);
    Object.assign(next.sides[side], changes);
    patch(next);
  }
  function changeRow(side: string, index: number, changes: Data) {
    const rows = cloneDraft(draft.sides[side].rows);
    Object.assign(rows[index], changes);
    changeSide(side, { rows });
  }
  async function refreshAssets() {
    const [p, s] = await Promise.all([
      action(kind + ".asset.catalog", { group: "trajectories" }),
      action(kind + ".asset.catalog", { group: "sessions" }),
    ]);
    setPresets(p.data.items);
    setSessions(s.data.items);
  }
  async function presetSaved(asset: Data, fingerprint: string) {
    setPresets((previous) => [
      ...previous.filter((p) => p.id !== asset.id),
      { id: asset.id, title: asset.title, asset, fingerprint },
    ]);
    const affected = ["right", "left"].filter(
      (side) => current.current.sides[side].trajectory?.preset_id === asset.id,
    );
    invalidateSides(affected);
    await refreshAssets();
  }
  function invalidateSides(affected: string[]) {
    const next = cloneDraft(current.current);
    for (const side of affected)
      if (next.sides[side].trajectory)
        delete next.sides[side].trajectory.confirmed_for;
    if (affected.length) {
      patch(next);
      setPreview((previous: Data) =>
        Object.fromEntries(
          Object.entries(previous).filter(([side]) => !affected.includes(side)),
        ),
      );
      setSideRuns((previous: Data) =>
        Object.fromEntries(
          Object.entries(previous).map(([side, record]: [string, Data]) => [
            side,
            affected.includes(side) ? { ...record, signature: "" } : record,
          ]),
        ),
      );
      if (runTarget === "all" || affected.includes(runTarget))
        setRunSignature("");
    }
  }
  async function templateSaved(id: string, asset: Data, fingerprint: string) {
    setTemplates((previous) => [
      ...previous.filter((t) => t.id !== id),
      { id, title: asset.metadata?.title || id, asset, fingerprint },
    ]);
    invalidateSides(
      ["right", "left"].filter(
        (side) => current.current.sides[side].template === id,
      ),
    );
    const catalog = await loadTemplateCatalog();
    setTemplates(catalog.items);
  }
  useEffect(() => {
    let alive = true;
    void (async () => {
      const d = await action(kind + ".describe", {});
      if (!alive) return;
      setDescription(d.data);
      await refreshAssets();
      const catalog = await loadTemplateCatalog();
      if (alive) {
        setTemplates(catalog.items);
        if (catalog.diagnostics.length)
          setMessage("部分模板格式读取失败，请检查资产后重新打开页面。");
      }
      const m = await action("machine.list", {});
      if (alive) setMachines(m.data.names);
      if (!sync) {
        const s = await action("undercut.standards", {});
        if (alive) setTables(s.data.standard.tables);
      }
    })().catch((e) => alive && props.fail(e));
    return () => {
      alive = false;
    };
  }, [kind]);
  useEffect(() => {
    if (sync) return;
    let alive = true;
    void (async () => {
      const catalog = await action("nc.parameter.catalog", {
        group: "standards",
      });
      const response = await action(
        "undercut.standards",
        draft.standard_id ? { standard_id: draft.standard_id } : {},
      );
      if (alive) {
        setStandardAssets(catalog.data.items);
        setTables(response.data.standard.tables);
      }
    })().catch((e) => alive && props.fail(e));
    return () => {
      alive = false;
    };
  }, [sync, draft.standard_id]);
  async function saveCurrentPreset(side: string) {
    try {
      const selected = presets.find(
        (p) => p.id === draft.sides[side].trajectory?.preset_id,
      );
      if (!selected) throw new Error("内置轨迹请另存为新预设");
      const asset = prepared.sides[side].trajectory;
      const saved = await action(kind + ".asset.save", {
        group: "trajectories",
        name: asset.id,
        asset,
        expected: selected.fingerprint,
      });
      await presetSaved(asset, saved.data.fingerprint);
      const next = cloneDraft(current.current);
      next.sides[side].trajectory = { preset_id: asset.id };
      patch(next);
      setMessage("轨迹预设已保存；原结果失效。");
    } catch (e) {
      setMessage("保存失败，草稿已保留：" + (e as Error).message);
      props.fail(e);
    }
  }
  async function completeAp2(target: string, replace = false) {
    const captured = cloneDraft(current.current);
    if (!sync) return { draft: captured, failures: [], filled: 0 };
    const result = await fillSyncAp2(
      captured,
      target,
      async (input) => (await action("sync-turning.recommend", input)).data,
      replace,
    );
    const sides = target === "all" ? ["right", "left"] : [target];
    if (
      sides.some(
        (side) =>
          sideSignature(captured, side) !==
          sideSignature(current.current, side),
      )
    )
      throw new Error("输入已修改，请重新计算 ap2；本次推荐未覆盖草稿。");
    const next = cloneDraft(current.current);
    for (const side of sides) next.sides[side] = result.draft.sides[side];
    if (result.filled) patch(next);
    setRecommendations((previous: Data) => {
      const updated = { ...previous };
      for (const side of sides)
        updated[side] = {
          rows: { ...previous[side]?.rows, ...result.recommendations[side] },
        };
      return updated;
    });
    return { ...result, draft: next };
  }
  function ap2Failure(
    failures: { side: string; index: number; message: string }[],
  ) {
    return failures
      .map(
        (f) =>
          `${f.side === "right" ? "右侧" : "左侧"}第${f.index + 1}行：${f.message}`,
      )
      .join("；");
  }
  async function calculate(target = "all") {
    let captured = cloneDraft(draft);
    setPending(true);
    const sides = target === "all" ? ["right", "left"] : [target];
    try {
      const completed = await completeAp2(target);
      captured = completed.draft;
      if (completed.failures.length)
        throw new Error(ap2Failure(completed.failures));
      const result = await action(
        kind + ".prepare",
        turningRequest(captured, kind, target),
      );
      setPreview((previous: Data) => {
        const next = { ...previous };
        for (const side of sides)
          if (
            previewSideSignature(captured, side) ===
            previewSideSignature(current.current, side)
          )
            next[side] = {
              data: result.data.sides[side],
              standard: result.data.standard,
              signature: previewSideSignature(captured, side),
            };
        return next;
      });
      setMessage("计算完成；预览不是交付文件。轨迹调整需确认后生成。");
    } catch (e) {
      setPreview((previous: Data) => {
        const next = { ...previous };
        for (const side of sides)
          if (
            previewSideSignature(captured, side) ===
            previewSideSignature(current.current, side)
          )
            delete next[side];
        return next;
      });
      setMessage((e as Error).message);
      props.fail(e);
    } finally {
      setPending(false);
    }
  }
  async function generate(target: string) {
    setPending(true);
    try {
      const completed = await completeAp2(target);
      if (completed.failures.length)
        throw new Error(ap2Failure(completed.failures));
      const captured = completed.draft,
        capturedSignature =
          target === "all" ? serial(captured) : sideSignature(captured, target);
      const id = await props.start(
        kind + ".generate",
        turningRequest(captured, kind, target),
        title +
          " · " +
          (target === "all" ? "全部" : target === "right" ? "右侧" : "左侧"),
      );
      setRunSignature(capturedSignature);
      setRunTarget(target);
      setRunId(id);
      setSideRuns((previous: Data) => ({
        ...previous,
        ...Object.fromEntries(
          (target === "all" ? ["right", "left"] : [target]).map((side) => [
            side,
            { id, signature: sideSignature(captured, side) },
          ]),
        ),
      }));
    } catch (e) {
      props.fail(e);
    } finally {
      setPending(false);
    }
  }
  async function recommend(side: string) {
    setPending(true);
    try {
      const result = await completeAp2(side, true);
      setMessage(
        `已自动填入 ${result.filled} 行 ap2。` +
          (result.failures.length
            ? ap2Failure(result.failures)
            : "可继续计算或生成。"),
      );
    } catch (e) {
      setMessage((e as Error).message);
      props.fail(e);
    } finally {
      setPending(false);
    }
  }
  async function saveSession() {
    try {
      const asset = {
        format_version: 1,
        id: sessionId,
        title: sessionName || sessionId,
        input: turningRequest(draft, kind, "all"),
      };
      const r = await action(kind + ".asset.save", {
        group: "sessions",
        name: sessionId,
        asset,
        expected: sessionExpected,
      });
      setSessionExpected(r.data.fingerprint);
      setMessage("会话已保存");
      await refreshAssets();
    } catch (e) {
      setMessage("保存失败，草稿已保留：" + (e as Error).message);
      props.fail(e);
    }
  }
  async function savePreset(side: string) {
    try {
      const name = prompt("新轨迹预设 ID（不会覆盖内置预设）");
      if (!name) return;
      const asset = cloneDraft(prepared.sides[side].trajectory);
      asset.id = name;
      asset.title = name;
      const r = await action(kind + ".asset.save", {
        group: "trajectories",
        name,
        asset,
        expected: null,
      });
      await presetSaved(asset, r.data.fingerprint);
      const next = cloneDraft(current.current);
      next.sides[side].trajectory = { preset_id: name };
      patch(next);
      setMessage("已另存为轨迹预设");
    } catch (e) {
      props.fail(e);
    }
  }
  function join(side: string) {
    try {
      const record = sideRuns[side],
        sideRun = props.runs[record?.id];
      if (
        !sideRun ||
        sideRun.status !== "succeeded" ||
        record.signature !== sideSignature(draft, side) ||
        sideDependencies[side as "right" | "left"].stale ||
        !sideRun.result?.data.prepared.sides[side]
      )
        throw new Error("请先成功生成当前侧别");
      const key = "nctool:managed-process:" + props.workspaceId;
      let process: Data;
      try {
        process = parseData(localStorage.getItem(key) || "");
      } catch {
        process = { name: "program", ops: [] };
      }
      const input = turningRequest(draft, kind, side);
      const id = crypto.randomUUID();
      const preceding = [...process.ops]
        .reverse()
        .find(
          (op: Data) =>
            op.business?.plugin === "sync-turning" &&
            op.business.input?.target === side,
        );
      process.ops.push({
        id,
        business: { plugin: kind, input },
        ...(sync && preceding ? { sync_from: preceding.id } : {}),
      });
      process.outputs ||= [];
      for (let ch = 1; ch <= (sync ? 2 : 1); ch++) {
        const name = sync ? `sync-${side}-CH${ch}.SPF` : `undercut-${side}.SPF`;
        let output = process.outputs.find((o: Data) => o.name === name);
        if (!output) {
          output = { name, segments: [] };
          process.outputs.push(output);
        }
        output.segments.push({ op: id, channel: ch });
      }
      localStorage.setItem(key, encode(process));
      setMessage("已加入受管工序与组合程序；组合时重新冻结并生成。");
    } catch (e) {
      props.fail(e);
    }
  }
  return (
    <div className="page-content turning-workbench">
      <PageTitle
        title={title}
        description="填写原始尺寸，计算并生成当前侧；也可一次生成左右两侧程序。"
      />
      <section className="panel turning-global-panel" aria-label="全局加工条件">
        <div className="panel-header">
          <h2>加工条件</h2>
          <span className="hint">对左右两侧生效</span>
        </div>
        <div className="panel-content turning-globals">
          {sync ? (
            <Numeric
              label="同步进给（按程序模式）"
              value={draft.sync_feed}
              change={(sync_feed) => patch({ ...draft, sync_feed })}
            />
          ) : (
            <>
              <Numeric
                label="毛坯直径 (mm)"
                value={draft.raw_diameter}
                change={(raw_diameter) => patch({ ...draft, raw_diameter })}
              />
              <Numeric
                label="安全距离 (mm)"
                value={draft.safety_dis}
                change={(safety_dis) => patch({ ...draft, safety_dis })}
              />
            </>
          )}
          {!sync && (
            <label>
              工艺标准表
              <select
                aria-label="工艺标准表"
                value={draft.standard_id || "v3-grooves"}
                onChange={(e) =>
                  patch({ ...draft, standard_id: e.target.value })
                }
              >
                <option value="v3-grooves">V3 ES/FS 标准表</option>
                {standardAssets
                  .filter((s) => s.id !== "v3-grooves")
                  .map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.title}
                    </option>
                  ))}
              </select>
            </label>
          )}
          <label>
            目标机床
            <select
              aria-label="目标机床"
              value={draft.machine_id || ""}
              onChange={(e) => {
                const next = { ...draft };
                if (e.target.value) next.machine_id = e.target.value;
                else delete next.machine_id;
                patch(next);
              }}
            >
              <option value="">通用语法配置</option>
              {machines.map((m) => (
                <option key={m}>{m}</option>
              ))}
            </select>
          </label>
          <button
            className="button secondary"
            disabled={pending}
            onClick={() => void calculate()}
          >
            计算与预览
          </button>
          <button
            className="button primary"
            disabled={
              pending || ["queued", "running"].includes(run?.status || "")
            }
            onClick={() => void generate("all")}
          >
            生成全部
          </button>
        </div>
      </section>
      <details className="panel turning-session">
        <summary>会话、导入与导出</summary>
        <div className="panel-content">
          <label>
            读取会话
            <select
              aria-label="读取会话"
              value=""
              onChange={(e) => {
                const item = sessions.find((s) => s.id === e.target.value);
                if (item) {
                  patch(cloneDraft(item.asset.input));
                  setSessionId(item.id);
                  setSessionName(item.title);
                  setSessionExpected(item.fingerprint);
                  setRunId(null);
                }
              }}
            >
              <option value="">选择会话（替换当前草稿）</option>
              {sessions.map((s) => (
                <option value={s.id} key={s.id}>
                  {s.title}
                </option>
              ))}
            </select>
          </label>
          <div className="turning-globals">
            <label>
              会话 ID
              <input
                aria-label="会话 ID"
                value={sessionId}
                onChange={(e) => {
                  setSessionId(e.target.value);
                  setSessionExpected(null);
                }}
              />
            </label>
            <label>
              会话名称
              <input
                value={sessionName}
                onChange={(e) => setSessionName(e.target.value)}
              />
            </label>
            <button onClick={() => void saveSession()}>保存会话</button>
            <button
              onClick={() =>
                download(
                  encode(
                    {
                      format_version: 1,
                      input: turningRequest(draft, kind, "all"),
                    },
                    true,
                  ),
                  kind + "-session.json",
                  "application/json",
                )
              }
            >
              导出会话
            </button>
            <label>
              导入会话或 V3 配置
              <input
                aria-label="导入会话或 V3 配置"
                type="file"
                accept=".json"
                onChange={async (e) => {
                  try {
                    const file = e.target.files?.[0];
                    if (file)
                      setImported(
                        importTurning(parseData(await file.text()), kind),
                      );
                  } catch (error) {
                    props.fail(error);
                  }
                  e.target.value = "";
                }}
              />
            </label>
          </div>
          {imported && (
            <>
              <p>{imported.notes.join("；")}</p>
              <pre>{encode(imported.input, true)}</pre>
              <button
                onClick={() => {
                  patch(imported.input);
                  setImported(undefined);
                  setSessionExpected(null);
                  setSessionId("");
                }}
              >
                应用导入会话
              </button>
            </>
          )}
        </div>
      </details>
      {message && (
        <p role="status" className="hint">
          {message}
        </p>
      )}
      <ChoiceTabs
        label="加工侧别"
        idPrefix={kind + "-side"}
        className="turning-side-tabs"
        value={activeSide}
        choices={[
          { value: "right", label: "右侧" },
          { value: "left", label: "左侧" },
        ]}
        change={setActiveSide}
      />
      {["right", "left"].map((side) => {
        const settings = draft.sides[side],
          sideTitle = side === "right" ? "右侧" : "左侧",
          sideRun = props.runs[sideRuns[side]?.id],
          sideStale =
            sideRuns[side]?.signature !== sideSignature(draft, side) ||
            sideDependencies[side as "right" | "left"].stale;
        const validPreview =
            preview[side]?.signature === previewSideSignature(draft, side),
          result = prepared.sides[side],
          mode = modes[side];
        const artifact =
          sideRun?.status === "succeeded"
            ? sideRun.result?.artifacts?.filter((a) =>
                a.name.includes("-" + side),
              )
            : [];
        const status =
          sideRun && ["queued", "running"].includes(sideRun.status)
            ? "生成中"
            : sideRun?.status === "failed"
              ? "生成失败"
              : sideRun && ["cancelled", "interrupted"].includes(sideRun.status)
                ? "生成已取消或中断"
                : sideRun && sideStale
                  ? "结果已过期"
                  : artifact?.length
                    ? "生成成功"
                    : validPreview
                      ? "计算预览"
                      : "尚未计算";

        return (
          <section
            className={`panel turning-side ${activeSide === side ? "selected" : ""}`}
            key={side}
            hidden={activeSide !== side}
            role="tabpanel"
            id={`${kind}-side-${side}-panel`}
            aria-labelledby={`${kind}-side-${side}`}
            aria-label={sideTitle + title}
          >
            <header className="turning-side-header">
              <h2>
                {sideTitle}
                {title}
              </h2>
              <div className="toolbar">
                <button disabled={pending} onClick={() => void calculate(side)}>
                  计算{sideTitle}
                </button>
                <button
                  className="button primary"
                  disabled={
                    pending || ["queued", "running"].includes(run?.status || "")
                  }
                  onClick={() => void generate(side)}
                >
                  生成{sideTitle}
                </button>
              </div>
            </header>
            <div className="turning-side-body">
              <div className="turning-inputs">
                {sync && (
                  <div className="turning-globals">
                    <Numeric
                      label={sideTitle + "同步起始号"}
                      value={settings.start_sync_num}
                      change={(start_sync_num) =>
                        changeSide(side, { start_sync_num })
                      }
                    />
                    <Numeric
                      label={sideTitle + "固定 ap1（直径减量 mm）"}
                      value={settings.ap_1}
                      change={(ap_1) => changeSide(side, { ap_1 })}
                    />
                    <Numeric
                      label={sideTitle + "ap2 最小值"}
                      value={settings.ap2_min}
                      change={(ap2_min) => changeSide(side, { ap2_min })}
                    />
                    <Numeric
                      label={sideTitle + "ap2 最大值"}
                      value={settings.ap2_max}
                      change={(ap2_max) => changeSide(side, { ap2_max })}
                    />
                    <button
                      disabled={pending}
                      onClick={() => void recommend(side)}
                    >
                      计算{sideTitle} ap2
                    </button>
                  </div>
                )}
                {sync && (
                  <p className="hint">
                    ap2 可留空，计算或生成时自动填入有效值；手填值保留。计算 ap2
                    可重新填写本侧各行。
                  </p>
                )}
                <div className="turning-table-scroll">
                  <table
                    className={`turning-table ${sync ? "sync" : "groove"}`}
                  >
                    <colgroup>
                      <col className="turning-row-number" />
                      {!sync && (
                        <>
                          <col className="turning-type-column" />
                          <col className="turning-standard-column" />
                        </>
                      )}
                      {Array.from({ length: 5 }, (_, index) => (
                        <col key={index} className="turning-number-column" />
                      ))}
                      <col />
                      <col className="turning-actions-column" />
                    </colgroup>
                    <thead>
                      <tr>
                        <th className="turning-row-number">序号</th>
                        {!sync && (
                          <>
                            <th>类型</th>
                            <th>标准</th>
                          </>
                        )}
                        {(sync
                          ? ["sta_x", "sta_z", "end_x", "end_z", "ap_2"]
                          : ["d1", "z", "d2", "c", "angle"]
                        ).map((k) => (
                          <th key={k} className="turning-number-column">
                            {labels[k]}
                          </th>
                        ))}
                        <th>{sync ? "刀次 / 推荐" : "计算"}</th>
                        <th className="turning-actions-column">行操作</th>
                      </tr>
                    </thead>
                    <tbody>
                      {settings.rows.map((row: Data, index: number) => {
                        const candidate = recommendations[side]?.rows[row.id];
                        const rec =
                          candidate?.conditions === ap2Conditions(row, settings)
                            ? candidate
                            : undefined;
                        const rowResult = result?.rows.find(
                          (r: Data) => r.id === row.id,
                        )?.prepared;
                        const c = rowResult?.contexts?.[0];
                        return (
                          <tr key={row.id}>
                            <td className="turning-row-number">{index + 1}</td>
                            {!sync && (
                              <>
                                <td>
                                  <select
                                    aria-label={`${sideTitle}第${index + 1}行类型`}
                                    value={row.gtype}
                                    onChange={(e) => {
                                      const rows = cloneDraft(settings.rows);
                                      rows[index] = changeGrooveType(
                                        row,
                                        e.target.value,
                                      );
                                      changeSide(side, { rows });
                                    }}
                                  >
                                    <option>ES</option>
                                    <option>FS</option>
                                  </select>
                                </td>
                                <td>
                                  <select
                                    aria-label={`${sideTitle}第${index + 1}行标准`}
                                    value={row.std_key || ""}
                                    onChange={(e) =>
                                      changeRow(side, index, {
                                        std_key: e.target.value,
                                      })
                                    }
                                  >
                                    <option value="">选择标准</option>
                                    {Object.entries(
                                      tables[row.gtype] || {},
                                    ).map(
                                      ([name, dimensions]: [string, Data]) => (
                                        <option key={name} value={name}>
                                          {name} · r={numberText(dimensions.r)}
                                        </option>
                                      ),
                                    )}
                                  </select>
                                </td>
                              </>
                            )}
                            {(sync
                              ? ["sta_x", "sta_z", "end_x", "end_z", "ap_2"]
                              : ["d1", "z", "d2", "c", "angle"]
                            ).map((k) => (
                              <td key={k}>
                                {!sync &&
                                row.gtype === "FS" &&
                                ["d2", "c", "angle"].includes(k) ? (
                                  <span>—</span>
                                ) : (
                                  <Numeric
                                    label={`${sideTitle}第${index + 1}行 ${labels[k]}${k === "angle" ? " (°)" : " (mm)"}`}
                                    value={row[k]}
                                    change={(v) =>
                                      changeRow(side, index, { [k]: v })
                                    }
                                  />
                                )}
                              </td>
                            ))}
                            <td className="turning-row-summary">
                              {sync ? (
                                <>
                                  <output>
                                    {c
                                      ? numberText(c.cut_num) + " 刀"
                                      : rowResult?.skip_text
                                        ? "无需切削"
                                        : "待计算"}
                                  </output>
                                  {c && (
                                    <small>
                                      实际 X {numberText(c.actual_sta_x)}
                                      {side === "left" &&
                                        " / Z " + numberText(c.actual_sta_z)}
                                    </small>
                                  )}
                                  {rec && (
                                    <>
                                      <small>
                                        {rec.success
                                          ? `计算值 ${numberText(rec.adopted_ap_2)} / ${numberText(rec.cut_num)} 刀`
                                          : rec.diagnostic}
                                      </small>
                                      {rec.diagnostic && (
                                        <small>{rec.diagnostic}</small>
                                      )}
                                    </>
                                  )}
                                </>
                              ) : c ? (
                                <small>
                                  切割 X {numberText(c.D1_CUT)}
                                  <br />
                                  起点 Z {numberText(c.Z_START)}
                                </small>
                              ) : (
                                "待计算"
                              )}
                            </td>
                            <td className="turning-actions-column">
                              <div className="turning-row-actions">
                                <button
                                  aria-label={`${sideTitle}复制第${index + 1}行`}
                                  title="复制此行"
                                  onClick={() => {
                                    const rows = cloneDraft(settings.rows);
                                    rows.splice(index + 1, 0, {
                                      ...cloneDraft(row),
                                      id: crypto.randomUUID(),
                                    });
                                    changeSide(side, { rows });
                                  }}
                                >
                                  <Copy size={16} aria-hidden="true" />
                                </button>
                                <button
                                  disabled={index === 0}
                                  aria-label={`${sideTitle}上移第${index + 1}行`}
                                  title="上移此行"
                                  onClick={() => {
                                    const rows = cloneDraft(settings.rows);
                                    [rows[index - 1], rows[index]] = [
                                      rows[index],
                                      rows[index - 1],
                                    ];
                                    changeSide(side, { rows });
                                  }}
                                >
                                  <ArrowUp size={16} aria-hidden="true" />
                                </button>
                                <button
                                  aria-label={`${sideTitle}删除第${index + 1}行`}
                                  title="删除此行"
                                  onClick={() =>
                                    changeSide(side, {
                                      rows: settings.rows.filter(
                                        (_: Data, i: number) => i !== index,
                                      ),
                                    })
                                  }
                                >
                                  <Trash2 size={16} aria-hidden="true" />
                                </button>
                              </div>
                            </td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
                <button
                  className="button secondary"
                  onClick={() =>
                    changeSide(side, {
                      rows: [...settings.rows, emptyRow(kind)],
                    })
                  }
                >
                  添加{sideTitle}
                  {sync ? "台阶" : "槽"}
                </button>
                <details className="turning-format">
                  <summary>高级设置 · 模板格式</summary>
                  <div className="turning-detail-body">
                    <label>
                      模板格式
                      <select
                        aria-label={sideTitle + "模板格式"}
                        value={settings.template || ""}
                        onChange={(e) => {
                          const next = cloneDraft(draft);
                          if (e.target.value)
                            next.sides[side].template = e.target.value;
                          else delete next.sides[side].template;
                          patch(next);
                        }}
                      >
                        <option value="">内置 V3 轨迹格式</option>
                        {templates
                          .filter(
                            (t) =>
                              t.asset.metadata?.nc?.operation?.id ===
                              (sync ? `sync.${side}` : "groove"),
                          )
                          .map((t) => (
                            <option key={t.id} value={t.id}>
                              {t.title || t.id}
                              {safeNumber(
                                t.asset.metadata?.nc?.trajectory_plan_version,
                              ) !== 1
                                ? "（旧自由模板）"
                                : ""}
                            </option>
                          ))}
                      </select>
                    </label>
                    {settings.template &&
                      safeNumber(
                        templates.find((t) => t.id === settings.template)?.asset
                          .metadata?.nc?.trajectory_plan_version,
                      ) !== 1 && (
                        <p role="alert" className="inline-error">
                          此模板不支持轨迹计划。请选择内置格式或导入升级后的模板；原模板保留兼容
                          API 生成方式。
                        </p>
                      )}
                    <p className="hint">
                      专用模板在此处选择和编辑，通用模板库继续隐藏这些模板。
                    </p>
                    <SpecializedTemplateEditor
                      storageKey={`${storageKey}:template-editor:${side}`}
                      sideTitle={sideTitle}
                      templateId={settings.template || ""}
                      operation={sync ? `sync.${side}` : "groove"}
                      onSaved={templateSaved}
                      fail={props.fail}
                    />
                  </div>
                </details>
                <details className="turning-trajectory">
                  <summary>轨迹预设与本次调整</summary>
                  <div className="turning-detail-body">
                    <label>
                      轨迹预设
                      <select
                        aria-label={sideTitle + "轨迹预设"}
                        value={
                          settings.trajectory?.preset_id || "v3-compatible"
                        }
                        onChange={(e) =>
                          changeSide(side, {
                            trajectory: {
                              preset_id: e.target.value,
                              overrides: settings.trajectory?.overrides || {},
                            },
                          })
                        }
                      >
                        <option value="v3-compatible">V3 兼容轨迹</option>
                        {presets.map((p) => (
                          <option key={p.id} value={p.id}>
                            {p.title}
                          </option>
                        ))}
                      </select>
                    </label>
                    <TrajectoryPresetEditor
                      kind={kind}
                      storageKey={`${storageKey}:preset-editor:${side}`}
                      sideTitle={sideTitle}
                      presetId={
                        settings.trajectory?.preset_id || "v3-compatible"
                      }
                      builtin={description?.builtin_trajectory}
                      schema={description?.trajectory_parameter_schema}
                      labels={labels}
                      onSaved={presetSaved}
                      fail={props.fail}
                    />
                    <p className="hint">
                      以下参数仅调整本次加工；编辑和保存预设请使用上方编辑器。
                    </p>
                    <div className="turning-globals">
                      {Object.keys(
                        description?.trajectory_parameter_schema.properties ||
                          {},
                      ).map((k) => {
                        const selected =
                          presets.find(
                            (p) => p.id === settings.trajectory?.preset_id,
                          )?.asset || description?.builtin_trajectory;
                        return (
                          <Numeric
                            key={k}
                            label={sideTitle + " " + labels[k]}
                            value={
                              settings.trajectory?.overrides?.[k] ??
                              selected?.parameters?.[k]
                            }
                            change={(v) =>
                              changeSide(side, {
                                trajectory: {
                                  ...settings.trajectory,
                                  overrides: {
                                    ...settings.trajectory?.overrides,
                                    [k]: v ?? new DraftNumber(""),
                                  },
                                  confirmed_for: undefined,
                                },
                              })
                            }
                          />
                        );
                      })}
                    </div>
                    <div className="toolbar">
                      <button
                        onClick={() =>
                          changeSide(side, {
                            trajectory: {
                              preset_id:
                                settings.trajectory?.preset_id ||
                                "v3-compatible",
                            },
                          })
                        }
                      >
                        恢复预设值
                      </button>
                      <button
                        disabled={!result || !validPreview}
                        onClick={() =>
                          changeSide(side, {
                            trajectory: {
                              ...settings.trajectory,
                              confirmed_for: result.confirmed_for,
                            },
                          })
                        }
                      >
                        确认{sideTitle}轨迹调整
                      </button>
                      <button
                        disabled={!result || !validPreview}
                        onClick={() => void savePreset(side)}
                      >
                        将本次调整另存预设
                      </button>
                      <button
                        disabled={
                          !result ||
                          !validPreview ||
                          !presets.some(
                            (p) => p.id === settings.trajectory?.preset_id,
                          )
                        }
                        onClick={() => void saveCurrentPreset(side)}
                      >
                        保存本次调整到预设
                      </button>
                    </div>
                    <label>
                      导入轨迹预设
                      <input
                        type="file"
                        accept=".json"
                        aria-label={sideTitle + "导入轨迹预设"}
                        onChange={async (e) => {
                          try {
                            const file = e.target.files?.[0];
                            if (file) {
                              const asset = parseData(await file.text());
                              await action(kind + ".asset.save", {
                                group: "trajectories",
                                name: asset.id,
                                asset,
                                expected: null,
                              });
                              await refreshAssets();
                              setMessage("轨迹预设已导入，请明确选择。");
                            }
                          } catch (error) {
                            props.fail(error);
                          }
                          e.target.value = "";
                        }}
                      />
                    </label>
                    <button
                      disabled={!result || !validPreview}
                      onClick={() => {
                        const asset = cloneDraft(result.trajectory);
                        asset.id = "custom-" + kind;
                        asset.title = "自定义轨迹";
                        download(
                          encode(asset, true),
                          kind + "-trajectory.json",
                          "application/json",
                        );
                      }}
                    >
                      导出当前轨迹预设
                    </button>
                    <p className="hint">
                      调整后需重新计算并确认。原有人工调整在切换预设后保留。
                    </p>
                  </div>
                </details>
                {!sync &&
                  settings.rows.map((row: Data) => (
                    <StandardSection
                      key={row.id}
                      type={row.gtype}
                      dimensions={tables[row.gtype]?.[row.std_key]}
                    />
                  ))}
                {!sync && result && (
                  <details>
                    <summary>标准与几何计算</summary>
                    <pre>
                      {encode(
                        {
                          standard:
                            preview[side]?.standard ||
                            sideRun?.result?.data.prepared.standard,
                          rows: result.rows.map((r: Data) => ({
                            id: r.id,
                            geometry: r.prepared.contexts[0],
                          })),
                        },
                        true,
                      )}
                    </pre>
                  </details>
                )}
              </div>
              <div className="turning-results">
                <div className="turning-result-header">
                  <h3>程序与轨迹</h3>
                  <span
                    role="status"
                    className={
                      "turning-status " +
                      (sideStale && sideRun ? "warning" : "")
                    }
                  >
                    {status}
                  </span>
                </div>
                {settings.template && (
                  <p className="hint">
                    当前程序使用自定义模板；轨迹图只显示预设轨迹，不包含模板中额外编写的运动代码。
                  </p>
                )}
                <div className="toolbar">
                  <ChoiceTabs
                    label={sideTitle + "结果视图"}
                    idPrefix={`${kind}-${side}-view`}
                    value={mode}
                    choices={[
                      { value: "code", label: "代码" },
                      { value: "path", label: "2D 轨迹" },
                    ]}
                    change={(value) => setModes({ ...modes, [side]: value })}
                  />
                  <button
                    className="button secondary"
                    disabled={!artifact?.length || sideStale}
                    onClick={() => join(side)}
                  >
                    加入组合 · {sideTitle}
                  </button>
                </div>
                {sideStale && sideRun && (
                  <p className="stale-banner">
                    输入或资源已变化，旧结果失效。请重新生成。
                  </p>
                )}
                {sideRun?.status === "failed" && (
                  <p role="alert" className="inline-error">
                    {sideRun.error?.message || "本次生成失败，没有可交付文件。"}
                  </p>
                )}
                <div
                  role="tabpanel"
                  id={`${kind}-${side}-view-path-panel`}
                  aria-labelledby={`${kind}-${side}-view-path`}
                  hidden={mode !== "path"}
                >
                  <PathPreview
                    previews={
                      result?.rows.flatMap(
                        (r: Data) => r.prepared.previews || [],
                      ) || []
                    }
                  />
                </div>
                <div
                  role="tabpanel"
                  id={`${kind}-${side}-view-code-panel`}
                  aria-labelledby={`${kind}-${side}-view-code`}
                  hidden={mode !== "code"}
                >
                  {validPreview && (!artifact?.length || sideStale) && (
                    <p className="hint turning-preview-hint">
                      计算预览 · 生成成功后才能复制或导出。
                    </p>
                  )}
                  {sync && (
                    <ChoiceTabs
                      label={sideTitle + "程序通道"}
                      idPrefix={`${kind}-${side}-channel`}
                      className="turning-channel-tabs"
                      value={channels[side]}
                      choices={[
                        { value: "0", label: "CH1" },
                        { value: "1", label: "CH2" },
                      ]}
                      change={(value) =>
                        setChannels({ ...channels, [side]: value })
                      }
                    />
                  )}
                  <div
                    className={
                      "turning-channel-grid" + (!sync ? " single" : "")
                    }
                  >
                    {(sync ? [0, 1] : [0]).map((ch) => (
                      <div
                        key={ch}
                        id={`${kind}-${side}-channel-${ch}-panel`}
                        className={
                          "turning-channel " +
                          (channels[side] === String(ch) ? "selected" : "")
                        }
                        role={sync ? "tabpanel" : "region"}
                        aria-labelledby={
                          sync ? `${kind}-${side}-channel-${ch}` : undefined
                        }
                        aria-label={!sync ? sideTitle + "程序" : undefined}
                      >
                        <div className="turning-channel-header">
                          <h3>{sync ? `CH${ch + 1}` : "程序"}</h3>
                          <div className="toolbar">
                            <button
                              className="button secondary"
                              aria-label={`${sideTitle} CH${ch + 1}复制`}
                              disabled={!artifact?.[ch] || sideStale}
                              onClick={() =>
                                void navigator.clipboard
                                  .writeText(artifact![ch].text)
                                  .catch(props.fail)
                              }
                            >
                              复制
                            </button>
                            <button
                              className="button secondary"
                              disabled={!artifact?.[ch] || sideStale}
                              onClick={() =>
                                download(artifact![ch].text, artifact![ch].name)
                              }
                            >
                              导出 SPF
                            </button>
                          </div>
                        </div>
                        {validPreview &&
                          sideRun?.result?.data.prepared.sides[side] && (
                            <FragmentDiff
                              before={
                                sideRun.result.data.prepared.sides[side]
                                  .channels[ch]
                              }
                              after={preview[side].data.channels[ch]}
                            />
                          )}
                        <pre
                          tabIndex={0}
                          aria-label={`${sideTitle} CH${ch + 1}程序`}
                        >
                          {(!sideStale && artifact?.[ch]
                            ? artifact[ch].text
                            : validPreview
                              ? result?.channels?.[ch]
                              : artifact?.[ch]?.text) || "尚未生成"}
                        </pre>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            </div>
          </section>
        );
      })}
      {run && (
        <>
          <div className="turning-run-status" role="status">
            <span>
              最近任务 · {run.title} ·{" "}
              {{
                queued: "等待执行",
                running: "生成中",
                succeeded: "已完成",
                failed: "生成失败",
                cancelled: "已取消",
                interrupted: "已中断",
              }[run.status] || run.status}
            </span>
            {["queued", "running"].includes(run.status) && (
              <button
                className="button secondary"
                onClick={() =>
                  void api("runs/" + run.id + "/cancel", {}).catch(props.fail)
                }
              >
                取消生成
              </button>
            )}
          </div>
          <details className="panel turning-run-detail">
            <summary>生成记录与详细诊断</summary>
            <RunDetail run={run} stale={stale} />
          </details>
        </>
      )}
    </div>
  );
}
