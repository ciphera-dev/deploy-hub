import type { CSSProperties } from "react";

export interface LegendItemProps {
  color?: string;
  label: string;
  /** soulignement pointille sous le libelle (defaut true) */
  underline?: boolean;
  style?: CSSProperties;
}

export function LegendItem({ color = "var(--viz-1)", label, underline = true, style }: LegendItemProps) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 8, font: "400 11px/1 var(--font-sans)", color: "var(--text-muted)", whiteSpace: "nowrap", ...style }}>
      <span style={{ width: 6, height: 6, borderRadius: 99, background: color }} />
      <span style={{ paddingBottom: 2, borderBottom: underline ? "1px dashed var(--gray-500)" : "none" }}>{label}</span>
    </span>
  );
}
