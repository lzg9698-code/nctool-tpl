import { lazy, Suspense } from "react";
import { ChevronRight, FileCode2, FileInput, Plus } from "lucide-react";

const StarterExamples = lazy(() =>
  import("./StarterExamples").then((module) => ({
    default: module.StarterExamples,
  })),
);

export interface WelcomeTemplate {
  id: string;
  title: string;
  description: string;
  tags: string[];
  fingerprint: string;
}

interface TemplateWelcomeProps {
  items: WelcomeTemplate[];
  onCreate: () => void;
  onImport: () => void;
  onOpen: (item: WelcomeTemplate) => void;
  onSaved: () => void;
  emptyReady: boolean;
}

export function TemplateWelcome({
  items,
  onCreate,
  onImport,
  onOpen,
  onSaved,
  emptyReady,
}: TemplateWelcomeProps) {
  return (
    <div className="welcome">
      <div className="welcome-icon">
        <FileCode2 size={36} />
      </div>
      <p className="eyebrow">YOUR TEMPLATE WORKSPACE</p>
      <h1>让模板成为可重复的工作</h1>
      <p>
        选择一个模板并填写参数，或创建自己的模板。
        <br />
        模板源码、参数定义与输出文件都在同一个工作区。
      </p>
      <div className="welcome-actions">
        <button className="button primary" onClick={onCreate}>
          <Plus size={16} />
          新建模板
        </button>
        {emptyReady && (
          <Suspense fallback={<span>正在加载入门示例…</span>}>
            <StarterExamples onSaved={async () => onSaved()} />
          </Suspense>
        )}
        <button
          className="button secondary"
          aria-label="导入模板"
          title="导入模板"
          onClick={onImport}
        >
          <FileInput size={16} />
          导入 .j2 / JSON
        </button>
      </div>
      {items.length > 0 && (
        <div className="recent-cards">
          {items.slice(0, 4).map((item) => (
            <button key={item.id} onClick={() => onOpen(item)}>
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
  );
}
