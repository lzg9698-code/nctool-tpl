/**
 * NC 语法高亮：轻量纯函数 tokenizer（不引 Monaco / CodeMirror，见方案 §4.8）。
 *
 * 输入 NC 文本 → 输出**转义后**的 HTML 字符串，供 `<pre dangerouslySetInnerHTML>` 渲染。
 * 所有非高亮文本先做 HTML 转义，杜绝模板内容注入。
 *
 * token 配色类（见 `styles/industrial.css`）：
 * - `.cmt` 注释（`;` 到行尾、`(...)` 内）
 * - `.kw`  G/M 代码与地址字母（G54 / M30 / X / F）
 * - `.num` 数字
 * - `.var` `#变量` 与括号表达式
 * - `.fn`  多字母标识（如 CYCLE951）
 */

const ESC: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
};

/** HTML 转义（`& < > "`）。 */
export function escapeHtml(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ESC[c] ?? c);
}

// 顺序敏感：G/M 代码与 #变量必须先于通用标识符匹配。
const TOKEN =
  /G\d+(?:\.\d+)?|M\d+|#[0-9]+|[A-Za-z_]+|[+-]?\d+(?:\.\d+)?|\s+|[^\s]/g;

function classify(tok: string): string | null {
  if (/^G\d/.test(tok) || /^M\d+$/.test(tok)) return "kw";
  if (tok.startsWith("#")) return "var";
  if (/^[A-Za-z_]/.test(tok)) return tok.length === 1 ? "kw" : "fn";
  if (/^[+-]?\d/.test(tok)) return "num";
  return null;
}

/** 高亮一段**非注释**代码（内部函数）。 */
function tokenizeCode(seg: string): string {
  if (!seg) return "";
  let out = "";
  for (const m of seg.matchAll(TOKEN)) {
    const tok = m[0];
    const cls = classify(tok);
    out += cls
      ? `<span class="${cls}">${escapeHtml(tok)}</span>`
      : escapeHtml(tok);
  }
  return out;
}

function comment(text: string): string {
  return `<span class="cmt">${escapeHtml(text)}</span>`;
}

/** 高亮单行（不含量行号）。 */
export function highlightLine(line: string): string {
  let out = "";
  let i = 0;
  let codeStart = 0;
  while (i < line.length) {
    const ch = line[i];
    if (ch === ";") {
      out += tokenizeCode(line.slice(codeStart, i));
      out += comment(line.slice(i));
      return out;
    }
    if (ch === "(") {
      out += tokenizeCode(line.slice(codeStart, i));
      // 找匹配的 `)`（支持嵌套）；未闭合则吃到行尾
      let depth = 0;
      let j = i;
      for (; j < line.length; j++) {
        if (line[j] === "(") depth++;
        else if (line[j] === ")") {
          depth--;
          if (depth === 0) break;
        }
      }
      const end = j < line.length ? j + 1 : line.length;
      out += comment(line.slice(i, end));
      i = end;
      codeStart = end;
      continue;
    }
    i++;
  }
  out += tokenizeCode(line.slice(codeStart));
  return out;
}

/**
 * 高亮整段 NC 文本：逐行处理，行首插入独立列的行号 span（`.ln`）。
 *
 * `\r\n` 归一为 `\n`；去掉末尾 split 产生的空行（避免多出一个空行号）。
 */
export function highlightNc(code: string): string {
  const normalized = code.replace(/\r\n/g, "\n");
  const lines = normalized.length ? normalized.split("\n") : [];
  if (lines.length && lines[lines.length - 1] === "") lines.pop();
  return lines
    .map((l, idx) => `<span class="ln">${idx + 1}</span>${highlightLine(l)}`)
    .join("\n");
}
