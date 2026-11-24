import type { CSSProperties } from "react";
import { Delta } from "../core/Delta";

export interface TileProps {
  label: string;
  value: string;
  /** variation en pourcentage ; le signe donne la direction */
  delta?: number;
  hint?: string;
  style?: CSSProperties;
}

const pct = new Intl.NumberFormat("fr-CH", { maximumFractionDigits: 1 });

/** Carte KPI : libelle, valeur, variation optionnelle et indication. */
export function Tile({ label, value, delta, hint, style }: TileProps) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10, minWidth: 0, padding: 16, borderRadius: "var(--radius-lg)", background: "var(--surface-raised)", border: "1px solid var(--border-default)", ...style }}>
      <div style={{ font: "400 12px/1.2 var(--font-sans)", color: "var(--text-subtle)" }}>{label}</div>
      <div style={{ font: "500 22px/1.1 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.01em" }}>{value}</div>
      {delta !== undefined && <Delta value={pct.format(Math.abs(delta)) + " %"} direction={delta >= 0 ? "up" : "down"} />}
      {hint && <div style={{ font: "400 11px/1.3 var(--font-sans)", color: "var(--text-faint)" }}>{hint}</div>}
    </div>
  );
}
