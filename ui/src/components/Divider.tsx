import { useRef, useEffect } from "react";
export function Divider({
  value,
  onChange,
  author,
}: {
  value: number;
  onChange: (v: number) => void;
  author: boolean;
}) {
  const cleanup = useRef<(() => void) | null>(null);
  useEffect(() => () => cleanup.current?.(), []);
  return (
    <div
      className={"pane-divider " + (author ? "author" : "")}
      role="separator"
      aria-label="调整面板宽度"
      aria-orientation="vertical"
      aria-valuemin={25}
      aria-valuemax={70}
      aria-valuenow={value}
      tabIndex={0}
      style={{ left: `calc(${value}% - 7px)` }}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
          e.preventDefault();
          onChange(
            Math.min(
              70,
              Math.max(25, value + (e.key === "ArrowLeft" ? -2 : 2)),
            ),
          );
        }
      }}
      onPointerDown={(e) => {
        const parent = e.currentTarget.parentElement;
        if (!parent) return;
        const bounds = parent.getBoundingClientRect();
        e.preventDefault();
        document.body.style.cursor = "col-resize";
        document.body.style.userSelect = "none";
        const move = (event: PointerEvent) =>
          onChange(
            Math.min(
              70,
              Math.max(
                25,
                ((event.clientX - bounds.left) / bounds.width) * 100,
              ),
            ),
          );
        const finish = () => {
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", finish);
          document.body.style.cursor = "";
          document.body.style.userSelect = "";
          cleanup.current = null;
        };
        cleanup.current = finish;
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", finish);
      }}
    />
  );
}
