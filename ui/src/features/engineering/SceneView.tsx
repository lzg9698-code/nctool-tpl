import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Checkbox, Slider, Space, Typography } from "antd";
import { bounds, pathData, pointAt, type Scene } from "./scene";

export function SceneView({
  scene,
  selected,
  onSelect,
}: {
  scene: Scene;
  selected: string;
  onSelect: (id: string) => void;
}) {
  const fit = useMemo(() => bounds(scene), [scene]),
    [camera, setCamera] = useState(fit),
    [playing, setPlaying] = useState(false),
    [progress, setProgress] = useState(0),
    [hidden, setHidden] = useState<string[]>([]);
  const svg = useRef<SVGSVGElement>(null),
    drag = useRef<{ x: number; y: number; camera: number[] } | null>(null);
  useEffect(() => {
    setPlaying(false);
    setProgress(0);
    setCamera(fit);
  }, [fit]);
  useEffect(() => {
    if (!playing) return;
    let frame = 0,
      last = 0;
    const tick = (time: number) => {
      if (last) setProgress((p) => Math.min(1, p + (time - last) / 8000));
      last = time;
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    const stop = () => {
      if (document.hidden) setPlaying(false);
    };
    document.addEventListener("visibilitychange", stop);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("visibilitychange", stop);
    };
  }, [playing]);
  useEffect(() => {
    if (progress >= 1) setPlaying(false);
  }, [progress]);
  const path = scene.path.map((id) => scene.items.find((s) => s.id === id)!),
    total = path.reduce((sum, s) => sum + s.length, 0);
  let remaining = total * progress,
    cursor: [number, number] | undefined;
  for (const s of path) {
    if (remaining <= s.length) {
      cursor = pointAt(s, s.length ? remaining / s.length : 0);
      break;
    }
    remaining -= s.length;
  }
  const layers = [...new Set(scene.items.map((s) => s.layer))];
  return (
    <section className="engineering-scene" aria-label="二维场景">
      <Space wrap>
        <Button onClick={() => setCamera(fit)}>适配全图</Button>
        <Button
          aria-label={playing ? "暂停" : "播放"}
          disabled={!path.length}
          onClick={() => {
            if (progress === 1) setProgress(0);
            setPlaying(!playing);
          }}
        >
          {playing ? "暂停" : "播放"}
        </Button>
        {layers.map((layer) => (
          <Checkbox
            key={layer}
            checked={!hidden.includes(layer)}
            onChange={(e) =>
              setHidden(
                e.target.checked
                  ? hidden.filter((l) => l !== layer)
                  : [...hidden, layer],
              )
            }
          >
            {layer}
          </Checkbox>
        ))}
      </Space>
      <svg
        ref={svg}
        role="img"
        aria-label="二维轨迹图，支持滚轮缩放和拖动平移"
        viewBox={camera.join(" ")}
        style={{
          width: "100%",
          height: 420,
          touchAction: "none",
          background: "#f7f9fc",
        }}
        onWheel={(e) => {
          e.preventDefault();
          const factor = e.deltaY > 0 ? 1.15 : 1 / 1.15;
          setCamera(([x, y, w, h]) => [
            x + (w - w * factor) / 2,
            y + (h - h * factor) / 2,
            w * factor,
            h * factor,
          ]);
        }}
        onPointerDown={(e) => {
          drag.current = { x: e.clientX, y: e.clientY, camera };
          e.currentTarget.setPointerCapture(e.pointerId);
        }}
        onPointerUp={() => {
          drag.current = null;
        }}
        onPointerCancel={() => {
          drag.current = null;
        }}
        onPointerMove={(e) => {
          if (!drag.current || !svg.current) return;
          const rect = svg.current.getBoundingClientRect(),
            d = drag.current,
            scale = Math.max(
              d.camera[2] / rect.width,
              d.camera[3] / rect.height,
            );
          setCamera([
            d.camera[0] - (e.clientX - d.x) * scale,
            d.camera[1] - (e.clientY - d.y) * scale,
            d.camera[2],
            d.camera[3],
          ]);
        }}
      >
        <title>解析二维场景；坐标等比例显示</title>
        {[0, 1, 2, 3, 4].map((i) => {
          const x = camera[0] + (camera[2] * i) / 4,
            y = camera[1] + (camera[3] * i) / 4;
          return (
            <g key={i} fill="#596579" fontSize={camera[2] / 45}>
              <text x={x} y={camera[1] + camera[3] * 0.97}>
                {Number(x.toFixed(3))}
              </text>
              <text x={camera[0] + camera[2] * 0.01} y={y}>
                {Number((-y * scene.axes.y_scale).toFixed(3))}
              </text>
            </g>
          );
        })}
        {scene.items
          .filter((s) => !hidden.includes(s.layer))
          .map((s) =>
            s.kind === "point" ? (
              <circle
                key={s.id}
                cx={s.start[0]}
                cy={-s.start[1]}
                r={camera[2] / 150}
                fill={s.entity === selected ? "#d46b08" : "#315bea"}
                onPointerDown={(e) => e.stopPropagation()}
                onClick={() => onSelect(s.entity)}
              >
                <title>{s.entity}</title>
              </circle>
            ) : s.kind === "text" ? (
              <text
                key={s.id}
                x={s.start[0]}
                y={-s.start[1]}
                fontSize={camera[2] / 60}
                fill="#425169"
                onPointerDown={(e) => e.stopPropagation()}
                onClick={() => onSelect(s.entity)}
              >
                {s.text}
              </text>
            ) : (
              <path
                key={s.id}
                d={pathData(s)}
                fill="none"
                stroke={
                  s.entity === selected
                    ? "#d46b08"
                    : s.layer === layers[0]
                      ? "#a3adba"
                      : "#315bea"
                }
                strokeWidth={s.entity === selected ? 3 : 1.5}
                vectorEffect="non-scaling-stroke"
                style={{ cursor: "pointer" }}
                onPointerDown={(e) => e.stopPropagation()}
                onClick={() => onSelect(s.entity)}
              >
                <title>
                  {s.entity} · {s.id}
                </title>
              </path>
            ),
          )}
        {cursor && (
          <circle
            cx={cursor[0]}
            cy={-cursor[1]}
            r={camera[2] / 150}
            fill="#d4380d"
          />
        )}
      </svg>
      <Slider
        aria-label="路径进度"
        min={0}
        max={1000}
        value={Math.round(progress * 1000)}
        onChange={(v) => {
          setPlaying(false);
          setProgress(v / 1000);
        }}
      />
      <Typography.Text type="secondary">
        {scene.axes.x} · {scene.axes.y}（显示倍率 {scene.axes.y_scale}
        ）；图形按原始坐标等比例。{Math.round(progress * 100)}%
      </Typography.Text>
    </section>
  );
}
