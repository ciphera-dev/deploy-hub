import { api } from "@/lib/api/client";
import { unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";
import { errorCode, stepUpRefusal } from "@/features/auth/step-up";

export type GitSettings = components["schemas"]["GitSettings"];
export type UpdateGitSettings = components["schemas"]["UpdateGitSettings"];
export type DeployHook = components["schemas"]["DeployHook"];
export type CreatedDeployHook = components["schemas"]["CreatedDeployHook"];

export const gitQueryKey = (projectId: string) => ["projects", projectId, "git"] as const;
export const hooksQueryKey = (projectId: string) => ["projects", projectId, "deploy-hooks"] as const;

/** `code` : code d'erreur de l'API (ex. `second_factor_required`). */
type Failure = { ok: false; message: string; code?: string };

function refusal(status: number, error: unknown, fallback: string): string {
  const message = (error as { error?: { message?: unknown } } | undefined)?.error?.message;
  if (status === 401) return stepUpRefusal(errorCode(error));
  if (status === 429) return "Trop de tentatives. Patientez avant de réessayer.";
  if (status === 403) return "Accès refusé : votre rôle ne permet pas cette action.";
  if ((status === 409 || status === 422 || status === 404 || status === 503) && typeof message === "string") return message;
  return fallback;
}

/** GET /api/projects/{id}/git (viewer et plus). */
export async function fetchGitSettings(projectId: string): Promise<GitSettings> {
  return unwrap(await api.GET("/api/projects/{id}/git", { params: { path: { id: projectId } } }));
}

/** PATCH /api/projects/{id}/git (developer et plus) : seuls les champs presents changent. */
export async function saveGitSettings(projectId: string, body: UpdateGitSettings): Promise<{ ok: true; settings: GitSettings } | Failure> {
  let result;
  try {
    result = await api.PATCH("/api/projects/{id}/git", { params: { path: { id: projectId } }, body });
  } catch {
    return { ok: false, message: "Serveur injoignable. Réessayez." };
  }
  if (result.data) return { ok: true, settings: result.data };
  return { ok: false, message: refusal(result.response.status, result.error, "Enregistrement impossible pour le moment. Réessayez.") };
}

/** GET /api/projects/{id}/deploy-hooks (admin). */
export async function fetchDeployHooks(projectId: string): Promise<DeployHook[]> {
  return unwrap(await api.GET("/api/projects/{id}/deploy-hooks", { params: { path: { id: projectId } } }));
}

/**
 * POST /api/projects/{id}/deploy-hooks (admin, mot de passe exige, plus le code du second
 * facteur si le compte en a un) : le secret n'est renvoye qu'ici, jamais stocke cote navigateur.
 */
export async function createDeployHook(projectId: string, name: string, password: string, expiresAt?: string, code?: string): Promise<{ ok: true; created: CreatedDeployHook } | Failure> {
  let result;
  try {
    result = await api.POST("/api/projects/{id}/deploy-hooks", { params: { path: { id: projectId } }, body: { name, password, ...(expiresAt ? { expires_at: expiresAt } : {}), ...(code ? { code } : {}) } });
  } catch {
    return { ok: false, message: "Serveur injoignable. Réessayez." };
  }
  if (result.data) return { ok: true, created: result.data };
  return { ok: false, message: refusal(result.response.status, result.error, "Création impossible pour le moment. Réessayez."), code: errorCode(result.error) };
}

/** DELETE /api/projects/{id}/deploy-hooks/{hookId} (admin). */
export async function deleteDeployHook(projectId: string, hookId: string): Promise<{ ok: true } | Failure> {
  let result;
  try {
    result = await api.DELETE("/api/projects/{id}/deploy-hooks/{hookId}", { params: { path: { id: projectId, hookId } } });
  } catch {
    return { ok: false, message: "Serveur injoignable. Réessayez." };
  }
  if (result.response.ok) return { ok: true };
  return { ok: false, message: refusal(result.response.status, result.error, "Suppression impossible pour le moment. Réessayez.") };
}

/** POST /api/projects/{id}/restart (developer et plus) : redémarre le déploiement servi. */
export async function restartProject(projectId: string): Promise<{ ok: true } | Failure> {
  let result;
  try {
    result = await api.POST("/api/projects/{id}/restart", { params: { path: { id: projectId } } });
  } catch {
    return { ok: false, message: "Serveur injoignable. Réessayez." };
  }
  if (result.data) return { ok: true };
  return { ok: false, message: refusal(result.response.status, result.error, "Redémarrage impossible pour le moment. Réessayez.") };
}

/** URL complete d'un chemin du panneau (les hooks et le webhook sont servis par le panneau lui-meme). */
export function panelUrl(path: string): string {
  return `${globalThis.location?.origin ?? ""}${path}`;
}
