import type { CSSProperties, ReactNode } from "react";

export interface InsightTextProps {
  /** phrase d'ouverture en gris */
  children: ReactNode;
  /** expression mise en avant en blanc */
  highlight?: string;
  /** suite en gris */
  after?: string;
  style?: CSSProperties;
}

export function InsightText({ children, highlight, after, style }: InsightTextProps) {
  return (
    <p style={{ margin: 0, font: "400 24px/1.3 var(--font-sans)", color: "var(--text-muted)", textWrap: "pretty", letterSpacing: "-0.005em", ...style }}>
      {children}
      {highlight && (
        <>
          {" "}
          <span style={{ color: "var(--text-strong)", fontWeight: 500 }}>{highlight}</span>
        </>
      )}
      {after && <> {after}</>}
    </p>
  );
}
