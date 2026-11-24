import type { CSSProperties, ReactNode } from "react";
import { Icon, type IconName } from "../core/Icon";

export interface RadialGaugeProps {
  /** part de 0 a 1 de graduations allumees */
  value?: number;
  segments?: number;
  size?: number;
  /** icone lucide dans le disque central */
  icon?: IconName;
  label?: string;
  amount?: ReactNode;
  /** arc en degres, 260 par defaut */
  sweep?: number;
  style?: CSSProperties;
}

export function RadialGauge({ value = 0.78, segments = 56, size = 224, icon = "Wallet", label = "Total Revenue", amount, sweep = 260, style }: RadialGaugeProps) {
  const c = size / 2;
  const ro = size / 2 - 2;
  const len = size * 0.1;
  const on = Math.round(value * segments);
  const ticks = Array.from({ length: segments }, (_, i) => {
    const a = -sweep / 2 + sweep * (i / (segments - 1));
    return <rect key={i} x={c - 2.6} y={c - ro} width="5.2" height={len} rx="2.6" fill={i < on ? "var(--viz-1)" : "var(--viz-3)"} transform={"rotate(" + a + " " + c + " " + c + ")"} />;
  });
  return (
    <div style={{ position: "relative", width: size, height: size * 0.86, ...style }}>
      <svg width={size} height={size} style={{ position: "absolute", top: 0, left: 0 }} aria-hidden="true">
        {ticks}
      </svg>
      <div style={{ position: "absolute", left: 0, right: 0, top: size * 0.33, display: "flex", flexDirection: "column", alignItems: "center", gap: 8 }}>
        <span style={{ width: 30, height: 30, borderRadius: 99, background: "var(--surface-selected)", display: "flex", alignItems: "center", justifyContent: "center", marginBottom: 6 }}>
          <Icon name={icon} size={14} color="var(--text-muted)" />
        </span>
        <span style={{ font: "400 11px/1 var(--font-sans)", color: "var(--text-muted)" }}>{label}</span>
        <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{amount}</span>
      </div>
    </div>
  );
}
