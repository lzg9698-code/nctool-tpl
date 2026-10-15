import {
  Check,
  ChevronRight,
  Code2,
  Copy,
  Download,
  FileCode2,
  FileInput,
  ListChecks,
  LoaderCircle,
  Play,
  Plus,
  Save,
  Trash2,
  X,
} from "lucide-react";
import { Divider } from "../../components/Divider";
import { Menu } from "../../components/WorkbenchUi";
import { download, encode, isDirty, type Data } from "../../lib/data";

import { AuthorPanes } from "./AuthorPanes";
import { ParametersPane } from "./ParametersPane";
import { ResultPane } from "./ResultPane";
import type { WorkbenchController } from "./useWorkbenchController";
export function TemplateWorkbench({
  controller,
}: {
  controller: WorkbenchController;
}) {
  const {
    items,
    active,
    paneRatio,
    saving,
    recovered,
    setRecovered,
    setParamMode,
    setJsonError,
    imported,
    visibleDocuments,
    doc,
    documentChoices,
    selection,
    generation,
    resizePane,
    editDoc,
    openTemplate,
    useTemplate,
    create,
    closeDocument,
    saveDocument,
    copyTemplate,
    deleteTemplate,
    generate,
    busy,
    generateHint,
    setActive,
    setErrors,
  } = controller;
  return (
    <>
      <div className="document-tabs">
        {visibleDocuments.map((document) => (
          <div
            key={document.key}
            className={
              "document-tab " + (document.key === active ? "active" : "")
            }
          >
            <button
              onClick={() => {
                setActive(document.key);
                setErrors({});
                setJsonError("");
                setParamMode("form");
              }}
            >
              <FileCode2 size={14} />
              {document.title}
              {isDirty(document) && <span className="dirty-dot" />}
            </button>
            <button
              aria-label={"关闭" + document.title}
              onClick={() => closeDocument(document.key)}
            >
              <X size={12} />
            </button>
          </div>
        ))}
        <button className="icon-button" title="新建模板" onClick={create}>
          <Plus size={16} />
        </button>
      </div>
      {recovered && (
        <div className="draft-banner">
          <Check size={14} />
          已恢复上次的工作区草稿
          <button className="text-button" onClick={() => setRecovered(false)}>
            知道了
          </button>
        </div>
      )}
      {!doc ? (
        <div className="welcome">
          <div className="welcome-start">
            <div className="welcome-icon">
              <FileCode2 size={30} />
            </div>
            <h1>打开模板，开始工作</h1>
            <p>
              从模板库选择一个模板，填写参数并生成文件。也可以新建模板，或导入已有的
              Jinja 源码。
            </p>
            <div className="welcome-actions">
              <button className="button primary" onClick={create}>
                <Plus size={16} />
                新建模板
              </button>
              <button
                className="button secondary"
                aria-label="导入模板"
                title="导入模板"
                onClick={() => imported.current?.click()}
              >
                <FileInput size={16} />
                导入 .j2 / JSON
              </button>
            </div>
          </div>
          {items.length > 0 && (
            <div className="recent-cards">
              {items.slice(0, 4).map((item) => (
                <button key={item.id} onClick={() => void openTemplate(item)}>
                  <FileCode2 size={20} />
                  <strong>{item.title}</strong>
                  <span>
                    {item.description || "填写参数并生成文件"}
                    <ChevronRight size={14} />
                  </span>
                </button>
              ))}
            </div>
          )}
          <div className="welcome-guide">
            <div>
              <span>01</span>
              <strong>制作模板</strong>
              <p>编写 Jinja 源码，定义参数类型。</p>
            </div>
            <div>
              <span>02</span>
              <strong>填写参数</strong>
              <p>通过表单输入，预设可以反复使用。</p>
            </div>
            <div>
              <span>03</span>
              <strong>生成与交付</strong>
              <p>检查结果，导出文件并保留执行记录。</p>
            </div>
          </div>
        </div>
      ) : (
        <>
          <header className="workbench-toolbar">
            <div className="toolbar-row document-row">
              <div className="document-title">
                <input
                  aria-label="模板名称"
                  value={doc.title}
                  title={doc.title}
                  onChange={(e) => editDoc({ title: e.target.value })}
                />
                <span className="document-state">
                  {isDirty(doc) ? (
                    <span className="dirty-dot" />
                  ) : (
                    <Check size={12} />
                  )}
                  {isDirty(doc) ? "有未保存修改" : "已保存"}
                  {doc.id ? "" : " · 新模板"}
                </span>
              </div>
              <div className="toolbar-actions">
                <button
                  className="button secondary"
                  onClick={() => void saveDocument()}
                  disabled={saving}
                >
                  {saving ? (
                    <LoaderCircle size={15} className="spin" />
                  ) : (
                    <Save size={15} />
                  )}
                  保存
                </button>
                <Menu>
                  <button onClick={() => void copyTemplate()}>
                    <Copy size={14} />
                    复制模板
                  </button>
                  <button
                    onClick={() =>
                      download(
                        encode(
                          {
                            ...doc.asset,
                            metadata: {
                              ...doc.asset.metadata,
                              title: doc.title,
                            },
                          },
                          true,
                        ),
                        doc.title + ".template.json",
                        "application/json",
                      )
                    }
                  >
                    <Download size={14} />
                    导出模板
                  </button>
                  {doc.id && (
                    <button className="danger" onClick={deleteTemplate}>
                      <Trash2 size={14} />
                      删除模板
                    </button>
                  )}
                </Menu>
              </div>
            </div>
            <div className="toolbar-row mode-row">
              <div className="mode-controls">
                <div className="segmented">
                  <button
                    className={doc.mode === "use" ? "selected" : ""}
                    onClick={() => useTemplate()}
                  >
                    <ListChecks size={14} />
                    使用模板
                  </button>
                  <button
                    className={doc.mode === "edit" ? "selected" : ""}
                    onClick={() => editDoc({ mode: "edit" })}
                  >
                    <Code2 size={14} />
                    编辑模板
                  </button>
                </div>
                <label className="generation-choice">
                  生成方式
                  <select
                    aria-label="模板生成方式"
                    value={generation?.id || doc.generationAction || ""}
                    onChange={(event) =>
                      editDoc({ generationAction: event.target.value })
                    }
                  >
                    {!generation && (
                      <option value={doc.generationAction || ""}>
                        请选择可用方式
                      </option>
                    )}
                    {documentChoices.map((entry: Data) => (
                      <option key={entry.id} value={entry.id}>
                        {entry.ui_schema.document_input.title || entry.title}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              {doc.mode === "edit" && (
                <div className="author-generate">
                  <button
                    className="button primary"
                    disabled={busy || !generation}
                    title={generateHint}
                    onClick={() => void generate()}
                  >
                    <Play size={15} />
                    生成文件
                  </button>
                  {busy && <span className="action-hint">{generateHint}</span>}
                </div>
              )}
            </div>
          </header>
          {selection.error && (
            <div className="generation-notice inline-error" role="alert">
              {selection.error}
            </div>
          )}
          {generation?.ui_schema.document_input.match_metadata && (
            <div className="generation-notice" role="status">
              <strong>{generation.ui_schema.document_input.title}</strong> ·{" "}
              {generation.ui_schema.document_input.description}
            </div>
          )}
          <div
            className={
              "workbench-body " + (doc.mode === "edit" ? "author" : "runner")
            }
            style={{
              gridTemplateColumns: `minmax(250px, ${paneRatio[doc.mode]}fr) minmax(250px, ${100 - paneRatio[doc.mode]}fr)`,
            }}
          >
            <Divider
              value={paneRatio[doc.mode]}
              onChange={resizePane}
              author={doc.mode === "edit"}
            />
            {doc.mode === "edit" ? (
              <>
                <AuthorPanes controller={controller} />
              </>
            ) : (
              <ParametersPane controller={controller} />
            )}
            <ResultPane controller={controller} />
          </div>
        </>
      )}
    </>
  );
}
