import type { CSSProperties, ReactNode } from "react";

export interface SidebarProps {
  /** BrandMark + WorkspaceSwitcher */
  top?: ReactNode;
  /** NavSections */
  children?: ReactNode;
  /** UpdateCard, liens de pied, UserMenu */
  bottom?: ReactNode;
  /** rail d'icones seules (68px) */
  collapsed?: boolean;
  style?: CSSProperties;
}

export function Sidebar({ top, children, bottom, collapsed = false, style }: SidebarProps) {
  return (
    <aside
      style={{
        width: collapsed ? "var(--sidebar-width-collapsed)" : "var(--sidebar-width)",
        flex: "none",
        display: "flex",
        flexDirection: "column",
        background: "var(--surface-sidebar)",
        padding: collapsed ? "22px 12px 16px" : "22px 14px 16px",
        gap: 24,
        overflow: "hidden",
        transition: "width var(--dur-base) var(--ease-out),padding var(--dur-base) var(--ease-out)",
        ...style,
      }}
    >
      {top}
      <div className="ds-scroll-hidden" style={{ flex: 1, minHeight: 0, overflowY: "auto", scrollbarWidth: "none", msOverflowStyle: "none", display: "flex", flexDirection: "column", gap: 20, margin: "0 -14px", padding: "0 14px" }}>
        {children}
      </div>
      {bottom && <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>{bottom}</div>}
    </aside>
  );
}
