import { api } from "@/lib/api/client";
import { unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";

export type ProjectVariable = components["schemas"]["ProjectVariable"];
export type PutProjectVariable = components["schemas"]["PutProjectVariable"];
export type GlobalVariable = components["schemas"]["GlobalVariable"];
export type NewGlobalVariable = components["schemas"]["NewGlobalVariable"];
export type UpdateGlobalVariable = components["schemas"]["UpdateGlobalVariable"];
export type VariablesStatus = components["schemas"]["VariablesStatus"];

export const projectVariablesQueryKey = (projectId: string, env: string) => ["projects", projectId, "variables", env] as const;
/** Sous la clé de la liste : invalidé avec elle. */
export const variablesStatusQueryKey = (projectId: string, env: string) => [...projectVariablesQueryKey(projectId, env), "status"] as const;
export const GLOBAL_VARIABLES_QUERY_KEY = ["variables"] as const;

/** Environnements de la maquette (portée des variables globales), dans l'ordre affiché. */
export const ENVIRONMENTS = ["production", "preview", "staging"] as const;

/** Avertissement d'une écriture : secret de moins de 4 caractères, accepté mais jamais masqué dans les journaux. */
export const TOO_SHORT_TO_REDACT = "too_short_to_redact";

/** Clés des variables renvoyées avec l'avertissement `too_short_to_redact`. */
export function shortSecretKeys(rows: { key: string; warnings?: string[] }[]): string[] {
  return rows.filter((r) => r.warnings?.includes(TOO_SHORT_TO_REDACT)).map((r) => r.key);
}

/** Message affiché après l'enregistrement d'un secret trop court (`null` s'il n'y en a pas). */
export function shortSecretNotice(keys: string[]): string | null {
  if (keys.length === 0) return null;
  return `${keys.join(", ")} : secret de moins de 4 caractères, enregistré mais non masqué dans les journaux.`;
}

/** `production` → `Production`. */
export function envLabel(name: string): string {
  return name.charAt(0).toUpperCase() + name.slice(1);
}

type Failure = { ok: false; message: string };
const UNREACHABLE: Failure = { ok: false, message: "Serveur injoignable. Réessayez." };

function refusal(status: number, error: unknown, fallback: string): string {
  const message = (error as { error?: { message?: unknown } } | undefined)?.error?.message;
  if (status === 403) return "Accès refusé : votre rôle ne permet pas cette action.";
  if ((status === 404 || status === 409 || status === 422) && typeof message === "string") return message;
  return fallback;
}

/** GET /api/projects/{id}/variables?env= (ops, developer, admin, owner) : un secret n'a jamais de valeur. */
export async function fetchProjectVariables(projectId: string, env: string): Promise<ProjectVariable[]> {
  return unwrap(await api.GET("/api/projects/{id}/variables", { params: { path: { id: projectId }, query: { env } } }));
}

/** GET /api/projects/{id}/variables/status?env= : modification non déployée (suppressions comprises). */
export async function fetchVariablesStatus(projectId: string, env: string): Promise<VariablesStatus> {
  return unwrap(await api.GET("/api/projects/{id}/variables/status", { params: { path: { id: projectId }, query: { env } } }));
}

/** PUT /api/projects/{id}/variables?env= (developer et plus) : renvoie la liste à jour. */
export async function putProjectVariable(projectId: string, env: string, body: PutProjectVariable): Promise<{ ok: true; list: ProjectVariable[] } | Failure> {
  let result;
  try {
    result = await api.PUT("/api/projects/{id}/variables", { params: { path: { id: projectId }, query: { env } }, body });
  } catch {
    return UNREACHABLE;
  }
  if (result.data) return { ok: true, list: result.data };
  return { ok: false, message: refusal(result.response.status, result.error, "Enregistrement impossible pour le moment. Réessayez.") };
}

/** PUT /api/projects/{id}/variables/bulk?env= : le contenu `.env` remplace les variables de l'environnement. */
export async function bulkProjectVariables(projectId: string, env: string, content: string): Promise<{ ok: true; list: ProjectVariable[] } | Failure> {
  let result;
  try {
    result = await api.PUT("/api/projects/{id}/variables/bulk", { params: { path: { id: projectId }, query: { env } }, body: { content } });
  } catch {
    return UNREACHABLE;
  }
  if (result.data) return { ok: true, list: result.data };
  return { ok: false, message: refusal(result.response.status, result.error, "Enregistrement impossible pour le moment. Réessayez.") };
}

/** DELETE /api/projects/{id}/variables/{key}?env= */
export async function deleteProjectVariable(projectId: string, env: string, key: string): Promise<{ ok: true } | Failure> {
  let result;
  try {
    result = await api.DELETE("/api/projects/{id}/variables/{key}", { params: { path: { id: projectId, key }, query: { env } } });
  } catch {
    return UNREACHABLE;
  }
  if (result.response.ok) return { ok: true };
  return { ok: false, message: refusal(result.response.status, result.error, "Suppression impossible pour le moment. Réessayez.") };
}

/** GET /api/variables (admin). */
export async function fetchGlobalVariables(): Promise<GlobalVariable[]> {
  return unwrap(await api.GET("/api/variables"));
}

/** POST /api/variables (admin). */
export async function createGlobalVariable(body: NewGlobalVariable): Promise<{ ok: true; variable: GlobalVariable } | Failure> {
  let result;
  try {
    result = await api.POST("/api/variables", { body });
  } catch {
    return UNREACHABLE;
  }
  if (result.data) return { ok: true, variable: result.data };
  return { ok: false, message: refusal(result.response.status, result.error, "Enregistrement impossible pour le moment. Réessayez.") };
}

/** PATCH /api/variables/{id} (admin) : valeur vide = conservée. */
export async function updateGlobalVariable(id: string, body: UpdateGlobalVariable): Promise<{ ok: true; variable: GlobalVariable } | Failure> {
  let result;
  try {
    result = await api.PATCH("/api/variables/{id}", { params: { path: { id } }, body });
  } catch {
    return UNREACHABLE;
  }
  if (result.data) return { ok: true, variable: result.data };
  return { ok: false, message: refusal(result.response.status, result.error, "Enregistrement impossible pour le moment. Réessayez.") };
}

/** DELETE /api/variables/{id} (admin). */
export async function deleteGlobalVariable(id: string): Promise<{ ok: true } | Failure> {
  let result;
  try {
    result = await api.DELETE("/api/variables/{id}", { params: { path: { id } } });
  } catch {
    return UNREACHABLE;
  }
  if (result.response.ok) return { ok: true };
  return { ok: false, message: refusal(result.response.status, result.error, "Suppression impossible pour le moment. Réessayez.") };
}

/** GET /api/variables/export?env= (admin) : `.env` sans les secrets (ni leurs clés). */
export async function exportGlobalVariables(env?: string): Promise<{ ok: true; text: string } | Failure> {
  let result;
  try {
    result = await api.GET("/api/variables/export", { params: { query: env ? { env } : {} }, parseAs: "text" });
  } catch {
    return UNREACHABLE;
  }
  if (result.response.ok && typeof result.data === "string") return { ok: true, text: result.data };
  return { ok: false, message: refusal(result.response.status, result.error, "Export impossible pour le moment. Réessayez.") };
}

/** Lignes `CLE=valeur` d'un contenu `.env` (aperçu de l'import ; le serveur valide). */
export function parseDotenv(text: string): [string, string][] {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("#") && l.includes("="))
    .map((l) => {
      const i = l.indexOf("=");
      return [l.slice(0, i).trim(), l.slice(i + 1).trim().replace(/^["']|["']$/g, "")];
    });
}

/** Télécharge `text` sous le nom `name` (fichier généré dans le navigateur, rien n'est stocké). */
export function download(name: string, text: string) {
  const a = document.createElement("a");
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}
