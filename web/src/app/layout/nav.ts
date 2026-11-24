import type { IconName } from "@/design-system";
import type { FileRouteTypes } from "@/routeTree.gen";

export type RoutePath = FileRouteTypes["to"];
/** Version qui livre l'ecran quand il n'est pas dans la v0.1. */
export type Since = "v0.2" | "v0.3";

interface NavBase {
  icon: IconName;
  label: string;
}

/** Entree disponible (route) ou prevue plus tard (visible, desactivee, badge « Bientôt »). */
export type NavEntry = NavBase & ({ to: RoutePath; since?: undefined } | { since: Since; to?: undefined });

export interface NavGroup {
  label: string;
  items: NavEntry[];
}

// Portage de AppSidebar.jsx : memes sections, libelles et icones (noms lucide en PascalCase).
// Les compteurs et pastilles de la maquette (données factices) viendront de l'API.
export const NAV: NavGroup[] = [
  {
    label: "Pilotage",
    items: [
      { icon: "Gauge", label: "Vue d'ensemble", to: "/" },
      { icon: "Layers", label: "Projets", to: "/projects" },
      { icon: "Rocket", label: "Déploiements", to: "/deployments" },
      { icon: "ChartLine", label: "Monitoring", since: "v0.2" },
      { icon: "Radar", label: "Sécurité", since: "v0.3" },
    ],
  },
  {
    label: "Ressources",
    items: [
      { icon: "Server", label: "Serveurs", since: "v0.2" },
      { icon: "Container", label: "Conteneurs", since: "v0.2" },
      { icon: "Database", label: "Bases de données", since: "v0.2" },
      { icon: "Plug", label: "Connecteurs", to: "/connectors" },
      { icon: "Globe", label: "Domaines", since: "v0.2" },
    ],
  },
  {
    label: "Plateforme",
    items: [
      { icon: "Ticket", label: "Tickets", since: "v0.3" },
      { icon: "Settings", label: "Paramètres", to: "/settings" },
    ],
  },
];

/** Menu du compte (NavUser de la maquette). */
export const USER_MENU: NavEntry[] = [
  { icon: "User", label: "Mon profil", to: "/profile" },
  { icon: "Users", label: "Équipe", since: "v0.2" },
  { icon: "Braces", label: "Variables globales", to: "/variables" },
  { icon: "KeyRound", label: "Clés API", since: "v0.2" },
  { icon: "Webhook", label: "Webhooks", since: "v0.3" },
  { icon: "Keyboard", label: "Raccourcis clavier", to: "/shortcuts" },
  { icon: "BookOpen", label: "Documentation", since: "v0.3" },
];

export const SOON_BADGE = "Bientôt";
export const soonTooltip = (since: Since) => `Disponible en ${since}`;

/** L'entree est active quand l'URL est la sienne ou l'un de ses descendants. */
export function isActivePath(pathname: string, to: string): boolean {
  if (to === "/") return pathname === "/";
  return pathname === to || pathname.startsWith(`${to}/`);
}
