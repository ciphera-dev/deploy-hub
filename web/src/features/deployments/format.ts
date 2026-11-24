import type { Deployment, DeploymentStage, DeploymentStatus, DeploymentStep } from "./api";

/** Les 6 étapes du pipeline, dans l'ordre (ProjectDeployments.jsx#STAGES). */
export const STAGES: { key: DeploymentStage; label: string }[] = [
  { key: "clone", label: "Clonage" },
  { key: "build", label: "Build" },
  { key: "image", label: "Image" },
  { key: "start", label: "Démarrage" },
  { key: "healthcheck", label: "Healthcheck" },
  { key: "switch", label: "Bascule" },
];

export const stageLabel = (stage: DeploymentStage): string => STAGES.find((s) => s.key === stage)?.label ?? stage;

/** ProjectDeployments.jsx#ST : puce de statut + libellé. */
export const STATUS_VIEW: Record<DeploymentStatus, { dot: "ok" | "error" | "pending" | "off"; label: string }> = {
  queued: { dot: "pending", label: "En file" },
  running: { dot: "pending", label: "En cours" },
  success: { dot: "ok", label: "Réussi" },
  failed: { dot: "error", label: "Échec" },
  cancelled: { dot: "off", label: "Annulé" },
};

export const TRIGGER_VIEW = {
  push: { icon: "GitCommitHorizontal", label: "push" },
  manual: { icon: "MousePointerClick", label: "manuel" },
  rollback: { icon: "Undo2", label: "rollback" },
  hook: { icon: "Webhook", label: "deploy hook" },
} as const;

/** « 38 s », « 1 min 36 s », « 2 min 05 s » (ProjectDeployments.jsx#dur). */
export function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return "—";
  const total = Math.round(ms / 1000);
  if (total < 60) return `${total} s`;
  return `${Math.floor(total / 60)} min ${String(total % 60).padStart(2, "0")} s`;
}

/** Durée d'une étape en secondes (« 62 s »), « < 1 s » en dessous. */
export function formatStepSeconds(ms: number | null | undefined): string {
  if (ms == null) return "—";
  return ms < 500 ? "< 1 s" : `${Math.round(ms / 1000)} s`;
}

export const shortSha = (sha: string | null | undefined): string | null => (sha ? sha.slice(0, 7) : null);

/** Les données n'ont pas de message de commit : le libellé vient du déclencheur. */
export function deploymentTitle(d: Deployment): string {
  if (d.trigger === "rollback") return "Rollback";
  if (d.trigger === "manual") return "Déploiement manuel";
  if (d.trigger === "hook") return "Deploy hook";
  return d.branch ? `Push sur ${d.branch}` : "Push";
}

export type StageState = "done" | "running" | "failed" | "cancelled" | "pending";

/** État d'une étape de la frise ; une étape absente n'a pas commencé (grisée). */
export function stageState(d: Deployment, stage: DeploymentStage): { state: StageState; step?: DeploymentStep } {
  const step = d.steps.find((s) => s.stage === stage);
  if (!step) return { state: "pending" };
  if (step.status === "succeeded") return { state: "done", step };
  if (step.status === "failed") return { state: "failed", step };
  if (step.status === "cancelled") return { state: "cancelled", step };
  // Étape encore ouverte sur un déploiement terminé : l'issue la ferme (échec présumé, annulation).
  if (d.status === "failed") return { state: "failed", step };
  if (d.status === "cancelled") return { state: "cancelled", step };
  return { state: "running", step };
}

/** Garde « déjà en cours » : un déploiement de la liste est en file ou en cours. */
export const inProgress = (list: Deployment[] | undefined): boolean => !!list?.some((d) => d.status === "queued" || d.status === "running");
