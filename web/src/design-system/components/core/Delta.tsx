import type { CSSProperties } from "react";

export interface DeltaProps {
  /** ex. "2,7 %" */
  value: string;
  direction?: "up" | "down";
  /** texte attenue a droite, ex. "vs 30 jours precedents" */
  suffix?: string;
  style?: CSSProperties;
}

export function Delta({ value, direction = "up", suffix, style }: DeltaProps) {
  const up = direction === "up";
  const c = up ? "var(--text-positive)" : "var(--text-negative)";
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 5, font: "400 11px/1 var(--font-sans)", whiteSpace: "nowrap", ...style }}>
      <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden="true">
        <circle cx="6" cy="6" r="6" fill={c} />
        <path d={up ? "M6 3.2v5.6M3.6 5.6 6 3.2l2.4 2.4" : "M6 8.8V3.2M3.6 6.4 6 8.8l2.4-2.4"} stroke="#0b0b0b" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
      <span style={{ color: c }}>{value}</span>
      {suffix && <span style={{ color: "var(--text-subtle)" }}>{suffix}</span>}
    </span>
  );
}
