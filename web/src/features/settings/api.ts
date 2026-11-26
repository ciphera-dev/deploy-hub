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
export type TeamMember = components["schemas"]["TeamMember"];
export type PendingInvitation = components["schemas"]["PendingInvitation"];
export type RemovedMember = components["schemas"]["RemovedMember"];
export type TeamRole = "owner" | "admin" | "ops" | "developer" | "viewer";
export type SystemVersion = components["schemas"]["SystemVersion"];

export const NOTIFICATIONS_QUERY_KEY = ["settings", "notifications"] as const;
export const GENERAL_QUERY_KEY = ["settings", "general"] as const;
export const DEPLOY_QUERY_KEY = ["settings", "deployments"] as const;
export const SECURITY_QUERY_KEY = ["settings", "security"] as const;
export const DANGER_QUERY_KEY = ["settings", "danger"] as const;
export const PANEL_DOMAIN_QUERY_KEY = ["settings", "panel-domain"] as const;
export const TEAM_MEMBERS_QUERY_KEY = ["team", "members"] as const;
export const TEAM_INVITATIONS_QUERY_KEY = ["team", "invitations"] as const;
export const SYSTEM_VERSION_QUERY_KEY = ["system", "version"] as const;

/** Version installee et derniere version publiee verifiee (`latest_expired` : signature echue). */
export async function fetchSystemVersion(): Promise<SystemVersion> {
  return unwrap(await api.GET("/api/system/version"));
}

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

/** Reglages SMTP sans la confirmation renforcee (ajoutee par [`saveNotifications`]). */
export type SmtpUpdate = Omit<NotificationUpdate, "current_password" | "code">;

/**
 * Enregistre les reglages (proprietaire, confirmation renforcee : mot de passe du compte et code du
 * second facteur s'il en a un) ; sans `password`, le mot de passe SMTP enregistre est conserve.
 */
export function saveNotifications(body: SmtpUpdate, stepUp: StepUp): Promise<Result<NotificationSettings>> {
  const confirm = { current_password: stepUp.password, ...(stepUp.code ? { code: stepUp.code } : {}) };
  return settle(() => api.PUT("/api/settings/notifications", { body: { ...body, ...confirm } }));
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

/**
 * Mot de passe, plus le code du second facteur (application ou code de secours) si le compte
 * en a un ; refus 409 `other_owners` tant que d'autres proprietaires existent.
 */
export function deleteOrganization(password: string, confirm: string, code?: string): Promise<Result<OrganizationDeleted>> {
  return settle(() => api.DELETE("/api/organization", { body: { password, confirm, ...(code ? { code } : {}) } }));
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

/** Revérifie le DNS et replanifie tout de suite le certificat (reprise après une panne). */
export function recheckPanelDomain(): Promise<Result<PanelDomain>> {
  return settle(() => api.POST("/api/settings/panel-domain/recheck"));
}

/** Option « Inclure les sous-domaines (HSTS) », utile seulement pour un domaine enregistrable lui-meme (`apex`). */
export function setPanelDomainHsts(includeSubdomains: boolean): Promise<Result<PanelDomain>> {
  return settle(() => api.PUT("/api/settings/panel-domain/hsts", { body: { include_subdomains: includeSubdomains } }));
}

export function removePanelDomain(): Promise<Result<null>> {
  return settle(() => api.DELETE("/api/settings/panel-domain"), null);
}

export async function fetchTeamMembers(): Promise<TeamMember[]> {
  return unwrap(await api.GET("/api/team/members"));
}

export async function fetchTeamInvitations(): Promise<PendingInvitation[]> {
  return unwrap(await api.GET("/api/team/invitations"));
}

/**
 * Confirmation renforcee des changements d'equipe privilegies (invitation, role, retrait, lien de
 * reinitialisation, remise a zero du 2FA) : mot de passe de l'auteur, plus le code de son second
 * facteur (application ou code de secours) s'il en a un.
 */
export type StepUp = { password: string; code?: string };

export function changeMemberRole(id: string, role: TeamRole, stepUp: StepUp): Promise<Result<null>> {
  return settle(() => api.PATCH("/api/team/members/{id}", { params: { path: { id } }, body: { role, ...stepUp } }), null);
}

/** Retire le membre : ses sessions sont revoquees, ses deploy hooks et invitations non utilisees supprimes. */
export function removeMember(id: string, stepUp: StepUp): Promise<Result<RemovedMember>> {
  return settle(() => api.DELETE("/api/team/members/{id}", { params: { path: { id } }, body: stepUp }));
}

/** Lien a usage unique (30 minutes) ; il n'est jamais redonne. */
export function createResetLink(id: string, stepUp: StepUp): Promise<Result<{ link: string }>> {
  return settle(() => api.POST("/api/team/members/{id}/reset-link", { params: { path: { id } }, body: stepUp }));
}

/** Supprime TOTP, passkeys et codes de secours du membre et revoque ses sessions. */
export function resetMemberMfa(id: string, stepUp: StepUp): Promise<Result<null>> {
  return settle(() => api.POST("/api/team/members/{id}/reset-mfa", { params: { path: { id } }, body: stepUp }), null);
}

export function inviteMember(body: { email: string; name?: string; role: TeamRole }, stepUp: StepUp): Promise<Result<{ link: string }>> {
  return settle(() => api.POST("/api/team/invitations", { body: { ...body, ...stepUp } }));
}

export function cancelInvitation(id: string): Promise<Result<null>> {
  return settle(() => api.DELETE("/api/team/invitations/{id}", { params: { path: { id } } }), null);
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
