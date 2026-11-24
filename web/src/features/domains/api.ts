import { api } from "@/lib/api/client";
import { unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";

export type Domain = components["schemas"]["Domain"];
export type ProjectDomains = components["schemas"]["ProjectDomains"];
export type NewDomain = components["schemas"]["NewDomain"];
export type UpdateDomain = components["schemas"]["UpdateDomain"];

export const domainsQueryKey = (projectId: string) => ["projects", projectId, "domains"] as const;

type Failure = { ok: false; message: string; code?: string };
const UNREACHABLE: Failure = { ok: false, message: "Serveur injoignable. Réessayez." };

function refusal(status: number, error: unknown, fallback: string): Failure {
  const e = (error as { error?: { message?: unknown; code?: unknown } } | undefined)?.error;
  const code = typeof e?.code === "string" ? e.code : undefined;
  if (status === 403) return { ok: false, message: "Accès refusé : votre rôle ne permet pas cette action.", code };
  if ((status === 404 || status === 409 || status === 422) && typeof e?.message === "string") return { ok: false, message: e.message, code };
  return { ok: false, message: fallback, code };
}

/** GET /api/domains?project= (viewer et plus). */
export async function fetchDomains(projectId: string): Promise<ProjectDomains> {
  return unwrap(await api.GET("/api/domains", { params: { query: { project: projectId } } }));
}

async function run<T>(call: () => Promise<{ data?: T; error?: unknown; response: Response }>, fallback: string): Promise<{ ok: true; data: T } | Failure> {
  let result;
  try {
    result = await call();
  } catch {
    return UNREACHABLE;
  }
  if (result.response.ok) return { ok: true, data: result.data as T };
  return refusal(result.response.status, result.error, fallback);
}

/** POST /api/domains?project= : domaine ajouté, DNS à vérifier. */
export function createDomain(projectId: string, body: NewDomain) {
  return run(() => api.POST("/api/domains", { params: { query: { project: projectId } }, body }), "Ajout impossible pour le moment. Réessayez.");
}

/** PATCH /api/domains?project= : domaine généré activé ou non. */
export function setGeneratedDomain(projectId: string, enabled: boolean) {
  return run(() => api.PATCH("/api/domains", { params: { query: { project: projectId } }, body: { generated_domain: enabled } }), "Modification impossible pour le moment. Réessayez.");
}

/** PATCH /api/domains/{id} */
export function updateDomain(id: string, body: UpdateDomain) {
  return run(() => api.PATCH("/api/domains/{id}", { params: { path: { id } }, body }), "Modification impossible pour le moment. Réessayez.");
}

/** DELETE /api/domains/{id} */
export function deleteDomain(id: string) {
  return run(() => api.DELETE("/api/domains/{id}", { params: { path: { id } } }), "Suppression impossible pour le moment. Réessayez.");
}

/** POST /api/domains/{id}/verify : A/AAAA vers l'IP du nœud ; correct : en ligne, certificat demandé. */
export function verifyDomain(id: string) {
  return run(() => api.POST("/api/domains/{id}/verify", { params: { path: { id } } }), "Vérification impossible pour le moment. Réessayez.");
}

/** POST /api/domains/{id}/renew */
export function renewDomain(id: string) {
  return run(() => api.POST("/api/domains/{id}/renew", { params: { path: { id } } }), "Renouvellement impossible pour le moment. Réessayez.");
}

/** POST /api/domains/{id}/primary : renvoie les domaines du projet. */
export function makePrimary(id: string) {
  return run(() => api.POST("/api/domains/{id}/primary", { params: { path: { id } } }), "Modification impossible pour le moment. Réessayez.");
}

/** POST /api/domains/{id}/redirect */
export function setRedirect(id: string, enabled: boolean) {
  return run(() => api.POST("/api/domains/{id}/redirect", { params: { path: { id } }, body: { enabled } }), "Modification impossible pour le moment. Réessayez.");
}

/** Domaine apex (deux étiquettes) : enregistrements A/AAAA sur « @ ». */
export function isApex(host: string): boolean {
  return host.split(".").length <= 2;
}

/** Enregistrements DNS à créer : A (ou AAAA) vers l'IP du nœud, et AAAA vers son IPv6 si elle est configurée. */
export function dnsRecords(host: string, nodeIp: string | null | undefined, nodeIp6?: string | null): { type: string; host: string; value: string }[] {
  const name = isApex(host) ? "@" : host.split(".").slice(0, -2).join(".");
  return [nodeIp, nodeIp6].filter((ip): ip is string => !!ip).map((ip) => ({ type: ip.includes(":") ? "AAAA" : "A", host: name, value: ip }));
}

/** Jours restants avant `iso` (arrondi inférieur). */
export function daysLeft(iso: string | null | undefined, now = Date.now()): number | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  return Number.isNaN(t) ? null : Math.floor((t - now) / 86_400_000);
}

/** « 14 mars 2027 ». */
export function formatDay(iso: string): string {
  return new Date(iso).toLocaleDateString("fr-FR", { day: "numeric", month: "short", year: "numeric" });
}
