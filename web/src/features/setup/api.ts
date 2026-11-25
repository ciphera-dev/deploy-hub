import { api } from "@/lib/api/client";
import { ApiError, unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";

export type SetupCheck = components["schemas"]["SetupCheck"];
export type SetupPayload = components["schemas"]["SetupComplete"];

/**
 * Refus propres a l'assistant (jeton invalide, jeton absent, assistant deja termine) :
 * ils sont rendus par l'ecran, pas par la politique d'erreur globale (ecran 500, redirection).
 */
export type Rejected = { rejected: 401 | 410 | 503; expired?: boolean };

const isRejection = (status: number): status is Rejected["rejected"] => status === 401 || status === 410 || status === 503;

/** Refus rendu par l'ecran ; `expired` : jeton reconnu mais plus de 24 h (`setup_token_expired`). */
function rejection(status: Rejected["rejected"], error: unknown): Rejected {
  const code = (error as { error?: { code?: unknown } } | undefined)?.error?.code;
  return code === "setup_token_expired" ? { rejected: status, expired: true } : { rejected: status };
}

const NEW_LINK = "lancez « sudo ciphera admin setup-token » sur le serveur puis ouvrez la nouvelle adresse.";

/** Message affiché pour un refus 401 ou 503 (le 410 renvoie vers la connexion). */
export function rejectionMessage(r: Rejected): string {
  if (r.expired) return `Lien d'installation expiré (24 h) : ${NEW_LINK}`;
  if (r.rejected === 401) return `Jeton d'installation invalide. Ouvrez l'adresse affichée par le serveur, ou ${NEW_LINK}`;
  return `Jeton d'installation introuvable sur le serveur : ${NEW_LINK}`;
}

const header = (token: string) => ({ "X-Setup-Token": token });

export async function fetchStatus(): Promise<{ initialized: boolean }> {
  return unwrap(await api.GET("/api/setup/status"));
}

export async function fetchChecks(token: string): Promise<{ checks: SetupCheck[] } | Rejected> {
  const result = await api.POST("/api/setup/checks", { params: { header: header(token) } });
  if (isRejection(result.response.status)) return rejection(result.response.status, result.error);
  return unwrap(result);
}

export async function submitSetup(token: string, body: SetupPayload): Promise<{ created: true } | Rejected> {
  const result = await api.POST("/api/setup/complete", { params: { header: header(token) }, body });
  if (isRejection(result.response.status)) return rejection(result.response.status, result.error);
  // 422 et autres refus : ApiError (statut seul), affiche par l'ecran de finalisation.
  if (!result.response.ok) throw new ApiError(result.response.status, "/api/setup/complete");
  return { created: true };
}
