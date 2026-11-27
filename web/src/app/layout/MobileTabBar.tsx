import { useRouter, useRouterState } from "@tanstack/react-router";
import type { MouseEvent, Ref } from "react";

import { Icon } from "@/design-system";

import { isActivePath, NAV, type NavEntry } from "./nav";

// Onglets du bas : les entrees de la barre laterale qui ont une route. Monitoring n'en a pas
// encore (v0.2) : Connecteurs prend sa place jusqu'a sa livraison.
const TAB_LABELS = ["Vue d'ensemble", "Projets", "Déploiements", "Connecteurs"];

// Libelles courts : quatre onglets + « Plus » tiennent sur 320 px.
const SHORT: Record<string, string> = { "Vue d'ensemble": "Accueil" };

const TABS = TAB_LABELS.map((label) => NAV.flatMap((g) => g.items).find((e) => e.label === label)).filter((e): e is NavEntry & { to: NonNullable<NavEntry["to"]> } => !!e && !!e.to);

export interface MobileTabBarProps {
  moreOpen: boolean;
  moreRef?: Ref<HTMLButtonElement>;
  onMore: () => void;
}

/** Barre d'onglets fixee en bas sous 768 px (applications mobiles). */
export function MobileTabBar({ moreOpen, onMore, moreRef }: MobileTabBarProps) {
  const router = useRouter();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const onTab = TABS.some((t) => isActivePath(pathname, t.to));
  return (
    <nav aria-label="Navigation principale" className="app-tabbar">
      {TABS.map((t) => {
        const active = isActivePath(pathname, t.to);
        return (
          <a
            key={t.label}
            title={t.label}
            href={router.buildLocation({ to: t.to }).href}
            className="app-tabbar__item"
            aria-current={active ? "page" : undefined}
            onClick={(e: MouseEvent) => {
              if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
              e.preventDefault();
              void router.navigate({ to: t.to });
            }}
          >
            <Icon name={t.icon} size={20} />
            <span>{SHORT[t.label] ?? t.label}</span>
          </a>
        );
      })}
      <button ref={moreRef} type="button" className="app-tabbar__item" aria-haspopup="dialog" aria-expanded={moreOpen} data-active={!onTab || moreOpen ? "true" : undefined} onClick={onMore}>
        <Icon name="Ellipsis" size={20} />
        <span>Plus</span>
      </button>
    </nav>
  );
}
