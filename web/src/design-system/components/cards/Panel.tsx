import type { CSSProperties, ReactNode } from "react";
import { Icon, type IconName } from "../core/Icon";

export interface PanelProps {
  title?: ReactNode;
  /** icone lucide avant le titre : titre blanc 13/500 au lieu d'un libelle gris */
  icon?: IconName;
  /** emplacement d'en-tete a droite, en general un <Button variant="ghost"> */
  action?: ReactNode;
  children?: ReactNode;
  padding?: string;
  /** position du filet */
  divider?: "top" | "bottom" | "both";
  style?: CSSProperties;
}

export function Panel({ title, icon, action, children, padding = "24px 24px", divider, style }: PanelProps) {
  return (
    <section
      style={{
        padding,
        borderTop: divider === "top" || divider === "both" ? "1px solid var(--border-subtle)" : "none",
        borderBottom: divider === "bottom" || divider === "both" ? "1px solid var(--border-subtle)" : "none",
        display: "flex",
        flexDirection: "column",
        gap: 18,
        minWidth: 0,
        ...style,
      }}
    >
      {(title || action) && (
        <header style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10, font: (icon ? "500 13px" : "400 12px") + "/1 var(--font-sans)", color: icon ? "var(--text-strong)" : "var(--text-subtle)" }}>
            {icon && <Icon name={icon} size={16} color="var(--text-body)" />}
            {title}
          </div>
          {action}
        </header>
      )}
      {children}
    </section>
  );
}
