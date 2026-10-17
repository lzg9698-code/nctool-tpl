import { useRef, useState } from "react";
import { api } from "../lib/api";
import { buildStarterExample, type StarterBundle, type StarterExample, type StarterExampleId } from "../lib/starterExamples";
import { Modal } from "./WorkbenchUi";

export interface StarterExamplesProps {
  onSaved: () => Promise<void>;
  fail?: (error: unknown) => void;
}

type Prepared = { example: StarterExample; bundle: StarterBundle; validated: boolean };

export function StarterExamples({ onSaved, fail }: StarterExamplesProps) {
  const [prepared, setPrepared] = useState<Prepared>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [savedNotice, setSavedNotice] = useState("");
  const busyRef = useRef(false);

  function open(id: StarterExampleId) {
    if (busyRef.current) return;
    setError("");
    setSavedNotice("");
    setPrepared({ ...buildStarterExample(id), validated: false });
  }

  function close() {
    if (busyRef.current) return;
    setPrepared(undefined);
    setError("");
    setSavedNotice("");
  }

  async function validate() {
    if (!prepared || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError("");
    try {
      await api("bundle/validate", { bundle: prepared.bundle });
      setPrepared(current => current ? { ...current, validated: true } : current);
    } catch (cause) {
      const message = (cause as Error).message || "示例校验失败";
      setError(message);
      fail?.(cause);
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  async function save() {
    if (!prepared || !prepared.validated || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError("");
    try {
      await api("bundle/import", { bundle: prepared.bundle });
    } catch (cause) {
      const message = (cause as Error).message || "示例保存失败";
      setError(message);
      fail?.(cause);
      busyRef.current = false;
      setBusy(false);
      return;
    }
    // Import has committed. A catalog refresh failure must never invite a second import.
    setPrepared(undefined);
    busyRef.current = false;
    setBusy(false);
    try {
      await onSaved();
    } catch (cause) {
      const message = `示例已保存，但刷新目录失败：${(cause as Error).message || "请稍后刷新"}`;
      setSavedNotice(message);
      fail?.(cause);
    }
  }

  return <>
    <button type="button" className="button secondary" onClick={() => open("plain-text")}>查看入门示例</button>
    {savedNotice && <p role="alert" className="inline-error">{savedNotice}</p>}
    {prepared && <Modal title={`入门示例：${prepared.example.title}`} onClose={close} buttons={<>
      <button type="button" className="button secondary" disabled={busy} onClick={close}>取消</button>
      {!prepared.validated ? <button type="button" className="button primary" disabled={busy} onClick={() => void validate()}>{busy ? "正在校验…" : "校验示例"}</button> : <button type="button" className="button primary" disabled={busy} onClick={() => void save()}>{busy ? "正在保存…" : "确认保存整个示例"}</button>}
    </>}>
      <div className="toolbar" aria-label="选择示例">
        <button type="button" className="button secondary" disabled={busy} onClick={() => open("plain-text")}>普通文本</button>
        <button type="button" className="button secondary" disabled={busy} onClick={() => open("shared-parameters")}>共享参数</button>
      </div>
      <p>{prepared.example.description}</p>
      <h3>模板源码</h3><pre>{prepared.example.source}</pre>
      <h3>默认预期输出</h3><pre>{prepared.example.expected}</pre>
      {!prepared.example.definition && <p>执行 action：<code>{prepared.example.executionAction}</code>；输出文件：<code>{prepared.example.outputName}</code></p>}
      {prepared.example.definition && <>
        <h3>共享参数的对应关系</h3>
        <p>目标字段 <code>/report/name</code> 绑定到共享定义“名称”（ID：<code>{prepared.example.definition.id}</code>）。目标字段 <code>/report/unit</code> 由模板默认值提供。</p>
        <ul>
          <li>定义默认值：{String(prepared.example.definition.default)}</li>
          <li>模板默认值：名称“{prepared.example.defaults.report.name}”，单位“{prepared.example.defaults.report.unit}”</li>
          <li>共享预置“示例报告”（ID：<code>{prepared.example.preset?.id}</code>）：名称“零件 A”</li>
        </ul>
        <p>执行 action：<code>{prepared.example.executionAction}</code>；输出文件：<code>{prepared.example.outputName}</code></p>
        <details><summary>查看完整定义、预置和绑定 JSON</summary><pre>{JSON.stringify({ definition: prepared.example.definition, preset: prepared.example.preset, bindings: prepared.example.bindings }, null, 2)}</pre></details>
      </>}
      {prepared.validated && <p role="status">示例校验通过。确认后才会将这个完整资产包导入工作区。</p>}
      {error && <p role="alert" className="inline-error">{error}。预览已保留，可以重试。</p>}
    </Modal>}
  </>;
}
