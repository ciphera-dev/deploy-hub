import { useState, type ButtonHTMLAttributes, type CSSProperties, type MouseEvent } from "react";
import { Icon, type IconName } from "./Icon";

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "style" | "onClick"> {
  /** Icone lucide, "Ellipsis" par defaut */
  icon?: IconName;
  /** Cote du carre en px, 32 par defaut */
  size?: number;
  /** secondary = bordure (defaut), ghost = sans bordure ni fond jusqu'au survol */
  variant?: "secondary" | "ghost";
  label?: string;
  onClick?: (e: MouseEvent<HTMLButtonElement>) => void;
  style?: CSSProperties;
}

export function IconButton({ icon = "Ellipsis", size = 32, variant = "secondary", label, onClick, style, ...rest }: IconButtonProps) {
  const [h, setH] = useState(false);
  return (
    <button
      type="button"
      aria-label={label || icon}
      onClick={onClick}
      onMouseEnter={() => setH(true)}
      onMouseLeave={() => setH(false)}
      {...rest}
      style={{
        width: size,
        height: size,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        borderRadius: "var(--radius-md)",
        background: h ? "var(--surface-control-hover)" : variant === "ghost" ? "transparent" : "var(--surface-control)",
        border: variant === "ghost" ? "1px solid transparent" : "1px solid var(--border-strong)",
        boxShadow: variant === "ghost" ? "none" : "var(--shadow-control)",
        color: variant === "ghost" && !h ? "var(--text-muted)" : "var(--text-body)",
        cursor: "pointer",
        padding: 0,
        ...style,
      }}
    >
      <Icon name={icon} size={15} />
    </button>
  );
}
