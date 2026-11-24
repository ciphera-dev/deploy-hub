import { api } from "@/lib/api/client";
import { unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";

export type ProjectOverviewData = components["schemas"]["ProjectOverview"];
export type ProjectService = components["schemas"]["ProjectService"];
export type ServiceState = components["schemas"]["ServiceState"];
export type Sparkline = components["schemas"]["Sparkline"];

export const overviewQueryKey = (projectId: string) => ["projects", projectId, "overview"] as const;
export const servicesQueryKey = (projectId: string) => ["projects", projectId, "services"] as const;

/** GET /api/projects/{id}/overview (viewer et plus). */
export async function fetchOverview(projectId: string): Promise<ProjectOverviewData> {
  return unwrap(await api.GET("/api/projects/{id}/overview", { params: { path: { id: projectId } } }));
}

/** GET /api/projects/{id}/services (viewer et plus). */
export async function fetchServices(projectId: string): Promise<ProjectService[]> {
  return unwrap(await api.GET("/api/projects/{id}/services", { params: { path: { id: projectId } } }));
}

/** Format de `run_as` : `uid` ou `uid:gid` numériques (comme le serveur). */
export const RUN_AS_RE = /^[0-9]{1,10}(:[0-9]{1,10})?$/;

/** PATCH /api/projects/{id}/services/{sid} : utilisateur et racine en lecture seule (developer et plus). */
export async function updateServiceSecurity(projectId: string, serviceId: string, body: { run_as: string | null; read_only_rootfs: boolean }): Promise<{ ok: true; data: ProjectService } | { ok: false; message: string }> {
  let result;
  try {
    result = await api.PATCH("/api/projects/{id}/services/{sid}", { params: { path: { id: projectId, sid: serviceId } }, body });
  } catch {
    return { ok: false, message: "Serveur injoignable. Réessayez." };
  }
  if (result.data) return { ok: true, data: result.data };
  const { status } = result.response;
  const message = (result.error as { error?: { message?: unknown } } | undefined)?.error?.message;
  if (status === 403) return { ok: false, message: "Accès refusé : cette action demande le rôle développeur." };
  if ((status === 404 || status === 422) && typeof message === "string") return { ok: false, message };
  return { ok: false, message: "Enregistrement impossible pour le moment. Réessayez." };
}

export type ServiceAction = "restart" | "stop";

/** POST /api/projects/{id}/services/{sid}/{restart,stop} (developer et plus). */
export async function serviceAction(projectId: string, serviceId: string, action: ServiceAction): Promise<{ ok: true } | { ok: false; message: string }> {
  const path = action === "restart" ? "/api/projects/{id}/services/{sid}/restart" : "/api/projects/{id}/services/{sid}/stop";
  let result;
  try {
    result = await api.POST(path, { params: { path: { id: projectId, sid: serviceId } } });
  } catch {
    return { ok: false, message: "Serveur injoignable. Réessayez." };
  }
  if (result.data) return { ok: true };
  const { status } = result.response;
  const message = (result.error as { error?: { message?: unknown } } | undefined)?.error?.message;
  if (status === 403) return { ok: false, message: "Accès refusé : cette action demande le rôle développeur." };
  if ((status === 404 || status === 409 || status === 503) && typeof message === "string") return { ok: false, message };
  return { ok: false, message: action === "restart" ? "Redémarrage impossible pour le moment. Réessayez." : "Arrêt impossible pour le moment. Réessayez." };
}
