import type { CSSProperties } from "react";

export interface BarProps {
  value: number;
  max: number;
  tone?: "default" | "warn" | "danger";
  style?: CSSProperties;
}

const FILL = { default: "var(--viz-1)", warn: "#d9a441", danger: "var(--text-negative)" };

/** Barre de progression / jauge lineaire. */
export function Bar({ value, max, tone = "default", style }: BarProps) {
  const ratio = max > 0 ? Math.max(0, Math.min(1, value / max)) : 0;
  return (
    <div role="progressbar" aria-valuemin={0} aria-valuemax={max} aria-valuenow={value} data-tone={tone} style={{ height: 6, borderRadius: 999, background: "var(--viz-4)", overflow: "hidden", ...style }}>
      <div style={{ width: ratio * 100 + "%", height: "100%", borderRadius: 999, background: FILL[tone] }} />
    </div>
  );
}
