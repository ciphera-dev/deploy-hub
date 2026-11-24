import type { CSSProperties, ReactNode } from "react";
import { Delta } from "../core/Delta";

export interface StatProps {
  label: string;
  value: ReactNode;
  /** ex. "2,7 %" */
  delta?: string;
  direction?: "up" | "down";
  deltaSuffix?: string;
  /** lg = valeur d'en-tete de 24px */
  size?: "md" | "lg";
  /** libelle sous la valeur */
  labelBelow?: boolean;
  style?: CSSProperties;
}

export function Stat({ label, value, delta, direction = "up", deltaSuffix, size = "md", labelBelow = false, style }: StatProps) {
  const lg = size === "lg";
  const L = <div style={{ font: "400 12px/1.2 var(--font-sans)", color: "var(--text-subtle)" }}>{label}</div>;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: lg ? 6 : 10, minWidth: 0, ...style }}>
      {!labelBelow && L}
      <div style={{ font: "500 " + (lg ? 24 : 22) + "px/1.1 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.01em" }}>{value}</div>
      {labelBelow && L}
      {delta && <Delta value={delta} direction={direction} suffix={deltaSuffix} />}
    </div>
  );
}
