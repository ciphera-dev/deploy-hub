import type { DeploymentStatus } from "@/features/deployments/api";
import type { IconName } from "@/design-system";

import type { Project, ProjectStatus } from "../api";
import { buildLabel } from "../format";
import type { ProjectService, ServiceState, Sparkline } from "./api";

/** Puce et libellé d'un service (data.js#services `s`). */
export const SERVICE_STATE_VIEW: Record<ServiceState, { dot: "ok" | "error" | "pending" | "off"; label: string }> = {
  none: { dot: "off", label: "Pas encore déployé" },
  deploying: { dot: "pending", label: "Build en cours" },
  running: { dot: "ok", label: "En ligne" },
  stopped: { dot: "off", label: "Arrêté" },
  failed: { dot: "error", label: "Échec du build" },
  unreachable: { dot: "error", label: "Serveur injoignable" },
  down: { dot: "error", label: "Hors ligne" },
};

/** Pastille du projet (ProjectOverview.jsx#SIcon) : même rendu qu'un déploiement de même issue. */
export const PROJECT_ICON: Record<ProjectStatus, DeploymentStatus> = {
  none: "cancelled",
  building: "running",
  running: "success",
  failed: "failed",
  stopped: "cancelled",
  unreachable: "failed",
  down: "failed",
};

/** Réplicas « 1 / 1 » (data.js#services `rep`). */
export const replicasLabel = (s: ProjectService): string => `${s.replicas.running} / ${s.replicas.desired}`;

/** Ligne « image » d'un service (data.js `img`) : méthode de build, ou image du projet. */
export function imageLabel(project: Project): string {
  return project.source.type === "image" ? project.source.image : buildLabel(project.build);
}

/** Icône de la ligne « image » (ProjectOverview.jsx#ServiceCard). */
export function imageIcon(project: Project): IconName {
  if (project.source.type === "image") return "Container";
  return project.build?.kind === "dockerfile" ? "FileCode" : "Package";
}

/** Valeur d'un indicateur ; « — » sans mesure. */
export function kpiValue(s: Sparkline, format: (v: number) => string): string {
  return s.value == null ? "—" : format(s.value);
}

const fr = (v: number, digits: number) => v.toLocaleString("fr-FR", { minimumFractionDigits: digits, maximumFractionDigits: digits });

/** « 1,24 M », « 850 » (requêtes). */
export function formatCount(v: number): string {
  if (v >= 1_000_000) return `${fr(v / 1_000_000, 2)} M`;
  if (v >= 1000) return `${fr(v / 1000, 1)} k`;
  return fr(v, 0);
}

export const formatMs = (v: number): string => `${fr(v, 0)} ms`;
export const formatPercent = (digits: number) => (v: number): string => `${fr(v, digits)} %`;
