import { useState, type CSSProperties } from "react";
import { Icon } from "../core/Icon";

export interface WorkspaceSwitcherProps {
  name?: string;
  /** avatar seul */
  collapsed?: boolean;
  onClick?: () => void;
  style?: CSSProperties;
}

export function WorkspaceSwitcher({ name = "Ciphera", collapsed = false, onClick, style }: WorkspaceSwitcherProps) {
  const [h, setH] = useState(false);
  return (
    <button
      type="button"
      title={collapsed ? name : undefined}
      onClick={onClick}
      onMouseEnter={() => setH(true)}
      onMouseLeave={() => setH(false)}
      style={{ display: "flex", alignItems: "center", justifyContent: collapsed ? "center" : "flex-start", gap: 10, width: "100%", height: collapsed ? 44 : 48, padding: collapsed ? 0 : "0 12px 0 10px", borderRadius: "var(--radius-lg)", background: h ? "var(--surface-control-hover)" : "var(--surface-control)", border: "1px solid var(--border-strong)", boxShadow: "var(--shadow-control)", color: "var(--text-body)", font: "500 14px/1 var(--font-sans)", cursor: "pointer", ...style }}
    >
      <span style={{ width: 24, height: 24, borderRadius: 99, background: "var(--grad-brand)", flex: "none" }} />
      {!collapsed && (
        <>
          <span style={{ flex: 1, textAlign: "left", whiteSpace: "nowrap" }}>{name}</span>
          <Icon name="ChevronsUpDown" size={14} color="var(--text-muted)" />
        </>
      )}
    </button>
  );
}
