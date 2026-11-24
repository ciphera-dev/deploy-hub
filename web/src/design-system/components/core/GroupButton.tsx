import { useState, type CSSProperties, type MouseEvent, type ReactNode } from "react";
import { Icon, type IconName } from "./Icon";

export interface GroupButtonProps {
  icon?: IconName;
  children?: ReactNode;
  onClick?: (e: MouseEvent<HTMLButtonElement>) => void;
  style?: CSSProperties;
}

export function GroupButton({ icon, children, onClick, style }: GroupButtonProps) {
  const [h, setH] = useState(false);
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseEnter={() => setH(true)}
      onMouseLeave={() => setH(false)}
      style={{ display: "inline-flex", alignItems: "center", gap: 7, height: 30, padding: "0 10px", background: h ? "var(--surface-control-hover)" : "transparent", border: 0, borderRadius: "var(--radius-md)", color: "var(--text-body)", font: "500 13px/1 var(--font-sans)", cursor: "pointer", whiteSpace: "nowrap", ...style }}
    >
      {icon && <Icon name={icon} size={14} />}
      {children}
    </button>
  );
}
