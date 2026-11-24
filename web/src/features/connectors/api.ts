import { api } from "@/lib/api/client";
import { unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";

export type GithubStatus = components["schemas"]["GithubStatus"];
export type GithubRepo = components["schemas"]["GithubRepo"];

export const GITHUB_STATUS_QUERY_KEY = ["connectors", "github"] as const;
export const GITHUB_REPOS_QUERY_KEY = ["git", "github", "repos"] as const;

type Failure = { ok: false; message: string };

function messageOf(error: unknown): string | undefined {
  const message = (error as { error?: { message?: unknown } } | undefined)?.error?.message;
  return typeof message === "string" ? message : undefined;
}

/** GET /api/connectors/github (owner et admin) : aucun secret n'est renvoye. */
export async function fetchGithubStatus(): Promise<GithubStatus> {
  return unwrap(await api.GET("/api/connectors/github"));
}

/**
 * Demande le manifest puis soumet le formulaire GitHub (le navigateur quitte
 * le panneau). Le `state` est porte par `post_url` ; rien n'est garde cote navigateur.
 */
export async function startGithubManifest(submit: (postUrl: string, manifest: unknown) => void = submitManifest): Promise<Failure | { ok: true }> {
  let result;
  try {
    result = await api.POST("/api/connectors/github/manifest", { body: { panel_url: globalThis.location?.origin ?? null } });
  } catch {
    return { ok: false, message: "Serveur injoignable. Réessayez." };
  }
  if (result.data) {
    submit(result.data.post_url, result.data.manifest);
    return { ok: true };
  }
  const { status } = result.response;
  if (status === 403) return { ok: false, message: "Accès refusé : réservé aux administrateurs." };
  return { ok: false, message: messageOf(result.error) ?? "Connexion à GitHub impossible pour le moment. Réessayez." };
}

/** Formulaire POST vers GitHub (flux « manifest » : champ `manifest` en JSON). */
function submitManifest(postUrl: string, manifest: unknown) {
  const form = document.createElement("form");
  form.method = "post";
  form.action = postUrl;
  const field = document.createElement("input");
  field.type = "hidden";
  field.name = "manifest";
  field.value = JSON.stringify(manifest);
  form.appendChild(field);
  document.body.appendChild(form);
  form.submit();
}

/** GET /api/connectors/github/callback : convertit le code renvoye par GitHub en App. */
export async function completeGithubManifest(code: string, state: string): Promise<Failure | { ok: true; status: GithubStatus }> {
  let result;
  try {
    result = await api.GET("/api/connectors/github/callback", { params: { query: { code, state } } });
  } catch {
    return { ok: false, message: "Serveur injoignable. Réessayez." };
  }
  if (result.data) return { ok: true, status: result.data };
  return { ok: false, message: messageOf(result.error) ?? "Connexion à GitHub impossible. Recommencez." };
}

/** DELETE /api/connectors/github : l'App reste a supprimer sur GitHub. */
export async function disconnectGithub(): Promise<boolean> {
  try {
    const result = await api.DELETE("/api/connectors/github");
    return result.response.ok;
  } catch {
    return false;
  }
}

/** GET /api/git/providers/github/repos : `null` si GitHub n'est pas connecte (saisie libre). */
export async function fetchGithubRepos(): Promise<GithubRepo[] | null> {
  let result;
  try {
    result = await api.GET("/api/git/providers/github/repos");
  } catch {
    return null;
  }
  if (result.data) return result.data;
  return null;
}
