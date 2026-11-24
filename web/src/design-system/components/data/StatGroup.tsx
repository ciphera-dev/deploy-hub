import { Children, type CSSProperties, type ReactNode } from "react";

export interface StatGroupProps {
  /** Stat en colonnes egales, separees par des filets */
  children: ReactNode;
  style?: CSSProperties;
}

export function StatGroup({ children, style }: StatGroupProps) {
  const items = Children.toArray(children);
  return (
    <div style={{ display: "grid", gridTemplateColumns: "repeat(" + items.length + ",minmax(0,1fr))", ...style }}>
      {items.map((c, i) => (
        <div key={i} style={{ padding: "0 24px", paddingLeft: i === 0 ? 0 : 24, borderLeft: i === 0 ? "none" : "1px solid var(--border-subtle)" }}>
          {c}
        </div>
      ))}
    </div>
  );
}
