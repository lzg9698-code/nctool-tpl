import { useEffect, useState } from "react";
import {
  ArrowLeft,
  BookOpen,
  Check,
  Copy,
  Play,
  Search,
  ExternalLink,
} from "lucide-react";
import { PageTitle, Empty } from "./WorkbenchUi";
import { action } from "../lib/api";
import { encode, type Data } from "../lib/data";
import {
  syntaxCategories,
  syntaxExamples,
  type SyntaxExample,
} from "../lib/syntaxGuide";

export function SyntaxGuide({
  capabilities,
  back,
  notify,
}: {
  capabilities: Data;
  back: () => void;
  notify: (message: string, error?: boolean) => void;
}) {
  const [category, setCategory] = useState(syntaxCategories[0]);
  const [query, setQuery] = useState("");
  const [copied, setCopied] = useState("");
  const [runs, setRuns] = useState<
    Record<string, { pending?: boolean; text?: string; error?: string }>
  >({});
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(""), 2000);
    return () => clearTimeout(timer);
  }, [copied]);
  const available = (example: SyntaxExample) =>
    !example.plugin ||
    capabilities.actions.some(
      (entry: Data) =>
        entry.id === (example.plugin === "math" ? "math.info" : "nc.generate"),
    );
  const matching = syntaxExamples.filter((example) => {
    if (!query.trim()) return example.category === category;
    const needle = query.trim().toLocaleLowerCase();
    return [
      example.title,
      example.description,
      example.source,
      example.note || "",
      example.category,
    ]
      .join(" ")
      .toLocaleLowerCase()
      .includes(needle);
  });
  async function copy(example: SyntaxExample) {
    try {
      await navigator.clipboard.writeText(example.source);
      setCopied(example.id);
      notify("已复制模板片段");
    } catch {
      notify("无法访问剪贴板，请直接选择片段文字复制", true);
    }
  }
  async function render(example: SyntaxExample) {
    setRuns((current) => ({ ...current, [example.id]: { pending: true } }));
    try {
      const result = await action(example.action || "template.render", {
        ...example.input,
        source: example.source,
      });
      setRuns((current) => ({
        ...current,
        [example.id]: { text: result.data.text },
      }));
    } catch (error) {
      setRuns((current) => ({
        ...current,
        [example.id]: { error: (error as Error).message },
      }));
    }
  }
  return (
    <div className="page-content syntax-page">
      <PageTitle
        title="模板语法"
        description="查语法、复制片段，用示例数据试跑。返回工作区后可继续编辑原来的草稿。"
        action={
          <button className="button secondary" onClick={back}>
            <ArrowLeft size={15} />
            返回工作区
          </button>
        }
      />
      <section className="syntax-intro panel">
        <div>
          <BookOpen size={20} />
          <strong>三种基本标记</strong>
          <span>普通文字会直接保留在结果中。</span>
        </div>
        <dl>
          <div>
            <dt>
              <code>{"{{ 变量或表达式 }}"}</code>
            </dt>
            <dd>输出数据</dd>
          </div>
          <div>
            <dt>
              <code>{"{% 控制语句 %}"}</code>
            </dt>
            <dd>条件、循环、定义与引用</dd>
          </div>
          <div>
            <dt>
              <code>{"{# 注释 #}"}</code>
            </dt>
            <dd>只给作者看，不输出</dd>
          </div>
        </dl>
        <p>
          默认严格模式：缺少需要的变量会报错。可选变量请用 default 或 is defined
          明确处理。这里使用 Jinja 兼容语法；运行结果以当前引擎为准。
        </p>
      </section>
      <div className="syntax-tools">
        <div className="syntax-categories" aria-label="语法分类">
          {syntaxCategories.map((item) => (
            <button
              key={item}
              aria-pressed={!query && category === item}
              className={!query && category === item ? "selected" : ""}
              onClick={() => {
                setCategory(item);
                setQuery("");
              }}
            >
              {item}
            </button>
          ))}
        </div>
        <label className="syntax-search">
          <Search size={15} />
          <input
            aria-label="搜索模板语法"
            placeholder="搜索语法、关键词或过滤器…"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
      </div>
      {query && (
        <p className="hint" role="status">
          跨分类找到 {matching.length} 个示例
        </p>
      )}
      <div className="syntax-examples">
        {matching.map((example) => (
          <article className="panel syntax-card" key={example.id}>
            <header>
              <div>
                <span className="syntax-badge">
                  {example.plugin
                    ? example.plugin === "math"
                      ? "数学插件"
                      : "NC 插件"
                    : "通用模板"}
                </span>
                <h2>{example.title}</h2>
              </div>
              <button
                className="button secondary"
                aria-label={"复制示例：" + example.title}
                onClick={() => void copy(example)}
              >
                {copied === example.id ? (
                  <Check size={14} />
                ) : (
                  <Copy size={14} />
                )}{" "}
                {copied === example.id ? "已复制" : "复制片段"}
              </button>
            </header>
            <p>{example.description}</p>
            <pre className="syntax-code" tabIndex={0}>
              <code>{example.source}</code>
            </pre>
            <details className="syntax-input">
              <summary>
                示例参数{example.input.templates ? "与引用模板" : ""}
              </summary>
              <pre>{encode(example.input, true)}</pre>
            </details>
            {example.note && <p className="syntax-note">{example.note}</p>}
            <div className="syntax-run">
              <button
                className="button secondary"
                disabled={!available(example) || runs[example.id]?.pending}
                onClick={() => void render(example)}
              >
                <Play size={14} />
                {runs[example.id]?.pending ? "试跑中…" : "试跑示例"}
              </button>
              <span>
                {available(example)
                  ? "使用示例数据，不修改你的模板或参数。"
                  : "当前未启用所需插件，请在插件与设置中启用并重启。"}
              </span>
            </div>
            {runs[example.id]?.error && (
              <div className="inline-error" role="alert">
                {runs[example.id].error}
              </div>
            )}
            {runs[example.id]?.text !== undefined && (
              <div className="syntax-result" role="status">
                <strong>示例输出</strong>
                <pre>{runs[example.id].text || "（空输出）"}</pre>
              </div>
            )}
          </article>
        ))}
      </div>
      {!matching.length && (
        <Empty
          title="没有匹配的语法"
          description="试试变量、循环、include、default 或 nc_fixed。"
        />
      )}
      <section className="syntax-tips">
        <h2>遇到错误时</h2>
        <ul>
          <li>
            <strong>变量未定义：</strong>
            检查名称、嵌套路径与参数类型；可选字段用默认值或存在性判断。
          </li>
          <li>
            <strong>过滤器不存在：</strong>
            核对拼写、当前插件和渲染扩展。普通文本渲染不会自动注入 machine。
          </li>
          <li>
            <strong>引用模板找不到：</strong>
            使用已保存模板的稳定标识，保留引用关系；不要使用本地绝对路径。
          </li>
          <li>
            <strong>换行意外消失：</strong>检查标签边缘的 -
            和空白选项。引擎通常会移除模板末尾的一个换行。
          </li>
        </ul>
        <a
          href="https://docs.rs/minijinja/2.24.0/minijinja/syntax/index.html"
          target="_blank"
          rel="noreferrer"
        >
          查看 MiniJinja 完整语法参考 <ExternalLink size={13} />
        </a>
      </section>
    </div>
  );
}
