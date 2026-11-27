import { useRouter } from "@tanstack/react-router";

import { BrandMark, Icon, IconButton, Menu, WorkspaceSwitcher } from "@/design-system";

import { useMedia, NARROW_QUERY } from "./useMedia";
import { accountMenuItems, type AppSidebarUser } from "./AppSidebar";

export interface AppHeaderProps {
  user: AppSidebarUser;
  collapsed: boolean;
  onToggle: () => void;
  /** sous 768 px : pas de bouton de repli (navigation en barre d'onglets) */
  mobile?: boolean;
  onLogout?: () => void;
}

const isApple = () => typeof navigator !== "undefined" && /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent);

// Variables locales : le texte et les controles de la barre restent clairs sur le degrade.
const HEADER_VARS = {
  "--text-strong": "#fff",
  "--text-body": "rgba(255,255,255,.9)",
  "--text-muted": "rgba(255,255,255,.8)",
  "--text-subtle": "rgba(255,255,255,.72)",
  "--surface-control-hover": "rgba(255,255,255,.12)",
  "--border-strong": "rgba(255,255,255,.2)",
  "--surface-control": "rgba(10,10,14,.35)",
} as Record<string, string>;

/**
 * En-tete du panneau (maquette dashboard/index.html) : logo, repli, espace ; recherche au
 * centre ; notifications et compte a droite, sur le degrade du theme par defaut.
 * La recherche et les notifications n'ont pas encore de service : elles restent visibles
 * mais inactives (meme principe que les entrees « Bientot » de la barre laterale).
 */
export function AppHeader({ user, collapsed, onToggle, onLogout, mobile = false }: AppHeaderProps) {
  const router = useRouter();
  const narrow = useMedia(NARROW_QUERY);
  const initials = (user.name || "").split(" ").map((s) => s[0]).join("").slice(0, 2).toUpperCase();
  const shortcut = isApple() ? "⌘K" : "Ctrl K";
  return (
    <div className="app-header" style={{ color: "#fff", ...HEADER_VARS }}>
      <div className="app-header__left">
        <span className="app-header__brand">
          <BrandMark />
        </span>
        {!mobile && (
          <IconButton variant="ghost" size={28} icon={collapsed ? "PanelLeftOpen" : "PanelLeftClose"} label={collapsed ? "Déplier le menu" : "Replier le menu"} onClick={onToggle} />
        )}
        <WorkspaceSwitcher collapsed={narrow} style={mobile ? { height: 40 } : undefined} />
      </div>
      <button type="button" className="app-header__search" aria-disabled="true" aria-label="Rechercher" title="Recherche : bientôt disponible" onClick={(e) => e.preventDefault()}>
        <Icon name="Search" size={14} />
        <span className="app-header__search-label">Rechercher un projet, un serveur, un domaine…</span>
        <span className="app-header__kbd">{shortcut}</span>
      </button>
      <div className="app-header__right">
        <span className="app-header__bell">
          <IconButton size={mobile ? 40 : 32} icon="Bell" label="Notifications" aria-disabled="true" title="Notifications : bientôt disponibles" onClick={(e) => e.preventDefault()} />
        </span>
        <Menu
          label="Menu du compte"
          side="bottom"
          align="end"
          width={240}
          items={accountMenuItems(router, onLogout)}
          trigger={
            <button type="button" className="app-header__avatar" aria-label="Mon compte" title={user.name}>
              {user.avatar ? <img src={user.avatar} alt="" /> : <span>{initials}</span>}
            </button>
          }
        />
      </div>
    </div>
  );
}
