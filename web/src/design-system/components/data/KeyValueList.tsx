import type { CSSProperties, ReactNode } from "react";

export interface KeyValueListProps {
  title?: string;
  rows: { label: string; value: ReactNode }[];
  style?: CSSProperties;
}

export function KeyValueList({ title, rows = [], style }: KeyValueListProps) {
  return (
    <div style={{ display: "flex", flexDirection: "column", ...style }}>
      {title && <div style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)", marginBottom: 22 }}>{title}</div>}
      <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        {rows.map((r, i) => (
          <div key={i} style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, minHeight: 20 }}>
            <span style={{ font: "400 13px/1 var(--font-sans)", color: "var(--text-muted)" }}>{r.label}</span>
            <span style={{ font: "400 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{r.value}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
