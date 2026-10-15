import { BookOpen, Search } from "lucide-react";
import { Editor } from "../../components/Editor";
import { Form } from "../../components/Form";
import { SchemaEditor } from "../../components/SchemaEditor";
import { JsonEditor } from "../../components/WorkbenchUi";
import { defaultValue, mergeDefaults } from "../../lib/data";

import {
  normalizeSchema,
  type WorkbenchController,
} from "./useWorkbenchController";
export function AuthorPanes({
  controller,
}: {
  controller: WorkbenchController;
}) {
  const {
    items,
    setPage,
    inspector,
    setInspector,
    code,
    jump,
    doc,
    editDoc,
    discover,
  } = controller;
  if (!doc) return null;
  return (
    <>
      <section className="source-pane">
        <div className="pane-toolbar">
          <div className="pane-tabs">
            <button
              className={doc.tab === "source" ? "selected" : ""}
              onClick={() => editDoc({ tab: "source" })}
            >
              模板源码
            </button>
            <button
              className={doc.tab === "schema" ? "selected" : ""}
              onClick={() => editDoc({ tab: "schema" })}
            >
              高级规格
            </button>
            <button
              className={doc.tab === "settings" ? "selected" : ""}
              onClick={() => editDoc({ tab: "settings" })}
            >
              模板信息
            </button>
          </div>
          {doc.tab === "source" && (
            <button
              className="text-button"
              onClick={() => setPage("syntax-guide")}
            >
              <BookOpen size={13} />
              语法提示
            </button>
          )}
          {doc.tab === "source" && (
            <button className="text-button" onClick={() => void discover()}>
              <Search size={13} />
              识别参数
            </button>
          )}
        </div>
        {doc.tab === "source" ? (
          <Editor
            ref={code}
            editorKey={doc.key}
            value={doc.asset.source}
            onChange={(source) =>
              editDoc((d) => ({
                ...d,
                asset: { ...d.asset, source },
              }))
            }
            line={jump}
            label="模板源码"
          />
        ) : doc.tab === "schema" ? (
          <JsonEditor
            editorKey={doc.key}
            value={normalizeSchema(doc.asset)}
            onChange={(schema) =>
              editDoc((d) => ({
                ...d,
                asset: { ...d.asset, schema },
              }))
            }
            label="高级参数规格"
          />
        ) : (
          <div className="pane-scroll metadata-editor">
            <label>
              说明
              <textarea
                rows={3}
                value={doc.asset.metadata?.description || ""}
                onChange={(e) =>
                  editDoc((d) => ({
                    ...d,
                    asset: {
                      ...d.asset,
                      metadata: {
                        ...d.asset.metadata,
                        description: e.target.value,
                      },
                    },
                  }))
                }
              />
            </label>
            <label>
              标签（逗号分隔）
              <input
                value={(doc.asset.tags || []).join(", ")}
                onChange={(e) =>
                  editDoc((d) => ({
                    ...d,
                    asset: {
                      ...d.asset,
                      tags: e.target.value
                        .split(/[,，]/)
                        .map((x) => x.trim())
                        .filter(Boolean),
                    },
                  }))
                }
              />
            </label>
            <label>
              渲染扩展（显式启用，逗号分隔）
              <input
                aria-label="渲染扩展"
                value={(
                  doc.extensions ??
                  doc.asset.metadata?.execution?.extensions ??
                  []
                ).join(", ")}
                onChange={(e) =>
                  editDoc({
                    extensions: e.target.value
                      .split(/[,，]/)
                      .map((value) => value.trim())
                      .filter(Boolean),
                  })
                }
                placeholder="例如 math"
              />
            </label>
            <p className="hint">
              扩展需要在设置中启用对应插件；空值使用通用模板能力。
            </p>
            <label>
              输出文件名
              <input
                value={doc.outputName}
                onChange={(e) => editDoc({ outputName: e.target.value })}
              />
            </label>
            <label>
              模板标识
              <input readOnly value={doc.id || "保存后自动生成"} />
            </label>
            <p className="hint">
              模板名称可以修改，模板标识保持稳定，已有引用不受影响。
            </p>
            <label>
              插入其他模板引用
              <select
                defaultValue=""
                onChange={(e) => {
                  if (e.target.value) {
                    editDoc({ tab: "source" });
                    setTimeout(
                      () =>
                        code.current?.insert(
                          `{% include "${e.target.value}" %}`,
                        ),
                      50,
                    );
                  }
                }}
              >
                <option value="">选择模板</option>
                {items
                  .filter((item) => item.id !== doc.id)
                  .map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.title}
                    </option>
                  ))}
              </select>
            </label>
          </div>
        )}
        <div className="editor-status">
          <span>Jinja · UTF-8</span>
          <span>{doc.asset.source.split("\n").length} 行</span>
          <span>Ctrl+S 保存 · Ctrl+Enter 生成</span>
        </div>
      </section>
      <section className="definition-pane">
        <div className="pane-toolbar">
          <div className="pane-tabs">
            <button
              className={inspector === "definition" ? "selected" : ""}
              onClick={() => setInspector("definition")}
            >
              参数定义
            </button>
            <button
              className={inspector === "defaults" ? "selected" : ""}
              onClick={() => setInspector("defaults")}
            >
              默认参数
            </button>
          </div>
        </div>
        <div className="pane-scroll">
          {inspector === "definition" ? (
            <SchemaEditor
              schema={normalizeSchema(doc.asset)}
              onChange={(schema) =>
                editDoc((d) => ({
                  ...d,
                  asset: { ...d.asset, schema },
                }))
              }
              onInsert={(name) => code.current?.insert("{{ " + name + " }}")}
            />
          ) : (
            <>
              <p className="hint">
                默认值会应用到新的使用任务；不覆盖已经填写的值。
              </p>
              <Form
                schema={normalizeSchema(doc.asset)}
                value={doc.asset.defaults || {}}
                onChange={(defaults) =>
                  editDoc((d) => ({
                    ...d,
                    asset: { ...d.asset, defaults },
                  }))
                }
              />
              <button
                className="button secondary full-width"
                onClick={() =>
                  editDoc({
                    context: doc.asset.metadata?.parameters
                      ? {}
                      : mergeDefaults(
                          defaultValue(normalizeSchema(doc.asset)),
                          doc.asset.defaults,
                        ),
                    jsonInput: undefined,
                  })
                }
              >
                {doc.asset.metadata?.parameters
                  ? "清除本次覆盖，恢复共享参数取值"
                  : "将默认参数用于本次试运行"}
              </button>
            </>
          )}
        </div>
      </section>
    </>
  );
}
