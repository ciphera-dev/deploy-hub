import type { Build, ProjectStatus, Source } from "./api";

export const STATUS_VIEW: Record<ProjectStatus, { dot: "ok" | "error" | "pending" | "off"; label: string }> = {
  none: { dot: "off", label: "Pas encore déployé" },
  building: { dot: "pending", label: "Build en cours" },
  running: { dot: "ok", label: "En ligne" },
  failed: { dot: "error", label: "Échec du build" },
  stopped: { dot: "off", label: "Arrêté" },
  unreachable: { dot: "error", label: "Serveur injoignable" },
  down: { dot: "error", label: "Hors ligne" },
};

export function buildLabel(build: Build | null | undefined): string {
  if (!build) return "Image";
  return { railpack: "Railpack", dockerfile: "Dockerfile", compose: "Compose" }[build.kind];
}

/** Dépôt (`propriétaire/nom`) ou image, tel qu'affiché. */
export function sourceLabel(source: Source): string {
  return source.type === "git" ? source.repository : source.image;
}

export const branchOf = (source: Source): string | null => (source.type === "git" ? source.branch : null);

/** « il y a 4 min », « il y a 2 j »… (le serveur envoie des dates RFC 3339). */
export function timeAgo(iso: string, now = Date.now()): string {
  const seconds = Math.round((now - Date.parse(iso)) / 1000);
  if (Number.isNaN(seconds)) return "—";
  if (seconds < 60) return "à l'instant";
  if (seconds < 3600) return `il y a ${Math.floor(seconds / 60)} min`;
  if (seconds < 86400) return `il y a ${Math.floor(seconds / 3600)} h`;
  if (seconds < 86400 * 30) return `il y a ${Math.floor(seconds / 86400)} j`;
  return `il y a ${Math.floor(seconds / (86400 * 30))} mois`;
}
