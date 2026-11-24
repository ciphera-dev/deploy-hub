import { api } from "@/lib/api/client";
import { unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";

export type Deployment = components["schemas"]["Deployment"];
export type DeploymentStep = components["schemas"]["DeploymentStep"];
export type DeploymentStage = components["schemas"]["DeploymentStage"];
export type DeploymentStatus = components["schemas"]["DeploymentStatus"];

export const DEPLOYMENTS_QUERY_KEY = ["deployments"] as const;
/** Liste (tous les projets, ou un projet). */
export const deploymentsQueryKey = (project?: string) => ["deployments", "list", project ?? "all"] as const;
export const deploymentQueryKey = (id: string) => ["deployments", "one", id] as const;

/** En file ou en cours : le déploiement n'est pas terminé. */
export const isActive = (d: Pick<Deployment, "status">): boolean => d.status === "queued" || d.status === "running";

/** Rafraîchissement tant qu'un déploiement est en file ou en cours (étapes et statut ; les lignes arrivent par le flux des logs). */
export const POLL_MS = 3000;
export const pollWhileActive = (items: Deployment[] | undefined): number | false => (items?.some(isActive) ? POLL_MS : false);

/** GET /api/deployments (plus récent d'abord). */
export async function fetchDeployments(project?: string, status?: DeploymentStatus, limit = 200): Promise<Deployment[]> {
  return unwrap(await api.GET("/api/deployments", { params: { query: { ...(project ? { project } : {}), ...(status ? { status } : {}), limit } } }));
}

export const activeQueryKey = (project: string) => ["deployments", "active", project] as const;

/** Question posée au serveur : le projet a-t-il un déploiement en file ou en cours ? (le filtre `status` n'en accepte qu'un) */
export async function hasActiveDeployment(project: string): Promise<boolean> {
  const [queued, running] = await Promise.all([fetchDeployments(project, "queued", 1), fetchDeployments(project, "running", 1)]);
  return queued.length + running.length > 0;
}

/** GET /api/deployments/{id} */
export async function fetchDeployment(id: string): Promise<Deployment> {
  return unwrap(await api.GET("/api/deployments/{id}", { params: { path: { id } } }));
}

export type ActionResult = { ok: true; deployment: Deployment } | { ok: false; message: string };

function refusal(status: number, error: unknown, fallback: string): string {
  const message = (error as { error?: { message?: unknown } } | undefined)?.error?.message;
  if (status === 403) return "Accès refusé : cette action demande le rôle développeur.";
  if ((status === 409 || status === 422 || status === 404) && typeof message === "string") return message;
  return fallback;
}

async function act(call: () => Promise<{ data?: Deployment; error?: unknown; response: Response }>, fallback: string): Promise<ActionResult> {
  let result;
  try {
    result = await call();
  } catch {
    return { ok: false, message: "Serveur injoignable. Réessayez." };
  }
  if (result.data) return { ok: true, deployment: result.data };
  return { ok: false, message: refusal(result.response.status, result.error, fallback) };
}

/** POST /api/projects/{id}/redeploy : un déploiement manuel de la production (mis en file) ; le serveur refuse (409) si un déploiement est déjà en file ou en cours. */
export function createDeployment(projectId: string): Promise<ActionResult> {
  return act(() => api.POST("/api/projects/{id}/redeploy", { params: { path: { id: projectId } } }), "Déploiement impossible pour le moment. Réessayez.");
}

/** POST /api/deployments/{id}/cancel */
export function cancelDeployment(id: string): Promise<ActionResult> {
  return act(() => api.POST("/api/deployments/{id}/cancel", { params: { path: { id } } }), "Annulation impossible pour le moment. Réessayez.");
}

/** POST /api/deployments/{id}/rollback */
export function rollbackDeployment(id: string): Promise<ActionResult> {
  return act(() => api.POST("/api/deployments/{id}/rollback", { params: { path: { id } } }), "Rollback impossible pour le moment. Réessayez.");
}
