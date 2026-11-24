import { useState, type ButtonHTMLAttributes, type CSSProperties, type MouseEvent, type ReactNode } from "react";
import { Icon, type IconName } from "./Icon";

interface VariantStyle {
  background: string;
  color: string;
  border: string;
  hover: string;
  hoverColor?: string;
}

const V: Record<string, VariantStyle> = {
  secondary: { background: "var(--surface-control)", color: "var(--text-body)", border: "1px solid var(--border-strong)", hover: "var(--surface-control-hover)" },
  filled: { background: "var(--surface-button)", color: "var(--text-strong)", border: "1px solid transparent", hover: "var(--surface-button-hover)" },
  ghost: { background: "transparent", color: "var(--text-muted)", border: "1px solid transparent", hover: "transparent", hoverColor: "var(--text-strong)" },
  inverse: { background: "var(--surface-inverse)", color: "var(--text-inverse)", border: "1px solid transparent", hover: "#ffffff" },
};
const S = { sm: { h: 28, px: 10, fs: 12, gap: 6, ic: 13 }, md: { h: 32, px: 10, fs: 13, gap: 7, ic: 14 } };

export interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "style" | "onClick"> {
  /** secondary = controle avec bordure ; filled = bloc gris ; ghost = texte + icone ; inverse = blanc (pilule active) */
  variant?: "secondary" | "filled" | "ghost" | "inverse";
  size?: "sm" | "md";
  /** Icone lucide avant le libelle */
  icon?: IconName;
  /** Icone lucide apres le libelle */
  iconRight?: IconName;
  block?: boolean;
  pill?: boolean;
  disabled?: boolean;
  children?: ReactNode;
  onClick?: (e: MouseEvent<HTMLButtonElement>) => void;
  style?: CSSProperties;
}

export function Button({ variant = "secondary", size = "md", icon, iconRight, block = false, pill = false, disabled = false, children, onClick, style, ...rest }: ButtonProps) {
  const [h, setH] = useState(false);
  const v = V[variant] ?? V.secondary;
  const s = S[size] ?? S.md;
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      onMouseEnter={() => setH(true)}
      onMouseLeave={() => setH(false)}
      {...rest}
      style={{
        display: block ? "flex" : "inline-flex",
        width: block ? "100%" : undefined,
        alignItems: "center",
        justifyContent: "center",
        gap: s.gap,
        height: s.h,
        padding: variant === "ghost" ? "0 2px" : "0 " + s.px + "px",
        borderRadius: pill ? 999 : "var(--radius-md)",
        background: h && !disabled ? v.hover : v.background,
        color: h && v.hoverColor ? v.hoverColor : v.color,
        border: v.border,
        boxShadow: variant === "secondary" ? "var(--shadow-control)" : "none",
        font: "500 " + s.fs + "px/1 var(--font-sans)",
        whiteSpace: "nowrap",
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.4 : 1,
        transition: "background var(--dur-fast) var(--ease-out),color var(--dur-fast)",
        ...style,
      }}
    >
      {icon && <Icon name={icon} size={s.ic} />}
      {children}
      {iconRight && <Icon name={iconRight} size={s.ic} />}
    </button>
  );
}
