import { useMemo } from "react";
import { diffLines } from "diff";
export function DiffView({
  before,
  after,
  beforeTitle,
  afterTitle,
}: {
  before: string;
  after: string;
  beforeTitle: string;
  afterTitle: string;
}) {
  const changes = useMemo(() => {
    if (before.length + after.length > 2 * 1024 * 1024) return null;
    return diffLines(before, after, { timeout: 1000 });
  }, [before, after]);
  let oldLine = 0,
    newLine = 0;
  return (
    <section className="diff-view">
      <header>
        <strong>{beforeTitle}</strong>
        <span>→</span>
        <strong>{afterTitle}</strong>
      </header>
      {changes === null ? (
        <p className="hint">结果超过比较大小限制，请导出文件后比较。</p>
      ) : !changes ? (
        <p className="hint">比较超时，请导出文件后比较。</p>
      ) : (
        <div className="diff-scroll">
          {changes.flatMap((part, index) =>
            part.value
              .replace(/\n$/, "")
              .split("\n")
              .map((line, row) => {
                const left = part.added ? "" : String(++oldLine);
                const right = part.removed ? "" : String(++newLine);
                return (
                  <div
                    className={
                      "diff-line " +
                      (part.added ? "added" : part.removed ? "removed" : "")
                    }
                    key={index + "-" + row}
                  >
                    <span>{left}</span>
                    <span>{right}</span>
                    <code>
                      {part.added ? "+" : part.removed ? "-" : " "}{" "}
                      {line || " "}
                    </code>
                  </div>
                );
              }),
          )}
        </div>
      )}
    </section>
  );
}
