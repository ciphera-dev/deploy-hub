import type { CSSProperties } from "react";
import { LegendItem } from "../core/LegendItem";

export interface SegmentedBarSegment {
  /** largeur relative, en general un pourcentage */
  value: number;
  /** etiquette au-dessus, value+"%" par defaut */
  label?: string;
  /** nom dans la legende */
  name: string;
  color: string;
}

export interface SegmentedBarProps {
  segments: SegmentedBarSegment[];
  showLegend?: boolean;
  style?: CSSProperties;
}

export function SegmentedBar({ segments = [], showLegend = true, style }: SegmentedBarProps) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 0, ...style }}>
      <div style={{ display: "flex", gap: 4 }}>
        {segments.map((s, i) => (
          <div key={i} style={{ flex: s.value, minWidth: 0 }}>
            <div style={{ font: "400 11px/1 var(--font-sans)", color: "var(--text-muted)", marginBottom: 6 }}>{s.label ?? s.value + "%"}</div>
            <div style={{ width: 1, height: 14, background: "var(--gray-500)", marginBottom: 6 }} />
            <div style={{ height: 12, borderRadius: 999, background: s.color }} />
          </div>
        ))}
      </div>
      {showLegend && (
        <div style={{ display: "flex", gap: 18, marginTop: 22 }}>
          {segments.map((s, i) => (
            <LegendItem key={i} color={s.color} label={s.name} underline={false} />
          ))}
        </div>
      )}
    </div>
  );
}
