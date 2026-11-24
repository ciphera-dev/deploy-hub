import { useState, type ComponentPropsWithRef, type MouseEvent } from "react";
import { Icon, type IconName } from "../core/Icon";

export interface NavItemProps extends Omit<ComponentPropsWithRef<"a">, "children" | "style" | "onClick" | "href"> {
  icon?: IconName;
  label: string;
  active?: boolean;
  /** md = navigation principale, sm = liens de pied */
  size?: "md" | "sm";
  /** icone seule, libelle en infobulle */
  collapsed?: boolean;
  /** compteur attenue a droite */
  count?: number | string;
  /** pastille verte de notification */
  badge?: number | string;
  /** adresse reelle (clic droit, nouvel onglet) ; "#" par defaut, alors le clic est neutralise */
  href?: string;
  /** entree non disponible : non cliquable, grisee (reste focusable pour afficher son infobulle) */
  disabled?: boolean;
  /** pastille de texte a droite (ex. « Bientôt ») */
  tag?: string;
  /** appele au clic ; l'appelant gere le defaut du navigateur quand `href` est une vraie adresse */
  onClick?: (e: MouseEvent<HTMLAnchorElement>) => void;
}

export function NavItem({ icon, label, active = false, size = "md", collapsed = false, count, badge, href = "#", disabled = false, tag, onClick, title, ...rest }: NavItemProps) {
  const [h, setH] = useState(false);
  const sm = size === "sm";
  return (
    <a
      {...rest}
      href={disabled ? undefined : href}
      title={title ?? (collapsed ? label : undefined)}
      aria-current={active ? "page" : undefined}
      role={disabled ? "link" : undefined}
      aria-disabled={disabled || undefined}
      tabIndex={disabled ? 0 : rest.tabIndex}
      onClick={(e) => {
        if (disabled || href === "#") e.preventDefault();
        if (!disabled) onClick?.(e);
      }}
      onMouseEnter={() => setH(true)}
      onMouseLeave={() => setH(false)}
      style={{
        position: "relative",
        display: "flex",
        alignItems: "center",
        justifyContent: collapsed ? "center" : "flex-start",
        gap: sm ? 9 : 10,
        whiteSpace: "nowrap",
        height: sm ? 28 : "var(--nav-item-h)",
        padding: sm ? "0 8px" : "0 10px",
        borderRadius: "var(--radius-md)",
        background: active ? "var(--surface-selected)" : h && !disabled ? "var(--surface-control-hover)" : "transparent",
        color: active || (h && !disabled) ? "var(--text-strong)" : "var(--text-body)",
        opacity: disabled ? 0.55 : 1,
        cursor: disabled ? "not-allowed" : undefined,
        font: (sm ? "400 12px" : "500 14px") + "/1 var(--font-sans)",
        textDecoration: "none",
        transition: "background var(--dur-fast)",
      }}
    >
      {icon && <Icon name={icon} size={sm ? 14 : 16} color={active ? "var(--text-strong)" : "var(--text-muted)"} />}
      {!collapsed && <span style={{ flex: 1 }}>{label}</span>}
      {!collapsed && count != null && <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>{count}</span>}
      {!collapsed && tag && <span style={{ height: 18, padding: "0 7px", borderRadius: 99, background: "var(--surface-selected)", color: "var(--text-subtle)", font: "500 10px/18px var(--font-sans)", whiteSpace: "nowrap" }}>{tag}</span>}
      {badge != null &&
        (collapsed ? (
          <span style={{ position: "absolute", top: 7, right: 9, width: 7, height: 7, borderRadius: 99, background: "var(--green-500)" }} />
        ) : (
          <span style={{ minWidth: 18, height: 18, padding: "0 5px", borderRadius: 99, background: "var(--green-500)", color: "#06130d", font: "600 11px/18px var(--font-sans)", textAlign: "center" }}>{badge}</span>
        ))}
    </a>
  );
}
