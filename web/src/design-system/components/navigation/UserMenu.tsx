import type { ComponentPropsWithRef } from "react";
import { Icon } from "../core/Icon";

export interface UserMenuProps extends Omit<ComponentPropsWithRef<"button">, "children" | "style"> {
  name: string;
  email: string;
  /** URL locale de l'avatar ; initiales par defaut */
  avatar?: string;
  /** avatar seul */
  collapsed?: boolean;
}

export function UserMenu({ name, email, avatar, collapsed = false, title, type = "button", ...rest }: UserMenuProps) {
  const initials = (name || "").split(" ").map((s) => s[0]).join("").slice(0, 2);
  return (
    <button
      {...rest}
      type={type}
      title={title ?? (collapsed ? name : undefined)}
      style={{ display: "flex", alignItems: "center", justifyContent: collapsed ? "center" : "flex-start", gap: 10, width: "100%", padding: collapsed ? "6px 0" : "6px 8px", background: "transparent", border: 0, borderRadius: "var(--radius-md)", cursor: "pointer", textAlign: "left" }}
    >
      {avatar ? (
        <img src={avatar} alt="" style={{ width: 28, height: 28, borderRadius: 99, objectFit: "cover" }} />
      ) : (
        <span style={{ width: 28, height: 28, borderRadius: 99, background: "var(--gray-900)", color: "var(--text-inverse)", display: "flex", alignItems: "center", justifyContent: "center", font: "600 11px/1 var(--font-sans)" }}>{initials}</span>
      )}
      {!collapsed && (
        <>
          <span style={{ flex: 1, display: "flex", flexDirection: "column", gap: 3, whiteSpace: "nowrap" }}>
            <span style={{ font: "500 12px/1 var(--font-sans)", color: "var(--text-strong)" }}>{name}</span>
            <span style={{ font: "400 10px/1 var(--font-sans)", color: "var(--text-subtle)" }}>{email}</span>
          </span>
          <Icon name="ChevronsUpDown" size={14} color="var(--text-muted)" />
        </>
      )}
    </button>
  );
}
