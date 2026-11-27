import { useRouter, useRouterState } from "@tanstack/react-router";
import type { MouseEvent } from "react";

import { Menu, NavItem, NavSection, Sidebar, UserMenu, type MenuItem } from "@/design-system";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";

import { isActivePath, NAV, SOON_BADGE, soonTooltip, USER_MENU, type NavEntry } from "./nav";

export interface AppSidebarUser {
  name: string;
  email: string;
  /** URL de l'avatar (meme origine) ; initiales sinon */
  avatar?: string;
}

export interface AppSidebarProps {
  user: AppSidebarUser;
  collapsed?: boolean;
  onLogout?: () => void;
}

/** Clic simple : navigation SPA ; clic modifie (nouvel onglet) : comportement du navigateur. */
function isPlainLeftClick(e: MouseEvent): boolean {
  return e.button === 0 && !e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey;
}

/** Entrees du menu du compte (pied de la barre laterale et avatar de l'en-tete). */
export function accountMenuItems(router: ReturnType<typeof useRouter>, onLogout?: () => void): MenuItem[] {
  const toMenuAction = (entry: NavEntry): MenuItem =>
    entry.since
      ? { icon: entry.icon, label: entry.label, disabled: true, tag: soonTooltip(entry.since), onSelect: () => undefined }
      : { icon: entry.icon, label: entry.label, onSelect: () => void router.navigate({ to: entry.to }) };

  return [
    ...USER_MENU.slice(0, 1).map(toMenuAction),
    "separator",
    ...USER_MENU.slice(1, 5).map(toMenuAction),
    "separator",
    ...USER_MENU.slice(5).map(toMenuAction),
    "separator",
    { icon: "LogOut", label: "Se déconnecter", danger: true, onSelect: () => onLogout?.() },
  ];
}

export function AppSidebar({ user, collapsed = false, onLogout }: AppSidebarProps) {
  const router = useRouter();
  const pathname = useRouterState({ select: (s) => s.location.pathname });

  const renderEntry = (entry: NavEntry) => {
    if (entry.since) {
      const tip = soonTooltip(entry.since);
      return (
        <Tooltip key={entry.label}>
          <TooltipTrigger asChild>
            <NavItem icon={entry.icon} label={entry.label} collapsed={collapsed} disabled tag={SOON_BADGE} title={collapsed ? `${entry.label} (${tip})` : undefined} />
          </TooltipTrigger>
          <TooltipContent side="right">{tip}</TooltipContent>
        </Tooltip>
      );
    }
    const to = entry.to;
    return (
      <NavItem
        key={entry.label}
        icon={entry.icon}
        label={entry.label}
        collapsed={collapsed}
        href={router.buildLocation({ to }).href}
        active={isActivePath(pathname, to)}
        onClick={(e) => {
          if (!isPlainLeftClick(e)) return;
          e.preventDefault();
          void router.navigate({ to });
        }}
      />
    );
  };

  const menuItems = accountMenuItems(router, onLogout);

  return (
    <TooltipProvider delayDuration={150}>
      <Sidebar
        collapsed={collapsed}
        style={{ height: "100%", background: "transparent" }}
        bottom={
          <div style={{ borderTop: "1px solid var(--border-subtle)", margin: "0 -12px", padding: "10px 8px 0" }}>
            <Menu label="Menu du compte" side="top" align="start" width={240} items={menuItems} trigger={<UserMenu name={user.name} email={user.email} avatar={user.avatar} collapsed={collapsed} />} />
          </div>
        }
      >
        <nav aria-label="Navigation principale" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
          {NAV.map((group) => (
            <NavSection key={group.label} eyebrow collapsed={collapsed} label={group.label} gap={2}>
              {group.items.map(renderEntry)}
            </NavSection>
          ))}
        </nav>
      </Sidebar>
    </TooltipProvider>
  );
}
