import { useState, type RefObject } from "react";
import { X } from "lucide-react";
import { api } from "../lib/api";
import { parseData, type Data } from "../lib/data";

interface Props {
  picker: RefObject<HTMLInputElement | null>;
  refreshCatalog: () => Promise<void>;
  notify: (message: string) => void;
  fail: (error: Data) => void;
}
export function WorkspaceImport({
  picker: bundleInput,
  refreshCatalog: catalog,
  notify,
  fail,
}: Props) {
  const [importBundle, setImportBundle] = useState<Data>(null);
  const [bundlePreview, setBundlePreview] = useState<Data>(null);
  return (
    <>
      <input
        className="hidden"
        ref={bundleInput}
        type="file"
        accept=".json"
        onChange={async (event) => {
          const file = event.target.files?.[0];
          event.target.value = "";
          if (!file) return;
          try {
            if (file.size > 64 * 1024 * 1024)
              throw new Error("资产包超过 64 MiB");
            const bundle = parseData(await file.text());
            const preview = await api("bundle/validate", { bundle });
            setImportBundle(bundle);
            setBundlePreview(preview);
          } catch (error) {
            fail(error);
          }
        }}
      />
      {importBundle && (
        <div className="modal-backdrop">
          <div
            className="modal"
            role="dialog"
            aria-modal="true"
            aria-label="导入资产集合"
          >
            <div className="modal-header">
              <h2>确认导入资产集合</h2>
              <button
                className="icon-button"
                onClick={() => {
                  setImportBundle(null);
                  setBundlePreview(null);
                }}
              >
                <X size={18} />
              </button>
            </div>
            <div className="modal-body">
              <p>
                保留所有资产标识、源码、配置和引用关系。已有标识冲突会阻止导入，不覆盖当前工作区。
              </p>
              {bundlePreview?.reports.map((report: Data) => (
                <div key={report.collection}>
                  <h3>
                    {report.collection} · {String(report.report.count)} 项
                  </h3>
                  <ul>
                    {report.report.mapping?.map((mapping: Data) => (
                      <li key={mapping.group + mapping.from}>
                        {mapping.group}: {mapping.from} → {mapping.to}
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </div>
            <div className="modal-footer">
              <button
                className="button secondary"
                onClick={() => setImportBundle(null)}
              >
                取消
              </button>
              <button
                className="button primary"
                onClick={async () => {
                  try {
                    await api("bundle/import", { bundle: importBundle });
                    await catalog();
                    setImportBundle(null);
                    setBundlePreview(null);
                    notify("资产集合已导入");
                  } catch (error) {
                    fail(error);
                  }
                }}
              >
                确认导入
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
