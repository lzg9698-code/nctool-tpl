import { highlightNc } from "../lib/highlight";

interface Props {
  code: string;
  blocked: boolean;
  error: { kind: string; message: string } | null;
}

export default function NcPreview({ code, blocked, error }: Props) {
  if (error) {
    return (
      <div className="nc-error">
        <div className="nc-error-kind">{error.kind}</div>
        <div>{error.message}</div>
      </div>
    );
  }
  if (blocked) {
    return (
      <div className="nc-blocked">
        校验未通过，已阻止生成 G-code（不产出错误程序）。请修正左侧参数。
      </div>
    );
  }
  if (!code) {
    return (
      <div className="nc-empty">选择模板并填写参数后，此处实时预览 NC 代码。</div>
    );
  }
  return (
    <pre
      className="nc-preview"
      // 内容已由 highlightNc 全量 HTML 转义，无注入面。
      dangerouslySetInnerHTML={{ __html: highlightNc(code) }}
    />
  );
}
