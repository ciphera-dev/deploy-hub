import type { CSSProperties, ReactNode } from "react";

export interface NavSectionProps {
  /** petit titre gris */
  label?: string;
  children: ReactNode;
  gap?: number;
  /** titre de 10px en majuscules espacees */
  eyebrow?: boolean;
  /** masque le texte du titre (garde sa place) */
  collapsed?: boolean;
  style?: CSSProperties;
}

export function NavSection({ label, children, gap = 2, collapsed = false, eyebrow = false, style }: NavSectionProps) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap, ...style }}>
      {label && (
        <div
          style={{
            opacity: collapsed ? 0 : 1,
            whiteSpace: "nowrap",
            padding: "0 6px 8px",
            font: (eyebrow ? "500 10px" : "400 12px") + "/1 var(--font-sans)",
            letterSpacing: eyebrow ? ".08em" : 0,
            textTransform: eyebrow ? "uppercase" : "none",
            color: eyebrow ? "var(--text-faint)" : "var(--text-subtle)",
          }}
        >
          {label}
        </div>
      )}
      {children}
    </div>
  );
}
