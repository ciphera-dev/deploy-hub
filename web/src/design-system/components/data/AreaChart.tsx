import { useEffect, useId, useRef, useState, type CSSProperties, type MouseEvent } from "react";

type Pt = [number, number];

function path(pts: Pt[]): string {
  if (!pts.length) return "";
  let d = "M" + pts[0][0] + "," + pts[0][1];
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[i - 1] || pts[i];
    const p1 = pts[i];
    const p2 = pts[i + 1];
    const p3 = pts[i + 2] || p2;
    const t = 6;
    d += " C" + (p1[0] + (p2[0] - p0[0]) / t) + "," + (p1[1] + (p2[1] - p0[1]) / t) + " " + (p2[0] - (p3[0] - p1[0]) / t) + "," + (p2[1] - (p3[1] - p1[1]) / t) + " " + p2[0] + "," + p2[1];
  }
  return d;
}

export interface AreaChartProps {
  /** valeurs y, une par point */
  data: number[];
  /** etiquette x de chaque point */
  pointLabels: string[];
  /** titre de l'infobulle par point (pointLabels par defaut) */
  tooltipLabels?: string[];
  /** indices de pointLabels affiches sur l'axe x */
  ticks?: number[];
  yMax?: number;
  yTicks?: number[];
  yFormat?: (v: number) => string;
  series?: string;
  valueFormat?: (v: number) => string;
  /** hauteur du trace en px, 170 par defaut */
  height?: number;
  /** garde curseur et infobulle visibles hors survol */
  persistent?: boolean;
  defaultIndex?: number;
  style?: CSSProperties;
}

export function AreaChart({
  data = [],
  pointLabels = [],
  tooltipLabels,
  ticks = [],
  yMax = 100,
  yTicks = [100, 80, 60, 40, 20, 0],
  yFormat = (v) => (v === 0 ? "0" : v + "k"),
  series = "MRR",
  valueFormat = (v) => "$" + Math.round(v) + "K",
  height = 170,
  persistent = false,
  defaultIndex,
  style,
}: AreaChartProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [w, setW] = useState(560);
  const [idx, setIdx] = useState(defaultIndex ?? data.length - 1);
  const [hov, setHov] = useState(false);
  const show = hov || persistent;
  useEffect(() => {
    if (!ref.current || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver((e) => setW(e[0].contentRect.width));
    ro.observe(ref.current);
    return () => ro.disconnect();
  }, []);
  const pad = 6;
  const n = data.length;
  const x = (i: number) => pad + (w - pad * 2) * (i / (n - 1 || 1));
  const y = (v: number) => height - (v / yMax) * height;
  const pts: Pt[] = data.map((v, i) => [x(i), y(v)]);
  const line = path(pts);
  const area = line + " L" + x(n - 1) + "," + height + " L" + x(0) + "," + height + " Z";
  const onMove = (e: MouseEvent<HTMLDivElement>) => {
    if (!ref.current) return;
    const r = ref.current.getBoundingClientRect();
    const f = (e.clientX - r.left - pad) / (w - pad * 2);
    setIdx(Math.max(0, Math.min(n - 1, Math.round(f * (n - 1)))));
  };
  const ax = x(idx);
  const ay = y(data[idx] || 0);
  const flip = ax > w * 0.55;
  const gid = useId().replace(/:/g, "");
  return (
    <div style={{ display: "grid", gridTemplateColumns: "34px minmax(0,1fr)", columnGap: 8, font: "400 11px/1 var(--font-sans)", ...style }}>
      <div style={{ position: "relative", height }}>
        {yTicks.map((t) => (
          <div key={t} style={{ position: "absolute", left: 0, top: y(t) - 5, color: "var(--text-body)" }}>{yFormat(t)}</div>
        ))}
      </div>
      <div ref={ref} onMouseMove={onMove} onMouseEnter={() => setHov(true)} onMouseLeave={() => setHov(false)} style={{ position: "relative", height, cursor: "crosshair" }}>
        <svg width={w} height={height} style={{ position: "absolute", inset: 0, overflow: "visible" }} aria-hidden="true">
          <defs>
            <linearGradient id={"a" + gid} x1="0" x2="0" y1="0" y2="1">
              <stop offset="0" stopColor="#fff" stopOpacity=".10" />
              <stop offset=".6" stopColor="#fff" stopOpacity=".03" />
              <stop offset="1" stopColor="#fff" stopOpacity="0" />
            </linearGradient>
          </defs>
          {yTicks.map((t) => (
            <line key={t} x1="0" x2={w} y1={y(t)} y2={y(t)} stroke="#1d1d1d" strokeDasharray="2 3" />
          ))}
          <path d={area} fill={"url(#a" + gid + ")"} />
          <path d={line} fill="none" stroke="var(--viz-line)" strokeWidth="1.5" />
          {show && (
            <>
              <line x1={ax} x2={ax} y1={-10} y2={height + 8} stroke="#8a8a8a" strokeWidth="1" />
              <circle cx={ax} cy={ay} r="4" fill="#9a9a9a" stroke="#0b0b0b" strokeWidth="1.5" />
            </>
          )}
        </svg>
        {show && (
          <div style={{ position: "absolute", top: -14, left: flip ? undefined : ax + 12, right: flip ? w - ax + 12 : undefined, minWidth: 108, padding: "9px 10px", background: "var(--surface-tooltip)", borderRadius: "var(--radius-md)", boxShadow: "var(--shadow-tooltip)", pointerEvents: "none", display: "flex", flexDirection: "column", gap: 10 }}>
            <div style={{ color: "var(--text-strong)" }}>{(tooltipLabels || pointLabels)[idx]}</div>
            <div style={{ display: "flex", alignItems: "center", gap: 8, font: "400 12px/1 var(--font-sans)" }}>
              <span style={{ width: 8, height: 8, borderRadius: 99, background: "var(--viz-1)" }} />
              <span style={{ color: "var(--text-body)", flex: 1 }}>{series}</span>
              <span style={{ color: "var(--text-strong)", fontWeight: 500, marginLeft: 16 }}>{valueFormat(data[idx] || 0)}</span>
            </div>
          </div>
        )}
      </div>
      <div />
      <div style={{ position: "relative", height: 30, marginTop: 6 }}>
        {ticks
          .filter((i) => !show || Math.abs(x(i) - ax) > 44)
          .map((i) => (
            <div key={i} style={{ position: "absolute", top: 8, left: x(i), transform: i === 0 ? "none" : "translateX(-50%)", color: "var(--text-body)", whiteSpace: "nowrap" }}>{pointLabels[i]}</div>
          ))}
        {show && (
          <div style={{ position: "absolute", top: 0, left: ax, transform: "translateX(-50%)", height: 28, padding: "0 14px", display: "flex", alignItems: "center", borderRadius: 999, background: "var(--surface-inverse)", color: "var(--text-inverse)", font: "500 12px/1 var(--font-sans)", whiteSpace: "nowrap" }}>{pointLabels[idx]}</div>
        )}
      </div>
    </div>
  );
}
