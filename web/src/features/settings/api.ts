import { api } from "@/lib/api/client";
import { unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";

export type NotificationSettings = components["schemas"]["NotificationSettings"];
export type NotificationUpdate = components["schemas"]["UpdateNotificationSettings"];
export type GeneralSettings = components["schemas"]["GeneralSettings"];
export type GeneralUpdate = components["schemas"]["UpdateGeneralSettings"];
export type DeploySettings = components["schemas"]["DeploySettings"];
export type OrgSecurity = components["schemas"]["OrgSecurity"];
export type OrgSecurityUpdate = components["schemas"]["UpdateOrgSecurity"];
export type DangerZone = components["schemas"]["DangerZone"];
export type OrganizationDeleted = components["schemas"]["OrganizationDeleted"];
export type PanelDomain = components["schemas"]["PanelDomain"];

export const NOTIFICATIONS_QUERY_KEY = ["settings", "notifications"] as const;
export const GENERAL_QUERY_KEY = ["settings", "general"] as const;
export const DEPLOY_QUERY_KEY = ["settings", "deployments"] as const;
export const SECURITY_QUERY_KEY = ["settings", "security"] as const;
export const DANGER_QUERY_KEY = ["settings", "danger"] as const;
export const PANEL_DOMAIN_QUERY_KEY = ["settings", "panel-domain"] as const;

/** Echec d'une ecriture : message affichable et code de l'API (ex. `ip_allowlist_lockout`). */
export type Failure = { ok: false; status: number; code: string; message: string };
export type Result<T> = { ok: true; data: T } | Failure;

const UNREACHABLE: Failure = { ok: false, status: 0, code: "unreachable", message: "Serveur injoignable. Réessayez." };

interface Raw<T> {
  data?: T;
  error?: unknown;
  response: Response;
}

/** Resultat d'openapi-fetch en succes ou en message : 422/409/401/502 gardent le message du serveur. */
async function settle<T>(request: () => Promise<Raw<T>>, okWithoutBody?: T): Promise<Result<T>> {
  let result: Raw<T>;
  try {
    result = await request();
  } catch {
    return UNREACHABLE;
  }
  const { status } = result.response;
  if (result.response.ok) return { ok: true, data: (result.data ?? okWithoutBody) as T };
  const err = (result.error as { error?: { code?: unknown; message?: unknown } } | undefined)?.error;
  const code = typeof err?.code === "string" ? err.code : "";
  const serverMessage = typeof err?.message === "string" ? err.message : "";
  let message = "Enregistrement impossible pour le moment. Réessayez.";
  if (status === 403 && code !== "ip_not_allowed") message = "Accès refusé : votre rôle ne permet pas cette action.";
  else if (status === 429) message = "Trop de tentatives. Réessayez dans quelques minutes.";
  else if (serverMessage && [401, 403, 404, 409, 422, 502].includes(status)) message = serverMessage;
  return { ok: false, status, code, message };
}

export async function fetchNotifications(): Promise<NotificationSettings> {
  return unwrap(await api.GET("/api/settings/notifications"));
}

export type SaveResult = { ok: true; settings: NotificationSettings } | { ok: false; message: string };

/** Enregistre les reglages ; sans `password`, le mot de passe enregistre est conserve. */
export async function saveNotifications(body: NotificationUpdate): Promise<SaveResult> {
  let result;
  try {
    result = await api.PUT("/api/settings/notifications", { body });
  } catch {
    return { ok: false, message: "Serveur injoignable. Réessayez." };
  }
  if (result.data) return { ok: true, settings: result.data };
  const message = (result.error as { error?: { message?: unknown } } | undefined)?.error?.message;
  if (result.response.status === 422 && typeof message === "string") return { ok: false, message };
  if (result.response.status === 403) return { ok: false, message: "Accès refusé : réservé aux administrateurs." };
  return { ok: false, message: "Enregistrement impossible pour le moment. Réessayez." };
}

export async function fetchGeneral(): Promise<GeneralSettings> {
  return unwrap(await api.GET("/api/settings/general"));
}

export function saveGeneral(body: GeneralUpdate): Promise<Result<GeneralSettings>> {
  return settle(() => api.PATCH("/api/settings/general", { body }));
}

export async function fetchDeploySettings(): Promise<DeploySettings> {
  return unwrap(await api.GET("/api/settings/deployments"));
}

export function saveDeploySettings(auto: boolean): Promise<Result<DeploySettings>> {
  return settle(() => api.PATCH("/api/settings/deployments", { body: { auto } }));
}

export async function fetchSecurity(): Promise<OrgSecurity> {
  return unwrap(await api.GET("/api/settings/security"));
}

/** Une liste qui exclut l'adresse courante est refusee par le serveur (`ip_allowlist_lockout`). */
export function saveSecurity(body: OrgSecurityUpdate): Promise<Result<OrgSecurity>> {
  return settle(() => api.PATCH("/api/settings/security", { body }));
}

export async function fetchDangerZone(): Promise<DangerZone> {
  return unwrap(await api.GET("/api/settings/danger"));
}

/** Export JSON (sans secrets) : le fichier est genere par le serveur, telecharge tel quel. */
export async function exportOrganization(): Promise<Result<string>> {
  return settle(() => api.POST("/api/organization/export", { parseAs: "text" }));
}

export function deleteOrganization(password: string, confirm: string): Promise<Result<OrganizationDeleted>> {
  return settle(() => api.DELETE("/api/organization", { body: { password, confirm } }));
}

export async function fetchPanelDomain(): Promise<PanelDomain> {
  return unwrap(await api.GET("/api/settings/panel-domain"));
}

export function setPanelDomain(domain: string): Promise<Result<PanelDomain>> {
  return settle(() => api.PUT("/api/settings/panel-domain", { body: { domain } }));
}

/** A appeler depuis le nouveau domaine ; avec une GitHub App, `githubSetupUrlUpdated` confirme le changement de la Setup URL. */
export function confirmPanelDomain(githubSetupUrlUpdated: boolean): Promise<Result<PanelDomain>> {
  return settle(() => api.POST("/api/settings/panel-domain/confirm", { body: githubSetupUrlUpdated ? { github_setup_url_updated: true } : {} }));
}

export function removePanelDomain(): Promise<Result<null>> {
  return settle(() => api.DELETE("/api/settings/panel-domain"), null);
}

/** Telecharge `text` sous le nom `name` (fichier genere dans le navigateur, rien n'est stocke). */
export function download(name: string, text: string, type = "application/json") {
  const a = document.createElement("a");
  const url = URL.createObjectURL(new Blob([text], { type }));
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}
