import { useState, type CSSProperties } from "react";
import { Icon, type IconName } from "../core/Icon";

export interface RadioCardProps {
  /** Icone lucide dans un disque gris */
  icon?: IconName;
  title: string;
  description?: string;
  selected?: boolean;
  onClick?: () => void;
  style?: CSSProperties;
}

export function RadioCard({ icon, title, description, selected = false, onClick, style }: RadioCardProps) {
  const [h, setH] = useState(false);
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseEnter={() => setH(true)}
      onMouseLeave={() => setH(false)}
      style={{ display: "flex", alignItems: "flex-start", gap: 12, width: "100%", padding: "14px", textAlign: "left", borderRadius: "var(--radius-lg)", background: selected ? "var(--surface-raised)" : h ? "var(--surface-control-hover)" : "var(--surface-control)", border: "1px solid " + (selected ? "var(--gray-600)" : "var(--border-strong)"), cursor: "pointer", color: "var(--text-body)", ...style }}
    >
      {icon && (
        <span style={{ width: 30, height: 30, borderRadius: 99, background: "var(--surface-selected)", display: "flex", alignItems: "center", justifyContent: "center", flex: "none" }}>
          <Icon name={icon} size={14} color={selected ? "var(--text-strong)" : "var(--text-muted)"} />
        </span>
      )}
      <span style={{ flex: 1, display: "flex", flexDirection: "column", gap: 6 }}>
        <span style={{ font: "500 13px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>{title}</span>
        {description && <span style={{ font: "400 12px/1.4 var(--font-sans)", color: "var(--text-subtle)" }}>{description}</span>}
      </span>
      <span style={{ width: 14, height: 14, borderRadius: 99, border: "1px solid " + (selected ? "var(--gray-950)" : "var(--gray-500)"), display: "flex", alignItems: "center", justifyContent: "center", flex: "none", marginTop: 2 }}>
        {selected && <span style={{ width: 6, height: 6, borderRadius: 99, background: "var(--gray-950)" }} />}
      </span>
    </button>
  );
}
