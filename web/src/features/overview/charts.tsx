import { useEffect, useRef, useState } from "react";

// Pages2.jsx#AreaChart : courbe lissée, graduations, info-bulle au survol et
// cinq libellés de jours. Une série vide ou nulle garde le cadre (aucune
// valeur inventée) : l'échelle reste à 1.

const H = 240;

export function AreaChart({ series, labels, fmt, col }: { series: number[]; labels: string[]; fmt: (v: number) => string; col: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [w, setW] = useState(800);
  const [i, setI] = useState<number | null>(null);
  useEffect(() => {
    if (!ref.current || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver((e) => setW(e[0]!.contentRect.width));
    ro.observe(ref.current);
    return () => ro.disconnect();
  }, []);
  const n = series.length;
  const max = Math.max(1, ...series) * 1.15;
  const x = (k: number) => (n > 1 ? (k / (n - 1)) * w : 0);
  const y = (v: number) => H - (v / max) * H;
  const pts = series.map((v, k) => [x(k), y(v)] as const);
  const path = pts
    .map((q, k) => {
      if (!k) return `M${q[0]},${q[1]}`;
      const pr = pts[k - 1]!;
      const cx = (pr[0] + q[0]) / 2;
      return `C${cx},${pr[1]} ${cx},${q[1]} ${q[0]},${q[1]}`;
    })
    .join(" ");
  const ticks = [0.25, 0.5, 0.75, 1].map((f) => max * f);
  const at = i != null && i < n ? i : null;
  return (
    <div
      ref={ref}
      role="img"
      aria-label={n ? "Courbe de la période" : "Aucune mesure"}
      style={{ position: "relative", height: H + 28 }}
      onMouseMove={(e) => {
        if (!ref.current || n === 0) return;
        const r = ref.current.getBoundingClientRect();
        setI(Math.max(0, Math.min(n - 1, Math.round(((e.clientX - r.left) / (w || 1)) * (n - 1)))));
      }}
      onMouseLeave={() => setI(null)}
    >
      <svg width={w} height={H} style={{ position: "absolute", left: 0, top: 0, overflow: "visible" }}>
        <defs>
          <linearGradient id="ovA" x1="0" x2="0" y1="0" y2="1">
            <stop offset="0" stopColor={col} stopOpacity=".16" />
            <stop offset="1" stopColor={col} stopOpacity="0" />
          </linearGradient>
        </defs>
        {ticks.map((v) => (
          <g key={v}>
            <line x1="0" x2={w} y1={y(v)} y2={y(v)} stroke="rgba(255,255,255,.05)" />
            {n > 0 && (
              <text x={w} y={y(v) - 6} textAnchor="end" fill="rgba(255,255,255,.3)" style={{ font: "400 10px var(--font-mono)" }}>
                {fmt(v)}
              </text>
            )}
          </g>
        ))}
        {n > 1 && (
          <>
            <path d={`${path} L${w},${H} L0,${H} Z`} fill="url(#ovA)" />
            <path d={path} fill="none" stroke={col} strokeWidth="1.75" />
          </>
        )}
        {at != null && (
          <>
            <line x1={x(at)} x2={x(at)} y1={y(series[at]!) + 8} y2={H} stroke={col} strokeOpacity=".35" strokeDasharray="3 3" />
            <circle cx={x(at)} cy={y(series[at]!)} r="9" fill={col} fillOpacity=".15" />
            <circle cx={x(at)} cy={y(series[at]!)} r="4" fill={col} stroke="#0b0b0b" strokeWidth="2" />
          </>
        )}
      </svg>
      {at != null && (
        <div style={{ position: "absolute", top: Math.max(-6, y(series[at]!) - 46), left: Math.min(w - 85, Math.max(85, x(at))), transform: "translateX(-50%)", display: "flex", alignItems: "center", gap: 8, height: 30, padding: "0 10px", borderRadius: 99, background: "#1b1b20", border: "1px solid rgba(255,255,255,.1)", boxShadow: "0 8px 20px -8px rgba(0,0,0,.8)", pointerEvents: "none", whiteSpace: "nowrap" }}>
          <span style={{ width: 7, height: 7, borderRadius: 99, background: col }} />
          <span style={{ font: "600 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{fmt(series[at]!)}</span>
          <span style={{ width: 1, height: 12, background: "rgba(255,255,255,.12)" }} />
          <span style={{ font: "400 11px/1 var(--font-sans)", color: "var(--text-muted)" }}>{labels[at]}</span>
        </div>
      )}
      {n > 0 && (
        <div style={{ position: "absolute", left: 0, right: 0, bottom: 0, display: "flex", justifyContent: "space-between", font: "400 11px/1 var(--font-sans)", color: "var(--text-faint)" }}>
          {[0, 0.25, 0.5, 0.75, 1].map((f) => (
            <span key={f}>{labels[Math.round(f * (n - 1))]}</span>
          ))}
        </div>
      )}
    </div>
  );
}
