import type { CSSProperties } from "react";

export interface TickBarProps {
  /** part de 0 a 1 de graduations allumees */
  value?: number;
  ticks?: number;
  height?: number;
  showPercent?: boolean;
  style?: CSSProperties;
}

export function TickBar({ value = 0.78, ticks = 96, height = 22, showPercent = true, style }: TickBarProps) {
  const on = Math.round(value * ticks);
  return (
    <div style={{ position: "relative", paddingTop: showPercent ? 18 : 0, ...style }}>
      {showPercent && (
        <div style={{ position: "absolute", top: 0, left: value * 100 + "%", transform: "translateX(-100%)", font: "500 11px/1 var(--font-sans)", color: "var(--text-strong)" }}>
          {Math.round(value * 100)}%
        </div>
      )}
      <div style={{ display: "flex", justifyContent: "space-between", height }}>
        {Array.from({ length: ticks }, (_, i) => (
          <span key={i} style={{ width: 1.5, background: i < on ? "var(--viz-1)" : "var(--viz-4)" }} />
        ))}
      </div>
    </div>
  );
}
