import type { CSSProperties, ReactElement } from "react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Icon, type IconName } from "../core/Icon";
import { IconButton } from "../core/IconButton";

export interface MenuAction {
  icon?: IconName;
  label: string;
  onSelect: () => void;
  /** action destructive : texte rouge */
  danger?: boolean;
  /** entree visible mais non selectionnable */
  disabled?: boolean;
  /** pastille a droite (ex. « Bientôt ») */
  tag?: string;
}

export type MenuItem = MenuAction | "separator";

export interface MenuProps {
  items: MenuItem[];
  /** Nom accessible du declencheur, "Actions" par defaut */
  label?: string;
  /** Declencheur personnalise (doit accepter ref et props de bouton) ; IconButton "Ellipsis" par defaut */
  trigger?: ReactElement;
  align?: "start" | "center" | "end";
  side?: "top" | "right" | "bottom" | "left";
  width?: number;
  style?: CSSProperties;
}

/** Menu contextuel « ... » : navigation clavier, fleches, Entree, Echap (Radix via shadcn). */
export function Menu({ items, label = "Actions", trigger, align = "end", side, width = 220, style }: MenuProps) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{trigger ?? <IconButton size={28} variant="ghost" icon="Ellipsis" label={label} />}</DropdownMenuTrigger>
      <DropdownMenuContent
        align={align}
        side={side}
        sideOffset={4}
        style={{ zIndex: 1000, width, padding: 4, background: "var(--surface-tooltip)", border: 0, borderRadius: "var(--radius-lg)", boxShadow: "var(--shadow-tooltip)", ...style }}
      >
        {items.map((it, i) =>
          it === "separator" ? (
            <DropdownMenuSeparator key={i} style={{ height: 1, margin: "4px 0", background: "var(--border-strong)" }} />
          ) : (
            <DropdownMenuItem
              key={i}
              disabled={it.disabled}
              onSelect={it.onSelect}
              className="data-[highlighted]:!bg-[var(--surface-selected)]"
              style={{ display: "flex", alignItems: "center", gap: 10, width: "100%", padding: "8px 10px", borderRadius: 6, cursor: it.disabled ? "not-allowed" : "pointer", opacity: it.disabled ? 0.55 : 1, color: it.danger ? "var(--text-negative)" : "var(--text-body)", font: "400 13px/1 var(--font-sans)" }}
            >
              {it.icon && <Icon name={it.icon} size={14} />}
              <span style={{ flex: 1 }}>{it.label}</span>
              {it.tag && <span style={{ font: "500 10px/1 var(--font-sans)", color: "var(--text-subtle)" }}>{it.tag}</span>}
            </DropdownMenuItem>
          ),
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
